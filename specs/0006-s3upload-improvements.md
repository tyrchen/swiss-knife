# S3 Upload Tool - Comprehensive Improvement Plan

## Executive Summary

After thorough analysis of the s3upload tool implementation, this document outlines critical improvements across code quality, features, and user experience. The tool is already well-structured with concurrent uploads and good UX, but there are significant opportunities for enhancement in robustness, performance, security, and feature completeness.

**Priority Classification:**

- **P0 (Critical)**: Security issues, data integrity problems, breaking bugs
- **P1 (High)**: Performance improvements, user-facing bugs, quality issues
- **P2 (Medium)**: Feature enhancements, UX improvements
- **P3 (Low)**: Nice-to-haves, polish

---

## 1. Code Quality Improvements

### 1.1 Progress Bar Enhancement (P1)

**Current Issue:**
The progress bar in `src/s3/upload.rs:33-56` doesn't actually track upload progress in real-time. The `ByteStream::from_path()` consumes the file immediately, so the progress bar goes from 0% to 100% instantly.

**Location:** `src/s3/upload.rs:15-59`

**Problem:**

```rust
// Current code - progress bar is set to file size but never updates during upload
pb.set_length(file_size);
let body = ByteStream::from_path(local_path).await?;  // Progress not tracked
client.put_object().body(body).send().await?;
```

**Solution:**
Implement a custom stream wrapper that updates the progress bar as bytes are read:

```rust
use tokio_util::io::ReaderStream;
use futures::TryStreamExt;

pub async fn upload_file_with_progress(
    client: &Client,
    bucket: &str,
    s3_key: &str,
    local_path: &Path,
    pb: Option<&ProgressBar>,
) -> Result<UploadResult> {
    let file = File::open(local_path).await?;
    let metadata = file.metadata().await?;
    let file_size = metadata.len();

    if let Some(pb) = pb {
        pb.set_length(file_size);
    }

    // Wrap file with progress tracking
    let pb_clone = pb.cloned();
    let stream = ReaderStream::new(file)
        .inspect_ok(move |chunk| {
            if let Some(pb) = &pb_clone {
                pb.inc(chunk.len() as u64);
            }
        });

    let body = ByteStream::from(stream);

    client
        .put_object()
        .bucket(bucket)
        .key(s3_key)
        .body(body)
        .content_length(file_size as i64)
        .send()
        .await?;

    Ok(UploadResult::Uploaded)
}
```

**Impact:** Users will see actual upload progress instead of instant jumps, especially important for large files.

---

### 1.2 Improved Error Handling and Context (P1)

**Current Issue:**
Error messages lack sufficient context. When an upload fails, users don't know which file failed or why.

**Location:** `src/s3upload.rs:572-578`

**Current:**

```rust
Err(e) => {
    stats.failed.fetch_add(1, Ordering::Relaxed);
    Ok(ProcessResult::Failed {
        filename: relative_path,
        error: format!("{:#}", e),
    })
}
```

**Problem:** Generic error messages without structured error types.

**Solution:**

1. **Define structured error types:**

```rust
// src/s3/error.rs (new file)
use thiserror::Error;

#[derive(Error, Debug)]
pub enum S3UploadError {
    #[error("File not found: {path}")]
    FileNotFound { path: String },

    #[error("Permission denied: {path}")]
    PermissionDenied { path: String },

    #[error("Network error: {message}")]
    NetworkError { message: String },

    #[error("S3 access denied for bucket '{bucket}': {message}")]
    S3AccessDenied { bucket: String, message: String },

    #[error("File too large: {size} bytes (max: {max} bytes)")]
    FileTooLarge { size: u64, max: u64 },

    #[error("Invalid S3 key: {key}")]
    InvalidS3Key { key: String },
}
```

2. **Add error context throughout:**

```rust
// src/s3/upload.rs
pub async fn upload_file(...) -> Result<UploadResult, S3UploadError> {
    let file = File::open(local_path)
        .await
        .map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                S3UploadError::FileNotFound {
                    path: local_path.display().to_string()
                }
            } else if e.kind() == std::io::ErrorKind::PermissionDenied {
                S3UploadError::PermissionDenied {
                    path: local_path.display().to_string()
                }
            } else {
                S3UploadError::NetworkError {
                    message: e.to_string()
                }
            }
        })?;

    // ... rest of upload
}
```

**Impact:** Better debugging experience, clearer error messages for users.

---

### 1.3 File Comparison Enhancement with ETag (P1)

**Current Issue:**
File comparison only checks size (`src/s3/compare.rs:34-41`), which can lead to false positives. Two files with the same size but different content are considered identical.

**Location:** `src/s3/compare.rs:16-48`

**Current:**

```rust
if local_size != remote_size {
    return Ok(FileComparison::Different);
}
// If sizes match, we consider them identical - INSUFFICIENT!
Ok(FileComparison::Identical)
```

**Solution:**
Implement proper ETag comparison with MD5 hashing:

```rust
use blake3::Hasher;
use tokio::io::AsyncReadExt;

pub async fn compare_file(
    client: &Client,
    bucket: &str,
    s3_key: &str,
    local_path: &Path,
) -> Result<FileComparison> {
    let local_metadata = tokio::fs::metadata(local_path).await?;
    let local_size = local_metadata.len();

    let head_result = client.head_object().bucket(bucket).key(s3_key).send().await;

    match head_result {
        Ok(head) => {
            let remote_size = head.content_length().unwrap_or(0) as u64;

            // Quick size check first
            if local_size != remote_size {
                return Ok(FileComparison::Different);
            }

            // Size matches - now compare content hash
            // For S3 simple uploads (non-multipart), ETag is MD5
            // For multipart, it's complex (MD5 of MD5s with part count suffix)

            if let Some(etag) = head.e_tag() {
                let etag_clean = etag.trim_matches('"');

                // Check if it's a multipart upload (contains '-')
                if etag_clean.contains('-') {
                    // For multipart uploads, we can't easily verify
                    // Consider identical based on size + existence
                    return Ok(FileComparison::Identical);
                }

                // Compute local file MD5 for single-part comparison
                let local_hash = compute_file_md5(local_path).await?;

                if local_hash == etag_clean {
                    Ok(FileComparison::Identical)
                } else {
                    Ok(FileComparison::Different)
                }
            } else {
                // No ETag available, fall back to size-only comparison
                Ok(FileComparison::Identical)
            }
        }
        Err(_) => Ok(FileComparison::NotFound),
    }
}

async fn compute_file_md5(path: &Path) -> Result<String> {
    use md5::{Md5, Digest};

    let mut file = tokio::fs::File::open(path).await?;
    let mut hasher = Md5::new();
    let mut buffer = vec![0u8; 8192];

    loop {
        let n = file.read(&mut buffer).await?;
        if n == 0 {
            break;
        }
        hasher.update(&buffer[..n]);
    }

    Ok(format!("{:x}", hasher.finalize()))
}
```

