use crate::models::types::{ChatCompletionRequest, Message};
use anyhow::{Result, anyhow};
use futures_util::stream::StreamExt;
use reqwest::Client;
use std::io::Write;

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

    pub async fn create_chat_completion<S>(&self, model_id: &str, prompt: S) -> Result<()>
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

        let response = self
            .client
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
        let mut line_buffer = LineBuffer::new();

        while let Some(chunk) = stream.next().await {
            let bytes = chunk.map_err(|e| anyhow!("Error parsing stream: {}", e))?;
            let lines = line_buffer.push(&bytes)?;

            for line in lines {
                match process_sse_line(&line)? {
                    SseEvent::Content(content) => {
                        print!("{}", content);
                        std::io::stdout()
                            .flush()
                            .map_err(|e| anyhow!("Error writing output: {}", e))?;
                    }
                    SseEvent::Done => return Ok(()),
                    SseEvent::Ignore => {}
                }
            }
        }

        // Process any remaining data in the buffer after the stream ends.
        if let Some(line) = line_buffer.flush()? {
            match process_sse_line(&line)? {
                SseEvent::Content(content) => {
                    print!("{}", content);
                    std::io::stdout()
                        .flush()
                        .map_err(|e| anyhow!("Error writing output: {}", e))?;
                }
                SseEvent::Done => return Ok(()),
                SseEvent::Ignore => {}
            }
        }

        Ok(())
    }
}

#[derive(Debug, PartialEq)]
enum SseEvent {
    Content(String),
    Done,
    Ignore,
}

fn process_sse_line(line: &str) -> Result<SseEvent> {
    let line = line.trim();
    if !line.starts_with("data:") {
        return Ok(SseEvent::Ignore);
    }

    let data = line.strip_prefix("data:").unwrap().trim();

    if data == "[DONE]" {
        return Ok(SseEvent::Done);
    }

    if data.is_empty() || data.starts_with(':') {
        return Ok(SseEvent::Ignore);
    }

    let response: crate::models::types::ChatCompletionResponse =
        serde_json::from_str(data).map_err(|e| anyhow!("Error parsing stream: {}", e))?;

    if let Some(choice) = response.choices.first()
        && let Some(content) = &choice.delta.content
    {
        return Ok(SseEvent::Content(content.clone()));
    }

    Ok(SseEvent::Ignore)
}

struct LineBuffer {
    buffer: Vec<u8>,
}

impl LineBuffer {
    fn new() -> Self {
        Self { buffer: Vec::new() }
    }

    fn push(&mut self, bytes: &[u8]) -> Result<Vec<String>> {
        self.buffer.extend_from_slice(bytes);

        let mut lines = Vec::new();

        while let Some(pos) = self.buffer.iter().position(|byte| *byte == b'\n') {
            let line_bytes: Vec<u8> = self.buffer.drain(..=pos).collect();
            let line = String::from_utf8(line_bytes[..line_bytes.len() - 1].to_vec())
                .map_err(|e| anyhow!("Error parsing stream: {}", e))?;
            lines.push(line);
        }

        Ok(lines)
    }

