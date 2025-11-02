# Swiss Knife 🔧

A collection of useful Rust CLI tools for various automation tasks. Built with performance and user experience in mind.

[中文版](README-CN.md) | [English](README.md)

## Overview

Swiss Knife is a growing collection of command-line utilities written in Rust, designed to handle common tasks efficiently with beautiful user interfaces.

## Available Tools

### 1. convert - Video Transcription & Content Generation

A high-performance tool for video transcription and AI-powered content generation.

**Features:**

- 🚀 **Concurrent Processing**: Leverages Tokio async runtime for parallel chunk transcription
- 📊 **Real-time Progress**: Beautiful progress bars for every processing step
- ♻️ **Smart Caching**: Automatically detects and reuses processed files
- 🎯 **Auto-chunking**: Long videos are automatically split and processed in parallel
- 🎨 **Colored Output**: User-friendly colored terminal output
- 📦 **Auto-compression**: Large audio files are compressed to meet API limits

### 2. imgen - AI Image Generation

Generate images using OpenAI's DALL-E API.

### 3. s3upload - AWS S3 File Uploader

A smart S3 uploader with intelligent file comparison and pre-signed URL generation.

**Features:**

- 📦 **Smart Upload**: Automatically skips files that are identical on S3
- 🔗 **Pre-signed URLs**: Generates 7-day valid URLs for sharing
- 📊 **Progress Bars**: Real-time upload progress tracking
- 📁 **Directory Support**: Upload single files or entire directories
- ⚡ **URL-Only Mode**: Generate pre-signed URLs without uploading
- 🎨 **Beautiful Output**: Colorful, user-friendly console interface

## Installation

### From crates.io (Recommended)

```bash
cargo install swiss-knife
```

### From Source

```bash
# Clone the repository
git clone https://github.com/tyrchen/swiss-knife
cd swiss-knife

# Build in release mode
cargo build --release

# Install to cargo bin directory
cargo install --path .
```

## Usage

### convert - Video Transcription

```bash
# Set up OpenAI API key
export OPENAI_API_KEY="your-api-key"

# Process a video file
convert <video_file>

# Example
convert ~/Videos/lecture.mp4
```

**Output Example:**

```text
🎬 Processing video: "lecture.mp4"

⠋ Analyzing video duration...
Video duration: 2500 seconds

⚠️  Video longer than 1300 seconds, processing in chunks...
   Will create 2 chunks

⠏ [########################################] 2/2 chunks processed
✅ All chunks merged into complete transcript

⠋ Generating content with GPT-5-mini...
✅ Content generated successfully!

✨ Processing complete!
📦 All files saved in /tmp
```

### s3upload - AWS S3 Uploader

```bash
# Create .env file with AWS configuration
cat > .env << EOF
AWS_REGION=us-west-2
S3_BUCKET=my-bucket
S3_TARGET_PATH=uploads
EOF

# Upload a single file
s3upload ./video.mp4

# Upload entire directory
s3upload ./videos

# Generate pre-signed URLs only (no upload)
s3upload ./video.mp4 --url-only
```

**Output Example:**

```text
📦 Target: s3://my-bucket/uploads/
✓ video.mp4 (15.2 MB)
  🔗 https://my-bucket.s3.amazonaws.com/uploads/video.mp4?X-Amz-...
↻ video2.mp4 (skipped - identical, 10.5 MB)
  🔗 https://my-bucket.s3.amazonaws.com/uploads/video2.mp4?X-Amz-...

Summary: 1 uploaded, 1 skipped, 0 failed
```

For detailed documentation, see [specs/s3upload-README.md](specs/s3upload-README.md)

## System Requirements

### For convert tool

- FFmpeg (for audio extraction)
- FFprobe (for video analysis)
- OpenAI API key

### For s3upload tool

- Valid AWS credentials (via environment, credentials file, or IAM role)
- S3 bucket with appropriate permissions

### General

- Rust 1.70+
- Linux/macOS/Windows

## Contributing

Contributions are welcome! Feel free to:

- Add new tools to the collection
- Improve existing tools
- Report issues or suggest features
- Submit pull requests

## Roadmap

- [ ] Add more video/audio processing tools
- [ ] Add text processing utilities
- [ ] Add file management tools
- [ ] Add network utilities
- [ ] Add development helper tools

## License

MIT License - see [LICENSE](LICENSE) file for details.

## Author

Tyr Chen <tyr.chen@gmail.com>
