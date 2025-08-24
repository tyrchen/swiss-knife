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

### More Tools Coming Soon

Stay tuned for additional utilities to be added to the Swiss Knife collection!

## System Requirements

### For convert tool

- FFmpeg (for audio extraction)
- FFprobe (for video analysis)
- OpenAI API key

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
