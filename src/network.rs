use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc,
};

use futures::StreamExt;
use indicatif::{MultiProgress, ProgressBar, ProgressStyle};
use percent_encoding::percent_decode_str;
use reqwest::{Client, Url};
use scraper::Selector;
use tokio::{fs::File, io::AsyncWriteExt, sync::Semaphore};
use tracing::{debug, trace};

use crate::{
    config::FilterRule,
    crawl_data::DownloadData,
    utils::{format_size, get_file_size, should_filter, should_skip_url, truncate_string},
};

/// Crawls the directory at the given URL and collects files to download.
pub async fn crawl_directory(
    client: &Client,
    url: &str,
    output_dir: &str,
    pb: &ProgressBar,
    total_size: &mut u64,
    filters: &[FilterRule],
    root_relative_path: &str,
) -> Result<(Vec<DownloadData>, u64, Vec<String>), Box<dyn std::error::Error + Send + Sync>> {
    // Send a GET request to the URL
    // random_sleep().await; // Sleep for a random duration before sending the request
    let response = client.get(url).send().await?;
    let body = response.text().await?;

    // Parse HTML body to extract links
    let document = scraper::Html::parse_document(&body);
    let selector = Selector::parse("a").unwrap();

    let mut files_to_download = Vec::new();
    let mut directories_to_create = Vec::new();

    for element in document.select(&selector) {
        if let Some(href) = element.value().attr("href") {
            trace!("Found link with href: {}", href);

            // Skip unwanted URLs based on specific patterns
            if should_skip_url(href) {
                continue;
            }

            let full_url = Url::parse(url)?.join(href)?;
            let relative_path = full_url.path();

            // Check if the URL matches any of the filter rules
            if should_filter(relative_path, filters).unwrap_or(false) {
                continue;
            }

            // Format the total size and set the message
            let formatted_size = format_size(*total_size);
            pb.set_message(format!("({:6}) Scanning: {}", formatted_size, full_url));

            pb.inc(1); // Increment the progress bar

            // Check if it's a directory (simple heuristic: ends with '/')
            if href.ends_with('/') {
                debug!("Found directory: {}", href);
                let new_output_dir = format!("{}/{}", output_dir, href.trim_end_matches('/'));

                let new_root_relative_path = if root_relative_path.is_empty() {
                    href
                } else {
                    &format!("{}/{}", root_relative_path, href)
                };

                directories_to_create.push(new_output_dir.clone());

                // Recurse into the directory
                let (sub_dir_files, _sub_dir_size, sub_directories_to_create) =
                    Box::pin(crawl_directory(
                        client,
                        full_url.as_str(),
                        &new_output_dir,
                        pb,
                        total_size,
                        filters,
                        new_root_relative_path,
                    ))
                    .await?;

                files_to_download.extend(sub_dir_files); // Collect files from subdirectory
                directories_to_create.extend(sub_directories_to_create); // Collect directories to create
            } else {
                // It's a file; add it to the list of files to download
                debug!("Found file: {}", href);
                files_to_download.push(DownloadData {
                    url: full_url.to_string(),
                    output_dir: format!("{}/{}", root_relative_path, href),
                });

                // Get the file size (using HEAD request to avoid downloading it)
                if let Ok(size) = get_file_size(client, &full_url).await {
                    *total_size += size;
                }
            }
        }
    }

    Ok((files_to_download, *total_size, directories_to_create))
}

/// Downloads files in parallel using async tasks.
pub async fn download_files_parallel(
    client: &Client,
    files: Vec<DownloadData>,
    output_dir: &str,
    concurrent_downloads: usize,
    total_size: u64,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let multi_pb = Arc::new(MultiProgress::new());
    let overall_pb = multi_pb.add(ProgressBar::new(total_size));
    overall_pb.set_style(
        ProgressStyle::default_bar()
            .template("Overall Progress [{wide_bar:.cyan/blue}] {bytes}/{total_bytes} ({eta})")
            .unwrap()
            .progress_chars("█▓▒░"),
    );

    // Limit the number of concurrent downloads
    let semaphore = Arc::new(Semaphore::new(concurrent_downloads));
    let total_size_downloaded = Arc::new(AtomicU64::new(0));

    let mut tasks = Vec::new();

    // We need to Arc the client to share it among tasks
    let client: Arc<Client> = Arc::new(client.clone());

    for mut file in files {
        let client = client.clone();
        let semaphore = semaphore.clone();
        let output_dir = output_dir.to_string();
        let total_size_downloaded = total_size_downloaded.clone();
        let multi_pb = multi_pb.clone();
        let overall_pb = overall_pb.clone();

        // Rename the file to decode any percent-encoded characters
        file.output_dir = percent_decode_str(&file.output_dir)
            .decode_utf8()?
            .into_owned();

        // Spawn a task for each file download
        let task = tokio::spawn(async move {
            let permit = semaphore.acquire().await.unwrap(); // Acquire a permit before starting

            // Create a progress bar for each file download
            let file_pb = multi_pb.add(ProgressBar::new_spinner());
            file_pb.set_style(
                ProgressStyle::default_bar()
                    .template("{msg} [{wide_bar:.cyan/blue}] {bytes}/{total_bytes} ({eta})")
                    .unwrap(),
            );
            file_pb.set_message(truncate_string(&file.url, 70).to_string());

            // Download and save the file
            match download_file(client, &file, &output_dir, &file_pb).await {
                Ok(size) => {
                    total_size_downloaded.fetch_add(size, Ordering::SeqCst);
                    overall_pb.set_position(total_size_downloaded.load(Ordering::SeqCst));
                    file_pb.finish_and_clear();
                }
                Err(e) => {
                    tracing::error!("Failed to download {}: {}", file, e);
                    file_pb.finish_and_clear();
                }
            }

            drop(permit); // Release the permit when done
        });

        tasks.push(task);
    }

    // Wait for all tasks to complete
    futures::future::join_all(tasks).await;

    overall_pb.finish_with_message("All downloads complete!");
    Ok(())
}

/// Downloads a file and saves it to the specified path, returning the size of the downloaded file.

/// Downloads a file and saves it to the specified path, returning the size of the downloaded file.
pub async fn download_file(
    client: Arc<Client>,
    dload_file: &DownloadData,
    output_path: &str,
    pb: &ProgressBar,
) -> Result<u64, Box<dyn std::error::Error>> {
    // Send the GET request to download the file
    let response = client.clone().get(dload_file.url.clone()).send().await?;

    // Ensure the response is successful
    if !response.status().is_success() {
        return Err(format!("Failed to download file: {}", dload_file).into());
    }

    // Get the total size of the file
    let total_size = response.content_length().unwrap_or(0);
    pb.set_length(total_size);

    // Open the output file
    let mut file = File::create(format!("{}/{}", output_path, dload_file.output_dir)).await?;

    // Write the content to the file in chunks
    let mut downloaded_size = 0;
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk?;
        file.write_all(&chunk).await?;
        downloaded_size += chunk.len() as u64;
        pb.set_position(downloaded_size);
    }

    debug!("Downloaded {}", dload_file);

    Ok(downloaded_size)
}
