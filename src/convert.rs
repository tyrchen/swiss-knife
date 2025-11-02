use anyhow::{Context, Result};
use clap::Parser;
use console::{style, Emoji};
use indicatif::{MultiProgress, ProgressBar, ProgressStyle};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;
use swiss_knife::{ContentResponse, OpenAIClient};
use tokio::sync::mpsc;
use tokio::task;

static MOVIE: Emoji<'_, '_> = Emoji("🎬 ", "");
static SPARKLES: Emoji<'_, '_> = Emoji("✨ ", "");
static CHECK: Emoji<'_, '_> = Emoji("✅ ", "");
static PACKAGE: Emoji<'_, '_> = Emoji("📦 ", "");
static WARNING: Emoji<'_, '_> = Emoji("⚠️  ", "");

#[derive(Parser, Debug)]
#[command(
    name = "convert",
    version = env!("CARGO_PKG_VERSION"),
    author = "Tyr Chen <tyr.chen@gmail.com>",
    about = "Video transcription and AI-powered content generation",
    long_about = "Extract audio from videos, transcribe using OpenAI Whisper, and generate content with GPT. \
                  Automatically handles long videos by splitting into chunks and processing in parallel. \
                  Supports caching to avoid reprocessing.",
    after_help = "Examples:\n  \
                  convert ./lecture.mp4                   # Transcribe and generate content\n  \
                  convert ~/Videos/presentation.mov       # Process video file\n\n\
                  Requirements:\n  \
                  - FFmpeg and FFprobe installed\n  \
                  - OPENAI_API_KEY environment variable set\n\n\
                  Features:\n  \
                  - Automatic chunking for long videos (>1300s)\n  \
                  - Parallel processing of chunks\n  \
                  - Smart caching to avoid reprocessing\n  \
                  - Audio compression for large files\n  \
                  - Real-time progress tracking\n\n\
                  For more information: https://github.com/tyrchen/swiss-knife"
)]
struct Args {
    /// Video file to process
    #[arg(value_name = "VIDEO_FILE")]
    video_file: PathBuf,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();

    if !args.video_file.exists() {
        anyhow::bail!("Video file does not exist: {:?}", args.video_file);
    }

    let video_name = args
        .video_file
        .file_stem()
        .context("Invalid video filename")?
        .to_string_lossy()
        .to_string();

    println!(
        "{} {}",
        MOVIE,
        style(format!("Processing video: {:?}", args.video_file)).bold()
    );
    println!();

    // Get video duration
    let spinner = ProgressBar::new_spinner();
    spinner.set_style(
        ProgressStyle::default_spinner()
            .template("{spinner:.green} {msg}")
            .unwrap(),
    );
    spinner.set_message("Analyzing video duration...");
    spinner.enable_steady_tick(Duration::from_millis(100));

    let duration = get_video_duration(&args.video_file)?;
    spinner.finish_with_message(format!(
        "Video duration: {} seconds",
        style(duration).cyan()
    ));

    let tmp_dir = PathBuf::from("/tmp");
    let transcript_file = tmp_dir.join(format!("{}_transcript.txt", video_name));

    // Process audio extraction and transcription
    let full_transcript = if duration > 1300 {
        process_long_video(&args.video_file, &video_name, duration, &tmp_dir).await?
    } else {
        process_short_video(&args.video_file, &video_name, &tmp_dir).await?
    };

    // Save full transcript
    fs::write(&transcript_file, &full_transcript)?;
    println!(
        "{} Transcript saved to: {}",
        CHECK,
        style(transcript_file.display()).dim()
    );
    println!();

    // Generate content
    let spinner = ProgressBar::new_spinner();
    spinner.set_style(
        ProgressStyle::default_spinner()
            .template("{spinner:.green} {msg}")
            .unwrap(),
    );
    spinner.set_message("Generating content with GPT-5-mini...");
    spinner.enable_steady_tick(Duration::from_millis(100));

    let content = generate_content_from_transcript(&full_transcript).await?;
    spinner.finish_with_message(format!("{} Content generated successfully!", CHECK));

    // Save all outputs
    save_outputs(&video_name, &tmp_dir, &content)?;

    println!();
    println!(
        "{} {}",
        SPARKLES,
        style("Processing complete!").green().bold()
    );
    println!("{} All files saved in {}", PACKAGE, style("/tmp").yellow());

    Ok(())
}

