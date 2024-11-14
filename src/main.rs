use std::{
    fs::create_dir_all,
    io::{self, Write},
    process,
    sync::Arc,
    time::Duration,
};

use indicatif::{ProgressBar, ProgressStyle};
use reqwest::{
    header::{HeaderMap, HeaderValue, ACCEPT, ACCEPT_ENCODING, ACCEPT_LANGUAGE, REFERER},
    Client, Url,
};
use scraper::Selector;
use tokio::{
    fs::File,
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    sync::Semaphore,
    task,
};
use tracing::{debug, info, trace, warn};

/// The URL of atar.rocks files. Include /files to point to the files directory.
const DEFAULT_URL: &str = "https://atar.rocks/files/";

/// The User-Agent header to use for the requests.
const DEFAULT_USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/58.0.3029.110 Safari/537.36";

/// Output directory for the downloaded files.
const DEFAULT_OUTPUT_DIR: &str = "./output";

/// Number of concurrent downloads to perform.
const DEFAULT_CONCURRENT_DOWNLOADS: usize = 5;

/// Create Http Client with custom headers
fn create_http_client(user_agent: &str) -> Client {
    let mut headers = HeaderMap::new();
    headers.insert(
        ACCEPT,
        HeaderValue::from_static(
            "text/html,application/xhtml+xml,application/xml;q=0.9,image/webp,*/*;q=0.8",
        ),
    );
    headers.insert(ACCEPT_LANGUAGE, HeaderValue::from_static("en-US,en;q=0.5"));
    headers.insert(
        ACCEPT_ENCODING,
        HeaderValue::from_static("gzip, deflate, br"),
    );
    headers.insert(REFERER, HeaderValue::from_static("https://example.com"));

    headers.insert(
        reqwest::header::USER_AGENT,
        HeaderValue::from_str(user_agent).unwrap(),
    );

    Client::builder()
        .default_headers(headers)
        .gzip(true)
        .brotli(true)
        .redirect(reqwest::redirect::Policy::limited(10))
        .danger_accept_invalid_certs(false)
        .build()
        .unwrap()
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize the logger.

    // In debug mode, log everything.
    #[cfg(debug_assertions)]
    let subscriber = tracing_subscriber::fmt()
        .with_max_level(tracing::Level::TRACE)
        .finish();

    // In release mode, only log INFO and above.
    #[cfg(not(debug_assertions))]
    let subscriber = tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .finish();

    tracing::subscriber::set_global_default(subscriber).expect("setting default subscriber failed");

    // Allow user to input custom URL, USER_AGENT, and OUTPUT_DIR
    let mut url = String::new();
    let mut user_agent = String::new();
    let mut output_dir = String::new();
    let mut concurrent_downloads = String::new();

    // Prompt user for URL, default to DEFAULT_URL
    print!("Enter URL (default: {DEFAULT_URL}): ");
    io::stdout().flush().unwrap();
    io::stdin().read_line(&mut url)?;
    let url = {
        let trimmed = url.trim();
        if trimmed.is_empty() {
            String::from(DEFAULT_URL)
        } else {
            trimmed.to_string()
        }
    };

    // Prompt user for User-Agent, default to DEFAULT_USER_AGENT
    print!("Enter User-Agent (If you do not know what this is, just press enter) (default: Check Source Code): ");
    io::stdout().flush().unwrap();
    io::stdin().read_line(&mut user_agent)?;
    let user_agent = {
        let trimmed = user_agent.trim();
        if trimmed.is_empty() {
            String::from(DEFAULT_USER_AGENT)
        } else {
            trimmed.to_string()
        }
    };

    // Prompt user for output directory, default to DEFAULT_OUTPUT_DIR
    print!("Enter output directory (default: {DEFAULT_OUTPUT_DIR}): ");
    io::stdout().flush().unwrap();
    io::stdin().read_line(&mut output_dir)?;
    let output_dir = {
        let trimmed = output_dir.trim();
        if trimmed.is_empty() {
            String::from(DEFAULT_OUTPUT_DIR)
        } else {
            trimmed.to_string()
        }
    };

    // Prompt user for concurrent downloads, default to DEFAULT_CONCURRENT_DOWNLOADS
    print!("Enter number of concurrent downloads (default: {DEFAULT_CONCURRENT_DOWNLOADS}): ");
    io::stdout().flush().unwrap();
    io::stdin().read_line(&mut concurrent_downloads)?;
    let concurrent_downloads = {
        let trimmed = concurrent_downloads.trim();
        if trimmed.is_empty() {
            DEFAULT_CONCURRENT_DOWNLOADS
        } else {
            trimmed.parse::<usize>().unwrap()
        }
    };

    // Create an HTTP client with custom headers
    let client = create_http_client(&user_agent);

    info!("URL: {}", url);
    info!("Downloads will be at: {}", output_dir);
    info!("Concurrent downloads: {}", concurrent_downloads);

    info!("Scanning website for files to download. This may take a while...");

    // Craw the website and collect files to download

    // Create a progress bar to display scanning status live
    let pb = ProgressBar::new_spinner();
    pb.set_style(
        ProgressStyle::with_template("{spinner:.green} ({elapsed}) Hits: {pos:3} | {msg}")?
            .progress_chars("─┼━"),
    );
    pb.set_message("Scanning...");
    pb.enable_steady_tick(Duration::from_millis(150)); // Enable a steady tick every 150ms

    // Begin crawling the website.
    let (download_list, total_size, directories_to_create) =
        crawl_directory(&client, DEFAULT_URL, DEFAULT_OUTPUT_DIR, &pb, &mut 0).await?;

    // Complete the progress bar when finished
    pb.finish_with_message("Scan complete.");

    // Present the list of files to download and total size
    info!(
        "Found {} files to download, estimated total size: {} bytes",
        download_list.len(),
        total_size
    );

    // Display file names and prompt the user for confirmation
    display_files_and_prompt(&download_list, total_size).await?;

    // Create directories for the files to download
    // Since we expect a large number of directories, we create them in parallel
    info!("Creating directories for the files...");

    let create_dir_tasks = directories_to_create.iter().map(|dir| {
        let dir = dir.clone();
        task::spawn(async move {
            create_dir_all(dir)?;
            Ok::<_, io::Error>(())
        })
    });

    // Wait for all directory creation tasks to complete
    let results = futures::future::join_all(create_dir_tasks).await;

    // Check if any directory creation task failed
    {
        let mut failed = false;
        for result in results {
            if let Err(e) = result {
                tracing::error!("Failed to create directory: {}", e);
                failed = true;
            }
        }

        if failed {
            tracing::error!("Failed to create directories for the files. Aborting download.");
            process::exit(1);
        }
    }

    // After crawling, download files asynchronously in parallel
    info!("Downloading files...");

    download_files_parallel(&client, download_list, concurrent_downloads).await?;

    // Download complete!
    info!("Download complete. Files have been saved to {}", output_dir);

    Ok(())
}