**Dependencies to add:**

```toml
md-5 = "0.10"
```

**Impact:** Prevents unnecessary re-uploads AND catches actual file changes that don't affect size.

---

### 1.4 Concurrent Operation Safety (P1)

**Current Issue:**
The mpsc channel pattern has some inefficiencies and potential issues:

**Location:** `src/s3upload.rs:162-198`

**Problems:**

1. Cloning receivers in line 89: `let mut rx = rx.clone()` - This is incorrect usage. Only the sender should be cloned.
2. Shared mutex for receiver: `Arc<Mutex<work_rx>>` adds unnecessary contention
3. No timeout handling for worker tasks
4. Potential deadlock if result_tx blocks

**Solution:**
Use a proper work-stealing pattern with individual worker receivers:

```rust
// Better pattern for concurrent uploads
async fn process_files_concurrent(
    files: Vec<PathBuf>,
    s3_client: S3Client,
    config: Config,
    max_concurrent: usize,
) -> Result<Vec<ProcessResult>> {
    use tokio::sync::Semaphore;

    let semaphore = Arc::new(Semaphore::new(max_concurrent));
    let stats = Arc::new(Stats::default());
    let multi = Arc::new(MultiProgress::new());

    let tasks: Vec<_> = files
        .into_iter()
        .map(|file_path| {
            let permit = Arc::clone(&semaphore);
            let s3_client = s3_client.clone();
            let config = config.clone();
            let stats = Arc::clone(&stats);
            let multi = Arc::clone(&multi);

            tokio::spawn(async move {
                let _permit = permit.acquire().await.unwrap();

                let pb = multi.add(ProgressBar::new(0));
                // ... process file

                pb.finish_and_clear();
                result
            })
        })
        .collect();

    // Collect results as they complete
    let results = futures::future::join_all(tasks).await;

    // Handle task panics and extract results
    results
        .into_iter()
        .filter_map(|r| r.ok())
        .collect()
}
```

**Impact:** More efficient concurrency, no mutex contention, simpler code.

---

### 1.5 Memory Management for Large Files (P0)

**Current Issue:**
`ByteStream::from_path()` loads entire file into memory, which can cause OOM for large files.

**Location:** `src/s3/upload.rs:39-41`

**Problem:**

```rust
let body = ByteStream::from_path(local_path).await?;  // Entire file in memory!
```

**Solution:**
Use streaming with chunked reading (already partially addressed with progress tracking suggestion in 1.1):

```rust
use tokio_util::io::ReaderStream;

const CHUNK_SIZE: usize = 5 * 1024 * 1024; // 5MB chunks

let file = File::open(local_path).await?;
let reader_stream = ReaderStream::with_capacity(file, CHUNK_SIZE);
let body = ByteStream::from(reader_stream);
```

**Impact:** Can upload files of any size without memory issues.

---

### 1.6 Add Retry Logic (P1)

**Current Issue:**
No retry mechanism for transient failures (network hiccups, throttling).

**Location:** `src/s3/upload.rs` - entire upload function

**Solution:**
Add exponential backoff retry:

```rust
use tokio::time::{sleep, Duration};

const MAX_RETRIES: u32 = 3;
const INITIAL_RETRY_DELAY: Duration = Duration::from_secs(1);

pub async fn upload_file_with_retry(
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
            Ok(result) => return Ok(result),
            Err(e) if attempts < MAX_RETRIES && is_retryable(&e) => {
                attempts += 1;
                if let Some(pb) = pb {
                    pb.set_message(format!("Retry {}/{}", attempts, MAX_RETRIES));
                }
                sleep(delay).await;
                delay *= 2; // Exponential backoff
            }
            Err(e) => return Err(e),
        }
    }
}

fn is_retryable(error: &anyhow::Error) -> bool {
    // Check if error is network-related or throttling
    let error_str = error.to_string().to_lowercase();
    error_str.contains("timeout")
        || error_str.contains("connection")
        || error_str.contains("throttl")
        || error_str.contains("503")
}
```

**Impact:** Much more robust against transient failures.

---

### 1.7 Configuration Validation (P1)

**Current Issue:**
No validation of configuration values. Invalid bucket names or regions fail at runtime.

**Location:** `src/config.rs:13-43`

**Solution:**
Add validation:

```rust
impl Config {
    pub fn from_env() -> Result<Self> {
        dotenv::dotenv().ok();

        let region = env::var("AWS_REGION")
            .context("AWS_REGION not found")?;
        Self::validate_region(&region)?;

        let profile = env::var("AWS_PROFILE").ok();

        let bucket = env::var("S3_BUCKET")
            .context("S3_BUCKET not found")?;
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

    fn validate_bucket_name(bucket: &str) -> Result<()> {
        if bucket.is_empty() {
            anyhow::bail!("S3_BUCKET cannot be empty");
        }
        if bucket.len() < 3 || bucket.len() > 63 {
            anyhow::bail!("S3_BUCKET must be between 3 and 63 characters");
        }
        if !bucket.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '.') {
            anyhow::bail!("S3_BUCKET contains invalid characters");
        }
        Ok(())
    }

    fn validate_region(region: &str) -> Result<()> {
        // Basic validation - could be expanded
        if region.is_empty() {
            anyhow::bail!("AWS_REGION cannot be empty");
        }
        Ok(())
    }

    fn validate_target_path(path: &str) -> Result<()> {
        // Check for invalid characters
        if path.contains("//") || path.contains("..") {
            anyhow::bail!("S3_TARGET_PATH contains invalid path segments");
        }
        Ok(())
    }
}
```

**Impact:** Fail fast with clear error messages instead of cryptic AWS errors.

---

### 1.8 Add Logging Infrastructure (P2)

**Current Issue:**
No structured logging. Debugging issues requires adding print statements.

**Solution:**
Add `tracing` for structured logging:

```toml
# Cargo.toml
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter"] }
```

```rust
// src/s3upload.rs
use tracing::{info, warn, error, debug};

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize tracing
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_env("RUST_LOG")
                .add_directive(tracing::Level::INFO.into())
        )
        .init();

    info!("Starting s3upload");

    // ... rest of main
}
```

