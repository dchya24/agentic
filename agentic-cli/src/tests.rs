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
        assert_eq!(config.model.id, "glm-4.7");
    }

    #[test]
    fn test_config_provider_type() {
        let config = Config::default().unwrap();
        assert_eq!(config.provider.provider_type, "openai-compatible");
    }

    #[test]
    fn test_error_types_display() {
        use crate::commands::CommandError;

        let provider_err = CommandError::Provider("connection timeout".to_string());
        assert!(provider_err.to_string().contains("Provider error"));

        let tool_err = CommandError::Tool("tool not found".to_string());
        assert!(tool_err.to_string().contains("Tool error"));

        let config_err = CommandError::Config("invalid config".to_string());
        assert!(config_err.to_string().contains("Configuration error"));

        let network_err = CommandError::Network("network unreachable".to_string());
        assert!(network_err.to_string().contains("Network error"));

        let max_retries = CommandError::MaxRetries;
        assert!(max_retries.to_string().contains("Maximum retries"));

        let cancelled = CommandError::Cancelled;
        assert!(cancelled.to_string().contains("cancelled"));
    }

    #[test]
    fn test_retryable_error_detection() {
        use crate::commands::is_retryable_error;

        let retryable_errors = vec![
            "network error",
            "connection refused",
            "timeout",
            "rate limit exceeded",
            "429 Too Many Requests",
            "503 Service Unavailable",
        ];

        let non_retryable_errors = vec![
            "invalid API key",
            "authentication failed",
            "model not found",
            "invalid request",
        ];

        for err in retryable_errors {
            assert!(
                is_retryable_error(err),
                "Expected '{}' to be retryable",
                err
            );
        }

        for err in non_retryable_errors {
            assert!(
                !is_retryable_error(err),
                "Expected '{}' to not be retryable",
                err
            );
        }
    }
}
