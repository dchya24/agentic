//! Transport-neutral runtime protocol types.

use serde::{Deserialize, Serialize};

use crate::attachments::Attachment;
use crate::events::Event;
use crate::safety::PermissionMode;
use crate::tools::QuestionAnswer;

pub const PROTOCOL_NAME: &str = "agentic";
pub const PROTOCOL_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProtocolEvent {
    pub v: u32,
    #[serde(rename = "requestId", skip_serializing_if = "Option::is_none")]
    pub request_id: Option<String>,
    #[serde(flatten)]
    pub event: Event,
}

impl ProtocolEvent {
    pub fn new(request_id: Option<String>, event: Event) -> Self {
        Self {
            v: PROTOCOL_VERSION,
            request_id,
            event,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProtocolRequest {
    pub v: u32,
    pub id: String,
    #[serde(flatten)]
    pub request: Request,
}

impl ProtocolRequest {
    pub fn new(id: impl Into<String>, request: Request) -> Self {
        Self {
            v: PROTOCOL_VERSION,
            id: id.into(),
            request,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Request {
    Init {
        #[serde(default)]
        overrides: InitOverrides,
    },
    Run {
        task: String,
        #[serde(default)]
        attachments: Vec<Attachment>,
    },
    Cancel,
    ResetSession,
    SearchMemory {
        query: String,
    },
    /// Ask the daemon for its registered tool set (for `/tools`).
    ListTools,
    /// Activate a skill in the daemon's orchestrator (for `/skill`).
    SkillActivate {
        name: String,
    },
    AddSystemMessage {
        content: String,
    },
    Plan {
        goal: String,
        #[serde(default = "default_true")]
        require_approval: bool,
    },
    ConfirmResponse {
        #[serde(rename = "requestId", default)]
        request_id: Option<String>,
        approved: bool,
    },
    QuestionResponse {
        #[serde(rename = "requestId", default)]
        request_id: Option<String>,
        #[serde(default)]
        answers: Vec<QuestionAnswer>,
    },
    Shutdown,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InitOverrides {
    pub config_path: Option<String>,
    pub permission_mode: Option<PermissionMode>,
    pub model: Option<String>,
    pub system_prompt: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeNotice {
    Ready,
    InitOk,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn protocol_event_flattens_event_payload() {
        let event = ProtocolEvent::new(
            Some("r2".into()),
            Event::ToolStarted {
                tool_call_id: "c1".into(),
                tool_name: "grep".into(),
                arguments: serde_json::json!({ "pattern": "foo" }),
            },
        );
        let value = serde_json::to_value(event).unwrap();
        assert_eq!(value["v"], PROTOCOL_VERSION);
        assert_eq!(value["requestId"], "r2");
        assert_eq!(value["type"], "tool_started");
        assert_eq!(value["toolCallId"], "c1");
    }
}
