use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

// Define the CrawlData struct
#[derive(Debug, Serialize, Deserialize)]
pub struct CrawlData {
    pub download_list: Vec<String>,
    pub total_size: u64,
    pub directories_to_create: Vec<String>,
    pub saved_at: DateTime<Utc>,
}
