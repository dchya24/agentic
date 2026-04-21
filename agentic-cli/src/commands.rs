use anyhow::Result;
use core_agentic::{ConfirmationRequest, Orchestrator, ToolRegistry};
use std::sync::Arc;

use crate::cli::ConfigAction;
use crate::config::Config;
use crate::confirmation::{prompt_confirmation, ConfirmationResponse};

static ALWAYS_CONFIRM: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

pub struct Commands {
    config: Config,
    orchestrator: Option<Orchestrator>,
}

impl Commands {
    pub fn new(config: Config) -> Self {
        let provider = config.to_provider_config();
        let provider = Arc::new(core_agentic::OpenAIProvider::new(provider));

        let tools = ToolRegistry::new();

        let mut orchestrator = Orchestrator::new(provider, tools);

        orchestrator.set_confirmation_handler(|request| {
            if ALWAYS_CONFIRM.load(std::sync::atomic::Ordering::Relaxed) {
                return true;
            }
            match prompt_confirmation(&request) {
                Some(ConfirmationResponse::Yes) => true,
                Some(ConfirmationResponse::Always) => {
                    ALWAYS_CONFIRM.store(true, std::sync::atomic::Ordering::Relaxed);
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

        println!("Running task: {}", task);

        let result = orchestrator.run(task)?;

        println!("\nResult:\n{}", result);

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
                // TODO: Open in editor
            }
            ConfigAction::Reset => {
                println!("Resetting config to default...");
                // TODO: Reset config
            }
        }

        Ok(())
    }
}
