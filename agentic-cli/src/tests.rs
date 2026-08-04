#[cfg(test)]
mod tests {
    use crate::cli::{Cli, Command};
    use clap::Parser;

    #[test]
    fn test_cli_run_command_parsing() {
        let cli = Cli::try_parse_from(["agentic", "run", "my task"]).unwrap();
        match cli.command {
            Some(Command::Run { task, .. }) => assert_eq!(task, "my task"),
            _ => panic!("Expected Run command"),
        }
    }

    #[test]
    fn test_cli_interactive_command_parsing() {
        let cli = Cli::try_parse_from(["agentic", "interactive"]).unwrap();
        match cli.command {
            Some(Command::Interactive) => {}
            _ => panic!("Expected Interactive command"),
        }
    }

    #[test]
    fn test_cli_version() {
        let cli = Cli::try_parse_from(["agentic", "version"]).unwrap();
        match cli.command {
            Some(Command::Version) => {}
            _ => panic!("Expected Version command"),
        }
    }

    // The two former tests for `crate::config::Config` were removed when
    // that module was deleted as dead code: the live config flow uses
    // `core_agentic::Config` (multi-provider), and the legacy single-
    // provider shape these tests asserted on no longer exists. Coverage
    // for the active config layer lives in core-agentic's own test suite.
    //
    // Tests for `CommandError` variants live alongside the type in
    // `error.rs`.
}