**Usage:**

```bash
RUST_LOG=debug s3upload ./file.mp4
```

**Impact:** Better debugging and monitoring capabilities.

---

## 2. Feature Enhancements

### 2.1 Multipart Upload Support (P1)

**Current Issue:**
All uploads use single PUT requests, which:

- Fail for files > 5GB (AWS limit)
- Slower for large files (no parallelization)
- Less resilient (must restart entire upload on failure)

**Solution:**
Implement multipart upload for files > 100MB:

```rust
// src/s3/multipart.rs (new file)
const MULTIPART_THRESHOLD: u64 = 100 * 1024 * 1024; // 100MB
const PART_SIZE: u64 = 10 * 1024 * 1024; // 10MB per part

pub async fn upload_large_file(
    client: &Client,
    bucket: &str,
    s3_key: &str,
    local_path: &Path,
    pb: Option<&ProgressBar>,
) -> Result<UploadResult> {
    let metadata = tokio::fs::metadata(local_path).await?;
    let file_size = metadata.len();

    if file_size < MULTIPART_THRESHOLD {
        return upload_file(client, bucket, s3_key, local_path, pb).await;
    }

    // Initiate multipart upload
    let multipart = client
        .create_multipart_upload()
        .bucket(bucket)
        .key(s3_key)
        .send()
        .await?;

    let upload_id = multipart.upload_id().unwrap();

    if let Some(pb) = pb {
        pb.set_length(file_size);
    }

    // Upload parts
    let mut parts = Vec::new();
    let mut file = tokio::fs::File::open(local_path).await?;
    let mut part_number = 1;
    let mut uploaded_bytes = 0u64;

    loop {
        let mut buffer = vec![0u8; PART_SIZE as usize];
        let bytes_read = file.read(&mut buffer).await?;

        if bytes_read == 0 {
            break;
        }

        buffer.truncate(bytes_read);

        let part_result = client
            .upload_part()
            .bucket(bucket)
            .key(s3_key)
            .upload_id(upload_id)
            .part_number(part_number)
            .body(ByteStream::from(buffer))
            .send()
            .await?;

        parts.push(
            aws_sdk_s3::types::CompletedPart::builder()
                .part_number(part_number)
                .e_tag(part_result.e_tag().unwrap_or(""))
                .build()
        );

        uploaded_bytes += bytes_read as u64;
        if let Some(pb) = pb {
            pb.set_position(uploaded_bytes);
        }

        part_number += 1;
    }

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
        .await?;

    Ok(UploadResult::Uploaded)
}
```

**Impact:** Can upload files of any size reliably with better performance.

---

### 2.2 Dry-Run Mode (P2)

**Feature:**
Add `--dry-run` flag to show what would be uploaded without actually uploading.

```rust
#[derive(Parser, Debug)]
struct Cli {
    // ... existing fields

    /// Perform a dry run (show what would be uploaded without uploading)
    #[arg(long)]
    dry_run: bool,
}
```

**Implementation:**

```rust
if cli.dry_run {
    println!("{}", style("DRY RUN MODE - No files will be uploaded").yellow().bold());

    for file in &files {
        let relative_path = get_relative_path(&cli.path, file)?;
        let s3_key = config.build_s3_key(&relative_path);
        let metadata = tokio::fs::metadata(file).await?;

        // Check if file exists on S3
        let comparison = compare_file(
            s3_client.client(),
            s3_client.bucket(),
            &s3_key,
            file
        ).await?;

        match comparison {
            FileComparison::NotFound => {
                println!("  {} {} → s3://{}/{} ({})",
                    style("WOULD UPLOAD").green(),
                    relative_path,
                    s3_client.bucket(),
                    s3_key,
                    format_size(metadata.len())
                );
            }
            FileComparison::Different => {
                println!("  {} {} → s3://{}/{} ({})",
                    style("WOULD UPDATE").yellow(),
                    relative_path,
                    s3_client.bucket(),
                    s3_key,
                    format_size(metadata.len())
                );
            }
            FileComparison::Identical => {
                println!("  {} {}",
                    style("WOULD SKIP").dim(),
                    relative_path
                );
            }
        }
    }

    return Ok(());
}
```

**Impact:** Safe way to preview uploads before executing.

---

### 2.3 Custom Pre-signed URL Expiration (P2)

**Feature:**
Allow users to specify URL expiration time.

```rust
#[derive(Parser, Debug)]
struct Cli {
    // ... existing fields

    /// Pre-signed URL expiration in hours (default: 168 = 7 days, max: 168)
    #[arg(long, default_value = "168")]
    url_expiry_hours: u64,
}
```

```rust
// src/s3/presign.rs
pub async fn generate_presigned_url_with_expiry(
    client: &Client,
    bucket: &str,
    s3_key: &str,
    expiry_hours: u64,
) -> Result<String> {
    // AWS presigned URL max is 7 days
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
```

**Impact:** Flexibility for different use cases (short-lived sharing links vs. longer access).

---

### 2.4 Metadata and Tags Support (P2)

**Feature:**
Support custom S3 object metadata and tags.

```rust
#[derive(Parser, Debug)]
struct Cli {
    // ... existing fields

    /// Custom metadata (key=value pairs, comma-separated)
    #[arg(long)]
    metadata: Option<String>,

    /// Tags (key=value pairs, comma-separated)
    #[arg(long)]
    tags: Option<String>,
}
```

```rust
// Parse metadata
let metadata: HashMap<String, String> = if let Some(meta) = &cli.metadata {
    meta.split(',')
        .filter_map(|pair| {
            let mut parts = pair.split('=');
            Some((parts.next()?.to_string(), parts.next()?.to_string()))
        })
        .collect()
} else {
    HashMap::new()
};

// Add to upload
client
    .put_object()
    .bucket(bucket)
    .key(s3_key)
    .body(body)
    .set_metadata(Some(metadata))
    .send()
    .await?;
```

**Example usage:**

```bash
s3upload file.mp4 --metadata "author=John,project=Demo" --tags "env=prod,type=video"
```

**Impact:** Better organization and searchability in S3.

---

### 2.5 Resume Interrupted Uploads (P1)

**Feature:**
Save upload state and resume interrupted multipart uploads.

