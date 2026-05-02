use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
pub struct ShirManModelsResponse {
    pub models: Vec<ShirManModel>,
}

#[derive(Debug, Deserialize)]
pub struct ShirManModel {
    pub id: String,
    pub name: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct ChatCompletionRequest {
    pub model: String,
    pub messages: Vec<Message>,
    pub stream: bool,
}

#[derive(Debug, Serialize)]
pub struct Message {
    pub role: String,
    pub content: String,
}

#[derive(Debug, Deserialize)]
pub struct ChatCompletionResponse {
    pub choices: Vec<Choice>,
}

#[derive(Debug, Deserialize)]
pub struct Choice {
    pub delta: Delta,
}

#[derive(Debug, Deserialize)]
pub struct Delta {
    #[serde(default)]
    pub content: Option<String>,
}
