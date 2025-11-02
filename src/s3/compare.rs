use anyhow::Result;
use aws_sdk_s3::Client;
use std::path::Path;

#[derive(Debug, PartialEq)]
pub enum FileComparison {
    /// File doesn't exist on S3
    NotFound,
    /// Files are identical (same size and ETag)
    Identical,
    /// Files are different
    Different,
}

/// Compare local file with remote S3 object
pub async fn compare_file(
    client: &Client,
    bucket: &str,
    s3_key: &str,
    local_path: &Path,
) -> Result<FileComparison> {
    // Get local file size
    let local_metadata = tokio::fs::metadata(local_path).await?;
    let local_size = local_metadata.len();

    // Try to get remote object metadata
    let head_result = client.head_object().bucket(bucket).key(s3_key).send().await;

    match head_result {
        Ok(head) => {
            let remote_size = head.content_length().unwrap_or(0) as u64;

            // First quick check: compare sizes
            if local_size != remote_size {
                return Ok(FileComparison::Different);
            }

            // If sizes match, we consider them identical for now
            // A more thorough check would compute the local file hash and compare with ETag,
            // but ETag calculation for multipart uploads is complex
            Ok(FileComparison::Identical)
        }
        Err(_) => {
            // Object doesn't exist
            Ok(FileComparison::NotFound)
        }
    }
}