fn get_video_duration(video_path: &Path) -> Result<u32> {
    let output = Command::new("ffprobe")
        .args([
            "-v",
            "error",
            "-show_entries",
            "format=duration",
            "-of",
            "default=noprint_wrappers=1:nokey=1",
            video_path.to_str().unwrap(),
        ])
        .output()
        .context("Failed to run ffprobe")?;

    if !output.status.success() {
        anyhow::bail!("ffprobe failed");
    }

    let duration_str = String::from_utf8(output.stdout)?;
    let duration: f64 = duration_str
        .trim()
        .parse()
        .context("Failed to parse video duration")?;

    Ok(duration as u32)
}

async fn process_short_video(
    video_path: &Path,
    video_name: &str,
    tmp_dir: &Path,
) -> Result<String> {
    let audio_file = tmp_dir.join(format!("{}.mp3", video_name));
    let transcript_file = tmp_dir.join(format!("{}_transcript.txt", video_name));

    // Check cache
    if transcript_file.exists() {
        println!("{} Using cached transcript", style("♻️").cyan());
        return fs::read_to_string(&transcript_file).context("Failed to read cached transcript");
    }

    // Extract audio if not exists
    if !audio_file.exists() {
        let spinner = ProgressBar::new_spinner();
        spinner.set_style(
            ProgressStyle::default_spinner()
                .template("{spinner:.green} {msg}")
                .unwrap(),
        );
        spinner.set_message("Extracting audio from video...");
        spinner.enable_steady_tick(Duration::from_millis(100));

        extract_audio(video_path, &audio_file, None, None)?;
        spinner.finish_with_message(format!("{} Audio extracted", CHECK));
    } else {
        println!("{} Using cached audio file", style("♻️").cyan());
    }

    // Check file size and compress if needed
    let audio_data = compress_if_needed(&audio_file).await?;

    // Transcribe
    let spinner = ProgressBar::new_spinner();
    spinner.set_style(
        ProgressStyle::default_spinner()
            .template("{spinner:.green} {msg}")
            .unwrap(),
    );
    spinner.set_message("Transcribing audio with gpt-4o-transcribe...");
    spinner.enable_steady_tick(Duration::from_millis(100));

    let client = OpenAIClient::new()?;
    let transcript = client
        .transcribe(audio_data, &format!("{}.mp3", video_name))
        .await?;

    spinner.finish_with_message(format!("{} Audio transcribed", CHECK));

    Ok(transcript)
}

async fn process_long_video(
    video_path: &Path,
    video_name: &str,
    duration: u32,
    tmp_dir: &Path,
) -> Result<String> {
    println!(
        "{} Video longer than 1300 seconds, processing in chunks...",
        WARNING
    );

    let num_chunks = duration.div_ceil(1300);
    println!("   Will create {} chunks", style(num_chunks).cyan().bold());
    println!();

    let (tx, mut rx) = mpsc::channel(num_chunks as usize);
    let client = OpenAIClient::new()?;

    // Create multi-progress bar
    let multi_progress = MultiProgress::new();
    let overall_progress = multi_progress.add(ProgressBar::new(num_chunks as u64));
    overall_progress.set_style(
        ProgressStyle::default_bar()
            .template("{spinner:.green} [{bar:40.cyan/blue}] {pos}/{len} chunks processed")
            .unwrap()
            .progress_chars("#>-"),
    );
    overall_progress.set_message("Processing chunks");

    // Process chunks concurrently
    let mut handles = Vec::new();

    for i in 0..num_chunks {
        let tx = tx.clone();
        let client = client.clone();
        let video_path = video_path.to_path_buf();
        let video_name = video_name.to_string();
        let tmp_dir = tmp_dir.to_path_buf();
        let chunk_progress = multi_progress.add(ProgressBar::new_spinner());
        chunk_progress.set_style(
            ProgressStyle::default_spinner()
                .template("    {spinner:.green} Chunk {msg}")
                .unwrap(),
        );

        let handle = task::spawn(async move {
            chunk_progress.set_message(format!("{}/{}: Starting...", i + 1, num_chunks));
            chunk_progress.enable_steady_tick(Duration::from_millis(100));

            let result = process_chunk(
                &video_path,
                &video_name,
                i,
                duration,
                &tmp_dir,
                &client,
                &chunk_progress,
            )
            .await;

            chunk_progress.finish_and_clear();
            tx.send((i, result)).await.unwrap();
        });

        handles.push(handle);
    }

    // Drop the original sender
    drop(tx);

    // Collect results
    let mut chunks = Vec::new();
    while let Some((index, result)) = rx.recv().await {
        match result {
            Ok(transcript) => {
                chunks.push((index, transcript));
                overall_progress.inc(1);
            }
            Err(e) => anyhow::bail!("Failed to process chunk {}: {}", index, e),
        }
    }

    // Wait for all tasks
    for handle in handles {
        handle.await?;
    }

    overall_progress.finish_with_message("All chunks processed!");

    // Sort chunks by index and combine
    chunks.sort_by_key(|c| c.0);
    let full_transcript = chunks
        .into_iter()
        .map(|(_, transcript)| transcript)
        .collect::<Vec<_>>()
        .join(" ");

    println!("{} All chunks merged into complete transcript", CHECK);
    Ok(full_transcript)
}

