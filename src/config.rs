use anyhow::{Context, Result};
use std::env;

#[derive(Debug, Clone)]
pub struct Config {
    pub region: String,
    pub profile: Option<String>,
    pub bucket: String,
    pub target_path: String,
}

impl Config {
    pub fn from_env() -> Result<Self> {
        dotenv::dotenv().ok(); // Load .env file if it exists

        let region = env::var("AWS_REGION")
            .context("AWS_REGION not found in environment. Please set it in .env file")?;

        let profile = env::var("AWS_PROFILE").ok();

        let bucket = env::var("S3_BUCKET")
            .context("S3_BUCKET not found in environment. Please set it in .env file")?;

        let target_path = env::var("S3_TARGET_PATH").unwrap_or_default();

        Ok(Self {
            region,
            profile,
            bucket,
            target_path,
        })
    }

    /// Construct S3 key from relative path
    pub fn build_s3_key(&self, relative_path: &str) -> String {
        let path = relative_path.trim_start_matches("./");
        if self.target_path.is_empty() {
            path.to_string()
        } else {
            format!("{}/{}", self.target_path.trim_end_matches('/'), path)
        }
    }
}
