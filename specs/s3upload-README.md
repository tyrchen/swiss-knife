# S3 Upload Tool

A fast, user-friendly CLI tool for uploading files to AWS S3 with intelligent file comparison and pre-signed URL generation.

## Features

- **Smart Upload**: Automatically skips files that are identical on S3 (compares file size)
- **Pre-signed URLs**: Generates 7-day valid pre-signed URLs for all uploaded files
- **Progress Tracking**: Beautiful progress bars for file uploads
- **Directory Support**: Upload single files or entire directories
- **URL-Only Mode**: Generate pre-signed URLs without uploading (for existing files)
- **Colorful Output**: User-friendly console output with colors and emojis

## Installation

Build the tool using Cargo:

```bash
cargo build --release --bin s3upload
```

The binary will be available at `target/release/s3upload`.

Optionally, install it globally:

```bash
cargo install --path . --bin s3upload
```

## Configuration

Create a `.env` file in your project root (or copy from `.env.example`):

```env
# AWS Configuration
AWS_REGION=us-west-2
AWS_PROFILE=default  # Optional
S3_BUCKET=my-bucket-name
S3_TARGET_PATH=uploads  # Optional prefix for all uploads
```

### Configuration Options

| Variable | Required | Description | Example |
|----------|----------|-------------|---------|
| `AWS_REGION` | Yes | AWS region where your S3 bucket is located | `us-west-2` |
| `AWS_PROFILE` | No | AWS CLI profile to use (defaults to default profile) | `my-profile` |
| `S3_BUCKET` | Yes | S3 bucket name | `my-bucket` |
| `S3_TARGET_PATH` | No | Path prefix for uploaded files (defaults to bucket root) | `uploads/videos` |

## AWS Credentials

The tool uses the standard AWS credential chain:

1. Environment variables (`AWS_ACCESS_KEY_ID`, `AWS_SECRET_ACCESS_KEY`)
2. AWS credentials file (`~/.aws/credentials`)
3. IAM role (if running on EC2/ECS/Lambda)

Make sure you have valid AWS credentials configured.

## Usage

### Upload Single File

```bash
s3upload ./video.mp4
```

This will:
1. Check if the file exists on S3
2. Upload if it's new or different
3. Generate a 7-day pre-signed URL
4. Display the URL

**Output:**
```
📦 Target: s3://my-bucket/uploads/
✓ video.mp4 (15.2 MB)
  🔗 https://my-bucket.s3.amazonaws.com/uploads/video.mp4?X-Amz-...

Summary: 1 uploaded, 0 skipped, 0 failed
```

### Upload Directory

```bash
s3upload ./videos
```

Uploads all files in the `videos` directory, maintaining the directory structure.

### Upload Current Directory

```bash
s3upload .
```

Uploads all files in the current directory and subdirectories.

### Generate Pre-signed URLs Only

Use the `--url-only` flag to generate pre-signed URLs without uploading:

```bash
# Single file
s3upload ./video.mp4 --url-only

# Directory
s3upload ./videos --url-only
```

**Output (URL-only mode):**
```
🔗 Generating pre-signed URLs...
✓ video.mp4
  🔗 https://my-bucket.s3.amazonaws.com/uploads/video.mp4?X-Amz-...
⚠ video2.mp4 (not found on S3)

Summary: 1 URL(s) generated, 1 not found
```

## How It Works

### Upload Mode

1. **File Discovery**: Collects all files from the specified path
2. **S3 Comparison**: For each file:
   - Checks if the file exists on S3
   - Compares file size with remote object
   - Skips upload if identical
3. **Upload**: Uploads new or modified files with progress tracking
4. **URL Generation**: Creates 7-day pre-signed URLs for all files
5. **Summary**: Displays upload statistics

### URL-Only Mode

1. **File Discovery**: Collects all files from the specified path
2. **S3 Check**: Verifies each file exists on S3
3. **URL Generation**: Generates pre-signed URLs for existing files
4. **Warnings**: Reports files not found on S3
5. **Summary**: Displays URL generation statistics

## File Comparison Strategy

The tool uses a fast file comparison strategy:

1. **Size Check**: Compares local file size with S3 object size
2. **Skip Decision**: If sizes match, the file is considered identical and skipped

This approach is fast and works well for most use cases. Files are re-uploaded only if:
- They don't exist on S3
- Their size has changed

