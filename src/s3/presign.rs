use anyhow::Result;
use aws_sdk_s3::{presigning::PresigningConfig, Client};
use std::time::Duration;

/// Generate a pre-signed URL with default 7-day expiration
///
/// # Arguments
///
/// * `client` - AWS S3 client
/// * `bucket` - S3 bucket name
/// * `s3_key` - S3 object key
///
/// # Returns
///
/// Pre-signed URL as a string
pub async fn generate_presigned_url(client: &Client, bucket: &str, s3_key: &str) -> Result<String> {
    generate_presigned_url_with_expiry(client, bucket, s3_key, 168).await
}

/// Generate a pre-signed URL with custom expiration
///
/// # Arguments
///
/// * `client` - AWS S3 client
/// * `bucket` - S3 bucket name
/// * `s3_key` - S3 object key
/// * `expiry_hours` - Expiration time in hours (max 168 = 7 days)
///
/// # Returns
///
/// Pre-signed URL as a string
///
/// # Notes
///
/// AWS limits pre-signed URLs to a maximum of 7 days (168 hours).
/// Values greater than 168 will be capped at 168.
pub async fn generate_presigned_url_with_expiry(
    client: &Client,
    bucket: &str,
    s3_key: &str,
    expiry_hours: u64,
) -> Result<String> {
    // AWS presigned URL max is 7 days (168 hours)
    let hours = expiry_hours.min(168);
    let expires_in = Duration::from_secs(hours * 60 * 60);

    let presigning_config = PresigningConfig::expires_in(expires_in)?;

    let presigned_request = client
        .get_object()
        .bucket(bucket)
        .key(s3_key)
        .presigned(presigning_config)
        .await?;

    Ok(presigned_request.uri().to_string())
}