async fn process_chunk(
    video_path: &Path,
    video_name: &str,
    chunk_index: u32,
    total_duration: u32,
    tmp_dir: &Path,
    client: &OpenAIClient,
    progress: &ProgressBar,
) -> Result<String> {
    let start_time = chunk_index * 1300;
    let mut chunk_duration = 1300;

    if start_time + chunk_duration > total_duration {
        chunk_duration = total_duration - start_time;
    }

    progress.set_message(format!(
        "{}/{}: Processing ({}-{}s)",
        chunk_index + 1,
        (total_duration.div_ceil(1300)),
        start_time,
        start_time + chunk_duration
    ));

    let chunk_audio_file = tmp_dir.join(format!("{}_chunk_{}.mp3", video_name, chunk_index));
    let chunk_transcript_file = tmp_dir.join(format!(
        "{}_chunk_{}_transcript.txt",
        video_name, chunk_index
    ));

    // Check cache
    if chunk_transcript_file.exists() {
        progress.set_message(format!(
            "{}/{}: Using cached transcript",
            chunk_index + 1,
            (total_duration.div_ceil(1300))
        ));
        return fs::read_to_string(&chunk_transcript_file)
            .context("Failed to read cached chunk transcript");
    }

    // Extract audio chunk if not exists
    if !chunk_audio_file.exists() {
        progress.set_message(format!(
            "{}/{}: Extracting audio",
            chunk_index + 1,
            (total_duration.div_ceil(1300))
        ));
        extract_audio(
            video_path,
            &chunk_audio_file,
            Some(start_time),
            Some(chunk_duration),
        )?;
    }

    // Compress if needed and transcribe
    progress.set_message(format!(
        "{}/{}: Transcribing",
        chunk_index + 1,
        (total_duration.div_ceil(1300))
    ));
    let audio_data = compress_if_needed(&chunk_audio_file).await?;
    let transcript = client
        .transcribe(
            audio_data,
            &format!("{}_chunk_{}.mp3", video_name, chunk_index),
        )
        .await?;

    // Save chunk transcript
    fs::write(&chunk_transcript_file, &transcript)?;
    progress.set_message(format!(
        "{}/{}: Completed",
        chunk_index + 1,
        (total_duration.div_ceil(1300))
    ));

    Ok(transcript)
}

fn extract_audio(
    video_path: &Path,
    output_path: &Path,
    start_time: Option<u32>,
    duration: Option<u32>,
) -> Result<()> {
    let mut cmd = Command::new("ffmpeg");
    cmd.arg("-i").arg(video_path);

    if let Some(start) = start_time {
        cmd.arg("-ss").arg(start.to_string());
    }

    if let Some(dur) = duration {
        cmd.arg("-t").arg(dur.to_string());
    }

    cmd.args([
        "-vn", "-acodec", "mp3", "-ab", "32k", "-ar", "16000", "-ac", "1", "-y",
    ])
    .arg(output_path);

    let output = cmd.output().context("Failed to run ffmpeg")?;

    if !output.status.success() {
        anyhow::bail!("ffmpeg failed to extract audio");
    }

    Ok(())
}

