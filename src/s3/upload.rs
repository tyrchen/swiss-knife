use anyhow::{Context, Result};
use aws_sdk_s3::{primitives::ByteStream, Client};
use indicatif::ProgressBar;
use std::path::Path;
use tokio::fs::File;

#[derive(Debug)]
pub enum UploadResult {
    Uploaded,
    #[allow(dead_code)]
    Skipped,
}

/// Upload a file to S3 with progress bar
pub async fn upload_file(
    client: &Client,
    bucket: &str,
    s3_key: &str,
    local_path: &Path,
    pb: Option<&ProgressBar>,
) -> Result<UploadResult> {
    let file = File::open(local_path)
        .await
        .context("Failed to open local file")?;

    let metadata = file
        .metadata()
        .await
        .context("Failed to get file metadata")?;
    let file_size = metadata.len();

    // Update progress bar
    if let Some(pb) = pb {
        pb.set_length(file_size);
        pb.set_message(format!("Uploading {}", local_path.display()));
    }

    // Create ByteStream from file
    let body = ByteStream::from_path(local_path)
        .await
        .context("Failed to create byte stream from file")?;

    // Upload to S3
    client
        .put_object()
        .bucket(bucket)
        .key(s3_key)
        .body(body)
        .content_length(file_size as i64)
        .send()
        .await
        .context("Failed to upload file to S3")?;

    if let Some(pb) = pb {
        pb.finish_with_message(format!("Uploaded {}", local_path.display()));
    }

    Ok(UploadResult::Uploaded)
}