```rust
// src/s3/resume.rs (new file)
use serde::{Serialize, Deserialize};

#[derive(Serialize, Deserialize)]
struct UploadState {
    upload_id: String,
    bucket: String,
    key: String,
    completed_parts: Vec<i32>,
    total_parts: i32,
}

pub async fn save_upload_state(state: &UploadState, local_path: &Path) -> Result<()> {
    let state_file = format!("{}..upload_state", local_path.display());
    let json = serde_json::to_string(state)?;
    tokio::fs::write(&state_file, json).await?;
    Ok(())
}

pub async fn load_upload_state(local_path: &Path) -> Result<Option<UploadState>> {
    let state_file = format!("{}.upload_state", local_path.display());

    match tokio::fs::read_to_string(&state_file).await {
        Ok(json) => {
            let state: UploadState = serde_json::from_str(&json)?;
            Ok(Some(state))
        }
        Err(_) => Ok(None),
    }
}

pub async fn resume_multipart_upload(
    client: &Client,
    state: UploadState,
    local_path: &Path,
) -> Result<()> {
    // List existing parts
    let parts = client
        .list_parts()
        .bucket(&state.bucket)
        .key(&state.key)
        .upload_id(&state.upload_id)
        .send()
        .await?;

    // Continue from last completed part
    // ... implementation

    Ok(())
}
```

**Impact:** Huge time saver for large files on unreliable networks.

---

### 2.6 Bandwidth Throttling (P2)

**Feature:**
Limit upload speed to avoid saturating connection.

```rust
#[derive(Parser, Debug)]
struct Cli {
    // ... existing fields

    /// Maximum upload speed in MB/s (0 = unlimited)
    #[arg(long, default_value = "0")]
    max_speed_mbps: f64,
}
```

```rust
// Implement rate limiter
use std::time::Instant;

struct RateLimiter {
    max_bytes_per_sec: f64,
    last_update: Instant,
    tokens: f64,
}

impl RateLimiter {
    fn new(mbps: f64) -> Self {
        Self {
            max_bytes_per_sec: mbps * 1024.0 * 1024.0,
            last_update: Instant::now(),
            tokens: 0.0,
        }
    }

    async fn acquire(&mut self, bytes: usize) {
        if self.max_bytes_per_sec == 0.0 {
            return; // Unlimited
        }

        let now = Instant::now();
        let elapsed = now.duration_since(self.last_update).as_secs_f64();
        self.tokens += elapsed * self.max_bytes_per_sec;
        self.tokens = self.tokens.min(self.max_bytes_per_sec * 2.0);
        self.last_update = now;

        if self.tokens < bytes as f64 {
            let wait_time = ((bytes as f64 - self.tokens) / self.max_bytes_per_sec);
            tokio::time::sleep(Duration::from_secs_f64(wait_time)).await;
            self.tokens = 0.0;
        } else {
            self.tokens -= bytes as f64;
        }
    }
}
```

**Impact:** Prevents network saturation, allows other applications to use bandwidth.

---

### 2.7 Directory Structure Preservation Options (P2)

**Feature:**
Options for how to handle directory structure when uploading.

```rust
#[derive(Parser, Debug)]
struct Cli {
    // ... existing fields

    /// Flatten directory structure (remove subdirectories)
    #[arg(long)]
    flatten: bool,

    /// Custom path prefix (overrides S3_TARGET_PATH for this upload)
    #[arg(long)]
    prefix: Option<String>,
}
```

**Implementation:**

```rust
fn get_relative_path(base: &Path, file: &Path, flatten: bool) -> Result<String> {
    if flatten {
        // Just use filename, ignore directory structure
        Ok(file
            .file_name()
            .context("Failed to get filename")?
            .to_string_lossy()
            .to_string())
    } else if base.is_file() {
        // For single file, just use the filename
        Ok(file
            .file_name()
            .context("Failed to get filename")?
            .to_string_lossy()
            .to_string())
    } else {
        // For directories, use relative path from base
        let rel_path = file
            .strip_prefix(base)
            .context("Failed to strip prefix")?
            .to_string_lossy()
            .to_string();
        Ok(rel_path)
    }
}
```

**Example:**

```bash
# Upload videos/2024/jan/file.mp4 as uploads/file.mp4
s3upload videos/ --flatten

# Upload to custom prefix
s3upload videos/ --prefix "archive/2024"
```

**Impact:** More flexible upload organization.

---

### 2.8 Content-Type Detection and Override (P2)

**Feature:**
Automatically detect and set Content-Type, with override option.

```rust
#[derive(Parser, Debug)]
struct Cli {
    // ... existing fields

    /// Override Content-Type for uploaded files
    #[arg(long)]
    content_type: Option<String>,
}
```

```rust
fn detect_content_type(path: &Path) -> String {
    match path.extension().and_then(|e| e.to_str()) {
        Some("mp4") => "video/mp4",
        Some("mov") => "video/quicktime",
        Some("avi") => "video/x-msvideo",
        Some("pdf") => "application/pdf",
        Some("jpg") | Some("jpeg") => "image/jpeg",
        Some("png") => "image/png",
        Some("gif") => "image/gif",
        Some("txt") => "text/plain",
        Some("json") => "application/json",
        Some("zip") => "application/zip",
        _ => "application/octet-stream",
    }
    .to_string()
}

// In upload function
let content_type = cli.content_type
    .clone()
    .unwrap_or_else(|| detect_content_type(local_path));

client
    .put_object()
    .bucket(bucket)
    .key(s3_key)
    .body(body)
    .content_type(content_type)
    .send()
    .await?;
```

**Impact:** Files served from S3 have correct MIME types for browsers.

---

### 2.9 Sync Mode (P2)

**Feature:**
Delete remote files that don't exist locally (like `rsync --delete`).

```rust
#[derive(Parser, Debug)]
struct Cli {
    // ... existing fields

    /// Sync mode: delete remote files not present locally
    #[arg(long)]
    sync: bool,
}
```

