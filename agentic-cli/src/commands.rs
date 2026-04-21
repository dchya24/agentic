use anyhow::Result;
use core_agentic::{Orchestrator, ToolRegistry};
use std::sync::Arc;

use crate::cli::ConfigAction;
use crate::config::Config;

pub struct Commands {
    config: Config,
    orchestrator: Option<Orchestrator>,
}

impl Commands {
    pub fn new(config: Config) -> Self {
        let provider = config.to_provider_config();
        let provider = Arc::new(core_agentic::OpenAIProvider::new(provider));

        let tools = ToolRegistry::new();

        let orchestrator = Orchestrator::new(provider, tools);

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
