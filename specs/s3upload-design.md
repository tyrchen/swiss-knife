# S3 Upload Tool - Design and Implementation Plan

## Overview
A Rust CLI tool for uploading files to AWS S3 with intelligent file comparison, progress tracking, and pre-signed URL generation.

## Features

### Core Functionality
1. **File Upload to S3**
   - Upload single files or directories
   - Skip files that already exist with identical content
   - Generate pre-signed URLs (7-day validity) after upload

2. **URL-Only Mode**
   - Generate pre-signed URLs without uploading
   - Warn if files don't exist remotely
   - Support both single files and directories

3. **Smart File Comparison**
   - Check if remote file exists
   - Compare file sizes and/or checksums (ETag)
   - Skip upload if files are identical

## Configuration (.env)

```env
AWS_REGION=us-west-2
AWS_PROFILE=default
S3_BUCKET=my-bucket
S3_TARGET_PATH=uploads/  # optional prefix for all uploads
```

## CLI Interface

```bash
# Upload single file
s3upload ./test.mp4

# Upload all files in current directory
s3upload .

# Upload directory
s3upload ./videos

# Generate pre-signed URL only (no upload)
s3upload ./test.mp4 --url-only
s3upload . --url-only
```

## Architecture

### Module Structure
```
src/
├── s3upload.rs          # Main binary
├── s3/
│   ├── mod.rs          # Module exports
│   ├── client.rs       # S3 client wrapper
│   ├── upload.rs       # Upload logic
│   ├── compare.rs      # File comparison
│   └── presign.rs      # Pre-signed URL generation
└── config.rs           # Configuration loading
```

### Dependencies
- `clap` (v4) - CLI argument parsing (already in project)
- `tokio` (v1) - Async runtime (already in project)
- `anyhow` (v1) - Error handling (already in project)
- `dotenv` - .env file loading
- `aws-config` - AWS configuration
- `aws-sdk-s3` - S3 SDK
- `indicatif` (v0.18) - Progress bars (already in project)
- `console` (v0.16) - Terminal colors (already in project)
- `blake3` (v1.8.2) - Fast hashing for local files (already in project)
- `tokio-util` - Streaming utilities (already in project)

## Implementation Details

### 1. Configuration Loading
```rust
struct Config {
    region: String,
    profile: Option<String>,
    bucket: String,
    target_path: String,
}
```

### 2. CLI Arguments
```rust
#[derive(Parser)]
struct Cli {
    /// File or directory to upload
    path: PathBuf,

    /// Only generate pre-signed URLs, don't upload
    #[arg(long)]
    url_only: bool,
}
```

### 3. File Processing Flow

**Upload Mode:**
1. Parse CLI arguments
2. Load configuration from .env
3. Initialize AWS S3 client
4. Collect files to process
5. For each file:
   - Check if remote file exists
   - Compare file size/hash
   - Upload if different or missing
   - Show progress bar during upload
   - Generate pre-signed URL
6. Display results summary

**URL-Only Mode:**
1. Parse CLI arguments
2. Load configuration
3. Initialize S3 client
4. Collect files to process
5. For each file:
   - Check if remote file exists
   - If exists: generate and display pre-signed URL
   - If not exists: show warning
6. Display results summary

### 4. File Comparison Strategy
- First check: file size comparison (fast)
- Second check: ETag comparison if available
- For uploads: compute local file hash to set metadata

### 5. Progress Display
- Overall progress: multi-progress bar
- Per-file progress: byte upload progress
- Status indicators:
  - ✓ Uploaded successfully
  - ↻ Skipped (identical)
  - ⚠ Warning (not found in URL-only mode)
  - ✗ Error

### 6. Pre-signed URL Generation
- Default expiration: 7 days (604800 seconds)
- Display URL immediately after upload
- Copy-friendly format

### 7. S3 Key Construction
- Pattern: `{S3_TARGET_PATH}/{relative_path_from_input}`
- Example: input `./videos/test.mp4` → S3 key `uploads/videos/test.mp4`
- For single file: use base filename only

## Error Handling
- Use `anyhow::Result<T>` for all fallible operations
- Provide clear error messages with context
- Continue processing other files if one fails
- Summary at end shows success/failure counts

## User Experience

### Output Examples

**Upload Mode:**
```
📦 Uploading to s3://my-bucket/uploads/
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━ 100%
✓ test.mp4 (15.2 MB)
  🔗 https://my-bucket.s3.amazonaws.com/uploads/test.mp4?X-Amz-...
↻ video2.mp4 (skipped - identical)
  🔗 https://my-bucket.s3.amazonaws.com/uploads/video2.mp4?X-Amz-...

Summary: 1 uploaded, 1 skipped, 0 failed
```

**URL-Only Mode:**
```
🔗 Generating pre-signed URLs...
✓ test.mp4
  🔗 https://my-bucket.s3.amazonaws.com/uploads/test.mp4?X-Amz-...
⚠ video3.mp4 (not found on S3)

Summary: 1 URL generated, 1 not found
```

## Implementation Steps

1. ✓ Create design document
2. Add AWS SDK dependencies to Cargo.toml
3. Create module structure (src/s3/*.rs, src/config.rs)
4. Implement configuration loading
5. Implement CLI argument parsing
6. Implement S3 client initialization
7. Implement file discovery and collection
8. Implement file comparison logic
9. Implement upload with progress bars
10. Implement pre-signed URL generation
11. Implement --url-only mode
12. Add comprehensive error handling
13. Test with various scenarios

## Testing Scenarios

1. Upload single file (new)
2. Upload single file (already exists, identical)
3. Upload single file (already exists, different)
4. Upload directory with multiple files
5. Upload current directory (.)
6. Generate URL for existing file
7. Generate URL for non-existent file
8. Generate URLs for directory
9. Handle missing .env file
10. Handle invalid AWS credentials
11. Handle network errors

## Security Considerations

- Never log AWS credentials
- Validate S3 bucket names
- Sanitize file paths to prevent directory traversal
- Use pre-signed URLs instead of public buckets
- Respect AWS credential chain (env vars, profile, IAM role)
