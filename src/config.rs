use anyhow::{Context, Result};
use std::env;

/// Configuration for S3 upload operations
#[derive(Debug, Clone)]
pub struct Config {
    pub region: String,
    pub profile: Option<String>,
    pub bucket: String,
    pub target_path: String,
}

impl Config {
    /// Load configuration from environment variables and .env file
    ///
    /// # Errors
    ///
    /// Returns an error if required environment variables are missing or invalid
    pub fn from_env() -> Result<Self> {
        dotenv::dotenv().ok(); // Load .env file if it exists

        let region = env::var("AWS_REGION")
            .context("AWS_REGION not found in environment. Please set it in .env file")?;
        Self::validate_region(&region)?;

        let profile = env::var("AWS_PROFILE").ok();

        let bucket = env::var("S3_BUCKET")
            .context("S3_BUCKET not found in environment. Please set it in .env file")?;
        Self::validate_bucket_name(&bucket)?;

        let target_path = env::var("S3_TARGET_PATH").unwrap_or_default();
        Self::validate_target_path(&target_path)?;

        Ok(Self {
            region,
            profile,
            bucket,
            target_path,
        })
    }

    /// Validate AWS region format
    fn validate_region(region: &str) -> Result<()> {
        if region.is_empty() {
            anyhow::bail!("AWS_REGION cannot be empty");
        }

        // Basic validation - ensure it looks like a region (contains a dash)
        if !region.contains('-') {
            anyhow::bail!(
                "AWS_REGION '{}' doesn't look like a valid region (e.g., us-west-2, eu-west-1)",
                region
            );
        }

        Ok(())
    }

    /// Validate S3 bucket name according to AWS rules
    fn validate_bucket_name(bucket: &str) -> Result<()> {
        if bucket.is_empty() {
            anyhow::bail!("S3_BUCKET cannot be empty");
        }

        if bucket.len() < 3 || bucket.len() > 63 {
            anyhow::bail!(
                "S3_BUCKET '{}' must be between 3 and 63 characters (got {})",
                bucket,
                bucket.len()
            );
        }

        // Check first and last characters
        if !bucket.chars().next().unwrap().is_ascii_lowercase()
            && !bucket.chars().next().unwrap().is_ascii_digit()
        {
            anyhow::bail!(
                "S3_BUCKET '{}' must start with a lowercase letter or number",
                bucket
            );
        }

        if !bucket.chars().last().unwrap().is_ascii_lowercase()
            && !bucket.chars().last().unwrap().is_ascii_digit()
        {
            anyhow::bail!(
                "S3_BUCKET '{}' must end with a lowercase letter or number",
                bucket
            );
        }

        // Check for invalid characters
        for c in bucket.chars() {
            if !c.is_ascii_lowercase() && !c.is_ascii_digit() && c != '-' && c != '.' {
                anyhow::bail!(
                    "S3_BUCKET '{}' contains invalid character '{}'. Only lowercase letters, numbers, hyphens, and periods are allowed",
                    bucket,
                    c
                );
            }
        }

        // Check for consecutive periods
        if bucket.contains("..") {
            anyhow::bail!("S3_BUCKET '{}' cannot contain consecutive periods", bucket);
        }

        // Check for IP address format (not allowed)
        if bucket
            .split('.')
            .all(|part| part.parse::<u8>().is_ok() && !part.is_empty())
        {
            anyhow::bail!(
                "S3_BUCKET '{}' cannot be formatted as an IP address",
                bucket
            );
        }

        Ok(())
    }

    /// Validate S3 target path
    fn validate_target_path(path: &str) -> Result<()> {
        if path.is_empty() {
            return Ok(());
        }

        // Check for invalid path segments
        if path.contains("//") {
            anyhow::bail!(
                "S3_TARGET_PATH '{}' contains consecutive slashes (not allowed)",
                path
            );
        }

        if path.contains("..") {
            anyhow::bail!(
                "S3_TARGET_PATH '{}' contains '..' (not allowed for security)",
                path
            );
        }

        // Check for absolute path (should be relative)
        if path.starts_with('/') {
            anyhow::bail!(
                "S3_TARGET_PATH '{}' should not start with '/' (use relative path)",
                path
            );
        }

        Ok(())
    }

