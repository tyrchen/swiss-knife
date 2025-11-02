use thiserror::Error;

/// Errors that can occur during S3 upload operations
#[derive(Error, Debug)]
#[allow(dead_code)] // Infrastructure for future error handling improvements
pub enum S3UploadError {
    /// File not found on local filesystem
    #[error("File not found: {path}")]
    FileNotFound { path: String },

    /// Permission denied accessing local file
    #[error("Permission denied: {path}")]
    PermissionDenied { path: String },

    /// Network-related error
    #[error("Network error: {message}")]
    NetworkError { message: String },

    /// S3 access denied
    #[error("S3 access denied for bucket '{bucket}': {message}")]
    S3AccessDenied { bucket: String, message: String },

    /// File size exceeds maximum allowed
    #[error("File too large: {size} bytes (max: {max} bytes)")]
    FileTooLarge { size: u64, max: u64 },

    /// Invalid S3 key format
    #[error("Invalid S3 key: {key}")]
    InvalidS3Key { key: String },

    /// IO error wrapper
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    /// AWS SDK error wrapper
    #[error("AWS error: {0}")]
    AwsSdk(String),

    /// Generic error wrapper
    #[error("{0}")]
    Other(#[from] anyhow::Error),
}

impl S3UploadError {
    /// Create an S3 access denied error from an AWS SDK error
    #[allow(dead_code)] // Will be used when we integrate structured errors
    pub fn from_aws_error<E: std::fmt::Display>(bucket: &str, error: E) -> Self {
        let error_str = error.to_string();
        if error_str.to_lowercase().contains("access denied")
            || error_str.to_lowercase().contains("forbidden")
        {
            Self::S3AccessDenied {
                bucket: bucket.to_string(),
                message: error_str,
            }
        } else {
            Self::AwsSdk(error_str)
        }
    }

    /// Create an error from an IO error with context
    #[allow(dead_code)] // Will be used when we integrate structured errors
    pub fn from_io_error(error: std::io::Error, path: &str) -> Self {
        match error.kind() {
            std::io::ErrorKind::NotFound => Self::FileNotFound {
                path: path.to_string(),
            },
            std::io::ErrorKind::PermissionDenied => Self::PermissionDenied {
                path: path.to_string(),
            },
            _ => Self::Io(error),
        }
    }

    /// Get a user-friendly error message with suggestions
    #[allow(dead_code)] // Will be used when we integrate structured errors
    pub fn user_message(&self) -> String {
        match self {
            Self::FileNotFound { path } => {
                format!(
                    "File not found: {}\n\nPossible solutions:\n  \
                     1. Check if the file path is correct\n  \
                     2. Verify the file exists: ls -la {}",
                    path, path
                )
            }
            Self::PermissionDenied { path } => {
                format!(
                    "Permission denied: {}\n\nPossible solutions:\n  \
                     1. Check file permissions: ls -l {}\n  \
                     2. Ensure you have read access to the file",
                    path, path
                )
            }
            Self::S3AccessDenied { bucket, message } => {
                format!(
                    "Access denied for bucket '{}': {}\n\nPossible solutions:\n  \
                     1. Check your AWS credentials: aws sts get-caller-identity\n  \
                     2. Verify bucket permissions for your IAM user/role\n  \
                     3. Ensure bucket exists: aws s3 ls s3://{}\n  \
                     4. Check AWS_REGION in .env matches bucket region",
                    bucket, message, bucket
                )
            }
            Self::NetworkError { message } => {
                format!(
                    "Network error: {}\n\nPossible solutions:\n  \
                     1. Check your internet connection\n  \
                     2. Verify AWS region is correct in .env\n  \
                     3. Try again with --max-concurrent 1\n  \
                     4. Check if you're behind a proxy/firewall",
                    message
                )
            }
            Self::FileTooLarge { size, max } => {
                format!(
                    "File too large: {} bytes (max: {} bytes)\n\nPossible solutions:\n  \
                     1. Enable multipart upload for files > 100MB\n  \
                     2. Split the file into smaller chunks",
                    size, max
                )
            }
            _ => self.to_string(),
        }
    }
}

/// Result type for S3 upload operations
#[allow(dead_code)] // Will be used when we integrate structured errors
pub type Result<T> = std::result::Result<T, S3UploadError>;