```rust
async fn sync_mode(
    client: &S3Client,
    config: &Config,
    local_files: &[PathBuf],
    base_path: &Path,
) -> Result<()> {
    // List all objects in S3 with the target prefix
    let mut continuation_token: Option<String> = None;
    let mut remote_keys = HashSet::new();

    loop {
        let mut list_req = client.client()
            .list_objects_v2()
            .bucket(client.bucket())
            .prefix(&config.target_path);

        if let Some(token) = continuation_token {
            list_req = list_req.continuation_token(token);
        }

        let response = list_req.send().await?;

        if let Some(contents) = response.contents() {
            for object in contents {
                if let Some(key) = object.key() {
                    remote_keys.insert(key.to_string());
                }
            }
        }

        if response.is_truncated() == Some(true) {
            continuation_token = response.next_continuation_token().map(|s| s.to_string());
        } else {
            break;
        }
    }

    // Build set of expected remote keys from local files
    let mut expected_keys = HashSet::new();
    for file in local_files {
        let relative_path = get_relative_path(base_path, file)?;
        let s3_key = config.build_s3_key(&relative_path);
        expected_keys.insert(s3_key);
    }

    // Find keys to delete
    let keys_to_delete: Vec<_> = remote_keys
        .difference(&expected_keys)
        .collect();

    if keys_to_delete.is_empty() {
        println!("{}", style("No remote files to delete").dim());
        return Ok(());
    }

    println!("{}", style(format!(
        "Will delete {} remote file(s)",
        keys_to_delete.len()
    )).yellow());

    for key in &keys_to_delete {
        println!("  {} {}", style("DELETE").red(), key);
    }

    // Delete in batches of 1000 (AWS limit)
    for chunk in keys_to_delete.chunks(1000) {
        let delete_objects: Vec<_> = chunk
            .iter()
            .map(|key| {
                aws_sdk_s3::types::ObjectIdentifier::builder()
                    .key(*key)
                    .build()
                    .unwrap()
            })
            .collect();

        client.client()
            .delete_objects()
            .bucket(client.bucket())
            .delete(
                aws_sdk_s3::types::Delete::builder()
                    .set_objects(Some(delete_objects))
                    .build()
                    .unwrap()
            )
            .send()
            .await?;
    }

    println!("{}", style(format!(
        "Deleted {} file(s)",
        keys_to_delete.len()
    )).green());

    Ok(())
}
```

**Impact:** True sync functionality for maintaining mirrors.

---

### 2.10 Checksums in Metadata (P2)

**Feature:**
Store file checksums in S3 metadata for verification.

```rust
use blake3::Hasher;

async fn compute_file_blake3(path: &Path) -> Result<String> {
    let mut file = tokio::fs::File::open(path).await?;
    let mut hasher = Hasher::new();
    let mut buffer = vec![0u8; 8192];

    loop {
        let n = file.read(&mut buffer).await?;
        if n == 0 {
            break;
        }
        hasher.update(&buffer[..n]);
    }

    Ok(hasher.finalize().to_hex().to_string())
}

// In upload function
let checksum = compute_file_blake3(local_path).await?;

let mut metadata = HashMap::new();
metadata.insert("blake3-checksum".to_string(), checksum);
metadata.insert("upload-tool".to_string(), "s3upload".to_string());
metadata.insert("upload-timestamp".to_string(),
    chrono::Utc::now().to_rfc3339());

client
    .put_object()
    .bucket(bucket)
    .key(s3_key)
    .body(body)
    .set_metadata(Some(metadata))
    .send()
    .await?;
```

**Impact:** File integrity verification, audit trail.

---

## 3. User Experience Improvements

### 3.1 Better Output Formatting (P2)

**Current Issue:**
Output is good but could be more informative.

**Improvements:**

1. **Show total size summary:**

```rust
println!("\n{}", style("═".repeat(60)).dim());
println!("{}", style(format!(
    "Total: {} uploaded ({} bytes), {} skipped, {} failed",
    stats.uploaded.load(Ordering::Relaxed),
    total_uploaded_bytes,
    stats.skipped.load(Ordering::Relaxed),
    stats.failed.load(Ordering::Relaxed)
)).bold());
println!("{}", style(format!(
    "Time: {:.2}s, Average speed: {:.2} MB/s",
    duration.as_secs_f64(),
    (total_uploaded_bytes as f64 / 1024.0 / 1024.0) / duration.as_secs_f64()
)).dim());
```

2. **Add upload statistics:**

```rust
#[derive(Debug, Default)]
struct Stats {
    uploaded: AtomicUsize,
    skipped: AtomicUsize,
    failed: AtomicUsize,
    urls_generated: AtomicUsize,
    not_found: AtomicUsize,
    total_bytes_uploaded: AtomicU64,  // NEW
    start_time: Option<Instant>,  // NEW
}
```

3. **Real-time status updates:**

```rust
// Update progress bar with current stats
let status_msg = format!(
    "✓{} ↻{} ✗{} | {:.1}MB/s",
    stats.uploaded.load(Ordering::Relaxed),
    stats.skipped.load(Ordering::Relaxed),
    stats.failed.load(Ordering::Relaxed),
    calculate_current_speed(&stats)
);
pb.set_message(status_msg);
```

**Impact:** Users have better visibility into upload progress and performance.

---

### 3.2 Interactive Mode for Conflicts (P2)

**Feature:**
Prompt user when files exist on S3.

```rust
#[derive(Parser, Debug)]
struct Cli {
    // ... existing fields

    /// Interactive mode: prompt for conflicts
    #[arg(long, short = 'i')]
    interactive: bool,
}
```

```rust
use std::io::{self, Write};

async fn handle_existing_file(
    filename: &str,
    comparison: FileComparison,
    interactive: bool,
) -> Result<UploadAction> {
    if !interactive {
        return Ok(UploadAction::Upload);
    }

    match comparison {
        FileComparison::Identical => Ok(UploadAction::Skip),
        FileComparison::Different => {
            println!("\n{} exists on S3 with different content",
                style(filename).yellow());
            print!("Overwrite? [y/N/a(ll)/q(uit)]: ");
            io::stdout().flush()?;

            let mut input = String::new();
            io::stdin().read_line(&mut input)?;

            match input.trim().to_lowercase().as_str() {
                "y" | "yes" => Ok(UploadAction::Upload),
                "a" | "all" => Ok(UploadAction::UploadAll),
                "q" | "quit" => Ok(UploadAction::Quit),
                _ => Ok(UploadAction::Skip),
            }
        }
        FileComparison::NotFound => Ok(UploadAction::Upload),
    }
}
```

**Impact:** More control for users, prevents accidental overwrites.

---

### 3.3 URL Clipboard Integration (P3)

**Feature:**
Automatically copy URLs to clipboard.

```toml
# Cargo.toml
copypasta = "0.10"
```

```rust
#[derive(Parser, Debug)]
struct Cli {
    // ... existing fields

    /// Copy pre-signed URLs to clipboard
    #[arg(long)]
    copy_url: bool,
}
```

```rust
if cli.copy_url && results.len() == 1 {
    if let ProcessResult::Uploaded { url, .. } = &results[0] {
        use copypasta::{ClipboardContext, ClipboardProvider};

        if let Ok(mut ctx) = ClipboardContext::new() {
            if ctx.set_contents(url.clone()).is_ok() {
                println!("  {} URL copied to clipboard", style("📋").dim());
            }
        }
    }
}
```

