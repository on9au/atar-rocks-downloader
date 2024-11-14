use std::fmt::Display;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

// Define the CrawlData struct
#[derive(Debug, Serialize, Deserialize)]
pub struct CrawlData {
    pub download_list: Vec<DownloadData>,
    pub total_size: u64,
    pub directories_to_create: Vec<String>,
    pub saved_at: DateTime<Utc>,
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
