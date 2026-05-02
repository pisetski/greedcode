use std::fs;
use std::io::{self, BufRead};

use anyhow::Result;
use clap::Parser;

mod api;
mod models;
mod output;

use api::openrouter::OpenRouterClient;
use api::shirman::ShirManClient;

fn load_dotenv() {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let env_path = format!("{}/.env", manifest_dir);

    if let Ok(file) = fs::File::open(&env_path) {
        let reader = io::BufReader::new(file);
        for line in reader.lines().map_while(Result::ok) {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            if let Some((key, value)) = line.split_once('=') {
                let key = key.trim();
                let value = value.trim();
                unsafe {
                    std::env::set_var(key, value);
                }
            }
        }
    }
}

#[derive(Parser)]
#[command(version, about)]
struct Cli {
    #[arg(help = "Prompt to send to the model")]
    prompt: Vec<String>,
}

#[tokio::main]
async fn main() -> Result<()> {
    load_dotenv();

    let cli = Cli::parse();

    if cli.prompt.is_empty() {
        eprintln!("Usage: greedcode \"<prompt>\"");
        std::process::exit(2);
    }

    let api_key = std::env::var("OPENROUTER_API_KEY")
        .map_err(|_| anyhow::anyhow!("OPENROUTER_API_KEY is required"))?;

    let prompt = cli.prompt.join(" ");

    let shirman = ShirManClient::new();
    let model = shirman.fetch_top_model().await?;

    let model_name = model.name.as_deref().unwrap_or(&model.id);
    eprintln!("Using model: {} ({})", model_name, model.id);

    let openrouter = OpenRouterClient::new(api_key);
    openrouter.create_chat_completion(&model.id, prompt).await?;

    Ok(())
}