**Impact:** Convenient for single-file uploads.

---

### 3.4 Better Error Messages with Suggestions (P1)

**Current Issue:**
Error messages don't provide actionable solutions.

**Solution:**
Add contextual help:

```rust
impl fmt::Display for S3UploadError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Self::S3AccessDenied { bucket, message } => {
                writeln!(f, "Access denied for bucket '{}'", bucket)?;
                writeln!(f, "  Error: {}", message)?;
                writeln!(f, "\nPossible solutions:")?;
                writeln!(f, "  1. Check your AWS credentials: aws sts get-caller-identity")?;
                writeln!(f, "  2. Verify bucket permissions for your IAM user/role")?;
                writeln!(f, "  3. Ensure bucket exists: aws s3 ls s3://{}", bucket)?;
                Ok(())
            }
            Self::NetworkError { message } => {
                writeln!(f, "Network error: {}", message)?;
                writeln!(f, "\nPossible solutions:")?;
                writeln!(f, "  1. Check your internet connection")?;
                writeln!(f, "  2. Verify AWS region is correct in .env")?;
                writeln!(f, "  3. Try again with --max-concurrent 1")?;
                Ok(())
            }
            // ... other errors
            _ => write!(f, "{:?}", self),
        }
    }
}
```

**Impact:** Users can self-diagnose and fix issues faster.

---

### 3.5 Configuration File Support (P2)

**Feature:**
Support configuration file in addition to .env.

```toml
# s3upload.toml
[aws]
region = "us-west-2"
profile = "default"

[s3]
bucket = "my-bucket"
target_path = "uploads"

[upload]
max_concurrent = 4
extensions = ["mp4", "mov", "avi"]
url_expiry_hours = 168

[behavior]
auto_confirm = false
dry_run = false
```

```rust
// src/config.rs
use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct ConfigFile {
    aws: AwsConfig,
    s3: S3Config,
    upload: UploadConfig,
    behavior: BehaviorConfig,
}

impl Config {
    pub fn from_file_and_env() -> Result<Self> {
        // Try to load from config file first
        let config_file = if Path::new("s3upload.toml").exists() {
            let content = std::fs::read_to_string("s3upload.toml")?;
            Some(toml::from_str::<ConfigFile>(&content)?)
        } else {
            None
        };

        // Load from env, falling back to config file
        dotenv::dotenv().ok();

        // ... merge config file and env vars

        Ok(config)
    }
}
```

**Impact:** Easier configuration management, especially for teams.

---

### 3.6 Shell Completion Scripts (P3)

**Feature:**
Generate shell completion for bash/zsh/fish.

```rust
use clap::CommandFactory;
use clap_complete::{generate, Shell};

#[derive(Parser, Debug)]
struct Cli {
    // ... existing fields

    /// Generate shell completion script
    #[arg(long, value_enum)]
    completions: Option<Shell>,
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    if let Some(shell) = cli.completions {
        let mut app = Cli::command();
        generate(shell, &mut app, "s3upload", &mut io::stdout());
        return Ok(());
    }

    // ... rest of main
}
```

**Usage:**

```bash
s3upload --completions bash > /etc/bash_completion.d/s3upload
s3upload --completions zsh > ~/.zsh/completions/_s3upload
```

**Impact:** Better CLI experience with auto-completion.

---

### 3.7 Progress Persistence (P3)

**Feature:**
Save progress to file for later review.

```rust
#[derive(Parser, Debug)]
struct Cli {
    // ... existing fields

    /// Save upload report to file
    #[arg(long)]
    report: Option<PathBuf>,
}
```

```rust
// After upload completes
if let Some(report_path) = &cli.report {
    let report = UploadReport {
        timestamp: chrono::Utc::now(),
        files_uploaded: uploaded_files,
        files_skipped: skipped_files,
        files_failed: failed_files,
        total_bytes: total_bytes,
        duration: duration,
    };

    let json = serde_json::to_string_pretty(&report)?;
    tokio::fs::write(report_path, json).await?;

    println!("{} Report saved to {}",
        style("✓").green(),
        report_path.display()
    );
}
```

**Impact:** Audit trail, easier debugging of upload sessions.

---

## 4. Testing and Quality Assurance

### 4.1 Add Unit Tests (P1)

**Current Issue:**
No tests exist.

**Solution:**
Add comprehensive test suite:

```rust
// tests/config_test.rs
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bucket_name_validation() {
        assert!(Config::validate_bucket_name("my-bucket").is_ok());
        assert!(Config::validate_bucket_name("ab").is_err()); // Too short
        assert!(Config::validate_bucket_name("MY-BUCKET").is_err()); // Uppercase
        assert!(Config::validate_bucket_name("my_bucket").is_err()); // Underscore
    }

    #[test]
    fn test_s3_key_construction() {
        let config = Config {
            region: "us-west-2".to_string(),
            profile: None,
            bucket: "test".to_string(),
            target_path: "uploads".to_string(),
        };

        assert_eq!(
            config.build_s3_key("file.mp4"),
            "uploads/file.mp4"
        );

        assert_eq!(
            config.build_s3_key("dir/file.mp4"),
            "uploads/dir/file.mp4"
        );
    }
}
```

```rust
// tests/integration_test.rs
#[tokio::test]
#[ignore] // Requires AWS credentials
async fn test_upload_flow() {
    // Create temp file
    let temp_file = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(temp_file.path(), b"test content").unwrap();

    // Upload
    let config = Config::from_env().unwrap();
    let client = S3Client::new(config.clone()).await.unwrap();

    let result = upload_file(
        client.client(),
        client.bucket(),
        "test/file.txt",
        temp_file.path(),
        None
    ).await;

    assert!(result.is_ok());

    // Cleanup
    client.client()
        .delete_object()
        .bucket(client.bucket())
        .key("test/file.txt")
        .send()
        .await
        .unwrap();
}
```

**Dependencies:**

```toml
[dev-dependencies]
tempfile = "3.0"
```

**Impact:** Confidence in code correctness, easier refactoring.

---

### 4.2 Add Integration Tests with LocalStack (P2)

**Solution:**
Use LocalStack for S3 testing without real AWS.

