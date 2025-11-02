mod config;
mod s3;

use anyhow::{Context, Result};
use clap::Parser;
use console::style;
use indicatif::{MultiProgress, ProgressBar, ProgressStyle};
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

use config::Config;
use s3::{compare::compare_file, generate_presigned_url, upload_file, S3Client, UploadResult};

#[derive(Parser, Debug)]
#[command(
    name = "s3upload",
    version = env!("CARGO_PKG_VERSION"),
    author = "Tyr Chen <tyr.chen@gmail.com>",
    about = "Upload files to AWS S3 with smart comparison and pre-signed URLs",
    long_about = "A smart S3 uploader that automatically skips identical files and generates 7-day valid pre-signed URLs. \
                  Supports directory uploads with file extension filtering. Configure via .env file with AWS credentials.",
    after_help = "Examples:\n  \
                  s3upload ./video.mp4                    # Upload single file\n  \
                  s3upload .                              # Upload all mp4/mov files in current directory\n  \
                  s3upload ./videos -e mp4,mov,avi        # Upload with custom extensions\n  \
                  s3upload ./video.mp4 --url-only         # Generate pre-signed URL only\n\n\
                  Configuration (.env):\n  \
                  AWS_REGION=us-west-2\n  \
                  S3_BUCKET=my-bucket\n  \
                  S3_TARGET_PATH=uploads\n\n\
                  For more information: https://github.com/tyrchen/swiss-knife"
)]
struct Cli {
    /// File or directory to upload
    path: PathBuf,

    /// Only generate pre-signed URLs, don't upload
    #[arg(long)]
    url_only: bool,

    /// Allowed file extensions (comma-separated, e.g., "mp4,mov,avi")
    #[arg(long, short = 'e', default_value = "mp4,mov", value_delimiter = ',')]
    extensions: Vec<String>,
}

#[derive(Debug, Default)]
struct Stats {
    uploaded: usize,
    skipped: usize,
    failed: usize,
    urls_generated: usize,
    not_found: usize,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let config = Config::from_env()?;

    // Initialize S3 client
    let s3_client = S3Client::new(config.clone()).await?;

    // Collect files to process
    let files = collect_files(&cli.path, &cli.extensions)?;

    if files.is_empty() {
        println!(
            "{}",
            style(format!(
                "No files found with extensions: {}",
                cli.extensions.join(", ")
            ))
            .yellow()
        );
        return Ok(());
    }

    println!(
        "{}",
        style(format!(
            "📦 Target: s3://{}/{}",
            s3_client.bucket(),
            config.target_path
        ))
        .cyan()
        .bold()
    );

    let multi = MultiProgress::new();
    let mut stats = Stats::default();

    if cli.url_only {
        // URL-only mode
        println!("{}", style("🔗 Generating pre-signed URLs...").cyan());

        for file_path in files {
            process_url_only(&s3_client, &config, &file_path, &mut stats).await?;
        }

        // Print summary
        println!();
        println!(
            "{}",
            style(format!(
                "Summary: {} URL(s) generated, {} not found",
                stats.urls_generated, stats.not_found
            ))
            .bold()
        );
    } else {
        // Upload mode
        for file_path in files {
            let pb = multi.add(ProgressBar::new(0));
            pb.set_style(
                ProgressStyle::default_bar()
                    .template("{spinner:.green} [{bar:40.cyan/blue}] {bytes}/{total_bytes} {msg}")
                    .unwrap()
                    .progress_chars("#>-"),
            );

            process_upload(&s3_client, &config, &file_path, &pb, &mut stats).await?;

            pb.finish_and_clear();
        }

        // Print summary
        println!();
        println!(
            "{}",
            style(format!(
                "Summary: {} uploaded, {} skipped, {} failed",
                stats.uploaded, stats.skipped, stats.failed
            ))
            .bold()
        );
    }

    Ok(())
}

