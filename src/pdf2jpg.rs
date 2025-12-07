use anyhow::{Context, Result};
use clap::Parser;
use console::{style, Emoji};
use indicatif::{ProgressBar, ProgressStyle};
use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::time::Duration;

static DOCUMENT: Emoji<'_, '_> = Emoji("📄 ", "");
static FOLDER: Emoji<'_, '_> = Emoji("📁 ", "");
static CHECK: Emoji<'_, '_> = Emoji("✅ ", "");
static GEAR: Emoji<'_, '_> = Emoji("⚙️  ", "");
static SPARKLES: Emoji<'_, '_> = Emoji("✨ ", "");

#[derive(Parser)]
#[command(
    name = "pdf2jpg",
    version = env!("CARGO_PKG_VERSION"),
    author = "Tyr Chen <tyr.chen@gmail.com>",
    about = "Convert PDF files to JPG images",
    long_about = "Convert each page of a PDF file to a separate JPG image. \
                  Supports custom output directory, JPEG quality, and DPI settings.",
    after_help = "Examples:\n  \
                  pdf2jpg document.pdf                    # Output: 001.jpg, 002.jpg, ...\n  \
                  pdf2jpg document.pdf -o ./images        # Convert to ./images directory\n  \
                  pdf2jpg document.pdf -q 90 -d 200       # High quality, 200 DPI\n  \
                  pdf2jpg document.pdf --prefix doc       # Output: doc_001.jpg, doc_002.jpg, ...\n\n\
                  Output:\n  \
                  For a file named 'test.pdf' with 3 pages (no prefix):\n    \
                  001.jpg\n    \
                  002.jpg\n    \
                  003.jpg\n\n\
                  Requirements:\n  \
                  - poppler (install via: brew install poppler)\n\n\
                  For more information: https://github.com/tyrchen/swiss-knife"
)]
struct Args {
    /// PDF file to convert
    #[arg(value_name = "PDF_FILE")]
    pdf_file: PathBuf,

    /// Output directory (default: current directory)
    #[arg(short, long, value_name = "DIR")]
    output: Option<PathBuf>,

    /// JPEG quality (1-100)
    #[arg(short, long, default_value = "85", value_parser = clap::value_parser!(u8).range(1..=100))]
    quality: u8,

    /// DPI for rendering
    #[arg(short, long, default_value = "150")]
    dpi: u16,

    /// Filename prefix (optional, e.g., --prefix doc produces doc_001.jpg)
    #[arg(short, long)]
    prefix: Option<String>,
}

