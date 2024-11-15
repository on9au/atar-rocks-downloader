mod config;
mod crawl_data;
mod network;
mod utils;

use std::{
    fs::create_dir_all,
    io::{self},
    path::Path,
    process,
    sync::{atomic::AtomicU64, Arc},
    time::Duration,
};

use chrono::Utc;
use clap::Parser;
use config::{Config, DEFAULT_CONFIG_PATH};
use crawl_data::CrawlData;
use indicatif::{ProgressBar, ProgressStyle};
use network::{crawl_directory, download_files_parallel};
use percent_encoding::percent_decode_str;
use tokio::task;
use tracing::{error, info, trace, warn};
use utils::{create_http_client, display_files_and_prompt};

/// Command-line arguments
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Path to the saved crawl data file
    #[arg(short, long, default_value = "crawl_data.bin")]
    crawl_data_path: String,

    /// Load crawl data from file instead of crawling the website
    #[arg(short, long)]
    load_from_file: bool,

    /// Save crawl data to file after crawling the website
    #[arg(short, long)]
    save_to_file: bool,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    // Parse the command-line arguments
    let args = Args::parse();

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

    // Display confirmation of the arguments passed
    if args.load_from_file {
        info!("Loading crawl data from file: {}", args.crawl_data_path);
    } else {
        info!("Crawling website to generate crawl data...");
    }

    if args.save_to_file {
        info!("Saving crawl data to file: {}", args.crawl_data_path);
    }

    if args.load_from_file && args.save_to_file {
        error!("Cannot load and save to the same file. Aborting.");
        // On Windows, the console window closes immediately after the program exits.
        // To prevent this, we wait for user input before exiting.
        #[cfg(windows)]
        {
            use std::io::prelude::*;
            info!("Press Enter to exit...");
            let _ = std::io::stdin().read(&mut [0u8]).unwrap();
        }
        process::exit(1);
    }

    // Load the configuration from the config file.
    let config = Config::from_file(DEFAULT_CONFIG_PATH).unwrap_or_else(|e| {
        error!("Failed to load configuration: {}", e);

        // Create a default configuration if loading fails
        warn!(
            "Creating default configuration file at {}",
            DEFAULT_CONFIG_PATH
        );

        // Write the default configuration to the file
        let config = Config::default();
        let config_str = toml::to_string_pretty(&config).unwrap();
        std::fs::write(DEFAULT_CONFIG_PATH, config_str).unwrap();

        info!(
            "Default configuration file created. Please edit the file and run the program again."
        );

        info!("The file is located at: {}", DEFAULT_CONFIG_PATH);

        // On Windows, the console window closes immediately after the program exits.
        // To prevent this, we wait for user input before exiting.
        #[cfg(windows)]
        {
            use std::io::prelude::*;
            info!("Press Enter to exit...");
            let _ = std::io::stdin().read(&mut [0u8]).unwrap();
        }

        // Exit the program
        process::exit(1);
    });

    trace!("Configuration loaded: {:#?}", config);

    let crawl_data: CrawlData;

    // Create an HTTP client with custom headers
    let client = create_http_client(&config.user_agent);

    if args.load_from_file {
        if !Path::new(&args.crawl_data_path).exists() {
            error!("Crawl data file does not exist: {}", args.crawl_data_path);
            // On Windows, the console window closes immediately after the program exits.
            // To prevent this, we wait for user input before exiting.
            #[cfg(windows)]
            {
                use std::io::prelude::*;
                info!("Press Enter to exit...");
                let _ = std::io::stdin().read(&mut [0u8]).unwrap();
            }
            process::exit(1);
        }

        // Read the crawl data from the file
        let data_str = tokio::fs::read(&args.crawl_data_path).await?;
        crawl_data = bincode::deserialize(&data_str)?;
        info!("Loaded crawl data from {}", args.crawl_data_path);
    } else {
        // Crawl the website and save the data if requested
        info!("Scanning website for files to download. This may take a very long time...");

        let pb = ProgressBar::new_spinner();
        pb.set_style(
            ProgressStyle::with_template("{spinner:.green} ({elapsed}) Hits: {pos:3} | {msg}")?
                .progress_chars("─┼━"),
        );
        pb.set_message("Scanning...");
        pb.enable_steady_tick(Duration::from_millis(150));

        let (download_list, total_size, directories_to_create) = crawl_directory(
            client.clone(),
            config.url,
            config.output_dir.clone(),
            pb.clone(),
            Arc::new(AtomicU64::from(0)),
            Arc::from(config.filter.as_slice()),
            "".to_string(),
        )
        .await?;

        pb.finish_with_message("Scan complete.");

        crawl_data = CrawlData {
            download_list,
            total_size,
            directories_to_create,
            saved_at: Utc::now(),
        };

        if args.save_to_file {
            let data_str = bincode::serialize(&crawl_data)?;
            tokio::fs::write(&args.crawl_data_path, data_str).await?;
            info!("Saved crawl data to {}", args.crawl_data_path);
        }
    }

    // Display file names and prompt the user for confirmation
    display_files_and_prompt(&crawl_data.download_list, crawl_data.total_size).await?;

    // Create directories for the files to download
    // Since we expect a large number of directories, we create them in parallel
    info!("Creating directories for the files...");

    let create_dir_tasks = crawl_data.directories_to_create.iter().map(|dir| {
        let dir = dir.clone();
        let dir = percent_decode_str(&dir).decode_utf8_lossy().to_string();
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
            // On Windows, the console window closes immediately after the program exits.
            // To prevent this, we wait for user input before exiting.
            #[cfg(windows)]
            {
                use std::io::prelude::*;
                info!("Press Enter to exit...");
                let _ = std::io::stdin().read(&mut [0u8]).unwrap();
            }
            process::exit(1);
        }
    }

    // After crawling, download files asynchronously in parallel
    info!("Downloading files...");

    download_files_parallel(
        &client,
        crawl_data.download_list,
        &config.output_dir,
        config.concurrent_downloads,
        crawl_data.total_size,
    )
    .await?;

    // Download complete!
    info!(
        "Download complete. Files have been saved to {}",
        config.output_dir
    );

    Ok(())
}
