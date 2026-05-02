use anyhow::{anyhow, Result};
use futures_util::stream::StreamExt;
use reqwest::Client;
use std::io::Write;
use crate::models::types::{ChatCompletionRequest, Message};

const OPENROUTER_API_URL: &str = "https://openrouter.ai/api/v1/chat/completions";

pub struct OpenRouterClient {
    client: Client,
    api_key: String,
}

impl OpenRouterClient {
    pub fn new(api_key: String) -> Self {
        Self {
            client: Client::new(),
            api_key,
        }
    }

    pub async fn create_chat_completion<S>(
        &self,
        model_id: &str,
        prompt: S,
    ) -> Result<()>
    where
        S: Into<String>,
    {
        let request = ChatCompletionRequest {
            model: model_id.to_string(),
            messages: vec![Message {
                role: "user".to_string(),
                content: prompt.into(),
            }],
            stream: true,
        };

        let response = self.client
            .post(OPENROUTER_API_URL)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .json(&request)
            .send()
            .await
            .map_err(|e| anyhow!("OpenRouter request failed: {}", e))?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(anyhow!("OpenRouter API error: HTTP {}: {}", status, body));
        }

        let mut stream = response.bytes_stream();
        let mut buffer = String::new();

        while let Some(chunk) = stream.next().await {
            let bytes = chunk.map_err(|e| anyhow!("Error parsing stream: {}", e))?;
            buffer.push_str(&String::from_utf8_lossy(&bytes));

            let line_count = buffer.matches('\n').count();
            if line_count == 0 {
                continue;
            }

            let last_newline_pos = buffer.rfind('\n').unwrap_or(0);
            let is_complete = last_newline_pos == buffer.len() - 1;

            if is_complete {
                let complete_buffer = buffer.clone();
                buffer.clear();
                for line in complete_buffer.lines() {
                    process_line(line)?;
                }
            } else {
                let complete_part = buffer[..last_newline_pos].to_string();
                buffer = buffer[last_newline_pos + 1..].to_string();
                for line in complete_part.lines() {
                    process_line(line)?;
                }
            }
        }

        Ok(())
    }
}

fn process_line(line: &str) -> Result<()> {
    let line = line.trim();
    if !line.starts_with("data:") {
        return Ok(());
    }

    let data = line.strip_prefix("data:").unwrap().trim();

    if data == "[DONE]" {
        std::process::exit(0);
    }

    if data.is_empty() || data.starts_with(':') {
        return Ok(());
    }

    let response: crate::models::types::ChatCompletionResponse =
        serde_json::from_str(data)
            .map_err(|e| anyhow!("Error parsing stream: {}", e))?;

    if let Some(choice) = response.choices.first() {
        if let Some(content) = &choice.delta.content {
            print!("{}", content);
            std::io::stdout().flush()
                .map_err(|e| anyhow!("Error parsing stream: {}", e))?;
        }
    }

    Ok(())
}