## Examples

### Example 1: Upload video files for sharing

```bash
# Set up configuration
cat > .env << EOF
AWS_REGION=us-west-2
S3_BUCKET=my-share-bucket
S3_TARGET_PATH=shared-videos
EOF

# Upload videos
s3upload ./recordings/

# Output:
# 📦 Target: s3://my-share-bucket/shared-videos/
# ✓ recording1.mp4 (125.3 MB)
#   🔗 https://my-share-bucket.s3.amazonaws.com/shared-videos/recording1.mp4?...
# ✓ recording2.mp4 (98.7 MB)
#   🔗 https://my-share-bucket.s3.amazonaws.com/shared-videos/recording2.mp4?...
#
# Summary: 2 uploaded, 0 skipped, 0 failed
```

### Example 2: Re-upload modified files

```bash
# First upload
s3upload ./documents/report.pdf

# Modify the file locally
echo "new content" >> ./documents/report.pdf

# Re-upload (will detect change via size difference)
s3upload ./documents/report.pdf

# Output:
# ✓ report.pdf (2.1 MB)
#   🔗 https://...
```

### Example 3: Generate URLs for existing files

```bash
# Upload files
s3upload ./presentations/*.pdf

# Later, get fresh pre-signed URLs without re-uploading
s3upload ./presentations/*.pdf --url-only

# Output:
# 🔗 Generating pre-signed URLs...
# ✓ slide1.pdf
#   🔗 https://...
# ✓ slide2.pdf
#   🔗 https://...
```

## S3 Key Structure

The S3 key (object path) is constructed as follows:

```
{S3_TARGET_PATH}/{relative_path}
```

**Examples:**

| Input Path | S3_TARGET_PATH | Resulting S3 Key |
|------------|----------------|------------------|
| `./video.mp4` | `uploads` | `uploads/video.mp4` |
| `./videos/test.mp4` | `media` | `media/videos/test.mp4` |
| `./doc.pdf` | *(empty)* | `doc.pdf` |

## Output Indicators

The tool uses visual indicators for clarity:

| Symbol | Meaning | Color |
|--------|---------|-------|
| ✓ | Successfully uploaded or URL generated | Green |
| ↻ | File skipped (identical to S3 version) | Yellow |
| ⚠ | Warning (file not found on S3 in URL-only mode) | Yellow |
| ✗ | Error occurred | Red |
| 🔗 | Pre-signed URL | Blue |
| 📦 | Upload target information | Cyan |

## Error Handling

The tool provides clear error messages for common issues:

- **Missing .env file**: Prompts to create configuration
- **Missing AWS credentials**: Suggests checking AWS configuration
- **Invalid bucket**: Reports S3 access errors
- **Network errors**: Shows connection issues
- **File not found**: Reports missing local files

Errors are reported but don't stop processing of other files. The final summary shows the count of failed uploads.

## Troubleshooting

### "AWS_REGION not found in environment"

Create a `.env` file with the required configuration (see Configuration section).

### "Access Denied" errors

Check that your AWS credentials have the necessary S3 permissions:
- `s3:PutObject` - For uploading files
- `s3:GetObject` - For generating pre-signed URLs
- `s3:HeadObject` - For checking file existence

### Files always re-uploading (never skipping)

This can happen if:
- The S3 object key is different (check `S3_TARGET_PATH` configuration)
- File is being modified between uploads
- Different S3 bucket or region in configuration

### Pre-signed URLs not working

Ensure:
- The S3 bucket allows the necessary permissions
- URLs are used within the 7-day validity period
- URLs are not modified or truncated when copied

## Performance Tips

1. **Use appropriate S3 region**: Upload to a region close to you for better performance
2. **Batch uploads**: Upload multiple files at once rather than one at a time
3. **Skip unchanged files**: The tool automatically does this, saving time and bandwidth
4. **Progress monitoring**: Use the progress bars to estimate completion time

## Security Considerations

- **Credential Safety**: Never commit `.env` file to version control
- **Pre-signed URL Expiry**: URLs expire after 7 days for security
- **Bucket Permissions**: Use least-privilege IAM policies
- **Public Access**: Consider bucket policies and ACLs carefully

## Contributing

This tool is part of the `swiss-knife` collection of CLI utilities. Contributions are welcome!

## License

MIT License - See LICENSE file for details
