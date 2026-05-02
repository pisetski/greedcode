use std::fs;
use std::io::{self, BufRead, IsTerminal, Write};

use anyhow::Result;
use clap::Parser;

mod api;
mod models;
mod output;

use api::openrouter::OpenRouterClient;
use api::shirman::ShirManClient;
use models::types::Message;
use output::{MarkdownMode, ResponseWriter};

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

    #[arg(long, short, help = "Enter interactive chat mode")]
    chat: bool,

    #[arg(long, value_enum, default_value_t = MarkdownMode::Auto)]
    markdown: MarkdownMode,
}

#[tokio::main]
async fn main() -> Result<()> {
    load_dotenv();

    let cli = Cli::parse();

    if let Err(usage) = validate_cli(&cli) {
        eprintln!("{}", usage);
        std::process::exit(2);
    }

    let api_key = std::env::var("OPENROUTER_API_KEY")
        .map_err(|_| anyhow::anyhow!("OPENROUTER_API_KEY is required"))?;

    let shirman = ShirManClient::new();
    let model = shirman.fetch_top_model().await?;

    let model_name = model.name.as_deref().unwrap_or(&model.id);
    eprintln!("Using model: {} ({})", model_name, model.id);

    let stdout_is_terminal = io::stdout().is_terminal();

    if cli.chat {
        run_chat_loop(&api_key, &model.id, stdout_is_terminal, cli.markdown).await
    } else {
        let prompt = cli.prompt.join(" ");
        let messages = vec![Message {
            role: "user".to_string(),
            content: prompt,
        }];

        let stdout = io::stdout();
        let stdout_lock = stdout.lock();
        let mut writer = ResponseWriter::new(stdout_lock, cli.markdown, stdout_is_terminal);
        let openrouter = OpenRouterClient::new(api_key);
        openrouter
            .create_chat_completion(&model.id, &messages, &mut writer)
            .await?;
        writer.finish()?;

        Ok(())
    }
}

fn validate_cli(cli: &Cli) -> std::result::Result<(), &'static str> {
    if cli.chat {
        if cli.prompt.is_empty() {
            Ok(())
        } else {
            Err("Usage: greedcode --chat")
        }
    } else if cli.prompt.is_empty() {
        Err("Usage: greedcode \"<prompt>\"")
    } else {
        Ok(())
    }
}

async fn run_chat_loop(
    api_key: &str,
    model_id: &str,
    stdout_is_terminal: bool,
    markdown: MarkdownMode,
) -> Result<()> {
    let stdin = io::stdin();
    let mut input = String::new();
    let mut history = Vec::new();

    let openrouter = OpenRouterClient::new(api_key.to_string());

    eprintln!("Chat mode. Type 'help' for commands, 'exit' or Ctrl+D to quit.");

    loop {
        eprint!("> ");
        if io::stderr().flush().is_err() {
            break;
        }

        input.clear();
        if stdin.read_line(&mut input)? == 0 {
            break;
        }

        let message = input.trim_end_matches(['\r', '\n']);
        let command_text = message.trim();
        if command_text.is_empty() {
            continue;
        }

        if let Some(cmd) = parse_chat_command(command_text) {
            match cmd {
                ChatCommand::Exit | ChatCommand::Quit => {
                    break;
                }
                ChatCommand::Clear => {
                    history.clear();
                }
                ChatCommand::Help => {
                    eprintln!("Commands:");
                    eprintln!("  help   - Show this message");
                    eprintln!("  clear  - Clear conversation history");
                    eprintln!("  exit, quit - Exit chat");
                }
            }
            continue;
        }

        let user_message = Message {
            role: "user".to_string(),
            content: message.to_string(),
        };
        history.push(user_message);

        let stdout = io::stdout();
        let stdout_lock = stdout.lock();
        let mut writer = ResponseWriter::new(stdout_lock, markdown, stdout_is_terminal);

        match openrouter
            .create_chat_completion(model_id, &history, &mut writer)
            .await
        {
            Ok(assistant_text) => {
                writer.finish()?;

                if !assistant_text.is_empty() {
                    let assistant_message = Message {
                        role: "assistant".to_string(),
                        content: assistant_text,
                    };
                    history.push(assistant_message);
                }

                if stdout_is_terminal {
                    eprintln!();
                }
            }
            Err(e) => {
                eprintln!("Error: {}", e);
                if !history.is_empty() {
                    history.pop();
                }
            }
        }
    }

    Ok(())
}

enum ChatCommand {
    Exit,
    Quit,
    Clear,
    Help,
}

fn parse_chat_command(input: &str) -> Option<ChatCommand> {
    let lowered = input.to_lowercase();
    match lowered.as_str() {
        "exit" => Some(ChatCommand::Exit),
        "quit" => Some(ChatCommand::Quit),
        "clear" => Some(ChatCommand::Clear),
        "help" => Some(ChatCommand::Help),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_chat_command_exit() {
        assert!(matches!(
            parse_chat_command("exit"),
            Some(ChatCommand::Exit)
        ));
        assert!(matches!(
            parse_chat_command("EXIT"),
            Some(ChatCommand::Exit)
        ));
    }

    #[test]
    fn test_parse_chat_command_quit() {
        assert!(matches!(
            parse_chat_command("quit"),
            Some(ChatCommand::Quit)
        ));
        assert!(matches!(
            parse_chat_command("QUIT"),
            Some(ChatCommand::Quit)
        ));
    }

    #[test]
    fn test_parse_chat_command_clear() {
        assert!(matches!(
            parse_chat_command("clear"),
            Some(ChatCommand::Clear)
        ));
    }

    #[test]
    fn test_parse_chat_command_help() {
        assert!(matches!(
            parse_chat_command("help"),
            Some(ChatCommand::Help)
        ));
    }

    #[test]
    fn test_parse_chat_command_unknown() {
        assert!(parse_chat_command("hello").is_none());
        assert!(parse_chat_command("").is_none());
    }

    #[test]
    fn test_validate_cli_requires_prompt_without_chat() {
        let cli = Cli {
            prompt: vec![],
            chat: false,
            markdown: MarkdownMode::Auto,
        };

        assert_eq!(validate_cli(&cli), Err("Usage: greedcode \"<prompt>\""));
    }

    #[test]
    fn test_validate_cli_allows_chat_without_prompt() {
        let cli = Cli {
            prompt: vec![],
            chat: true,
            markdown: MarkdownMode::Auto,
        };

        assert_eq!(validate_cli(&cli), Ok(()));
    }

    #[test]
    fn test_validate_cli_rejects_chat_prompt() {
        let cli = Cli {
            prompt: vec!["ignored".to_string()],
            chat: true,
            markdown: MarkdownMode::Auto,
        };

        assert_eq!(validate_cli(&cli), Err("Usage: greedcode --chat"));
    }
}
