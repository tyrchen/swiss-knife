mod config;
mod s3;

use anyhow::{Context, Result};
use clap::Parser;
use console::style;
use indicatif::{MultiProgress, ProgressBar, ProgressStyle};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use tokio::sync::{mpsc, Mutex};
use walkdir::WalkDir;

use config::Config;
use s3::{
    compare::compare_file, generate_presigned_url, upload_file, upload_multipart, S3Client,
    UploadResult, MULTIPART_THRESHOLD,
};
use tracing::{error, info};

// Future use - keeping imports for Phase 5 integration
#[allow(unused_imports)]
use s3::{detect_content_type, generate_presigned_url_with_expiry, parse_metadata, parse_tags};

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

    /// Maximum number of concurrent uploads
    #[arg(long, short = 'c', default_value = "4")]
    max_concurrent: usize,

    /// Perform a dry run (show what would be uploaded without uploading)
    #[arg(long)]
    dry_run: bool,

    /// Pre-signed URL expiration in hours (default: 168 = 7 days, max: 168)
    #[arg(long, default_value = "168")]
    url_expiry_hours: u64,

    /// Custom metadata (key=value pairs, comma-separated)
    #[arg(long)]
    metadata: Option<String>,

    /// Tags (key=value pairs, comma-separated)
    #[arg(long)]
    tags: Option<String>,

    /// Override Content-Type for uploaded files
    #[arg(long)]
    content_type: Option<String>,

    /// Flatten directory structure (remove subdirectories)
    #[arg(long)]
    flatten: bool,

    /// Custom path prefix (overrides S3_TARGET_PATH for this upload)
    #[arg(long)]
    prefix: Option<String>,

    /// Sync mode: delete remote files not present locally
    #[arg(long)]
    sync: bool,

    /// Interactive mode: prompt for conflicts
    #[arg(long, short = 'i')]
    interactive: bool,
}

#[derive(Debug)]
struct Stats {
    uploaded: AtomicUsize,
    skipped: AtomicUsize,
    failed: AtomicUsize,
    urls_generated: AtomicUsize,
    not_found: AtomicUsize,
    total_bytes_uploaded: std::sync::atomic::AtomicU64,
    start_time: std::time::Instant,
}

impl Default for Stats {
    fn default() -> Self {
        Self {
            uploaded: AtomicUsize::new(0),
            skipped: AtomicUsize::new(0),
            failed: AtomicUsize::new(0),
            urls_generated: AtomicUsize::new(0),
            not_found: AtomicUsize::new(0),
            total_bytes_uploaded: std::sync::atomic::AtomicU64::new(0),
            start_time: std::time::Instant::now(),
        }
    }
}

#[derive(Debug, Clone)]
enum ProcessResult {
    Uploaded {
        filename: String,
        size: String,
        url: String,
    },
    Skipped {
        filename: String,
        size: String,
        url: String,
    },
    Failed {
        filename: String,
        error: String,
    },
    UrlGenerated {
        filename: String,
        url: String,
    },
    NotFound {
        filename: String,
    },
}

impl Stats {
    fn print_upload_summary(&self) {
        let duration = self.start_time.elapsed();
        let total_bytes = self
            .total_bytes_uploaded
            .load(std::sync::atomic::Ordering::Relaxed);
        let uploaded_count = self.uploaded.load(Ordering::Relaxed);
        let skipped_count = self.skipped.load(Ordering::Relaxed);
        let failed_count = self.failed.load(Ordering::Relaxed);

        println!("\n{}", style("═".repeat(70)).dim());
        println!(
            "{}",
            style(format!(
                "Summary: {} uploaded, {} skipped, {} failed",
                uploaded_count, skipped_count, failed_count
            ))
            .bold()
        );

        if total_bytes > 0 {
            println!(
                "{}",
                style(format!(
                    "Total uploaded: {} ({} bytes)",
                    format_size(total_bytes),
                    total_bytes
                ))
                .dim()
            );
        }

        if duration.as_secs() > 0 {
            let speed = total_bytes as f64 / duration.as_secs_f64() / 1024.0 / 1024.0;
            println!(
                "{}",
                style(format!(
                    "Time: {:.2}s, Average speed: {:.2} MB/s",
                    duration.as_secs_f64(),
                    speed
                ))
                .dim()
            );
        }
    }