```rust
// tests/localstack_test.rs
use testcontainers::*;

#[tokio::test]
async fn test_with_localstack() {
    let docker = clients::Cli::default();
    let localstack = docker.run(LocalStack::default());

    let endpoint = format!(
        "http://localhost:{}",
        localstack.get_host_port_ipv4(4566)
    );

    // Configure client to use LocalStack
    let config = aws_config::from_env()
        .endpoint_url(endpoint)
        .load()
        .await;

    let client = aws_sdk_s3::Client::new(&config);

    // Create test bucket
    client
        .create_bucket()
        .bucket("test-bucket")
        .send()
        .await
        .unwrap();

    // Run tests
    // ...
}
```

**Dependencies:**

```toml
[dev-dependencies]
testcontainers = "0.15"
```

**Impact:** Fast, reliable integration tests without AWS costs.

---

### 4.3 Add Benchmarks (P2)

```rust
// benches/upload_benchmark.rs
use criterion::{black_box, criterion_group, criterion_main, Criterion, BenchmarkId};

fn bench_file_comparison(c: &mut Criterion) {
    let mut group = c.benchmark_group("file_comparison");

    for size in [1024, 1024 * 1024, 10 * 1024 * 1024].iter() {
        group.bench_with_input(
            BenchmarkId::from_parameter(size),
            size,
            |b, &size| {
                b.iter(|| {
                    // Benchmark comparison logic
                });
            },
        );
    }

    group.finish();
}

criterion_group!(benches, bench_file_comparison);
criterion_main!(benches);
```

**Dependencies:**

```toml
[dev-dependencies]
criterion = "0.5"
```

**Impact:** Data-driven performance optimization.

---

## 5. Documentation Improvements

### 5.1 Add Inline Documentation (P1)

**Solution:**
Add comprehensive rustdoc comments:

```rust
/// Uploads a file to AWS S3 with optional progress tracking.
///
/// # Arguments
///
/// * `client` - The AWS S3 client instance
/// * `bucket` - The name of the S3 bucket
/// * `s3_key` - The S3 object key (path)
/// * `local_path` - Path to the local file to upload
/// * `pb` - Optional progress bar for tracking upload progress
///
/// # Returns
///
/// Returns `Ok(UploadResult::Uploaded)` on success, or an error if the upload fails.
///
/// # Errors
///
/// This function will return an error if:
/// - The local file cannot be opened or read
/// - The S3 upload request fails
/// - Network connectivity issues occur
///
/// # Examples
///
/// ```no_run
/// use aws_sdk_s3::Client;
/// use std::path::Path;
///
/// # async fn example() -> anyhow::Result<()> {
/// let client = Client::new(&aws_config::load_from_env().await);
/// let result = upload_file(
///     &client,
///     "my-bucket",
///     "uploads/file.txt",
///     Path::new("local.txt"),
///     None
/// ).await?;
/// # Ok(())
/// # }
/// ```
pub async fn upload_file(
    client: &Client,
    bucket: &str,
    s3_key: &str,
    local_path: &Path,
    pb: Option<&ProgressBar>,
) -> Result<UploadResult> {
    // ...
}
```

**Impact:** Better code maintainability, easier for contributors.

---

### 5.2 Add Troubleshooting Guide (P2)

Create `docs/TROUBLESHOOTING.md`:

```markdown
# S3Upload Troubleshooting Guide

## Common Issues

### "Access Denied" Error

**Symptoms:**
```

Error: Access denied for bucket 'my-bucket'

```

**Causes & Solutions:**

1. **Invalid AWS Credentials**
   ```bash
   # Verify credentials
   aws sts get-caller-identity

   # Output should show your account
   ```

2. **Insufficient IAM Permissions**

   Required permissions:

   ```json
   {
     "Version": "2012-10-17",
     "Statement": [
       {
         "Effect": "Allow",
         "Action": [
           "s3:PutObject",
           "s3:GetObject",
           "s3:HeadObject",
           "s3:ListBucket"
         ],
         "Resource": [
           "arn:aws:s3:::my-bucket",
           "arn:aws:s3:::my-bucket/*"
         ]
       }
     ]
   }
   ```

3. **Wrong AWS Profile**

   ```bash
   # Check current profile
   echo $AWS_PROFILE

   # Set correct profile in .env
   AWS_PROFILE=correct-profile
   ```

### Upload Fails for Large Files

**Symptoms:**

- Timeout errors
- Connection reset
- Out of memory

**Solutions:**

1. **Reduce Concurrency**

   ```bash
   s3upload large-file.mp4 --max-concurrent 1
   ```

2. **Enable Multipart Upload** (automatic for files > 100MB)
   - Check implementation is using multipart for large files

3. **Increase Timeout** (if implemented)

   ```bash
   s3upload large-file.mp4 --timeout 600
   ```

### Pre-signed URLs Not Working

**Symptoms:**

- 403 Forbidden when accessing URL
- URL expires immediately

**Solutions:**

1. **Check Bucket Policy**
   Ensure bucket allows GetObject for the IAM role

2. **Verify URL Expiration**
   Default is 7 days, check when URL was generated

3. **Check Clock Skew**

   ```bash
   # Ensure system time is correct
   date
   ```

### Files Always Re-uploading (Never Skipped)

**Causes:**

- Different S3_TARGET_PATH between uploads
- File content changing (even if size same)
- Multipart uploads (ETags are different)

**Solutions:**

1. **Check Configuration Consistency**

   ```bash
   # View current config
   cat .env
   ```

2. **Use --dry-run to Preview**

   ```bash
   s3upload . --dry-run
   ```

3. **Enable Better File Comparison** (if implemented)
   Use MD5 checksums instead of just size

## Performance Issues

### Slow Uploads

**Diagnosis:**

```bash
# Check network speed
speedtest-cli

# Check AWS region latency
ping s3.us-west-2.amazonaws.com
```

**Solutions:**

1. **Use Closer Region**

   ```env
   AWS_REGION=us-west-2  # Change to nearest region
   ```

2. **Increase Concurrency**

   ```bash
   s3upload . --max-concurrent 8
   ```

3. **Enable Bandwidth Throttling** (if too aggressive)

   ```bash
   s3upload . --max-speed-mbps 10
   ```

## Debug Mode

Enable detailed logging:

```bash
RUST_LOG=debug s3upload file.mp4
```

Levels:

- `error` - Only errors
- `warn` - Warnings and errors
- `info` - General information (default)
- `debug` - Detailed debugging
- `trace` - Very verbose

## Getting Help

1. Check documentation: `s3upload --help`
2. Review logs with `RUST_LOG=debug`
3. File issue: <https://github.com/tyrchen/swiss-knife/issues>

Include:

- Error message
- Command used
- .env configuration (redact sensitive info)
- Debug logs

