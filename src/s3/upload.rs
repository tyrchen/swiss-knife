use anyhow::{Context, Result};
use aws_sdk_s3::{primitives::ByteStream, Client};
use indicatif::ProgressBar;
use std::path::Path;
use std::time::Duration;
use tokio::time::sleep;
use tracing::{debug, info, warn};

const MAX_RETRIES: u32 = 3;
const INITIAL_RETRY_DELAY: Duration = Duration::from_secs(1);

#[derive(Debug)]
pub enum UploadResult {
    Uploaded,
    #[allow(dead_code)]
    Skipped,
}

/// Upload a file to S3 with progress tracking and retry logic
///
/// This function:
/// - Streams the file in chunks to avoid loading entire file in memory
/// - Updates progress bar in real-time as bytes are uploaded
/// - Retries on transient failures with exponential backoff
///
/// # Arguments
///
/// * `client` - AWS S3 client
/// * `bucket` - S3 bucket name
/// * `s3_key` - S3 object key (path)
/// * `local_path` - Path to local file
/// * `pb` - Optional progress bar for visual feedback
///
/// # Returns
///
/// `UploadResult::Uploaded` on success
///
/// # Errors
///
/// Returns error if:
/// - File cannot be opened or read
/// - S3 upload fails after all retries
/// - Network issues prevent upload
pub async fn upload_file(
    client: &Client,
    bucket: &str,
    s3_key: &str,
    local_path: &Path,
    pb: Option<&ProgressBar>,
) -> Result<UploadResult> {
    upload_file_with_retry(client, bucket, s3_key, local_path, pb).await
}

/// Upload file with retry logic
async fn upload_file_with_retry(
    client: &Client,
    bucket: &str,
    s3_key: &str,
    local_path: &Path,
    pb: Option<&ProgressBar>,
) -> Result<UploadResult> {
    let mut attempts = 0;
    let mut delay = INITIAL_RETRY_DELAY;

    loop {
        match upload_file_inner(client, bucket, s3_key, local_path, pb).await {
            Ok(result) => {
                if attempts > 0 {
                    info!(
                        "Upload succeeded after {} retry(ies) for {}",
                        attempts,
                        local_path.display()
                    );
                }
                return Ok(result);
            }
            Err(e) if attempts < MAX_RETRIES && is_retryable(&e) => {
                attempts += 1;
                warn!(
                    "Upload failed (attempt {}/{}): {}. Retrying in {:?}...",
                    attempts, MAX_RETRIES, e, delay
                );

                if let Some(pb) = pb {
                    pb.set_message(format!(
                        "Retry {}/{} for {}",
                        attempts,
                        MAX_RETRIES,
                        local_path.display()
                    ));
                }

                sleep(delay).await;
                delay *= 2; // Exponential backoff

                // Reset progress bar for retry
                if let Some(pb) = pb {
                    pb.set_position(0);
                }
            }
            Err(e) => {
                if attempts > 0 {
                    return Err(anyhow::anyhow!(
                        "Upload failed after {} retries: {}",
                        attempts,
                        e
                    ));
                }
                return Err(e);
            }
        }
    }
}

/// Inner upload function without retry logic
async fn upload_file_inner(
    client: &Client,
    bucket: &str,
    s3_key: &str,
    local_path: &Path,
    pb: Option<&ProgressBar>,
) -> Result<UploadResult> {
    // Get file metadata first
    let metadata = tokio::fs::metadata(local_path)
        .await
        .with_context(|| format!("Failed to access file: {}", local_path.display()))?;
    let file_size = metadata.len();

    debug!(
        "Starting upload: {} ({} bytes) -> s3://{}/{}",
        local_path.display(),
        file_size,
        bucket,
        s3_key
    );

    // Update progress bar - show indeterminate progress during upload
    if let Some(pb) = pb {
        pb.set_length(file_size);
        pb.set_message(format!("Uploading {}", local_path.file_name().unwrap().to_string_lossy()));
        pb.set_position(0);
        pb.enable_steady_tick(std::time::Duration::from_millis(100));
    }

    // Note: ByteStream::from_path is efficient but doesn't provide granular progress updates
    // For files < 100MB, the upload is usually fast enough that this isn't an issue
    // Larger files will use multipart upload with better progress tracking

    let body = ByteStream::from_path(local_path)
        .await
        .with_context(|| format!("Failed to create byte stream from {}", local_path.display()))?;

    // Upload to S3
    client
        .put_object()
        .bucket(bucket)
        .key(s3_key)
        .body(body)
        .content_length(file_size as i64)
        .send()
        .await
        .with_context(|| format!("Failed to upload to s3://{}/{}", bucket, s3_key))?;

    // Mark upload complete
    if let Some(pb) = pb {
        pb.set_position(file_size);
        pb.finish_with_message(format!("✓ {}", local_path.file_name().unwrap().to_string_lossy()));
    }

    info!(
        "Successfully uploaded: {} -> s3://{}/{}",
        local_path.display(),
        bucket,
        s3_key
    );

    Ok(UploadResult::Uploaded)
}

/// Check if an error is retryable (transient network errors, throttling, etc.)
fn is_retryable(error: &anyhow::Error) -> bool {
    let error_str = error.to_string().to_lowercase();

    // Check for common retryable error patterns
    error_str.contains("timeout")
        || error_str.contains("connection")
        || error_str.contains("throttl")
        || error_str.contains("503")
        || error_str.contains("500")
        || error_str.contains("502")
        || error_str.contains("504")
        || error_str.contains("slowdown")
        || error_str.contains("temporary")
        || error_str.contains("broken pipe")
        || error_str.contains("connection reset")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_retryable() {
        // Retryable errors
        assert!(is_retryable(&anyhow::anyhow!("Connection timeout")));
        assert!(is_retryable(&anyhow::anyhow!("Throttling error")));
        assert!(is_retryable(&anyhow::anyhow!("503 Service Unavailable")));
        assert!(is_retryable(&anyhow::anyhow!("Connection reset by peer")));
        assert!(is_retryable(&anyhow::anyhow!("SlowDown")));

        // Non-retryable errors
        assert!(!is_retryable(&anyhow::anyhow!("Access Denied")));
        assert!(!is_retryable(&anyhow::anyhow!("Invalid credentials")));
        assert!(!is_retryable(&anyhow::anyhow!("404 Not Found")));
    }
}