    fn print_url_summary(&self) {
        println!(
            "{}",
            style(format!(
                "Summary: {} URL(s) generated, {} not found",
                self.urls_generated.load(Ordering::Relaxed),
                self.not_found.load(Ordering::Relaxed)
            ))
            .bold()
        );
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    // Load .env file early to get LOG_LEVEL
    dotenv::dotenv().ok();

    // Initialize tracing/logging with support for LOG_LEVEL from .env
    let log_level = std::env::var("LOG_LEVEL")
        .ok()
        .or_else(|| std::env::var("RUST_LOG").ok())
        .unwrap_or_else(|| "info".to_string());

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .or_else(|_| tracing_subscriber::EnvFilter::try_new(&log_level))
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .with_target(false)
        .with_level(true)
        .init();

    let cli = Cli::parse();

    info!("S3 Upload Tool v{}", env!("CARGO_PKG_VERSION"));
    info!("Concurrent workers: {}", cli.max_concurrent);

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

    let multi = Arc::new(MultiProgress::new());
    let stats = Arc::new(Stats::default());

    // Handle dry-run mode
    if cli.dry_run {
        println!(
            "{}",
            style("🔍 DRY RUN MODE - No files will be uploaded")
                .yellow()
                .bold()
        );
        println!();

        for file in &files {
            let relative_path = get_relative_path(&cli.path, file, cli.flatten)?;
            let s3_key = if let Some(ref prefix) = cli.prefix {
                format!(
                    "{}/{}",
                    prefix.trim_end_matches('/'),
                    relative_path.trim_start_matches("./")
                )
            } else {
                config.build_s3_key(&relative_path)
            };

            let metadata = tokio::fs::metadata(file).await?;
            let size = format_size(metadata.len());

            // Check if file exists on S3
            let comparison =
                compare_file(s3_client.client(), s3_client.bucket(), &s3_key, file).await?;

            match comparison {
                s3::FileComparison::NotFound => {
                    println!(
                        "  {} {} → s3://{}/{} ({})",
                        style("WOULD UPLOAD").green().bold(),
                        relative_path,
                        s3_client.bucket(),
                        s3_key,
                        size
                    );
                }
                s3::FileComparison::Different => {
                    println!(
                        "  {} {} → s3://{}/{} ({})",
                        style("WOULD UPDATE").yellow().bold(),
                        relative_path,
                        s3_client.bucket(),
                        s3_key,
                        size
                    );
                }
                s3::FileComparison::Identical => {
                    println!(
                        "  {} {} ({})",
                        style("WOULD SKIP").dim(),
                        relative_path,
                        size
                    );
                }
            }
        }

        return Ok(());
    }

    if cli.url_only {
        // URL-only mode - concurrent URL generation using mpsc
        println!(
            "{}",
            style(format!(
                "🔗 Generating pre-signed URLs ({} workers)...",
                cli.max_concurrent
            ))
            .cyan()
        );

        // Create work channel and results channel
        let (work_tx, work_rx) = mpsc::channel::<PathBuf>(100);
        let (result_tx, mut result_rx) = mpsc::channel::<ProcessResult>(100);
        let work_rx = Arc::new(Mutex::new(work_rx));

        // Spawn worker tasks
        let mut workers = Vec::new();
        for _ in 0..cli.max_concurrent {
            let work_rx = Arc::clone(&work_rx);
            let s3_client = s3_client.clone();
            let config = config.clone();
            let stats = Arc::clone(&stats);
            let base_path = cli.path.clone();
            let result_tx = result_tx.clone();

            workers.push(tokio::spawn(async move {
                loop {
                    let file_path = {
                        let mut rx_guard = work_rx.lock().await;
                        rx_guard.recv().await
                    };

                    match file_path {
                        Some(path) => {
                            let result = process_url_only_with_result(
                                &s3_client, &config, &path, &base_path, &stats,
                            )
                            .await;

                            if let Ok(r) = result {
                                let _ = result_tx.send(r).await;
                            }
                        }
                        None => break, // Channel closed
                    }
                }
            }));
        }
        drop(result_tx); // Drop original sender

        // Spawn result collector task
        let collector_handle = tokio::spawn(async move {
            let mut results = Vec::new();
            while let Some(result) = result_rx.recv().await {
                results.push(result);
            }
            results
        });

        // Producer: Send files to channel
        for file_path in files {
            work_tx.send(file_path).await.unwrap();
        }
        drop(work_tx); // Close channel to signal workers to exit

        // Wait for all workers to complete
        for worker in workers {
            if let Err(e) = worker.await {
                eprintln!("{} Worker panic: {}", style("✗").red(), e);
            }
        }

        // Collect and sort results
        let mut results = collector_handle.await.unwrap();
        results.sort_by(|a, b| {
            let a_name = match a {
                ProcessResult::UrlGenerated { filename, .. } => filename,
                ProcessResult::NotFound { filename } => filename,
                _ => "",
            };
            let b_name = match b {
                ProcessResult::UrlGenerated { filename, .. } => filename,
                ProcessResult::NotFound { filename } => filename,
                _ => "",
            };
            a_name.cmp(b_name)
        });

        // Print results
        println!();
        for result in results {
            match result {
                ProcessResult::UrlGenerated { filename, url } => {
                    println!("{} {}", style("✓").green(), style(&filename).green());
                    println!("  {} {}", style("🔗").blue(), style(&url).dim());
                }
                ProcessResult::NotFound { filename } => {
                    println!(
                        "{} {} {}",
                        style("⚠").yellow(),
                        style(&filename).yellow(),
                        style("(not found on S3)").dim()
                    );
                }
                _ => {}
            }
        }

        // Print summary
        println!();
        stats.print_url_summary();
    } else {
        // Upload mode - concurrent uploads using mpsc
        println!(
            "{}",
            style(format!(
                "⚡ Uploading with {} workers...",
                cli.max_concurrent
            ))
            .cyan()
        );

        // Create work channel and results channel
        let (work_tx, work_rx) = mpsc::channel::<PathBuf>(100);
        let (result_tx, mut result_rx) = mpsc::channel::<ProcessResult>(100);
        let work_rx = Arc::new(Mutex::new(work_rx));

        // Spawn worker tasks
        let mut workers = Vec::new();
        for _ in 0..cli.max_concurrent {
            let work_rx = Arc::clone(&work_rx);
            let s3_client = s3_client.clone();
            let config = config.clone();
            let stats = Arc::clone(&stats);
            let multi = Arc::clone(&multi);
            let base_path = cli.path.clone();
            let result_tx = result_tx.clone();

            workers.push(tokio::spawn(async move {
                loop {
                    let file_path = {
                        let mut rx_guard = work_rx.lock().await;
                        rx_guard.recv().await
                    };

                    match file_path {
                        Some(path) => {
                            let pb = multi.add(ProgressBar::new(0));
                            pb.set_style(
                                ProgressStyle::default_bar()
                                    .template(
                                        "{spinner:.green} [{bar:40.cyan/blue}] {bytes}/{total_bytes} {msg}",
                                    )
                                    .unwrap()
                                    .progress_chars("#>-"),
                            );

                            let result = process_upload_with_result(
                                &s3_client,
                                &config,
                                &path,
                                &base_path,
                                &pb,
                                &stats,
                            )
                            .await;

                            pb.finish_and_clear();

                            // Send result to results channel
                            if let Ok(r) = result {
                                let _ = result_tx.send(r).await;
                            }
                        }
                        None => break, // Channel closed
                    }
                }
            }));
        }
        drop(result_tx); // Drop original sender

        // Spawn result collector task
        let collector_handle = tokio::spawn(async move {
            let mut results = Vec::new();
            while let Some(result) = result_rx.recv().await {
                results.push(result);
            }
            results
        });

        // Producer: Send files to channel
        for file_path in files {
            work_tx.send(file_path).await.unwrap();
        }
        drop(work_tx); // Close channel to signal workers to exit

        // Wait for all workers to complete
        for worker in workers {
            if let Err(e) = worker.await {
                eprintln!("{} Worker panic: {}", style("✗").red(), e);
            }
        }

        // Collect and sort results
        let mut results = collector_handle.await.unwrap();
        results.sort_by(|a, b| {
            let a_name = match a {
                ProcessResult::Uploaded { filename, .. } => filename,
                ProcessResult::Skipped { filename, .. } => filename,
                ProcessResult::Failed { filename, .. } => filename,
                _ => "",
            };
            let b_name = match b {
                ProcessResult::Uploaded { filename, .. } => filename,
                ProcessResult::Skipped { filename, .. } => filename,
                ProcessResult::Failed { filename, .. } => filename,
                _ => "",
            };
            a_name.cmp(b_name)
        });

        // Print results
        println!();
        for result in results {
            match result {
                ProcessResult::Uploaded {
                    filename,
                    size,
                    url,
                } => {
                    println!(
                        "{} {} ({})",
                        style("✓").green(),
                        style(&filename).green(),
                        style(size).dim()
                    );
                    println!("  {} {}", style("🔗").blue(), style(&url).dim());
                }
                ProcessResult::Skipped {
                    filename,
                    size,
                    url,
                } => {
                    println!(
                        "{} {} ({})",
                        style("↻").yellow(),
                        style(&filename).dim(),
                        style(format!("skipped - identical, {}", size)).dim()
                    );
                    println!("  {} {}", style("🔗").blue(), style(&url).dim());
                }
                ProcessResult::Failed { filename, error } => {
                    println!(
                        "{} {} - {}",
                        style("✗").red(),
                        style(&filename).red(),
                        style(error).red()
                    );
                }
                _ => {}
            }
        }

        // Print summary
        println!();
        stats.print_upload_summary();
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
///
/// # Arguments
///
/// * `base` - Base path (file or directory)
/// * `file` - File to get relative path for
/// * `flatten` - If true, ignore directory structure
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

/// Process a file in upload mode and return result (for clean output)
async fn process_upload_with_result(
    s3_client: &S3Client,
    config: &Config,
    file_path: &Path,
    base_path: &Path,
    pb: &ProgressBar,
    stats: &Arc<Stats>,
) -> Result<ProcessResult> {
    let relative_path = get_relative_path(base_path, file_path, false)?;
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
            // Generate pre-signed URL
            let url =
                generate_presigned_url(s3_client.client(), s3_client.bucket(), &s3_key).await?;

            stats.skipped.fetch_add(1, Ordering::Relaxed);

            Ok(ProcessResult::Skipped {
                filename: relative_path,
                size: size_str,
                url,
            })
        }
        s3::FileComparison::NotFound | s3::FileComparison::Different => {
            // Choose upload strategy based on file size
            let upload_result = if file_size >= MULTIPART_THRESHOLD {
                info!(
                    "Using multipart upload for large file: {} ({} bytes)",
                    relative_path, file_size
                );
                upload_multipart(
                    s3_client.client(),
                    s3_client.bucket(),
                    &s3_key,
                    file_path,
                    Some(pb),
                )
                .await
                .map(|_| UploadResult::Uploaded)
            } else {
                upload_file(
                    s3_client.client(),
                    s3_client.bucket(),
                    &s3_key,
                    file_path,
                    Some(pb),
                )
                .await
            };

            match upload_result {
                Ok(UploadResult::Uploaded) => {
                    // Generate pre-signed URL
                    let url =
                        generate_presigned_url(s3_client.client(), s3_client.bucket(), &s3_key)
                            .await?;

                    stats.uploaded.fetch_add(1, Ordering::Relaxed);

                    Ok(ProcessResult::Uploaded {
                        filename: relative_path,
                        size: size_str,
                        url,
                    })
                }
                Ok(UploadResult::Skipped) => {
                    stats.skipped.fetch_add(1, Ordering::Relaxed);

                    let url =
                        generate_presigned_url(s3_client.client(), s3_client.bucket(), &s3_key)
                            .await?;

                    Ok(ProcessResult::Skipped {
                        filename: relative_path,
                        size: size_str,
                        url,
                    })
                }
                Err(e) => {
                    error!("Upload failed for {}: {:#}", relative_path, e);
                    stats.failed.fetch_add(1, Ordering::Relaxed);

                    Ok(ProcessResult::Failed {
                        filename: relative_path,
                        error: format!("{:#}", e),
                    })
                }
            }
        }
    }
}

/// Process a file in URL-only mode and return result (for clean output)
async fn process_url_only_with_result(
    s3_client: &S3Client,
    config: &Config,
    file_path: &Path,
    base_path: &Path,
    stats: &Arc<Stats>,
) -> Result<ProcessResult> {
    let relative_path = get_relative_path(base_path, file_path, false)?;
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

            stats.urls_generated.fetch_add(1, Ordering::Relaxed);

            Ok(ProcessResult::UrlGenerated {
                filename: relative_path,
                url,
            })
        }
        Err(_) => {
            stats.not_found.fetch_add(1, Ordering::Relaxed);

            Ok(ProcessResult::NotFound {
                filename: relative_path,
            })
        }
    }
}
