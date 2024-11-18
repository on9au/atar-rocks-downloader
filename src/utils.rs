use std::{io::Write, process};

use glob::Pattern;
use reqwest::{
    header::{HeaderMap, HeaderValue, ACCEPT, ACCEPT_ENCODING, ACCEPT_LANGUAGE, REFERER},
    Client, Url,
};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio_retry2::{strategy::{jitter, ExponentialBackoff, MaxInterval}, Retry, RetryError};
use tracing::{debug, info, warn};

use crate::{
    config::{FilterRule, RuleType},
    crawl_data::DownloadData,
};

/// Create Http Client with custom headers
pub fn create_http_client(user_agent: &str) -> Client {
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

/// Displays the files and total size, then prompts the user for confirmation.
pub async fn display_files_and_prompt(
    files: &[DownloadData],
    total_size: u64,
    skip_prompt: bool,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    // Display the files to download
    info!("Files to download:");
    for file in files {
        println!("{}", file.url);
    }

    // Display the total size
    info!("Total size: {} bytes", format_size(total_size));

    // Prompt the user for confirmation
    let mut user_input = String::new();
    let stdin = tokio::io::stdin();
    let mut reader = BufReader::new(stdin);

    // info!("Do you want to proceed with downloading the files? (Y/n)");

    // If the skip_prompt flag is set, skip the prompt
    if skip_prompt {
        info!("Skipping prompt due to --yes flag.");
        return Ok(());
    }

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
        // On Windows, the console window closes immediately after the program exits.
        // To prevent this, we wait for user input before exiting.
        #[cfg(windows)]
        {
            use std::io::prelude::*;
            info!("Press Enter to exit...");
            let _ = std::io::stdin().read(&mut [0u8]).unwrap();
        }
        process::exit(0);
    }

    // If the input is empty or 'y'/'yes', proceed
    info!("Proceeding with download...");
    Ok(())
}

/// Helper function to check if a URL should be skipped based on predefined conditions.
pub fn should_skip_url(href: &str) -> bool {
    href == "../"
        || href == "#"
        || href.starts_with("javascript:")
        || href.starts_with("mailto:")
        || href.starts_with("tel:")
        || href.starts_with('?')
}

async fn action(client: &Client, url: &Url) -> Result<u64, RetryError<Box<dyn std::error::Error + Send + Sync>>> {
    let response = client.head(url.clone()).send().await.map_err(|e| RetryError::transient(Box::new(e) as Box<dyn std::error::Error + Send + Sync>))?;
    if response.status().is_success() {
        if let Some(content_length) = response.headers().get(reqwest::header::CONTENT_LENGTH) {
            if let Ok(size) = content_length.to_str().map_err(|e| RetryError::transient(Box::new(e) as Box<dyn std::error::Error + Send + Sync>))?.parse::<u64>() {
                return Ok(size);
            }
        }
    }
    warn!("Failed to get file size for: {}", url);
    Ok(0) // Return 0 if the size is not available
}

#[allow(clippy::borrowed_box)] // it forces a &Box lmao
fn notify(err: &Box<dyn std::error::Error + Send + Sync>, duration: std::time::Duration) {
    warn!("Failed to get file size. Retrying... Error {err} occurred at {duration:?}");
}

/// Returns the file size from the Content-Length header (if available).
pub async fn get_file_size(client: &Client, url: &Url) -> Result<u64, Box<dyn std::error::Error + Send + Sync>> {
    let retry_strategy = ExponentialBackoff::from_millis(10)
            .factor(1)
            .max_delay_millis(100)
            .max_interval(10000)
            .map(jitter)
            .take(150);

        Retry::spawn_notify(retry_strategy, || action(client, url), notify).await
}

/// Formats a byte size into a human-readable format (e.g., "10.5 MB").
pub fn format_size(size: u64) -> String {
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

// Function to determine if a path should be filtered
pub fn should_filter(
    path: &str,
    filters: &[FilterRule],
) -> Result<bool, Box<dyn std::error::Error>> {
    let mut is_included = false;

    if filters.is_empty() {
        return Ok(false);
    }

    for rule in filters {
        let pattern = Pattern::new(&rule.pattern)?;
        if pattern.matches(path) {
            match rule.rule_type {
                RuleType::Include => {
                    debug!("Including path: {}", path);
                    is_included = true;
                }
                RuleType::Exclude => {
                    debug!("Excluding path: {}", path);
                    return Ok(true);
                }
            }
        }
    }

    if !is_included {
        debug!("Excluding path by default: {}", path);
    }

    Ok(!is_included)
}

/// Truncates a string to the specified length, adding "..." if truncated.
pub fn truncate_string(s: &str, max_length: usize) -> String {
    if s.len() > max_length {
        format!("{}...", &s[..max_length - 3])
    } else {
        s.to_string()
    }
}
