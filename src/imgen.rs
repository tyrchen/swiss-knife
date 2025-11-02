use anyhow::{Context, Result};
use clap::Parser;
use console::style;
use indicatif::{ProgressBar, ProgressStyle};
use serde::{Deserialize, Serialize};
use slug::slugify;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use swiss_knife::OpenAIClient;
use tokio::sync::Semaphore;

const MAX_CONCURRENT_REQUESTS: usize = 32;

#[derive(Parser)]
#[command(
    name = "imgen",
    version = env!("CARGO_PKG_VERSION"),
    author = "Tyr Chen <tyr.chen@gmail.com>",
    about = "Generate images from YAML configuration using OpenAI's DALL-E API",
    long_about = "Batch generate images using OpenAI's DALL-E based on YAML configuration. \
                  Supports multiple themes and prompts, parallel processing (up to 32 concurrent requests), \
                  and automatic caching to skip previously generated images.",
    after_help = "Examples:\n  \
                  imgen config.yaml                       # Generate images from YAML config\n  \
                  imgen themes.yaml                       # Process multiple themes and prompts\n\n\
                  YAML Configuration Format:\n  \
                  system_prompt: \"...\"                    # Base instructions for all images\n  \
                  style: \"minimalist\"                     # Art style to apply\n  \
                  themes:                                 # List of themes\n    \
                  - name: \"Nature\"\n      \
                  instructions: \"...\"\n  \
                  prompts:                                # List of prompts\n    \
                  - name: \"Sunset\"\n      \
                  prompt: \"...\"\n\n\
                  Requirements:\n  \
                  - OPENAI_API_KEY environment variable set\n\n\
                  Features:\n  \
                  - Concurrent image generation (32 max)\n  \
                  - Smart caching (skips existing images)\n  \
                  - Progress tracking with status\n  \
                  - Organized output by theme and prompt\n\n\
                  For more information: https://github.com/tyrchen/swiss-knife"
)]
struct Args {
    /// Path to the YAML configuration file
    #[arg(value_name = "YAML_FILE")]
    yaml_file: PathBuf,
}

#[derive(Serialize, Deserialize, Debug)]
struct Config {
    system_prompt: String,
    style: String,
    themes: Vec<Theme>,
    prompts: Vec<Prompt>,
}

#[derive(Serialize, Deserialize, Debug)]
struct Theme {
    name: String,
    instructions: String,
}

#[derive(Serialize, Deserialize, Debug)]
struct Prompt {
    name: String,
    prompt: String,
}

#[derive(Debug, Clone)]
struct ImageTask {
    theme_name: String,
    prompt_name: String,
    full_prompt: String,
    output_path: PathBuf,
    _hash: String,
    size: String,
}

impl Config {
    fn get_image_size(&self) -> &str {
        match self.style.as_str() {
            "square" => "1024x1024",
            "landscape" => "1536x1024",
            "portrait" => "1024x1536",
            _ => "1024x1024", // default to square
        }
    }
}

fn calculate_hash(system_prompt: &str, theme_instruction: &str, prompt: &str) -> String {
    let combined = format!("{}{}{}", system_prompt, theme_instruction, prompt);
    let hash = blake3::hash(combined.as_bytes());
    format!("{:.6}", hash.to_hex())
}

fn create_output_filename(prompt_name: &str, hash: &str) -> String {
    let slug = slugify(prompt_name);
    format!("{}-{}.png", slug, hash)
}