/// Collect all files to process from the given path, filtered by extensions
fn collect_files(path: &Path, allowed_extensions: &[String]) -> Result<Vec<PathBuf>> {
    let mut files = Vec::new();

    // Normalize extensions to lowercase for case-insensitive matching
    let extensions: Vec<String> = allowed_extensions
        .iter()
        .map(|ext| ext.trim_start_matches('.').to_lowercase())
        .collect();

    if path.is_file() {
        // Check if single file matches allowed extensions
        if let Some(ext) = path.extension() {
            let file_ext = ext.to_string_lossy().to_lowercase();
            if extensions.contains(&file_ext) {
                files.push(path.to_path_buf());
            }
        }
    } else if path.is_dir() {
        for entry in WalkDir::new(path)
            .into_iter()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_type().is_file())
        {
            let entry_path = entry.path();
            if let Some(ext) = entry_path.extension() {
                let file_ext = ext.to_string_lossy().to_lowercase();
                if extensions.contains(&file_ext) {
                    files.push(entry_path.to_path_buf());
                }
            }
        }
    } else {
        anyhow::bail!("Path does not exist: {}", path.display());
    }

    Ok(files)
}

/// Get relative path for S3 key construction
fn get_relative_path(base: &Path, file: &Path) -> Result<String> {
    if base.is_file() {
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

/// Format file size for display
fn format_size(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = KB * 1024;
    const GB: u64 = MB * 1024;

    if bytes >= GB {
        format!("{:.2} GB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.2} MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.2} KB", bytes as f64 / KB as f64)
    } else {
        format!("{} B", bytes)
    }
}

/// Process a file in upload mode
async fn process_upload(
    s3_client: &S3Client,
    config: &Config,
    file_path: &Path,
    pb: &ProgressBar,
    stats: &mut Stats,
) -> Result<()> {
    let base_path = Cli::parse().path;
    let relative_path = get_relative_path(&base_path, file_path)?;
    let s3_key = config.build_s3_key(&relative_path);

    // Get file size for display
    let metadata = tokio::fs::metadata(file_path).await?;
    let file_size = metadata.len();
    let size_str = format_size(file_size);

    // Compare with remote
    let comparison =
        compare_file(s3_client.client(), s3_client.bucket(), &s3_key, file_path).await?;

    match comparison {
        s3::FileComparison::Identical => {
            // Skip upload
            println!(
                "{} {} ({})",
                style("↻").yellow(),
                style(&relative_path).dim(),
                style(format!("skipped - identical, {}", size_str)).dim()
            );

            // Generate pre-signed URL
            let url =
                generate_presigned_url(s3_client.client(), s3_client.bucket(), &s3_key).await?;
            println!("  {} {}", style("🔗").blue(), style(&url).dim());

            stats.skipped += 1;
        }
        s3::FileComparison::NotFound | s3::FileComparison::Different => {
            // Upload file
            match upload_file(
                s3_client.client(),
                s3_client.bucket(),
                &s3_key,
                file_path,
                Some(pb),
            )
            .await
            {
                Ok(UploadResult::Uploaded) => {
                    println!(
                        "{} {} ({})",
                        style("✓").green(),
                        style(&relative_path).green(),
                        style(size_str).dim()
                    );

                    // Generate pre-signed URL
                    let url =
                        generate_presigned_url(s3_client.client(), s3_client.bucket(), &s3_key)
                            .await?;
                    println!("  {} {}", style("🔗").blue(), style(&url).dim());

                    stats.uploaded += 1;
                }
                Ok(UploadResult::Skipped) => {
                    stats.skipped += 1;
                }
                Err(e) => {
                    println!(
                        "{} {} - {}",
                        style("✗").red(),
                        style(&relative_path).red(),
                        style(e).red()
                    );
                    stats.failed += 1;
                }
            }
        }
    }

    Ok(())
}

/// Process a file in URL-only mode
async fn process_url_only(
    s3_client: &S3Client,
    config: &Config,
    file_path: &Path,
    stats: &mut Stats,
) -> Result<()> {
    let base_path = Cli::parse().path;
    let relative_path = get_relative_path(&base_path, file_path)?;
    let s3_key = config.build_s3_key(&relative_path);

    // Check if file exists on S3
    let head_result = s3_client
        .client()
        .head_object()
        .bucket(s3_client.bucket())
        .key(&s3_key)
        .send()
        .await;

    match head_result {
        Ok(_) => {
            // File exists, generate URL
            let url =
                generate_presigned_url(s3_client.client(), s3_client.bucket(), &s3_key).await?;

            println!("{} {}", style("✓").green(), style(&relative_path).green());
            println!("  {} {}", style("🔗").blue(), style(&url).dim());

            stats.urls_generated += 1;
        }
        Err(_) => {
            // File doesn't exist
            println!(
                "{} {} {}",
                style("⚠").yellow(),
                style(&relative_path).yellow(),
                style("(not found on S3)").dim()
            );

            stats.not_found += 1;
        }
    }

    Ok(())
}