    /// Construct S3 key from relative path
    ///
    /// # Arguments
    ///
    /// * `relative_path` - The relative path from the upload base directory
    ///
    /// # Returns
    ///
    /// The complete S3 object key including the target path prefix
    pub fn build_s3_key(&self, relative_path: &str) -> String {
        let path = relative_path.trim_start_matches("./");
        if self.target_path.is_empty() {
            path.to_string()
        } else {
            format!("{}/{}", self.target_path.trim_end_matches('/'), path)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bucket_name_validation() {
        // Valid bucket names
        assert!(Config::validate_bucket_name("my-bucket").is_ok());
        assert!(Config::validate_bucket_name("my.bucket.123").is_ok());
        assert!(Config::validate_bucket_name("abc").is_ok());
        assert!(Config::validate_bucket_name("my-bucket-name-123").is_ok());

        // Invalid bucket names
        assert!(Config::validate_bucket_name("ab").is_err()); // Too short
        assert!(Config::validate_bucket_name(&"a".repeat(64)).is_err()); // Too long
        assert!(Config::validate_bucket_name("MY-BUCKET").is_err()); // Uppercase
        assert!(Config::validate_bucket_name("my_bucket").is_err()); // Underscore
        assert!(Config::validate_bucket_name("-mybucket").is_err()); // Starts with dash
        assert!(Config::validate_bucket_name("mybucket-").is_err()); // Ends with dash
        assert!(Config::validate_bucket_name("my..bucket").is_err()); // Consecutive periods
        assert!(Config::validate_bucket_name("192.168.1.1").is_err()); // IP address format
        assert!(Config::validate_bucket_name("").is_err()); // Empty
    }

    #[test]
    fn test_region_validation() {
        // Valid regions
        assert!(Config::validate_region("us-west-2").is_ok());
        assert!(Config::validate_region("eu-west-1").is_ok());
        assert!(Config::validate_region("ap-southeast-1").is_ok());

        // Invalid regions
        assert!(Config::validate_region("").is_err()); // Empty
        assert!(Config::validate_region("uswest2").is_err()); // No dash
    }

    #[test]
    fn test_target_path_validation() {
        // Valid paths
        assert!(Config::validate_target_path("").is_ok());
        assert!(Config::validate_target_path("uploads").is_ok());
        assert!(Config::validate_target_path("uploads/videos").is_ok());

        // Invalid paths
        assert!(Config::validate_target_path("uploads//videos").is_err()); // Consecutive slashes
        assert!(Config::validate_target_path("../uploads").is_err()); // Contains ..
        assert!(Config::validate_target_path("/uploads").is_err()); // Absolute path
    }

    #[test]
    fn test_s3_key_construction() {
        let config = Config {
            region: "us-west-2".to_string(),
            profile: None,
            bucket: "test-bucket".to_string(),
            target_path: "uploads".to_string(),
        };

        assert_eq!(config.build_s3_key("file.mp4"), "uploads/file.mp4");
        assert_eq!(config.build_s3_key("./file.mp4"), "uploads/file.mp4");
        assert_eq!(
            config.build_s3_key("dir/file.mp4"),
            "uploads/dir/file.mp4"
        );

        // Test with empty target path
        let config_no_prefix = Config {
            region: "us-west-2".to_string(),
            profile: None,
            bucket: "test-bucket".to_string(),
            target_path: String::new(),
        };

        assert_eq!(config_no_prefix.build_s3_key("file.mp4"), "file.mp4");
        assert_eq!(
            config_no_prefix.build_s3_key("dir/file.mp4"),
            "dir/file.mp4"
        );
    }
}
