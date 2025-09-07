use anyhow::{Context, Result};
use reqwest::multipart;
use serde::{Deserialize, Serialize};
use std::env;

#[derive(Clone)]
pub struct OpenAIClient {
    client: reqwest::Client,
    api_key: String,
    base_url: String,
}

#[derive(Deserialize)]
pub struct TranscriptionResponse {
    pub text: String,
}

#[derive(Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}

#[derive(Serialize)]
pub struct ChatRequest {
    pub model: String,
    pub messages: Vec<ChatMessage>,
    pub temperature: f32,
    pub max_completion_tokens: u32,
    pub response_format: ResponseFormat,
}

#[derive(Serialize)]
pub struct ResponseFormat {
    #[serde(rename = "type")]
    pub format_type: String,
}

#[derive(Deserialize)]
pub struct ChatResponse {
    pub choices: Vec<Choice>,
}

#[derive(Deserialize)]
pub struct Choice {
    pub message: ChatMessage,
}

#[derive(Serialize, Deserialize)]
pub struct ContentResponse {
    pub titles: Vec<String>,
    pub descriptions: Vec<String>,
    pub status_updates: Vec<String>,
}

#[derive(Serialize)]
pub struct ImageGenerationRequest {
    pub model: String,
    pub prompt: String,
    pub n: u32,
    pub size: String,
}

#[derive(Deserialize)]
pub struct ImageGenerationResponse {
    pub data: Vec<ImageData>,
}

#[derive(Deserialize)]
pub struct ImageData {
    pub b64_json: String,
}

impl OpenAIClient {
    pub fn new() -> Result<Self> {
        let api_key =
            env::var("OPENAI_API_KEY").context("OPENAI_API_KEY environment variable not set")?;
        let base_url =
            env::var("OPENAI_BASE_URL").unwrap_or_else(|_| "https://api.openai.com/v1".to_string());

        let client = reqwest::Client::builder().use_rustls_tls().build()?;

        Ok(Self {
            client,
            api_key,
            base_url,
        })
    }

    pub async fn transcribe(&self, audio_data: Vec<u8>, filename: &str) -> Result<String> {
        let url = format!("{}/audio/transcriptions", self.base_url);

        let part = multipart::Part::bytes(audio_data)
            .file_name(filename.to_string())
            .mime_str("audio/mpeg")?;

        let form = multipart::Form::new()
            .part("file", part)
            .text("model", "gpt-4o-transcribe")
            .text("response_format", "json")
            .text("language", "zh");

        let response = self
            .client
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .multipart(form)
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await?;
            anyhow::bail!("API call failed with status {}: {}", status, text);
        }

        let result: TranscriptionResponse = response.json().await?;
        Ok(result.text)
    }

    pub async fn generate_content(&self, prompt: String) -> Result<ContentResponse> {
        let url = format!("{}/chat/completions", self.base_url);

        let system_message = ChatMessage {
            role: "system".to_string(),
            content: "你是一个专业的内容创作助手，擅长为视频内容生成吸引人的标题和描述。请用中文回复，并严格按照JSON格式输出。".to_string(),
        };

        let user_message = ChatMessage {
            role: "user".to_string(),
            content: prompt,
        };

        let request = ChatRequest {
            model: "gpt-5-mini".to_string(),
            messages: vec![system_message, user_message],
            temperature: 1.0,
            max_completion_tokens: 10000,
            response_format: ResponseFormat {
                format_type: "json_object".to_string(),
            },
        };

        let response = self
            .client
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .json(&request)
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await?;
            anyhow::bail!("GPT API call failed with status {}: {}", status, text);
        }

        let chat_response: ChatResponse = response.json().await?;

        if chat_response.choices.is_empty() {
            anyhow::bail!("No response from GPT API");
        }

        let content = &chat_response.choices[0].message.content;
        let content_response: ContentResponse =
            serde_json::from_str(content).context("Failed to parse GPT response as JSON")?;

        Ok(content_response)
    }

    pub async fn generate_image(&self, prompt: &str, size: &str) -> Result<Vec<u8>> {
        let url = format!("{}/images/generations", self.base_url);

        let request = ImageGenerationRequest {
            model: "gpt-image-1".to_string(),
            prompt: prompt.to_string(),
            n: 1,
            size: size.to_string(),
        };

        let response = self
            .client
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .json(&request)
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await?;
            anyhow::bail!(
                "Image generation API call failed with status {}: {}",
                status,
                text
            );
        }

        let result: ImageGenerationResponse = response.json().await?;

        if result.data.is_empty() {
            anyhow::bail!("No images returned from API");
        }

        // Decode base64 to bytes
        use base64::{engine::general_purpose::STANDARD, Engine as _};
        let image_bytes = STANDARD
            .decode(&result.data[0].b64_json)
            .context("Failed to decode base64 image data")?;

        Ok(image_bytes)
    }
}
