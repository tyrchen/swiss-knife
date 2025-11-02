use anyhow::Result;
use aws_sdk_s3::Client;
use md5::{Digest, Md5};
use std::path::Path;
use tokio::io::AsyncReadExt;
use tracing::{debug, trace};

#[derive(Debug, PartialEq)]
pub enum FileComparison {
    /// File doesn't exist on S3
    NotFound,
    /// Files are identical (same size and ETag)
    Identical,
    /// Files are different
    Different,
}

/// Compare local file with remote S3 object using size and ETag
///
/// # Arguments
///
/// * `client` - AWS S3 client
/// * `bucket` - S3 bucket name
/// * `s3_key` - S3 object key
/// * `local_path` - Path to local file
///
/// # Returns
///
/// * `FileComparison::NotFound` - File doesn't exist on S3
/// * `FileComparison::Identical` - Files match (same size and content hash)
/// * `FileComparison::Different` - Files differ
///
/// # Performance
///
/// - First checks file size (fast)
/// - Then compares MD5/ETag if sizes match (slower but accurate)
/// - For multipart uploads, falls back to size-only comparison
pub async fn compare_file(
    client: &Client,
    bucket: &str,
    s3_key: &str,
    local_path: &Path,
) -> Result<FileComparison> {
    trace!("Comparing local file {} with s3://{}/{}", local_path.display(), bucket, s3_key);

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
                debug!(
                    "File size mismatch: local={} bytes, remote={} bytes",
                    local_size, remote_size
                );
                return Ok(FileComparison::Different);
            }

            debug!("File sizes match ({} bytes), comparing content hash", local_size);

            // Size matches - now compare content hash
            // For S3 simple uploads (non-multipart), ETag is MD5
            // For multipart, it's complex (MD5 of MD5s with part count suffix like "abc-2")
            if let Some(etag) = head.e_tag() {
                let etag_clean = etag.trim_matches('"');

                // Check if it's a multipart upload (contains '-')
                if etag_clean.contains('-') {
                    debug!(
                        "Remote file uses multipart upload (ETag: {}), using size-only comparison",
                        etag_clean
                    );
                    // For multipart uploads, we can't easily verify the hash
                    // Consider identical based on size + existence
                    return Ok(FileComparison::Identical);
                }

                // Compute local file MD5 for single-part comparison
                trace!("Computing MD5 hash for local file");
                let local_hash = compute_file_md5(local_path).await?;

                if local_hash.eq_ignore_ascii_case(etag_clean) {
                    debug!("File content matches (MD5: {})", local_hash);
                    Ok(FileComparison::Identical)
                } else {
                    debug!(
                        "File content differs: local MD5={}, remote ETag={}",
                        local_hash, etag_clean
                    );
                    Ok(FileComparison::Different)
                }
            } else {
                debug!("No ETag available from S3, considering identical based on size");
                // No ETag available, fall back to size-only comparison
                Ok(FileComparison::Identical)
            }
        }
        Err(e) => {
            debug!("File not found on S3: {}", e);
            // Object doesn't exist
            Ok(FileComparison::NotFound)
        }
    }
}

/// Compute MD5 hash of a local file
///
/// This is used to compare with S3 ETag for non-multipart uploads.
/// The hash is computed in chunks to handle large files efficiently.
///
/// # Arguments
///
/// * `path` - Path to the file
///
/// # Returns
///
/// Hex-encoded MD5 hash string (lowercase)
async fn compute_file_md5(path: &Path) -> Result<String> {
    let mut file = tokio::fs::File::open(path).await?;
    let mut hasher = Md5::new();
    let mut buffer = vec![0u8; 8192]; // 8KB chunks

    loop {
        let n = file.read(&mut buffer).await?;
        if n == 0 {
            break;
        }
        hasher.update(&buffer[..n]);
    }

    Ok(format!("{:x}", hasher.finalize()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[tokio::test]
    async fn test_compute_file_md5() {
        let mut temp_file = NamedTempFile::new().unwrap();
        write!(temp_file, "hello world").unwrap();
        temp_file.flush().unwrap();

        let hash = compute_file_md5(temp_file.path()).await.unwrap();

        // MD5 of "hello world" is 5eb63bbbe01eeed093cb22bb8f5acdc3
        assert_eq!(hash, "5eb63bbbe01eeed093cb22bb8f5acdc3");
    }

    #[tokio::test]
    async fn test_compute_file_md5_empty() {
        let temp_file = NamedTempFile::new().unwrap();

        let hash = compute_file_md5(temp_file.path()).await.unwrap();

        // MD5 of empty file is d41d8cd98f00b204e9800998ecf8427e
        assert_eq!(hash, "d41d8cd98f00b204e9800998ecf8427e");
    }

    #[tokio::test]
    async fn test_compute_file_md5_large() {
        let mut temp_file = NamedTempFile::new().unwrap();

        // Write 1MB of data
        let data = vec![0u8; 1024 * 1024];
        temp_file.write_all(&data).unwrap();
        temp_file.flush().unwrap();

        let hash = compute_file_md5(temp_file.path()).await.unwrap();

        // Verify hash is computed (exact value depends on content)
        assert_eq!(hash.len(), 32); // MD5 is always 32 hex characters
    }
}
