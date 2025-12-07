# PDF to JPG Converter Design Document

## Overview

A command-line tool to convert PDF files to JPG images, one image per page. The tool follows the existing swiss-knife project conventions with a polished, user-friendly interface.

## Usage

```bash
pdf2jpg test.pdf -o ./out     # Convert PDF to images in ./out directory
pdf2jpg test.pdf              # Output to current directory
pdf2jpg test.pdf -q 90        # Custom JPEG quality (default: 85)
pdf2jpg test.pdf -d 150       # Custom DPI (default: 150)
pdf2jpg test.pdf --prefix doc # Custom filename prefix (default: PDF filename)
```

## Output

For a file named `test.pdf` with 5 pages:
```
./out/test_001.jpg
./out/test_002.jpg
./out/test_003.jpg
./out/test_004.jpg
./out/test_005.jpg
```

## Architecture

### Dependencies

| Dependency | Purpose |
|------------|---------|
| `clap` | Command-line argument parsing |
| `indicatif` | Progress bar display |
| `console` | Terminal styling |
| `anyhow` | Error handling |

External tools required:
- `pdftoppm` and `pdfinfo` from **poppler** (install via `brew install poppler`)

### Why Poppler?

- **Production-ready**: Well-maintained, widely used PDF processing library
- **Cross-platform**: Available on macOS, Linux, Windows
- **High quality**: Excellent rendering fidelity
- **Simple integration**: CLI tools that can be called from Rust
- **No complex linking**: Avoids static/dynamic library linking issues

This approach follows the pattern used by the existing `convert.rs` tool which uses `ffmpeg` for video processing.

### CLI Structure

```rust
#[derive(Parser)]
#[command(name = "pdf2jpg")]
struct Args {
    /// PDF file to convert
    #[arg(value_name = "PDF_FILE")]
    pdf_file: PathBuf,

    /// Output directory (default: current directory)
    #[arg(short, long, value_name = "DIR")]
    output: Option<PathBuf>,

    /// JPEG quality (1-100, default: 85)
    #[arg(short, long, default_value = "85")]
    quality: u8,

    /// DPI for rendering (default: 150)
    #[arg(short, long, default_value = "150")]
    dpi: u16,

    /// Filename prefix (default: PDF filename)
    #[arg(short, long)]
    prefix: Option<String>,
}
```

## Implementation

### Workflow

1. **Validate input** - Check PDF exists and has `.pdf` extension
2. **Check dependencies** - Verify `pdftoppm` and `pdfinfo` are available
3. **Get page count** - Use `pdfinfo` to determine total pages
4. **Convert pages** - Use `pdftoppm` to convert all pages to JPEG
5. **Rename files** - Normalize output filenames to `prefix_001.jpg` format
6. **Display results** - Show summary with file sizes

### User Experience

- Spinner while analyzing PDF
- Progress bar during conversion
- Clear summary showing all created files
- Helpful error messages with installation instructions

## Error Handling

| Error Type | Handling |
|------------|----------|
| poppler not installed | Clear installation instructions for macOS/Ubuntu/Windows |
| File not found | Clear error message with path |
| Invalid PDF | "Failed to get PDF info" with details |
| Permission denied | Show specific OS error |
| Invalid quality | "Quality must be between 1 and 100" |

## Output Example

```
⚙️   PDF to JPG Converter

📄 Input:   ./document.pdf
📁 Output:  ./images
  Quality: 85, DPI: 150

✔ PDF has 5 pages

✅  Conversion complete!

📁 Files created:
   document_001.jpg 245 KB
   document_002.jpg 312 KB
   document_003.jpg 198 KB
   document_004.jpg 267 KB
   document_005.jpg 223 KB

✨  5 files, total size: 1.2 MB
```

## Testing

### Unit Tests

- `test_format_size`: Verifies human-readable file size formatting

### Manual Testing

```bash
# Basic conversion
pdf2jpg test.pdf -o ./out

# High quality with custom DPI
pdf2jpg test.pdf -q 95 -d 300 -o ./hd

# Custom prefix
pdf2jpg test.pdf --prefix page -o ./out
```

## Installation Requirements

```bash
# macOS
brew install poppler

# Ubuntu/Debian
sudo apt-get install poppler-utils

# Windows
choco install poppler
```