/// Displays the files and total size, then prompts the user for confirmation.
async fn display_files_and_prompt(
    files: &[String],
    total_size: u64,
) -> Result<(), Box<dyn std::error::Error>> {
    // Display the files to download
    info!("Files to download:");
    for file in files {
        info!("{}", file);
    }

    // Display the total size
    info!("Total size: {} bytes", total_size);

    // Prompt the user for confirmation
    let mut user_input = String::new();
    let stdin = tokio::io::stdin();
    let mut reader = BufReader::new(stdin);

    // info!("Do you want to proceed with downloading the files? (Y/n)");

    // print and flush the message
    print!("\nDo you want to proceed with downloading the files? (Y/n): ");
    std::io::stdout().flush().unwrap();

    // Read the user input
    reader.read_line(&mut user_input).await?;

    // Trim the input to remove extra spaces or newlines
    let user_input = user_input.trim().to_lowercase();

    // If the input is 'n' or 'no', cancel the download
    if user_input == "n" || user_input == "no" {
        info!("Download canceled.");
        process::exit(0);
    }

    // If the input is empty or 'y'/'yes', proceed
    info!("Proceeding with download...");
    Ok(())
}

/// Crawls the directory at the given URL and collects files to download.
async fn crawl_directory(
    client: &Client,
    url: &str,
    output_dir: &str,
    pb: &ProgressBar,
    total_size: &mut u64,
) -> Result<(Vec<String>, u64, Vec<String>), Box<dyn std::error::Error>> {
    // Send a GET request to the URL
    // random_sleep().await; // Sleep for a random duration before sending the request
    let response = client.get(url).send().await?;
    let body = response.text().await?;

    // Parse HTML body to extract links
    let document = scraper::Html::parse_document(&body);
    let selector = Selector::parse("a")?;

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

            // Format the total size and set the message
            let formatted_size = format_size(*total_size);
            pb.set_message(format!("({:6}) Scanning: {}", formatted_size, full_url));

            pb.inc(1); // Increment the progress bar

            // Check if it's a directory (simple heuristic: ends with '/')
            if href.ends_with('/') {
                debug!("Found directory: {}", href);
                let new_output_dir = format!("{}/{}", output_dir, href.trim_end_matches('/'));

                directories_to_create.push(new_output_dir.clone());

                // Recurse into the directory
                let (sub_dir_files, _sub_dir_size, sub_directories_to_create) = Box::pin(
                    crawl_directory(client, full_url.as_str(), &new_output_dir, pb, total_size),
                )
                .await?;

                files_to_download.extend(sub_dir_files); // Collect files from subdirectory
                directories_to_create.extend(sub_directories_to_create); // Collect directories to create
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

    Ok((files_to_download, *total_size, directories_to_create))
}

/// Helper function to check if a URL should be skipped based on predefined conditions.
fn should_skip_url(href: &str) -> bool {
    href == "../"
        || href == "#"
        || href.starts_with("javascript:")
        || href.starts_with("mailto:")
        || href.starts_with("tel:")
        || href.starts_with('?')
}

/// Returns the file size from the Content-Length header (if available).
async fn get_file_size(client: &Client, url: &Url) -> Result<u64, Box<dyn std::error::Error>> {
    let response = client.head(url.clone()).send().await?;
    if response.status().is_success() {
        if let Some(content_length) = response.headers().get(reqwest::header::CONTENT_LENGTH) {
            if let Ok(size) = content_length.to_str()?.parse::<u64>() {
                return Ok(size);
            }
        }
    }
    warn!("Failed to get file size for: {}", url);
    Ok(0) // Return 0 if the size is not available
}

/// Formats a byte size into a human-readable format (e.g., "10.5 MB").
fn format_size(size: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = KB * 1024;
    const GB: u64 = MB * 1024;
    const TB: u64 = GB * 1024;

    let (value, unit) = if size >= TB {
        (size as f64 / TB as f64, "TB")
    } else if size >= GB {
        (size as f64 / GB as f64, "GB")
    } else if size >= MB {
        (size as f64 / MB as f64, "MB")
    } else if size >= KB {
        (size as f64 / KB as f64, "KB")
    } else {
        (size as f64, "bytes")
    };

    format!("{:.2} {}", value, unit)
}

/// Downloads files in parallel using async tasks.
async fn download_files_parallel(
    client: &Client,
    files: Vec<String>,
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

        // Spawn a task for each file download
        let task = tokio::spawn(async move {
            let permit = semaphore.acquire().await.unwrap(); // Acquire a permit before starting
            let file_name = file_url.split('/').last().unwrap();
            let file_path = format!("{}/{}", DEFAULT_OUTPUT_DIR, file_name);

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
async fn download_file(
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
