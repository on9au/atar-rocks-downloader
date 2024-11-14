/// The URL of atar.rocks files. Include /files to point to the files directory.
pub const DEFAULT_URL: &str = "https://atar.rocks/files/";

/// The User-Agent header to use for the requests.
pub const DEFAULT_USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/58.0.3029.110 Safari/537.36";

/// Output directory for the downloaded files.
pub const DEFAULT_OUTPUT_DIR: &str = "./output";

/// Number of concurrent downloads to perform.
pub const DEFAULT_CONCURRENT_DOWNLOADS: usize = 5;