fn main() -> Result<()> {
    let args = Args::parse();

    // Check if pdftoppm is available
    check_pdftoppm_installed()?;

    // Validate input file
    if !args.pdf_file.exists() {
        anyhow::bail!(
            "PDF file not found: {}",
            style(args.pdf_file.display()).red()
        );
    }

    if !args
        .pdf_file
        .extension()
        .map(|e| e.eq_ignore_ascii_case("pdf"))
        .unwrap_or(false)
    {
        anyhow::bail!(
            "File does not appear to be a PDF: {}",
            style(args.pdf_file.display()).yellow()
        );
    }

    // Determine output directory
    let output_dir = args.output.unwrap_or_else(|| PathBuf::from("."));

    // Create output directory if it doesn't exist
    if !output_dir.exists() {
        fs::create_dir_all(&output_dir).with_context(|| {
            format!(
                "Failed to create output directory: {}",
                output_dir.display()
            )
        })?;
    }

    // Use user-provided prefix or None
    let prefix = args.prefix.as_deref();
    // Internal prefix for pdftoppm (it requires one)
    let internal_prefix = "page";

    // Print header
    println!();
    println!("{} {}", GEAR, style("PDF to JPG Converter").bold().cyan());
    println!();
    println!(
        "{}Input:   {}",
        DOCUMENT,
        style(args.pdf_file.display()).green()
    );
    println!("{}Output:  {}", FOLDER, style(output_dir.display()).green());
    println!(
        "  Quality: {}, DPI: {}",
        style(args.quality).cyan(),
        style(args.dpi).cyan()
    );
    println!();

    // Get page count first
    let spinner = ProgressBar::new_spinner();
    spinner.set_style(
        ProgressStyle::default_spinner()
            .template("{spinner:.green} {msg}")
            .unwrap(),
    );
    spinner.set_message("Analyzing PDF...");
    spinner.enable_steady_tick(Duration::from_millis(100));

    let page_count = get_page_count(&args.pdf_file)?;
    spinner.finish_with_message(format!(
        "PDF has {} page{}",
        style(page_count).cyan().bold(),
        if page_count == 1 { "" } else { "s" }
    ));
    println!();

    if page_count == 0 {
        println!("{} PDF has no pages to convert", style("Warning:").yellow());
        return Ok(());
    }

    // Create progress bar for conversion
    let progress = ProgressBar::new(page_count as u64);
    progress.set_style(
        ProgressStyle::with_template(
            "  {spinner:.green} [{bar:40.cyan/blue}] {pos}/{len} pages {msg}",
        )?
        .progress_chars("━━─"),
    );
    progress.set_message("Converting...");
    progress.enable_steady_tick(Duration::from_millis(100));

    // Build output path prefix for pdftoppm
    let output_prefix = output_dir.join(internal_prefix);

    // Run pdftoppm to convert all pages
    let output = Command::new("pdftoppm")
        .args([
            "-jpeg",
            "-jpegopt",
            &format!("quality={}", args.quality),
            "-r",
            &args.dpi.to_string(),
        ])
        .arg(&args.pdf_file)
        .arg(&output_prefix)
        .output()
        .context("Failed to run pdftoppm. Make sure poppler is installed (brew install poppler)")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("pdftoppm failed: {}", stderr);
    }

    progress.finish_and_clear();

    // Collect and rename output files
    let mut converted_files: Vec<(String, u64)> = Vec::new();

    for page in 1..=page_count {
        // pdftoppm outputs files with format: prefix-01.jpg, prefix-02.jpg, etc.
        let pdftoppm_name = if page_count >= 10 {
            format!("{}-{:02}.jpg", internal_prefix, page)
        } else {
            format!("{}-{}.jpg", internal_prefix, page)
        };
        let pdftoppm_path = output_dir.join(&pdftoppm_name);

        // Also try the format with more digits for larger documents
        let pdftoppm_name_3digit = format!("{}-{:03}.jpg", internal_prefix, page);
        let pdftoppm_path_3digit = output_dir.join(&pdftoppm_name_3digit);

        let source_path = if pdftoppm_path.exists() {
            pdftoppm_path
        } else if pdftoppm_path_3digit.exists() {
            pdftoppm_path_3digit
        } else {
            // Try other common patterns
            let patterns = [
                format!("{}-{}.jpg", internal_prefix, page),
                format!("{}-{:04}.jpg", internal_prefix, page),
                format!("{}-{:05}.jpg", internal_prefix, page),
                format!("{}-{:06}.jpg", internal_prefix, page),
            ];

            let mut found = None;
            for pattern in patterns {
                let path = output_dir.join(&pattern);
                if path.exists() {
                    found = Some(path);
                    break;
                }
            }

            match found {
                Some(p) => p,
                None => continue, // Skip if file not found
            }
        };

        // Rename to our preferred format: prefix_001.jpg or just 001.jpg
        let target_name = match prefix {
            Some(p) => format!("{}_{:03}.jpg", p, page),
            None => format!("{:03}.jpg", page),
        };
        let target_path = output_dir.join(&target_name);

        if source_path != target_path {
            fs::rename(&source_path, &target_path).with_context(|| {
                format!(
                    "Failed to rename {} to {}",
                    source_path.display(),
                    target_path.display()
                )
            })?;
        }

        let file_size = fs::metadata(&target_path).map(|m| m.len()).unwrap_or(0);

        converted_files.push((target_name, file_size));
    }

    // Print summary
    println!("{} {}", CHECK, style("Conversion complete!").green().bold());
    println!();
    println!("{}Files created:", FOLDER);

    let total_size: u64 = converted_files.iter().map(|(_, size)| size).sum();

    // Show first few and last few files if there are many
    let show_limit = 5;
    if converted_files.len() <= show_limit * 2 {
        for (filename, size) in &converted_files {
            println!(
                "   {} {}",
                style(filename).dim(),
                style(format_size(*size)).dim()
            );
        }
    } else {
        // Show first few
        for (filename, size) in converted_files.iter().take(show_limit) {
            println!(
                "   {} {}",
                style(filename).dim(),
                style(format_size(*size)).dim()
            );
        }
        println!(
            "   {} ...",
            style(format!(
                "({} more files)",
                converted_files.len() - show_limit * 2
            ))
            .dim()
        );
        // Show last few
        for (filename, size) in converted_files.iter().rev().take(show_limit).rev() {
            println!(
                "   {} {}",
                style(filename).dim(),
                style(format_size(*size)).dim()
            );
        }
    }

    println!();
    println!(
        "{} {} files, total size: {}",
        SPARKLES,
        style(converted_files.len()).cyan().bold(),
        style(format_size(total_size)).cyan()
    );
    println!();

    Ok(())
}

/// Check if pdftoppm is installed
fn check_pdftoppm_installed() -> Result<()> {
    let output = Command::new("pdftoppm").arg("-v").output();

    match output {
        Ok(o) if o.status.success() || !o.stderr.is_empty() => Ok(()),
        _ => {
            anyhow::bail!(
                "pdftoppm not found. Please install poppler:\n  \
                 macOS:   brew install poppler\n  \
                 Ubuntu:  sudo apt-get install poppler-utils\n  \
                 Windows: choco install poppler"
            );
        }
    }
}

/// Get the number of pages in a PDF using pdfinfo
fn get_page_count(pdf_path: &PathBuf) -> Result<u32> {
    let output = Command::new("pdfinfo")
        .arg(pdf_path)
        .output()
        .context("Failed to run pdfinfo")?;

    if !output.status.success() {
        anyhow::bail!("Failed to get PDF info");
    }

    let stdout = String::from_utf8_lossy(&output.stdout);

    for line in stdout.lines() {
        if line.starts_with("Pages:") {
            let pages_str = line.trim_start_matches("Pages:").trim();
            return pages_str
                .parse()
                .with_context(|| format!("Failed to parse page count: {}", pages_str));
        }
    }

    anyhow::bail!("Could not find page count in PDF info")
}

/// Format file size in human-readable format
fn format_size(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = KB * 1024;

    if bytes >= MB {
        format!("{:.1} MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.0} KB", bytes as f64 / KB as f64)
    } else {
        format!("{} B", bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_size() {
        assert_eq!(format_size(500), "500 B");
        assert_eq!(format_size(1024), "1 KB");
        assert_eq!(format_size(2048), "2 KB");
        assert_eq!(format_size(1024 * 1024), "1.0 MB");
        assert_eq!(format_size(1024 * 1024 * 2 + 512 * 1024), "2.5 MB");
    }
}
