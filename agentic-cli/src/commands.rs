use anyhow::Result;
use core_agentic::{Config, Orchestrator, ToolRegistry};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::io::Write;
use termcolor::{Color, ColorSpec, StandardStream, WriteColor};

use crate::cli::ConfigAction;
use crate::confirmation::{prompt_confirmation, ConfirmationResponse};
use crate::markdown::render_markdown;

static ALWAYS_CONFIRM: AtomicBool = AtomicBool::new(false);
static RETRY_COUNT: AtomicU32 = AtomicU32::new(3);

#[derive(Debug)]
pub enum CommandError {
    Provider(String),
    Tool(String),
    Config(String),
    Network(String),
    MaxRetries,
    Cancelled,
}

impl std::fmt::Display for CommandError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CommandError::Provider(msg) => write!(f, "Provider error: {}", msg),
            CommandError::Tool(msg) => write!(f, "Tool error: {}", msg),
            CommandError::Config(msg) => write!(f, "Configuration error: {}", msg),
            CommandError::Network(msg) => write!(f, "Network error: {}", msg),
            CommandError::MaxRetries => write!(f, "Maximum retries exceeded"),
            CommandError::Cancelled => write!(f, "Operation cancelled by user"),
        }
    }
}

impl std::error::Error for CommandError {}

pub struct Commands {
    config: Config,
    orchestrator: Option<Orchestrator>,
}

impl Commands {
    pub fn new(config: Config) -> Self {
        let provider_config = config
            .to_provider_config()
            .ok_or_else(|| CommandError::Config("No provider configured".to_string()))
            .unwrap();
        let provider = Arc::new(core_agentic::OpenAIProvider::new(provider_config));

        let tools = ToolRegistry::new();
        for tool in core_agentic::tools::builtin_tools() {
            tools.register(tool);
        }

        let mut orchestrator = Orchestrator::new(provider, tools);

        orchestrator.set_confirmation_handler(|request| {
            if ALWAYS_CONFIRM.load(Ordering::Relaxed) {
                return true;
            }
            match prompt_confirmation(&request) {
                Some(ConfirmationResponse::Yes) => true,
                Some(ConfirmationResponse::Always) => {
                    ALWAYS_CONFIRM.store(true, Ordering::Relaxed);
                    true
                }
                Some(ConfirmationResponse::No) | Some(ConfirmationResponse::Quit) | None => false,
            }
        });

        Self {
            config,
            orchestrator: Some(orchestrator),
        }
    }

    pub fn run(&self, task: &str) -> Result<()> {
        let orchestrator = self
            .orchestrator
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("Orchestrator not initialized"))?;

        println!("\n🤖 Running task: {}\n", task);

        let runtime = tokio::runtime::Runtime::new()?;
        
        runtime.block_on(async {
            let result = orchestrator.run_stream(task, |chunk| {
                print_chunk(&chunk);
            }).await;

            match result {
                Ok(final_result) => {
                    println!("\n");
                    print_markdown_header("Final Result");
                    render_markdown(&final_result).ok();
                }
                Err(e) => {
                    print_error(&format!("Error: {}", e));
                }
            }
        });

        Ok(())
    }

    fn execute_with_retry(
        &self,
        orchestrator: &Orchestrator,
        task: &str,
    ) -> Result<String, CommandError> {
        let max_retries = RETRY_COUNT.load(Ordering::Relaxed);
        let mut last_error: Option<String> = None;

        for attempt in 0..max_retries {
            match orchestrator.run(task) {
                Ok(result) => return Ok(result),
                Err(e) => {
                    let error_msg = e.to_string();
                    last_error = Some(error_msg.clone());
                    let is_retryable = is_retryable_error(&error_msg);

                    if is_retryable && attempt < max_retries - 1 {
                        let delay = ((attempt + 1) * 2) as u64;
                        eprintln!(
                            "Attempt {}/{} failed: {}. Retrying in {}s...",
                            attempt + 1,
                            max_retries,
                            error_msg,
                            delay
                        );
                        std::thread::sleep(std::time::Duration::from_secs(delay));
                    } else {
                        break;
                    }
                }
            }
        }

        Err(CommandError::Provider(
            last_error.unwrap_or_else(|| "Unknown error".to_string()),
        ))
    }

    pub fn config(&self, action: &ConfigAction) -> Result<()> {
        match action {
            ConfigAction::Show => {
                let json = serde_json::to_string_pretty(&self.config)?;
                println!("{}", json);
            }
            ConfigAction::Edit => {
                println!("Opening config in editor...");
            }
            ConfigAction::Reset => {
                println!("Resetting config to default...");
            }
        }

        Ok(())
    }
}

pub fn is_retryable_error(error: &str) -> bool {
    let lower = error.to_lowercase();
    lower.contains("network")
        || lower.contains("connection")
        || lower.contains("timeout")
        || lower.contains("rate limit")
        || lower.contains("429")
        || lower.contains("503")
        || lower.contains("429")
}

fn print_chunk(chunk: &str) {
    let mut stdout = StandardStream::stdout(termcolor::ColorChoice::Always);
    
    // Check if chunk looks like markdown
    if chunk.starts_with('#') || chunk.starts_with("```") || chunk.starts_with('-') || chunk.starts_with('*') {
        // Render as markdown
        let _ = render_markdown(chunk);
    } else {
        // Print as plain text with subtle styling
        let _ = stdout.set_color(ColorSpec::new().set_fg(Some(Color::Rgb(200, 200, 200))));
        print!("{}", chunk);
        let _ = stdout.reset();
    }
    
    stdout.flush().ok();
}

fn print_markdown_header(text: &str) {
    let mut stdout = StandardStream::stdout(termcolor::ColorChoice::Always);
    let _ = stdout.set_color(
        ColorSpec::new()
            .set_bold(true)
            .set_fg(Some(Color::Rgb(255, 165, 0)))
    );
    println!("\n╔═══════════════════════════════════════╗");
    println!("║ {} ", text);
    println!("╚═══════════════════════════════════════╝");
    let _ = stdout.reset();
}

fn print_error(text: &str) {
    let mut stdout = StandardStream::stderr(termcolor::ColorChoice::Always);
    let _ = stdout.set_color(ColorSpec::new().set_fg(Some(Color::Red)));
    eprintln!("{}", text);
    let _ = stdout.reset();
}