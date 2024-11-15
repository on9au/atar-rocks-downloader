use serde::{Deserialize, Serialize};

/// Default path to the configuration file.
pub const DEFAULT_CONFIG_PATH: &str = "./config.toml";

/// The URL containing the files. Include paths like `/files/` or `/downloads/` if required to point to the directory containing the files.
pub const DEFAULT_URL: &str = "https://example.com/files/";

/// The User-Agent header to use for the requests.
pub const DEFAULT_USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/58.0.3029.110 Safari/537.36";

/// Output directory for the downloaded files.
pub const DEFAULT_OUTPUT_DIR: &str = "./output";

/// Number of concurrent downloads to perform.
pub const DEFAULT_CONCURRENT_DOWNLOADS: usize = 30;

#[derive(Debug, Serialize, Deserialize)]
pub struct Config {
    pub url: String,
    pub user_agent: String,
    pub output_dir: String,
    pub concurrent_downloads: usize,
    pub filter: Vec<FilterRule>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FilterRule {
    pub rule_type: RuleType,
    pub pattern: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum RuleType {
    Include,
    Exclude,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            url: DEFAULT_URL.to_string(),
            user_agent: DEFAULT_USER_AGENT.to_string(),
            output_dir: DEFAULT_OUTPUT_DIR.to_string(),
            concurrent_downloads: DEFAULT_CONCURRENT_DOWNLOADS,
            filter: vec![FilterRule {
                rule_type: RuleType::Include,
                pattern: "*".to_string(), // Include all files by default
            }],
        }
    }
}

impl Config {
    pub fn from_file(path: &str) -> Result<Self, Box<dyn std::error::Error>> {
        let config = std::fs::read_to_string(path)?;
        let config: Config = toml::from_str(&config)?;
        Ok(config)
    }
}
