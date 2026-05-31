//! Safety system for agentic AI agent orchestration.
//!
//! Provides risk scoring, configurable thresholds, pattern-based detection,
//! command blocklist, path sandboxing, rate limiting, and audit logging.
//!
//! Module layout:
//! - [`risk`] — `PermissionMode`, `RiskLevel`, `RiskScore`,
//!   `ConfirmationRequest`. Pure data types.
//! - [`config`] — `SafetyConfig`, `RiskPattern`, defaults.
//! - [`audit`] — `AuditEntry`, `AuditDecision`, `RateLimit`.
//! - [`engine`] — the `Safety` struct + `evaluate` pipeline +
//!   `SafetyDecision`.

mod audit;
mod config;
mod engine;
mod risk;

pub use audit::{AuditDecision, AuditEntry, RateLimit};
pub use config::{RiskPattern, SafetyConfig};
pub use engine::{Safety, SafetyDecision};
pub use risk::{ConfirmationRequest, PermissionMode, RiskLevel, RiskScore};

// ---------------------------------------------------------------------------
// Unit Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn safety() -> Safety {
        Safety::new()
    }

    // --- RiskLevel tests ---

    #[test]
    fn test_risk_level_from_score() {
        assert_eq!(RiskLevel::from_score(0.0), RiskLevel::Low);
        assert_eq!(RiskLevel::from_score(0.1), RiskLevel::Low);
        assert_eq!(RiskLevel::from_score(0.3), RiskLevel::Medium);
        assert_eq!(RiskLevel::from_score(0.5), RiskLevel::Medium);
        assert_eq!(RiskLevel::from_score(0.6), RiskLevel::High);
        assert_eq!(RiskLevel::from_score(0.79), RiskLevel::High);
        assert_eq!(RiskLevel::from_score(0.8), RiskLevel::Critical);
        assert_eq!(RiskLevel::from_score(1.0), RiskLevel::Critical);
    }

    #[test]
    fn test_risk_level_requires_confirmation() {
        assert!(!RiskLevel::Low.requires_confirmation());
        assert!(RiskLevel::Medium.requires_confirmation());
        assert!(RiskLevel::High.requires_confirmation());
        assert!(RiskLevel::Critical.requires_confirmation());
    }

    #[test]
    fn test_risk_level_as_str() {
        assert_eq!(RiskLevel::Low.as_str(), "low");
        assert_eq!(RiskLevel::Medium.as_str(), "medium");
        assert_eq!(RiskLevel::High.as_str(), "high");
        assert_eq!(RiskLevel::Critical.as_str(), "critical");
    }

    // --- RiskScore tests ---

    #[test]
    fn test_risk_score_clamped() {
        let score = RiskScore::new(-1.0, RiskLevel::Low);
        assert_eq!(score.value, 0.0);

        let score = RiskScore::new(2.0, RiskLevel::Critical);
        assert_eq!(score.value, 1.0);
    }

    // --- Blocklist tests ---

    #[test]
    fn test_blocked_rm_rf_root() {
        let s = safety();
        let score = s.score_command("run_command", Some("rm -rf /"));
        assert_eq!(score.level, RiskLevel::Critical);
        assert!(!s.evaluate("run_command", Some("rm -rf /")).allowed);
    }

    #[test]
    fn test_blocked_mkfs() {
        let s = safety();
        let score = s.score_command("run_command", Some("mkfs.ext4 /dev/sda1"));
        assert_eq!(score.level, RiskLevel::Critical);
    }

    #[test]
    fn test_blocked_dd() {
        let s = safety();
        let score = s.score_command("run_command", Some("dd if=/dev/zero of=/dev/sda"));
        assert_eq!(score.level, RiskLevel::Critical);
    }

    #[test]
    fn test_blocked_format() {
        let s = safety();
        let score = s.score_command("run_command", Some("format: C:"));
        assert_eq!(score.level, RiskLevel::Critical);
    }

    // --- Pattern-based scoring ---

    #[test]
    fn test_sudo_medium() {
        let s = safety();
        let score = s.score_command("run_command", Some("sudo apt install foo"));
        assert_eq!(score.level, RiskLevel::Medium);
    }

    #[test]
    fn test_curl_medium() {
        let s = safety();
        let score = s.score_command("run_command", Some("curl https://example.com"));
        assert_eq!(score.level, RiskLevel::Medium);
    }

    #[test]
    fn test_ls_low() {
        let s = safety();
        let score = s.score_command("run_command", Some("ls -la"));
        assert_eq!(score.level, RiskLevel::Low);
    }

    #[test]
    fn test_git_log_low() {
        let s = safety();
        let score = s.score_command("run_command", Some("git log --oneline"));
        assert_eq!(score.level, RiskLevel::Low);
    }

    #[test]
    fn test_git_reset_hard() {
        let s = safety();
        let score = s.score_command("run_command", Some("git reset --hard HEAD~1"));
        assert!(score.value >= 0.6);
    }

    #[test]
    fn test_rm_single_file() {
        let s = safety();
        let score = s.score_command("run_command", Some("rm file.txt"));
        assert!(score.value >= 0.3);
    }

    #[test]
    fn test_write_file_action() {
        let s = safety();
        let score = s.score_command("write_file", None);
        assert!(score.value >= 0.2);
    }

    #[test]
    fn test_delete_file_action() {
        let s = safety();
        let score = s.score_command("delete_file", None);
        assert!(score.value >= 0.3);
    }

    // --- Allowed commands allowlist ---

    #[test]
    fn test_allowed_commands_block_unknown() {
        let s = safety()
            .with_allowed_commands(vec!["ls".into(), "cat".into(), "git".into()]);

        let score = s.score_command("run_command", Some("ls -la"));
        assert_eq!(score.level, RiskLevel::Low);

        let score = s.score_command("run_command", Some("rm file.txt"));
        assert_eq!(score.level, RiskLevel::High);
    }

    #[test]
    fn test_allowed_commands_empty_allows_all() {
        let s = safety(); // default: empty allowed_commands
        let score = s.score_command("run_command", Some("npm install"));
        assert_ne!(score.level, RiskLevel::High); // not blocked by allowlist
    }

    // --- Path sandboxing ---

    #[test]
    fn test_sandbox_allows_within() {
        let s = safety().with_sandbox_paths(vec!["/home/user/project".into()]);
        assert!(s.is_path_allowed("/home/user/project/src/main.rs"));
    }

    #[test]
    fn test_sandbox_blocks_outside() {
        let s = safety().with_sandbox_paths(vec!["/home/user/project".into()]);
        assert!(!s.is_path_allowed("/etc/passwd"));
    }

    #[test]
    fn test_sandbox_blocks_traversal() {
        let s = safety().with_sandbox_paths(vec!["/home/user/project".into()]);
        assert!(!s.is_path_allowed("/home/user/project/../../../etc/passwd"));
    }

    #[test]
    fn test_no_sandbox_allows_all() {
        let s = safety(); // no sandbox configured
        assert!(s.is_path_allowed("/etc/passwd"));
        assert!(s.is_path_allowed("/any/random/path"));
    }

    #[test]
    fn test_sensitive_system_paths() {
        let s = safety();
        let score = s.score_file_operation("/etc/shadow", "write_file");
        assert!(score.value >= 0.3); // extra risk for sensitive path
    }

    // --- Rate limiting ---

    #[test]
    fn test_rate_limit_allows_within() {
        let mut config = SafetyConfig::default();
        config.rate_limits.insert(
            "run_command".into(),
            RateLimit {
                max_calls: 3,
                window_secs: 60,
            },
        );
        let s = Safety::with_config(config);

        assert!(s.check_rate_limit("run_command"));
        assert!(s.check_rate_limit("run_command"));
        assert!(s.check_rate_limit("run_command"));
    }

    #[test]
    fn test_rate_limit_blocks_over() {
        let mut config = SafetyConfig::default();
        config.rate_limits.insert(
            "run_command".into(),
            RateLimit {
                max_calls: 2,
                window_secs: 60,
            },
        );
        let s = Safety::with_config(config);

        assert!(s.check_rate_limit("run_command"));
        assert!(s.check_rate_limit("run_command"));
        assert!(!s.check_rate_limit("run_command")); // blocked
    }

    #[test]
    fn test_rate_limit_per_tool_isolation() {
        let mut config = SafetyConfig::default();
        config.rate_limits.insert(
            "tool_a".into(),
            RateLimit {
                max_calls: 1,
                window_secs: 60,
            },
        );
        let s = Safety::with_config(config);

        assert!(s.check_rate_limit("tool_a"));
        assert!(!s.check_rate_limit("tool_a")); // tool_a limited
        assert!(s.check_rate_limit("tool_b"));   // tool_b unaffected
    }

    #[test]
    fn test_rate_limit_no_config_allows_all() {
        let s = safety();
        for _ in 0..100 {
            assert!(s.check_rate_limit("any_tool"));
        }
    }

    // --- Evaluate (full pipeline) ---

    #[test]
    fn test_evaluate_blocks_critical() {
        let s = safety();
        let decision = s.evaluate("run_command", Some("rm -rf /"));
        assert!(!decision.allowed);
        assert!(!decision.needs_confirmation); // blocked outright
    }

    #[test]
    fn test_evaluate_allows_low() {
        let s = safety();
        let decision = s.evaluate("run_command", Some("ls -la"));
        assert!(decision.allowed);
        assert!(!decision.needs_confirmation);
    }

    #[test]
    fn test_evaluate_medium_needs_confirmation() {
        let s = safety();
        let decision = s.evaluate("run_command", Some("sudo apt install foo"));
        assert!(decision.allowed);
        assert!(decision.needs_confirmation);
    }

    #[test]
    fn test_evaluate_disabled_allows_all() {
        let mut config = SafetyConfig::default();
        config.enabled = false;
        let s = Safety::with_config(config);

        let decision = s.evaluate("run_command", Some("rm -rf /"));
        assert!(decision.allowed);
        assert!(!decision.needs_confirmation);
    }

    #[test]
    fn test_evaluate_no_confirmation_required() {
        let mut config = SafetyConfig::default();
        config.require_confirmation = false;
        let s = Safety::with_config(config);

        let decision = s.evaluate("run_command", Some("sudo apt install foo"));
        assert!(decision.allowed);
        assert!(!decision.needs_confirmation); // no confirmation needed
    }

    #[test]
    fn test_evaluate_sandbox_blocks_file_op() {
        let s = safety().with_sandbox_paths(vec!["/home/user/project".into()]);
        let decision = s.evaluate("write_file", Some("/etc/passwd"));
        assert!(!decision.allowed);
    }

    // --- Audit logging ---

    #[test]
    fn test_audit_log_records() {
        let s = safety();
        s.evaluate("run_command", Some("rm -rf /")); // blocked
        let log = s.audit_log();
        assert_eq!(log.len(), 1);
        assert_eq!(log[0].decision, AuditDecision::Blocked);
    }

    #[test]
    fn test_audit_log_clear() {
        let s = safety();
        s.evaluate("run_command", Some("rm -rf /"));
        assert!(!s.audit_log().is_empty());
        s.clear_audit_log();
        assert!(s.audit_log().is_empty());
    }

    #[test]
    fn test_audit_log_ring_buffer() {
        let mut config = SafetyConfig::default();
        config.audit_capacity = 3;
        let s = Safety::with_config(config);

        for i in 0..5 {
            s.evaluate("run_command", Some(&format!("test_cmd_{}", i)));
        }
        // Only last 3 entries kept
        let log = s.audit_log();
        assert!(log.len() <= 3);
    }

    // --- Needs confirmation legacy ---

    #[test]
    fn test_needs_confirmation_low() {
        let s = safety();
        assert!(!s.needs_confirmation("run_command", Some("ls -la")));
    }

    #[test]
    fn test_needs_confirmation_medium() {
        let s = safety();
        assert!(s.needs_confirmation("run_command", Some("sudo apt install foo")));
    }

    // --- Confirmation request ---

    #[test]
    fn test_create_request() {
        let s = safety();
        let req = s.create_request("run_command", "sudo rm file");
        assert_eq!(req.action, "run_command");
        assert_eq!(req.description, "sudo rm file");
        assert!(req.risk_score >= 0.0); // score is always valid
        assert!(!req.timestamp.to_rfc3339().is_empty());
    }

    // --- Configurable thresholds ---

    #[test]
    fn test_custom_thresholds() {
        let mut config = SafetyConfig::default();
        config.medium_threshold = 0.5; // stricter medium
        config.high_threshold = 0.7;
        config.critical_threshold = 0.9;
        let s = Safety::with_config(config);

        // score 0.35 → Low (because medium threshold is 0.5)
        let score = s.score_command("run_command", Some("curl https://example.com"));
        assert_eq!(score.level, RiskLevel::Low);
    }

    // --- Edge cases ---

    #[test]
    fn test_empty_command() {
        let s = safety();
        let score = s.score_command("run_command", Some(""));
        assert_eq!(score.level, RiskLevel::Low);
    }

    #[test]
    fn test_none_target() {
        let s = safety();
        let score = s.score_command("run_command", None);
        assert_eq!(score.level, RiskLevel::Low);
    }

    #[test]
    fn test_unicode_command() {
        let s = safety();
        let score = s.score_command("run_command", Some("echo 'こんにちは'"));
        assert_eq!(score.level, RiskLevel::Low);
    }

    #[test]
    fn test_very_long_command() {
        let s = safety();
        let long_cmd = "echo ".to_string() + &"a".repeat(10_000);
        let score = s.score_command("run_command", Some(&long_cmd));
        // Should not panic, should be low risk
        assert_eq!(score.level, RiskLevel::Low);
    }

    #[test]
    fn test_case_insensitive_blocklist() {
        let s = safety();
        let score = s.score_command("run_command", Some("RM -RF /"));
        assert_eq!(score.level, RiskLevel::Critical);
    }

    // --- Permission modes ---

    #[test]
    fn test_permission_mode_default_is_default() {
        assert_eq!(PermissionMode::default(), PermissionMode::Default);
    }

    #[test]
    fn test_permission_mode_parse() {
        assert_eq!(PermissionMode::parse("default"), Some(PermissionMode::Default));
        assert_eq!(PermissionMode::parse("PLAN"), Some(PermissionMode::Plan));
        assert_eq!(PermissionMode::parse("yolo"), Some(PermissionMode::Yolo));
        assert_eq!(PermissionMode::parse("readonly"), Some(PermissionMode::Plan));
        assert_eq!(PermissionMode::parse("trust"), Some(PermissionMode::Yolo));
        assert_eq!(PermissionMode::parse("bogus"), None);
    }

    #[test]
    fn test_plan_mode_blocks_writes() {
        let s = safety();
        s.set_mode(PermissionMode::Plan);
        assert!(!s.evaluate("write_file", Some("foo.txt")).allowed);
        assert!(!s.evaluate("edit_file", Some("foo.txt")).allowed);
        assert!(!s.evaluate("run_command", Some("ls")).allowed);
    }

    #[test]
    fn test_plan_mode_allows_reads() {
        let s = safety();
        s.set_mode(PermissionMode::Plan);
        assert!(s.evaluate("read_file", Some("foo.txt")).allowed);
        assert!(s.evaluate("list_files", Some(".")).allowed);
        assert!(s.evaluate("search_files", Some("pattern")).allowed);
    }

    #[test]
    fn test_yolo_mode_auto_approves() {
        let s = safety();
        s.set_mode(PermissionMode::Yolo);
        // sudo would normally be Medium and require confirmation.
        let decision = s.evaluate("run_command", Some("sudo apt install foo"));
        assert!(decision.allowed);
        assert!(!decision.needs_confirmation);
    }

    #[test]
    fn test_yolo_still_blocks_critical_blocklist() {
        let s = safety();
        s.set_mode(PermissionMode::Yolo);
        let decision = s.evaluate("run_command", Some("rm -rf /"));
        assert!(!decision.allowed); // safety net stays
    }

    #[test]
    fn test_default_mode_unchanged_behavior() {
        let s = safety();
        // Default mode preserves the original gating behavior.
        let decision = s.evaluate("run_command", Some("sudo apt install foo"));
        assert!(decision.allowed);
        assert!(decision.needs_confirmation);
    }

    #[test]
    fn test_mode_setter_and_getter() {
        let s = safety();
        assert_eq!(s.mode(), PermissionMode::Default);
        s.set_mode(PermissionMode::Yolo);
        assert_eq!(s.mode(), PermissionMode::Yolo);
    }

    #[test]
    fn test_needs_confirmation_yolo() {
        let s = safety();
        s.set_mode(PermissionMode::Yolo);
        assert!(!s.needs_confirmation("run_command", Some("sudo apt install foo")));
    }

    #[test]
    fn test_needs_confirmation_plan_writes() {
        let s = safety();
        s.set_mode(PermissionMode::Plan);
        // Plan mode denies outright — no confirmation needed (or possible).
        assert!(!s.needs_confirmation("write_file", Some("foo.txt")));
    }
}
