use anyhow::Result;
use aws_sdk_s3::{presigning::PresigningConfig, Client};
use std::time::Duration;

/// Generate a pre-signed URL valid for 7 days
pub async fn generate_presigned_url(client: &Client, bucket: &str, s3_key: &str) -> Result<String> {
    let expires_in = Duration::from_secs(7 * 24 * 60 * 60); // 7 days

    let presigning_config = PresigningConfig::expires_in(expires_in)?;

    let presigned_request = client
        .get_object()
        .bucket(bucket)
        .key(s3_key)
        .presigned(presigning_config)
        .await?;

    Ok(presigned_request.uri().to_string())
}
