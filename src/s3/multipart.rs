use anyhow::{Context, Result};
use aws_sdk_s3::{primitives::ByteStream, types::CompletedPart, Client};
use indicatif::ProgressBar;
use std::path::Path;
use tokio::io::AsyncReadExt;
use tracing::{debug, info};

// Threshold for using multipart upload (100MB)
pub const MULTIPART_THRESHOLD: u64 = 100 * 1024 * 1024;

// Size of each part (10MB) - AWS minimum is 5MB
const PART_SIZE: usize = 10 * 1024 * 1024;

/// Upload a large file using S3 multipart upload
///
/// Multipart upload is used for files larger than MULTIPART_THRESHOLD.
/// Benefits:
/// - Can upload files > 5GB (AWS single PUT limit)
/// - Better resilience (can retry individual parts)
/// - Parallel uploads possible (not yet implemented)
///
/// # Arguments
///
/// * `client` - AWS S3 client
/// * `bucket` - S3 bucket name
/// * `s3_key` - S3 object key (path)
/// * `local_path` - Path to local file
/// * `pb` - Optional progress bar
///
/// # Returns
///
/// Ok(()) on successful upload
pub async fn upload_multipart(
    client: &Client,
    bucket: &str,
    s3_key: &str,
    local_path: &Path,
    pb: Option<&ProgressBar>,
) -> Result<()> {
    let metadata = tokio::fs::metadata(local_path).await?;
    let file_size = metadata.len();

    info!(
        "Starting multipart upload for {} ({} bytes, {} parts)",
        local_path.display(),
        file_size,
        (file_size as usize).div_ceil(PART_SIZE)
    );

    // Initiate multipart upload
    let multipart = client
        .create_multipart_upload()
        .bucket(bucket)
        .key(s3_key)
        .send()
        .await
        .context("Failed to initiate multipart upload")?;

    let upload_id = multipart
        .upload_id()
        .context("No upload ID returned from S3")?;

    debug!("Multipart upload initiated with ID: {}", upload_id);

    if let Some(pb) = pb {
        pb.set_length(file_size);
        pb.set_position(0);
        pb.set_message(format!(
            "Multipart upload {}",
            local_path.file_name().unwrap().to_string_lossy()
        ));
    }

    // Upload parts
    let mut file = tokio::fs::File::open(local_path).await?;
    let mut parts = Vec::new();
    let mut part_number = 1i32;
    let mut uploaded_bytes = 0u64;

    loop {
        let mut buffer = vec![0u8; PART_SIZE];
        let bytes_read = file.read(&mut buffer).await?;

        if bytes_read == 0 {
            break; // EOF
        }

        buffer.truncate(bytes_read);

        debug!("Uploading part {} ({} bytes)", part_number, buffer.len());

        // Upload this part
        let part_result = client
            .upload_part()
            .bucket(bucket)
            .key(s3_key)
            .upload_id(upload_id)
            .part_number(part_number)
            .body(ByteStream::from(buffer))
            .send()
            .await
            .with_context(|| format!("Failed to upload part {}", part_number))?;

        // Store completed part info
        let completed_part = CompletedPart::builder()
            .part_number(part_number)
            .e_tag(part_result.e_tag().unwrap_or(""))
            .build();

        parts.push(completed_part);

        uploaded_bytes += bytes_read as u64;
        if let Some(pb) = pb {
            pb.set_position(uploaded_bytes);
        }

        part_number += 1;
    }

    debug!(
        "All {} parts uploaded, completing multipart upload",
        parts.len()
    );

    // Complete multipart upload
    let completed_multipart = aws_sdk_s3::types::CompletedMultipartUpload::builder()
        .set_parts(Some(parts))
        .build();

    client
        .complete_multipart_upload()
        .bucket(bucket)
        .key(s3_key)
        .upload_id(upload_id)
        .multipart_upload(completed_multipart)
        .send()
        .await
        .context("Failed to complete multipart upload")?;

    if let Some(pb) = pb {
        pb.finish_with_message(format!(
            "✓ {}",
            local_path.file_name().unwrap().to_string_lossy()
        ));
    }

    info!(
        "Successfully completed multipart upload: {} -> s3://{}/{}",
        local_path.display(),
        bucket,
        s3_key
    );

    Ok(())
}

/// Abort a multipart upload (for cleanup on error)
///
/// This should be called if an error occurs during multipart upload
/// to clean up any partial uploads on S3.
#[allow(dead_code)]
pub async fn abort_multipart_upload(
    client: &Client,
    bucket: &str,
    s3_key: &str,
    upload_id: &str,
) -> Result<()> {
    client
        .abort_multipart_upload()
        .bucket(bucket)
        .key(s3_key)
        .upload_id(upload_id)
        .send()
        .await
        .context("Failed to abort multipart upload")?;

    debug!("Aborted multipart upload {}", upload_id);

    Ok(())
}