    fn flush(&mut self) -> Result<Option<String>> {
        if self.buffer.is_empty() {
            Ok(None)
        } else {
            let line = String::from_utf8(std::mem::take(&mut self.buffer))
                .map_err(|e| anyhow!("Error parsing stream: {}", e))?;
            let line = line.trim_end().to_string();
            self.buffer.clear();
            if line.is_empty() {
                Ok(None)
            } else {
                Ok(Some(line))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_process_sse_line_content() {
        let line = r#"data: {"choices":[{"delta":{"content":"hello"}}]}"#;
        let event = process_sse_line(line).unwrap();
        assert_eq!(event, SseEvent::Content("hello".to_string()));
    }

    #[test]
    fn test_process_sse_line_done() {
        let line = "data: [DONE]";
        let event = process_sse_line(line).unwrap();
        assert_eq!(event, SseEvent::Done);
    }

    #[test]
    fn test_process_sse_line_done_with_whitespace() {
        let line = "data:   [DONE]  ";
        let event = process_sse_line(line).unwrap();
        assert_eq!(event, SseEvent::Done);
    }

    #[test]
    fn test_process_sse_line_empty_delta() {
        let line = r#"data: {"choices":[{"delta":{}}]}"#;
        let event = process_sse_line(line).unwrap();
        assert_eq!(event, SseEvent::Ignore);
    }

    #[test]
    fn test_process_sse_line_event_field() {
        let line = "event: message";
        let event = process_sse_line(line).unwrap();
        assert_eq!(event, SseEvent::Ignore);
    }

    #[test]
    fn test_process_sse_line_comment() {
        let line = ": this is a comment";
        let event = process_sse_line(line).unwrap();
        assert_eq!(event, SseEvent::Ignore);
    }

    #[test]
    fn test_process_sse_line_blank() {
        let line = "";
        let event = process_sse_line(line).unwrap();
        assert_eq!(event, SseEvent::Ignore);
    }

    #[test]
    fn test_process_sse_line_empty_data() {
        let line = "data: ";
        let event = process_sse_line(line).unwrap();
        assert_eq!(event, SseEvent::Ignore);
    }

    #[test]
    fn test_process_sse_line_malformed_json() {
        let line = r#"data: {"choices": [{"delta": {"content"#;
        let result = process_sse_line(line);
        assert!(result.is_err());
    }

    #[test]
    fn test_process_sse_line_no_choices() {
        let line = r#"data: {"choices":[]}"#;
        let event = process_sse_line(line).unwrap();
        assert_eq!(event, SseEvent::Ignore);
    }

    #[test]
    fn test_process_sse_line_multiple_content_fields() {
        // Only the first choice's content should be emitted.
        let line = r#"data: {"choices":[{"delta":{"content":"a"}},{"delta":{"content":"b"}}]}"#;
        let event = process_sse_line(line).unwrap();
        assert_eq!(event, SseEvent::Content("a".to_string()));
    }

    #[test]
    fn test_line_buffer_complete_lines() {
        let mut buf = LineBuffer::new();
        let lines = buf.push(b"line1\nline2\n").unwrap();
        assert_eq!(lines, vec!["line1".to_string(), "line2".to_string()]);
        assert!(buf.flush().unwrap().is_none());
    }

    #[test]
    fn test_line_buffer_partial_line() {
        let mut buf = LineBuffer::new();
        let lines = buf.push(b"part").unwrap();
        assert!(lines.is_empty());

        let lines = buf.push(b"ial\n").unwrap();
        assert_eq!(lines, vec!["partial".to_string()]);
    }

    #[test]
    fn test_line_buffer_multiple_chunks() {
        let mut buf = LineBuffer::new();
        let lines = buf.push(b"li").unwrap();
        assert!(lines.is_empty());

        let lines = buf.push(b"ne1\nli").unwrap();
        assert_eq!(lines, vec!["line1".to_string()]);

        let lines = buf.push(b"ne2\n").unwrap();
        assert_eq!(lines, vec!["line2".to_string()]);
        assert!(buf.flush().unwrap().is_none());
    }

    #[test]
    fn test_line_buffer_empty_chunk() {
        let mut buf = LineBuffer::new();
        let lines = buf.push(b"").unwrap();
        assert!(lines.is_empty());
        assert!(buf.flush().unwrap().is_none());
    }

    #[test]
    fn test_line_buffer_flush_trailing() {
        let mut buf = LineBuffer::new();
        let lines = buf.push(b"line1\n").unwrap();
        assert_eq!(lines, vec!["line1".to_string()]);

        let trailing = buf.push(b"trailing").unwrap();
        assert!(trailing.is_empty());

        assert_eq!(buf.flush().unwrap(), Some("trailing".to_string()));
    }

    #[test]
    fn test_line_buffer_flush_empty() {
        let mut buf = LineBuffer::new();
        let lines = buf.push(b"line1\n").unwrap();
        assert_eq!(lines, vec!["line1".to_string()]);
        assert!(buf.flush().unwrap().is_none());
    }

    #[test]
    fn test_line_buffer_flush_whitespace_only() {
        let mut buf = LineBuffer::new();
        buf.push(b"line1\n").unwrap();
        buf.push(b"   ").unwrap();
        assert!(buf.flush().unwrap().is_none());
    }

    #[test]
    fn test_line_buffer_crlf() {
        let mut buf = LineBuffer::new();
        let lines = buf.push(b"line1\r\nline2\n").unwrap();
        // \r is preserved as part of the line content; parser trims it later.
        assert_eq!(lines, vec!["line1\r".to_string(), "line2".to_string()]);
    }

    #[test]
    fn test_line_buffer_emoji_split() {
        let mut buf = LineBuffer::new();
        // Emoji bytes split across two chunks
        let lines = buf.push(b"Hello \xf0").unwrap();
        assert!(lines.is_empty());
        let lines = buf.push(b"\x9f\x98\x80\n").unwrap();
        assert_eq!(lines, vec!["Hello \u{1f600}".to_string()]);
    }
}