async fn compress_if_needed(audio_file: &Path) -> Result<Vec<u8>> {
    let metadata = fs::metadata(audio_file)?;
    let size_mb = metadata.len() / 1024 / 1024;

    if size_mb > 24 {
        let spinner = ProgressBar::new_spinner();
        spinner.set_style(
            ProgressStyle::default_spinner()
                .template("{spinner:.green} {msg}")
                .unwrap(),
        );
        spinner.set_message(format!("Compressing large file ({}MB)...", size_mb));
        spinner.enable_steady_tick(Duration::from_millis(100));

        let compressed_path = audio_file.with_extension("compressed.mp3");

        let output = Command::new("ffmpeg")
            .args([
                "-i",
                audio_file.to_str().unwrap(),
                "-acodec",
                "mp3",
                "-ab",
                "24k",
                "-ar",
                "16000",
                "-ac",
                "1",
                "-y",
                compressed_path.to_str().unwrap(),
            ])
            .output()?;

        if !output.status.success() {
            spinner.finish_with_message("Compression failed");
            anyhow::bail!("Failed to compress audio");
        }

        let data = fs::read(&compressed_path)?;
        fs::remove_file(&compressed_path)?;
        spinner.finish_with_message(format!("Compressed to {}MB", data.len() / 1024 / 1024));
        Ok(data)
    } else {
        fs::read(audio_file).context("Failed to read audio file")
    }
}

async fn generate_content_from_transcript(transcript: &str) -> Result<ContentResponse> {
    let prompt = format!(
        r#"基于以下视频转录内容，请生成：
1. 3个吸引人的标题选项（每个不超过16个字）
2. 2段详细的视频描述（每段300-500字）
3. 3个bilibili动态更新文案（每个150-250字）

请以JSON格式返回，格式如下：
{{
  "titles": ["标题1", "标题2", "标题3"],
  "descriptions": ["描述1", "描述2"],
  "status_updates": ["动态1", "动态2", "动态3"]
}}

转录内容：
{}"#,
        transcript
    );

    let client = OpenAIClient::new()?;
    client.generate_content(prompt).await
}

fn save_outputs(video_name: &str, tmp_dir: &Path, content: &ContentResponse) -> Result<()> {
    let spinner = ProgressBar::new_spinner();
    spinner.set_style(
        ProgressStyle::default_spinner()
            .template("{spinner:.green} {msg}")
            .unwrap(),
    );
    spinner.set_message("Saving output files...");
    spinner.enable_steady_tick(Duration::from_millis(100));

    // Save JSON
    let content_file = tmp_dir.join(format!("{}_content.json", video_name));
    let json = serde_json::to_string_pretty(content)?;
    fs::write(&content_file, json)?;

    // Save titles
    let titles_file = tmp_dir.join(format!("{}_titles.txt", video_name));
    let titles = content
        .titles
        .iter()
        .enumerate()
        .map(|(i, title)| format!("{}. {}", i + 1, title))
        .collect::<Vec<_>>()
        .join("\n");
    fs::write(&titles_file, titles)?;

    // Save descriptions
    let descriptions_file = tmp_dir.join(format!("{}_descriptions.txt", video_name));
    let descriptions = content
        .descriptions
        .iter()
        .enumerate()
        .map(|(i, desc)| format!("=== 描述 {} ===\n{}\n", i + 1, desc))
        .collect::<Vec<_>>()
        .join("\n");
    fs::write(&descriptions_file, descriptions)?;

    // Save status updates
    let status_file = tmp_dir.join(format!("{}_status.txt", video_name));
    let status_updates = content
        .status_updates
        .iter()
        .enumerate()
        .map(|(i, status)| format!("=== 动态 {} ===\n{}\n", i + 1, status))
        .collect::<Vec<_>>()
        .join("\n");
    fs::write(&status_file, status_updates)?;

    spinner.finish_with_message("All files saved!");
    println!();

    println!("{} {}:", style("Generated files").bold(), PACKAGE);
    println!(
        "  📝 Transcript: {}",
        style(
            tmp_dir
                .join(format!("{}_transcript.txt", video_name))
                .display()
        )
        .dim()
    );
    println!("  📋 Full content: {}", style(content_file.display()).dim());
    println!("  🏷️ Titles: {}", style(titles_file.display()).dim());
    println!(
        "  📄 Descriptions: {}",
        style(descriptions_file.display()).dim()
    );
    println!(
        "  💬 Status updates: {}",
        style(status_file.display()).dim()
    );
    println!();

    // Display preview of titles
    println!("{}", style("Generated titles:").bold().cyan());
    for (i, title) in content.titles.iter().enumerate() {
        println!("  {}. {}", style(i + 1).dim(), style(title).green());
    }

    Ok(())
}