async fn process_config(config_path: &Path) -> Result<()> {
    // Read and parse YAML config
    let config_content = fs::read_to_string(config_path)
        .with_context(|| format!("Failed to read config file: {}", config_path.display()))?;

    let config: Config =
        serde_yaml::from_str(&config_content).context("Failed to parse YAML configuration")?;

    println!(
        "{}",
        style(format!(
            "📝 Loaded config with {} themes and {} prompts",
            config.themes.len(),
            config.prompts.len()
        ))
        .green()
        .bold()
    );

    // Create OpenAI client
    let client = OpenAIClient::new().context("Failed to create OpenAI client")?;

    // Generate tasks for all theme-prompt combinations
    let mut tasks_by_theme: Vec<Vec<ImageTask>> = Vec::new();
    let image_size = config.get_image_size();

    for theme in &config.themes {
        // Create theme directory
        let theme_dir = Path::new(&theme.name);
        if !theme_dir.exists() {
            fs::create_dir_all(theme_dir)
                .with_context(|| format!("Failed to create directory: {}", theme_dir.display()))?;
        }

        let mut theme_tasks = Vec::new();

        for prompt in &config.prompts {
            // Calculate hash for this combination
            let hash = calculate_hash(&config.system_prompt, &theme.instructions, &prompt.prompt);

            // Create full prompt combining system prompt, theme instructions, and specific prompt
            let full_prompt = format!(
                "{}\n\n{}\n\n{}",
                config.system_prompt, theme.instructions, prompt.prompt
            );

            // Generate output filename and path
            let filename = create_output_filename(&prompt.name, &hash);
            let output_path = theme_dir.join(&filename);

            // Check if image already exists
            if output_path.exists() {
                println!(
                    "{}",
                    style(format!(
                        "⏭️  Skipping existing image: {}",
                        output_path.display()
                    ))
                    .yellow()
                );
                continue;
            }

            theme_tasks.push(ImageTask {
                theme_name: theme.name.clone(),
                prompt_name: prompt.name.clone(),
                full_prompt,
                output_path,
                _hash: hash,
                size: image_size.to_string(),
            });
        }

        if !theme_tasks.is_empty() {
            tasks_by_theme.push(theme_tasks);
        }
    }

    // Interleave tasks from different themes for better distribution
    let mut tasks = Vec::new();
    let max_prompts = tasks_by_theme.iter().map(|t| t.len()).max().unwrap_or(0);

    for i in 0..max_prompts {
        for theme_tasks in &tasks_by_theme {
            if i < theme_tasks.len() {
                tasks.push(theme_tasks[i].clone());
            }
        }
    }

    if tasks.is_empty() {
        println!("{}", style("✅ All images already exist!").green().bold());
        return Ok(());
    }

    println!(
        "{}",
        style(format!("🎨 Generating {} new images...", tasks.len()))
            .cyan()
            .bold()
    );

    // Create progress bar
    let pb = Arc::new(ProgressBar::new(tasks.len() as u64));
    pb.set_style(
        ProgressStyle::with_template(
            "{spinner:.green} [{elapsed_precise}] [{bar:40.cyan/blue}] {pos}/{len} {msg}",
        )?
        .progress_chars("#>-"),
    );
    pb.set_message("Generating images...");
    pb.enable_steady_tick(std::time::Duration::from_millis(100));

    // Use semaphore to limit concurrent requests (OpenAI has rate limits)
    let semaphore = Arc::new(Semaphore::new(MAX_CONCURRENT_REQUESTS));
    let client = Arc::new(client);

    // Create concurrent tasks
    let mut handles = Vec::new();

    for task in tasks {
        let client = Arc::clone(&client);
        let semaphore = Arc::clone(&semaphore);
        let pb_clone = Arc::clone(&pb);
        let theme_name = task.theme_name.clone();
        let prompt_name = task.prompt_name.clone();

        let handle = tokio::spawn(async move {
            // Acquire semaphore permit
            let _permit = semaphore.acquire().await.unwrap();

            // Update progress bar message
            pb_clone.set_message(format!("Processing {}/{}", theme_name, prompt_name));

            let result = generate_and_save_image(&client, &task).await;

            // Update progress
            pb_clone.inc(1);

            (prompt_name, theme_name, result)
        });

        handles.push(handle);
    }

    // Wait for all tasks to complete
    let results = futures::future::join_all(handles).await;

    pb.finish_and_clear();

    // Count successes and failures, and collect errors
    let mut success_count = 0;
    let mut failures = Vec::new();

    for result in results {
        match result {
            Ok((prompt_name, theme_name, Ok(_))) => {
                success_count += 1;
                println!("{}  {}/{}", style("✅").green(), theme_name, prompt_name);
            }
            Ok((prompt_name, theme_name, Err(e))) => {
                failures.push((prompt_name, theme_name, e.to_string()));
            }
            Err(e) => {
                failures.push(("Unknown".to_string(), "Unknown".to_string(), e.to_string()));
            }
        }
    }

    // Print failures if any
    for (prompt_name, theme_name, error) in &failures {
        eprintln!(
            "{}  {}/{}: {}",
            style("❌").red(),
            theme_name,
            prompt_name,
            error
        );
    }

    // Print summary
    println!();
    if failures.is_empty() {
        println!(
            "{}",
            style(format!(
                "🎉 All {} images generated successfully!",
                success_count
            ))
            .green()
            .bold()
        );
    } else {
        println!(
            "{}",
            style(format!(
                "🎉 Image generation completed! Success: {}, Failed: {}",
                success_count,
                failures.len()
            ))
            .yellow()
            .bold()
        );
    }

    Ok(())
}

async fn generate_and_save_image(client: &Arc<OpenAIClient>, task: &ImageTask) -> Result<()> {
    // Generate image (returns bytes directly now)
    let image_data = client
        .generate_image(&task.full_prompt, &task.size)
        .await
        .context("Failed to generate image")?;

    // Save image to file
    fs::write(&task.output_path, image_data)
        .with_context(|| format!("Failed to save image to {}", task.output_path.display()))?;

    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();

    if !args.yaml_file.exists() {
        anyhow::bail!(
            "Configuration file does not exist: {}",
            args.yaml_file.display()
        );
    }

    if let Err(e) = process_config(&args.yaml_file).await {
        eprintln!("{}", style(format!("Error: {}", e)).red().bold());
        std::process::exit(1);
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_calculate_hash() {
        let system_prompt = "test system";
        let theme_instruction = "test theme";
        let prompt = "test prompt";

        let hash1 = calculate_hash(system_prompt, theme_instruction, prompt);
        let hash2 = calculate_hash(system_prompt, theme_instruction, prompt);

        // Same inputs should produce same hash
        assert_eq!(hash1, hash2);
        assert_eq!(hash1.len(), 6); // Should be 6 characters

        // Different inputs should produce different hash
        let hash3 = calculate_hash("different", theme_instruction, prompt);
        assert_ne!(hash1, hash3);
    }

    #[test]
    fn test_create_output_filename() {
        let filename = create_output_filename("Memory Safety", "abc123");
        assert_eq!(filename, "memory-safety-abc123.png");

        let filename2 = create_output_filename("Concurrency-Safety", "def456");
        assert_eq!(filename2, "concurrency-safety-def456.png");
    }

    #[test]
    fn test_config_image_size() {
        let mut config = Config {
            system_prompt: "test".to_string(),
            style: "square".to_string(),
            themes: vec![],
            prompts: vec![],
        };

        assert_eq!(config.get_image_size(), "1024x1024");

        config.style = "landscape".to_string();
        assert_eq!(config.get_image_size(), "1536x1024");

        config.style = "portrait".to_string();
        assert_eq!(config.get_image_size(), "1024x1536");

        config.style = "unknown".to_string();
        assert_eq!(config.get_image_size(), "1024x1024"); // default
    }
}
