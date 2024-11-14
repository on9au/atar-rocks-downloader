mod config;
mod network;
mod utils;

use std::{
    fs::create_dir_all,
    io::{self, Write},
    process,
    time::Duration,
};

use config::{DEFAULT_CONCURRENT_DOWNLOADS, DEFAULT_OUTPUT_DIR, DEFAULT_URL, DEFAULT_USER_AGENT};
use indicatif::{ProgressBar, ProgressStyle};
use network::{crawl_directory, download_files_parallel};
use tokio::task;
use tracing::info;
use utils::{create_http_client, display_files_and_prompt};

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

    download_files_parallel(&client, download_list, &output_dir, concurrent_downloads).await?;

    // Download complete!
    info!("Download complete. Files have been saved to {}", output_dir);

    Ok(())
}
