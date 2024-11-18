use std::fmt::Display;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::utils::format_size;

// Define the CrawlData struct
#[derive(Debug, Serialize, Deserialize)]
pub struct CrawlData {
    pub download_list: Vec<DownloadData>,
    pub total_size: u64,
    pub directories_to_create: Vec<String>,
    pub saved_at: DateTime<Utc>,
}

impl Display for CrawlData {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "CrawlData: {} files, {} ({} bytes)\nSaved at: {}\n\n# Directories to create:\n{}\n\n# Files to download:\n{}",
            self.download_list.len(),
            format_size(self.total_size),
            self.total_size,
            self.saved_at,
            self.directories_to_create
                .iter()
                .fold(String::new(), |acc, dir| { acc + &format!("{}\n", dir) }),
            self.download_list.iter().fold(String::new(), |acc, file| { acc + &format!("{}\n", file) }),
        )
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct DownloadData {
    pub url: String,
    pub output_dir: String,
}

impl Display for DownloadData {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} -> {}", self.url, self.output_dir)
    }
}
