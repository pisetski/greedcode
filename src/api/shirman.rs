use anyhow::{anyhow, Result};
use reqwest::Client;
use crate::models::types::{ShirManModel, ShirManModelsResponse};

const SHIR_MAN_API_URL: &str = "https://shir-man.com/api/free-llm/top-models";

pub struct ShirManClient {
    client: Client,
}

impl ShirManClient {
    pub fn new() -> Self {
        Self {
            client: Client::new(),
        }
    }

    pub async fn fetch_top_model(&self) -> Result<ShirManModel> {
        let response = self.client
            .get(SHIR_MAN_API_URL)
            .send()
            .await
            .map_err(|e| anyhow!("Error fetching models: {}", e))?;

        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(anyhow!("Error fetching models: HTTP {}: {}", status, body));
        }

        let wrapper: ShirManModelsResponse = response
            .json()
            .await
            .map_err(|e| anyhow!("Error fetching models: {}", e))?;

        wrapper.models
            .into_iter()
            .next()
            .ok_or_else(|| anyhow!("No free models available"))
    }
}