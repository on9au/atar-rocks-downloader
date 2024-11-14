mod config;
mod network;
mod utils;

use std::{
    fs::create_dir_all,
    io::{self},
    process,
    time::Duration,
};

use config::{Config, DEFAULT_CONFIG_PATH};
use indicatif::{ProgressBar, ProgressStyle};
use network::{crawl_directory, download_files_parallel};
use tokio::task;
use tracing::{error, info, trace};
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

    // Load the configuration from the config file.
    let config = Config::from_file(DEFAULT_CONFIG_PATH).unwrap_or_else(|e| {
        error!("Failed to load configuration: {}", e);
        error!("Using default configuration instead.");

        // Create a default configuration if loading fails
        info!(
            "Creating default configuration file at {}",
            DEFAULT_CONFIG_PATH
        );

        // Write the default configuration to the file
        let config = Config::default();
        let config_str = toml::to_string_pretty(&config).unwrap();
        std::fs::write(DEFAULT_CONFIG_PATH, config_str).unwrap();

        // Return the default configuration
        Config::default()
    });

    trace!("Configuration loaded: {:#?}", config);

    // Create an HTTP client with custom headers
    let client = create_http_client(&config.user_agent);

    info!("Scanning website for files to download. This may take a very long time...");

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
    let (download_list, total_size, directories_to_create) = crawl_directory(
        &client,
        &config.url,
        &config.output_dir,
        &pb,
        &mut 0,
        &config.filter,
    )
    .await?;

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

    download_files_parallel(
        &client,
        download_list,
        &config.output_dir,
        config.concurrent_downloads,
    )
    .await?;

    // Download complete!
    info!(
        "Download complete. Files have been saved to {}",
        config.output_dir
    );

    Ok(())
}
