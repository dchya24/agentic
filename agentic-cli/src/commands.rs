use anyhow::Result;
use core_agentic::{Config, Orchestrator, ToolRegistry};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::io::Write;
use termcolor::{Color, ColorSpec, StandardStream, WriteColor};

use crate::cli::ConfigAction;
use crate::confirmation::{prompt_confirmation, ConfirmationResponse};
use crate::markdown::render_markdown;

static ALWAYS_CONFIRM: AtomicBool = AtomicBool::new(false);

#[derive(Debug)]
pub enum CommandError {
    Config(String),
}

impl std::fmt::Display for CommandError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CommandError::Config(msg) => write!(f, "Configuration error: {}", msg),
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

    pub async fn run(&self, task: &str) -> Result<()> {
        let orchestrator = self
            .orchestrator
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("Orchestrator not initialized"))?;

        println!("\n🤖 Running task: {}\n", task);

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

        Ok(())
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