#[cfg(test)]
mod tests {
    use crate::cli::{Cli, Command};
    use crate::config::Config;
    use clap::Parser;

    #[test]
    fn test_cli_run_command_parsing() {
        let cli = Cli::try_parse_from(&["agentic", "run", "my task"]).unwrap();
        match cli.command {
            Some(Command::Run { task }) => assert_eq!(task, "my task"),
            _ => panic!("Expected Run command"),
        }
    }

    #[test]
    fn test_cli_interactive_command_parsing() {
        let cli = Cli::try_parse_from(&["agentic", "interactive"]).unwrap();
        match cli.command {
            Some(Command::Interactive) => {}
            _ => panic!("Expected Interactive command"),
        }
    }

    #[test]
    fn test_cli_version() {
        let cli = Cli::try_parse_from(&["agentic", "version"]).unwrap();
        match cli.command {
            Some(Command::Version) => {}
            _ => panic!("Expected Version command"),
        }
    }

    #[test]
    fn test_config_default() {
        let config = Config::default().unwrap();
        assert_eq!(config.model.id, "gpt-4o");
    }

    #[test]
    fn test_config_provider_type() {
        let config = Config::default().unwrap();
        assert_eq!(config.provider.provider_type, "openai-compatible");
    }
}
