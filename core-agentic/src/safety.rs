//! Safety system for agentic

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfirmationRequest {
    pub action: String,
    pub description: String,
    pub risk_level: RiskLevel,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RiskLevel {
    Low,
    Medium,
    High,
    Critical,
}

impl RiskLevel {
    pub fn requires_confirmation(&self) -> bool {
        matches!(
            self,
            RiskLevel::Medium | RiskLevel::High | RiskLevel::Critical
        )
    }
}

pub struct Safety {
    blocked_commands: Vec<String>,
    allowed_commands: Vec<String>,
    require_confirmation: bool,
}

impl Default for Safety {
    fn default() -> Self {
        Self::new()
    }
}

impl Safety {
    pub fn new() -> Self {
        Self {
            blocked_commands: vec![
                "rm -rf /".to_string(),
                "rm -rf /*".to_string(),
                "del /f".to_string(),
                "format:".to_string(),
                "mkfs".to_string(),
                "dd if=/dev/zero".to_string(),
            ],
            allowed_commands: Vec::new(),
            require_confirmation: true,
        }
    }

    pub fn with_blocked_commands(mut self, commands: Vec<String>) -> Self {
        self.blocked_commands = commands;
        self
    }

    pub fn with_allowed_commands(mut self, commands: Vec<String>) -> Self {
        self.allowed_commands = commands;
        self
    }

    pub fn set_require_confirmation(&mut self, require: bool) {
        self.require_confirmation = require;
    }

    pub fn check_risk(&self, action: &str, target: Option<&str>) -> RiskLevel {
        let action_str = format!("{} {}", action, target.unwrap_or(""));

        for blocked in &self.blocked_commands {
            if action_str.to_lowercase().contains(&blocked.to_lowercase()) {
                return RiskLevel::Critical;
            }
        }

        if action == "run_command" {
            if let Some(t) = target {
                if t.contains("sudo") || t.contains("su -") {
                    return RiskLevel::Medium;
                }
                if t.starts_with("curl") || t.starts_with("wget") {
                    return RiskLevel::Medium;
                }
            }
        }

        if action == "write_file" || action == "delete_file" {
            return RiskLevel::Medium;
        }

        RiskLevel::Low
    }

    pub fn needs_confirmation(&self, action: &str, target: Option<&str>) -> bool {
        if !self.require_confirmation {
            return false;
        }

        let risk = self.check_risk(action, target);
        risk.requires_confirmation()
    }

    pub fn create_request(&self, action: &str, description: &str) -> ConfirmationRequest {
        let risk = self.check_risk(action, None);
        ConfirmationRequest {
            action: action.to_string(),
            description: description.to_string(),
            risk_level: risk,
        }
    }
}
