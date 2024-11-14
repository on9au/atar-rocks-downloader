use std::sync::Arc;

use indicatif::{ProgressBar, ProgressStyle};
use reqwest::{Client, Url};
use scraper::Selector;
use tokio::{fs::File, io::AsyncWriteExt, sync::Semaphore};
use tracing::{debug, info, trace};

use crate::{
    config::FilterRule,
    utils::{format_size, get_file_size, should_filter, should_skip_url},
};

/// Crawls the directory at the given URL and collects files to download.
pub async fn crawl_directory(
    client: &Client,
    start_url: &str,
    output_dir: &str,
    pb: &ProgressBar,
    total_size: &mut u64,
    filters: &[FilterRule],
) -> Result<(Vec<String>, u64, Vec<String>), Box<dyn std::error::Error>> {
    let mut files_to_download = Vec::new();
    let mut directories_to_create = Vec::new();
    // let mut tasks = Vec::new();

    // Use a stack to manage the directories to crawl
    let mut stack = vec![(start_url.to_string(), output_dir.to_string())];

    while let Some((url, output_dir)) = stack.pop() {
        // Send a GET request to the URL
        let response = client.get(&url).send().await?;
        let body = response.text().await?;

        // Parse HTML body to extract links
        let document = scraper::Html::parse_document(&body);
        let selector = Selector::parse("a")?;

        for element in document.select(&selector) {
            if let Some(href) = element.value().attr("href") {
                trace!("Found link with href: {}", href);

                // Skip unwanted URLs based on specific patterns
                if should_skip_url(href) {
                    continue;
                }

                let full_url = Url::parse(&url)?.join(href)?;
                let relative_path = full_url.path();

                // Check if the URL matches any of the filter rules
                if should_filter(relative_path, filters)? {
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

                    directories_to_create.push(new_output_dir.clone());

                    // Add the directory to the stack to crawl it later
                    stack.push((full_url.to_string(), new_output_dir));
                } else {
                    // It's a file; add it to the list of files to download
                    debug!("Found file: {}", href);
                    files_to_download.push(full_url.as_str().to_string());

                    // Get the file size (using HEAD request to avoid downloading it)
                    if let Ok(size) = get_file_size(client, &full_url).await {
                        *total_size += size;
                    }
                }
            }
        }
    }

    Ok((files_to_download, *total_size, directories_to_create))
}

/// Downloads files in parallel using async tasks.
pub async fn download_files_parallel(
    client: &Client,
    files: Vec<String>,
    output_dir: &str,
    concurrent_downloads: usize,
) -> Result<(), Box<dyn std::error::Error>> {
    let pb = Arc::new(tokio::sync::Mutex::new(
        ProgressBar::new(files.len() as u64),
    ));
    pb.lock().await.set_style(
        ProgressStyle::default_bar()
            .template("{bar:40} {pos}/{len} ({eta})")?
            .progress_chars("█▓▒░"),
    );

    // Limit the number of concurrent downloads
    let semaphore = Arc::new(Semaphore::new(concurrent_downloads));

    let mut tasks = Vec::new();

    // We need to Arc the client to share it among tasks
    let client: Arc<Client> = Arc::new(client.clone());

    for file_url in files {
        let client = client.clone();
        let pb = pb.clone();
        let semaphore = semaphore.clone();
        let output_dir = output_dir.to_string();

        // Spawn a task for each file download
        let task = tokio::spawn(async move {
            let permit = semaphore.acquire().await.unwrap(); // Acquire a permit before starting
            let file_name = file_url.split('/').last().unwrap();
            let file_path = format!("{}/{}", output_dir, file_name);

            // Download and save the file
            if let Err(e) = download_file(client, &file_url, &file_path).await {
                tracing::error!("Failed to download {}: {}", file_url, e);
            }

            let pb = pb.lock().await;
            pb.inc(1); // Increment the progress bar
            drop(permit); // Release the permit when done
        });

        tasks.push(task);
    }

    // Wait for all tasks to complete
    futures::future::join_all(tasks).await;

    pb.lock().await.finish_with_message("Download complete!");
    Ok(())
}

/// Downloads a file and saves it to the specified path.
pub async fn download_file(
    client: Arc<Client>,
    url: &str,
    output_path: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    // Send the GET request to download the file
    let response = client.clone().get(url).send().await?;

    // Ensure the response is successful
    if !response.status().is_success() {
        return Err(format!("Failed to download file: {}", url).into());
    }

    // Open the output file
    let mut file = File::create(output_path).await?;

    // Write the content to the file
    let content = response.bytes().await?.to_vec();
    file.write_all(&content).await?;

    info!("Downloaded {}", url);

    Ok(())
}
