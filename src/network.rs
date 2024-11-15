use std::{
    future::Future,
    pin::Pin,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
};

use futures::StreamExt;
use indicatif::{MultiProgress, ProgressBar, ProgressStyle};
use percent_encoding::percent_decode_str;
use reqwest::{Client, Response, Url};
use scraper::Selector;
use tokio::{fs::File, io::AsyncWriteExt, sync::Semaphore};
use tokio_retry2::{
    strategy::{jitter, ExponentialBackoff, MaxInterval},
    Retry, RetryError,
};
use tracing::{debug, trace, warn};

use crate::{
    config::FilterRule,
    crawl_data::DownloadData,
    utils::{format_size, get_file_size, should_filter, should_skip_url, truncate_string},
};

/// What the fuck, i mean it works at least ig
type CrawlDirectoryResult = Pin<
    Box<
        dyn Future<
                Output = Result<
                    (Vec<DownloadData>, u64, Vec<String>),
                    Box<dyn std::error::Error + Send + Sync>,
                >,
            > + Send,
    >,
>;

/// GET url
async fn get_url(
    client: &Client,
    url: &str,
) -> Result<Response, RetryError<Box<dyn std::error::Error + Send + Sync>>> {
    let result = client.get(url).send().await;

    result
        .map_err(|e| RetryError::transient(Box::new(e) as Box<dyn std::error::Error + Send + Sync>))
}

/// Notify a retry error
#[allow(clippy::borrowed_box)] // it forces a &Box lmao
fn notify(err: &Box<dyn std::error::Error + Send + Sync>, duration: std::time::Duration) {
    warn!("Error {err} occurred at {duration:?}");
}

/// Crawls the directory at the given URL and collects files to download.
pub fn crawl_directory(
    client: Client,
    url: String,
    output_dir: String,
    pb: ProgressBar,
    total_size: Arc<AtomicU64>,
    filters: Arc<[FilterRule]>,
    root_relative_path: String,
) -> CrawlDirectoryResult {
    Box::pin(async move {
        trace!("Crawling link: {}", url);
        // Initialize the tables
        let mut files_to_download: Vec<DownloadData> = Vec::new();
        let mut directories_to_create: Vec<String> = Vec::new();

        let retry_strategy = ExponentialBackoff::from_millis(10)
            .factor(1)
            .max_delay_millis(100)
            .max_interval(10000)
            .map(jitter)
            .take(15);

        // Send a GET request to the URL
        // let response = client.get(&url).send().await?;
        let response: Response =
            Retry::spawn_notify(retry_strategy, || get_url(&client, &url), notify).await?;

        // Filter the response content to get the directories and files urls
        let links = extract_links(&response.text().await?, &url, &filters).await?;

        // Concurrently crawl each link
        let mut tasks = Vec::new();

        for (href, link, is_dir) in links {
            let formatted_size = format_size(total_size.load(Ordering::SeqCst));
            pb.set_message(format!("({:6}) Scanning: {}", formatted_size, link));
            pb.inc(1);
            if is_dir {
                // Create a task to crawl the directory
                let client = client.clone();

                let total_size = total_size.clone();

                let pb = pb.clone();

                let filters = filters.clone();

                let new_root_relative_path = if root_relative_path.is_empty() {
                    href.to_string()
                } else {
                    format!("{}/{}", root_relative_path, href)
                };
                let new_output_dir = format!("{}/{}", output_dir, href);

                // Add to the list of directories to create
                directories_to_create.push(format!("{}/{}", new_output_dir, href));

                let new_url = link.to_string();

                let task = tokio::task::spawn(async move {
                    Box::pin(crawl_directory(
                        client,
                        new_url.clone(),
                        new_output_dir,
                        pb,
                        total_size,
                        filters,
                        new_root_relative_path,
                    ))
                    .await
                });

                tasks.push(task);
            } else {
                // Add the file to the download list
                let file_size = get_file_size(&client, &link).await.unwrap_or_else(|_| {
                    warn!("Failed to get file size for {}", link);
                    0
                });

                total_size.fetch_add(file_size, Ordering::SeqCst);

                files_to_download.push(DownloadData {
                    url: link.to_string(),
                    output_dir: format!("{}/{}", root_relative_path, href),
                });
            }
        }

        // Wait for all tasks to complete
        let results = futures::future::join_all(tasks).await;

        // Collect the results from the tasks
        for result in results {
            let (files, _, dirs) = result??;
            files_to_download.extend(files);
            directories_to_create.extend(dirs);
        }

        Ok((
            files_to_download,
            total_size.load(Ordering::SeqCst),
            directories_to_create,
        ))
    })
}

/// Extracts links from the HTML content using the given root relative path.
async fn extract_links(
    content: &str,
    url: &str,
    filters: &[FilterRule],
) -> Result<Vec<(String, Url, bool)>, Box<dyn std::error::Error + Send + Sync>> {
    trace!("Extracting links from content");

    let document = scraper::Html::parse_document(content);
    let selector = Selector::parse("a").unwrap();

    let mut links: Vec<(String, Url, bool)> = Vec::new();

    for element in document.select(&selector) {
        if let Some(href) = element.value().attr("href") {
            trace!("Found link: {}", href);

            if should_skip_url(href) {
                continue;
            }

            let full_url = Url::parse(url)?.join(href)?;
            let relative_path = full_url.path();

            if should_filter(relative_path, filters).unwrap_or(false) {
                continue;
            }

            links.push((href.to_string(), full_url, href.ends_with('/')));
        }
    }

    Ok(links)
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
            .template("{msg:70} [{wide_bar:.cyan/blue}] {bytes:12}/{total_bytes:12} ({eta:4})")
            .unwrap()
            .progress_chars("█▓▒░"),
    );

    overall_pb.set_message("Overall Progress");

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
                    .template(
                        "{msg:70} [{wide_bar:.cyan/blue}] {bytes:12}/{total_bytes:12} ({eta:4})",
                    )
                    .unwrap(),
            );
            file_pb.set_message(truncate_string(&file.output_dir, 70).to_string());

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
    // Check if the file already exists
    if let Ok(metadata) =
        tokio::fs::metadata(format!("{}/{}", output_path, dload_file.output_dir)).await
    {
        if metadata.is_file() {
            debug!("Skipping existing file: {}", dload_file);
            return Ok(metadata.len());
        }
    }

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
