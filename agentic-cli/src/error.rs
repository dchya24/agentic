
/// Centralized error type with user-friendly messages and suggestions.
#[derive(Debug)]
pub enum CommandError {
    Config(String),
    Validation(String),
    Provider(String),
    Tool(String),
    Network(String),
    MaxRetries,
    Cancelled,
    NotFound { what: String, suggestion: Option<String> },
}

impl std::fmt::Display for CommandError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CommandError::Config(msg) => write!(f, "Configuration error: {}", msg),
            CommandError::Validation(msg) => write!(f, "Validation error: {}", msg),
            CommandError::Provider(msg) => write!(f, "Provider error: {}", msg),
            CommandError::Tool(msg) => write!(f, "Tool error: {}", msg),
            CommandError::Network(msg) => write!(f, "Network error: {}", msg),
            CommandError::MaxRetries => write!(f, "Maximum retries exceeded. The request failed after multiple attempts."),
            CommandError::Cancelled => write!(f, "Operation cancelled by user."),
            CommandError::NotFound { what, suggestion } => {
                write!(f, "Not found: {}", what)?;
                if let Some(s) = suggestion {
                    write!(f, "\n  💡 Did you mean: {}?", s)?;
                }
                Ok(())
            }
        }
    }
}

impl std::error::Error for CommandError {}

impl CommandError {
    /// Check if an error message is retryable (network/rate-limit related)
    pub fn is_retryable(error_msg: &str) -> bool {
        let retryable_patterns = [
            "network error",
            "connection refused",
            "timeout",
            "rate limit",
            "429",
            "503",
            "502",
            "connection reset",
            "broken pipe",
            "eof",
        ];

        let lower = error_msg.to_lowercase();
        retryable_patterns.iter().any(|p| lower.contains(p))
    }

    /// Create a "not found" error with a suggestion
    pub fn not_found(what: impl Into<String>) -> Self {
        Self::NotFound {
            what: what.into(),
            suggestion: None,
        }
    }

    /// Create a "not found" error with a suggestion
    pub fn not_found_with_suggestion(what: impl Into<String>, suggestion: impl Into<String>) -> Self {
        Self::NotFound {
            what: what.into(),
            suggestion: Some(suggestion.into()),
        }
    }
}

/// Suggest a similar command from a list of known commands.
pub fn suggest_command(input: &str) -> Option<String> {
    let commands = [
        ("run", &["run", "exec", "execute", "start"] as &[&str]),
        ("interactive", &["interactive", "i", "repl", "chat", "shell"]),
        ("status", &["status", "info", "state", "check"]),
        ("config", &["config", "cfg", "conf"]),
        ("config init", &["init", "setup", "initialize"]),
        ("config show", &["show", "display", "cat", "print"]),
        ("config edit", &["edit", "modify", "change"]),
        ("config validate", &["validate", "check", "verify"]),
        ("config backup", &["backup", "save", "export"]),
        ("config restore", &["restore", "load", "import"]),
        ("config path", &["path", "where", "location"]),
        ("config reset", &["reset", "clear", "default"]),
        ("examples", &["examples", "help", "demo"]),
        ("version", &["version", "v", "ver"]),
    ];

    let input_lower = input.to_lowercase();
    for (cmd, aliases) in commands {
        if aliases.iter().any(|a| *a == input_lower || a.starts_with(&input_lower)) {
            return Some(format!("'agentic {}'", cmd));
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_display() {
        let err = CommandError::Config("invalid config".to_string());
        assert!(err.to_string().contains("Configuration error"));

        let err = CommandError::Provider("connection timeout".to_string());
        assert!(err.to_string().contains("Provider error"));

        let err = CommandError::Tool("tool not found".to_string());
        assert!(err.to_string().contains("Tool error"));

        let err = CommandError::Network("network unreachable".to_string());
        assert!(err.to_string().contains("Network error"));

        let err = CommandError::MaxRetries;
        assert!(err.to_string().contains("Maximum retries"));

        let err = CommandError::Cancelled;
        assert!(err.to_string().contains("cancelled"));
    }

    #[test]
    fn test_retryable_errors() {
        let retryable = vec![
            "network error",
            "connection refused",
            "timeout",
            "rate limit exceeded",
            "429 Too Many Requests",
            "503 Service Unavailable",
        ];

        let non_retryable = vec![
            "invalid API key",
            "authentication failed",
            "model not found",
            "invalid request",
        ];

        for err in retryable {
            assert!(CommandError::is_retryable(err), "Expected '{}' to be retryable", err);
        }

        for err in non_retryable {
            assert!(!CommandError::is_retryable(err), "Expected '{}' to NOT be retryable", err);
        }
    }

    #[test]
    fn test_not_found_with_suggestion() {
        let err = CommandError::not_found_with_suggestion("provider 'zai'", "agentic config init --provider zai");
        let msg = err.to_string();
        assert!(msg.contains("Not found"));
        assert!(msg.contains("Did you mean"));
    }

    #[test]
    fn test_suggest_command() {
        assert_eq!(suggest_command("init"), Some("'agentic config init'".to_string()));
        assert_eq!(suggest_command("run"), Some("'agentic run'".to_string()));
        assert_eq!(suggest_command("xyz"), None);
    }
}