```

**Impact:** Reduced support burden, faster issue resolution.

---

## 6. Security Enhancements

### 6.1 Credential Security Audit (P0)

**Current Issue:**
Need to ensure credentials are never logged or exposed.

**Solution:**
Implement security audit checklist:

```rust
// Ensure AWS credentials are never in debug output
impl fmt::Debug for S3Client {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("S3Client")
            .field("bucket", &self.config.bucket)
            .field("region", &self.config.region)
            // DO NOT include client (contains credentials)
            .finish()
    }
}

// Redact credentials in config display
impl fmt::Display for Config {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Config {{ region: {}, bucket: {}, profile: [REDACTED] }}",
            self.region, self.bucket
        )
    }
}
```

**Impact:** Prevents accidental credential exposure.

---

### 6.2 Server-Side Encryption Support (P1)

**Feature:**
Support S3 server-side encryption.

```rust
#[derive(Parser, Debug)]
struct Cli {
    // ... existing fields

    /// Enable server-side encryption (AES256 or aws:kms)
    #[arg(long, value_enum)]
    encryption: Option<EncryptionType>,

    /// KMS key ID (required if encryption=aws:kms)
    #[arg(long)]
    kms_key_id: Option<String>,
}

#[derive(Debug, Clone, ValueEnum)]
enum EncryptionType {
    Aes256,
    AwsKms,
}
```

```rust
let mut put_request = client
    .put_object()
    .bucket(bucket)
    .key(s3_key)
    .body(body);

if let Some(encryption) = &cli.encryption {
    match encryption {
        EncryptionType::Aes256 => {
            put_request = put_request
                .server_side_encryption(ServerSideEncryption::Aes256);
        }
        EncryptionType::AwsKms => {
            put_request = put_request
                .server_side_encryption(ServerSideEncryption::AwsKms);

            if let Some(kms_key) = &cli.kms_key_id {
                put_request = put_request.ssekms_key_id(kms_key);
            }
        }
    }
}

put_request.send().await?;
```

**Impact:** Data security at rest, compliance requirements.

---

### 6.3 IAM Role Validation (P2)

**Feature:**
Validate IAM permissions before upload.

```rust
async fn validate_permissions(client: &S3Client) -> Result<()> {
    // Test bucket access
    let head_bucket = client.client()
        .head_bucket()
        .bucket(client.bucket())
        .send()
        .await;

    if head_bucket.is_err() {
        anyhow::bail!(
            "Cannot access bucket '{}'. Check permissions.",
            client.bucket()
        );
    }

    // Test if we can list objects
    let list_result = client.client()
        .list_objects_v2()
        .bucket(client.bucket())
        .max_keys(1)
        .send()
        .await;

    if list_result.is_err() {
        eprintln!(
            "{} Cannot list objects in bucket. s3:ListBucket permission missing.",
            style("⚠").yellow()
        );
    }

    Ok(())
}
```

**Impact:** Early detection of permission issues.

---

## 7. Implementation Roadmap

### Phase 1: Critical Fixes (2-3 days)

- **P0 Items:**
  - 1.5 Memory management for large files
  - 6.1 Credential security audit

- **P1 Items:**
  - 1.1 Progress bar enhancement
  - 1.2 Error handling improvements
  - 1.3 ETag-based file comparison
  - 1.6 Retry logic
  - 1.7 Configuration validation

### Phase 2: Core Features (1 week)

- 2.1 Multipart upload support
- 1.4 Concurrent operation improvements
- 2.5 Resume interrupted uploads
- 4.1 Unit tests
- 5.1 Inline documentation

### Phase 3: Enhanced Features (1 week)

- 2.2 Dry-run mode
- 2.3 Custom URL expiration
- 2.4 Metadata and tags
- 2.8 Content-Type detection
- 3.4 Better error messages

### Phase 4: Advanced Features (1-2 weeks)

- 2.6 Bandwidth throttling
- 2.7 Directory structure options
- 2.9 Sync mode
- 2.10 Checksums in metadata
- 3.1 Output formatting
- 3.2 Interactive mode

### Phase 5: Polish & Documentation (3-4 days)

- 1.8 Logging infrastructure
- 3.5 Configuration file support
- 3.6 Shell completions
- 5.2 Troubleshooting guide
- 4.2 Integration tests

### Phase 6: Security & Reliability (2-3 days)

- 6.2 Server-side encryption
- 6.3 IAM role validation
- 4.3 Benchmarks
- Performance tuning

---

## 8. Metrics for Success

### Performance Metrics

- **Upload Speed:** Should achieve 80%+ of available bandwidth
- **Concurrency:** 4-8 concurrent uploads without degradation
- **Memory Usage:** < 100MB base + 10MB per concurrent upload
- **CPU Usage:** < 25% on modern systems

### Reliability Metrics

- **Success Rate:** > 99.9% for uploads with retry
- **Resume Success:** > 95% for interrupted large files
- **False Positive Skips:** < 0.1% (with ETag comparison)

### User Experience Metrics

- **Time to First Upload:** < 5 seconds (config + init)
- **Error Recovery:** Clear error message + suggested fix > 90% of cases
- **Documentation Coverage:** 100% of public APIs

---

## 9. Breaking Changes Considerations

None of the proposed changes should break existing usage, but consider:

1. **Configuration File:** New optional feature, doesn't affect .env users
2. **New CLI Flags:** All optional, backward compatible
3. **Error Types:** Internal change, wrapped in `anyhow::Error` for compatibility
4. **Progress Bar:** Enhanced behavior, not breaking

---

## 10. Conclusion

This improvement plan addresses:

- **Code Quality:** 8 major improvements (including critical memory/progress issues)
- **Features:** 10 new features (multipart, resume, sync, metadata, etc.)
- **UX:** 7 enhancements (better output, interactive mode, error messages)
- **Testing:** 3 new test strategies (unit, integration, benchmarks)
- **Security:** 3 security enhancements (encryption, validation, audit)

**Estimated Total Implementation Time:** 4-5 weeks for full implementation

**Priority Order for Maximum Impact:**

1. Fix critical bugs (P0: memory, security)
2. Improve reliability (P1: retry, errors, ETag comparison)
3. Add multipart upload (P1: enables large files)
4. Enhance UX (P1: better errors, output)
5. Add tests and documentation (P1: maintainability)
6. Implement advanced features (P2: sync, metadata, etc.)
7. Polish and optimize (P2-P3: convenience features)

The tool is already well-architected with good concurrency support. These improvements will make it production-ready, more reliable, and significantly more feature-complete while maintaining backward compatibility.
