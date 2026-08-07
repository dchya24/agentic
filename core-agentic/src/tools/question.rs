//! `question` tool — ask the user questions during execution.
//!
//! The agent calls this when it needs to gather preferences, clarify
//! instructions, or get a decision on implementation choices. Unlike
//! the confirmation handler (which is yes/no), `question` supports:
//!
//! - Free-text answers (the user types anything)
//! - Multiple-choice selection (the user picks from options)
//! - Multi-select (the user picks one or more options)
//!
//! **Mechanism**: Because `Tool::execute` is synchronous and must return
//! a `ToolResult<Value>`, the question tool uses a callback pattern
//! (the same approach as the orchestrator's `confirmation_handler`).
//! The CLI/TUI registers a handler at startup; the tool invokes it
//! when `execute` runs. If no handler is registered, the tool returns
//! a fallback response so the agent doesn't stall.

use std::collections::HashMap;
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

use crate::tool::{Tool, ToolError, ToolParam, ToolResult, ToolSchema};

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// A single question the agent wants to ask the user.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuestionPrompt {
    /// The question text shown to the user.
    pub question: String,
    /// Short header / label for the question (used in TUI rendering).
    #[serde(default)]
    pub header: Option<String>,
    /// Pre-defined options the user can choose from. Empty = free text.
    #[serde(default)]
    pub options: Vec<String>,
    /// Allow the user to type a custom answer instead of picking an option.
    #[serde(default)]
    pub custom: bool,
    /// Allow selecting multiple options (checkbox style).
    #[serde(default)]
    pub multiple: bool,
}

/// The user's answer to a single question.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuestionAnswer {
    /// The question that was asked (echoed back for context).
    pub question: String,
    /// The user's selected option(s) or free-text answer.
    pub answer: Vec<String>,
    /// Whether the user chose to skip / dismiss this question.
    #[serde(default)]
    pub skipped: bool,
}

/// Callback type for handling questions. Returns one `QuestionAnswer`
/// per `QuestionPrompt`.
///
/// The handler receives the full list of questions and returns the
/// same number of answers. Implementations may show them one at a time
/// or batched, synchronously or via a UI loop — that's the caller's
/// concern.
///
/// **Important for host integrations**: `handle` is invoked from the
/// middle of a tool execution while the host may still be rendering
/// progress elsewhere (e.g. the CLI's streaming "Thinking…" spinner
/// thread). Implementations that present an interactive prompt must
/// suspend that other rendering for the duration of the prompt, or the
/// two writers will corrupt each other's output. The CLI handler does
/// this by parking the spinner thread behind a gate while dialoguer is
/// on screen.
pub trait QuestionHandler: Send + Sync {
    fn handle(&self, questions: &[QuestionPrompt]) -> Vec<QuestionAnswer>;
}

/// A thread-safe holder for the question handler. `None` means no UI
/// is connected and the tool returns a fallback response.
pub(crate) static QUESTION_HANDLER: std::sync::LazyLock<Mutex<Option<Box<dyn QuestionHandler>>>> =
    std::sync::LazyLock::new(|| Mutex::new(None));

/// Register a question handler globally. Called once by the CLI at startup.
pub fn set_question_handler(handler: Box<dyn QuestionHandler>) {
    let mut slot = QUESTION_HANDLER.lock().unwrap();
    *slot = Some(handler);
}

/// Clear the registered handler (e.g. on shutdown).
pub fn clear_question_handler() {
    let mut slot = QUESTION_HANDLER.lock().unwrap();
    *slot = None;
}

// ---------------------------------------------------------------------------
// Fallback handler (when no UI is connected)
// ---------------------------------------------------------------------------

/// Answer produced when no question handler is registered. Returns a
/// skip for every question so the agent can proceed without blocking.
fn fallback_answers(questions: &[QuestionPrompt]) -> Vec<QuestionAnswer> {
    questions
        .iter()
        .map(|q| QuestionAnswer {
            question: q.question.clone(),
            answer: vec![],
            skipped: true,
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Tool implementation
// ---------------------------------------------------------------------------

pub struct QuestionTool;

impl QuestionTool {
    pub fn new() -> Self {
        Self
    }
}

impl Default for QuestionTool {
    fn default() -> Self {
        Self::new()
    }
}

impl Tool for QuestionTool {
    fn name(&self) -> &str {
        "question"
    }

    fn description(&self) -> &str {
        "Ask the user one or more questions during execution. Use this to gather \
         preferences, clarify ambiguous instructions, get decisions on implementation \
         choices, or present options. Each question can have pre-defined options \
         (multiple choice), accept free-text, or both. Returns the user's answers \
         or a 'skipped' marker if the user dismissed the question."
    }

    fn schema(&self) -> ToolSchema {
        let mut params = HashMap::new();
        params.insert(
            "questions".to_string(),
            ToolParam {
                param_type: "array".to_string(),
                description: Some(
                    "Array of questions to ask. Each question has: \
                     question (string, required) — the question text, \
                     header (string, optional) — short label, \
                     options (string[], optional) — pre-defined choices, \
                     custom (boolean, optional) — allow free-text answer, \
                     multiple (boolean, optional) — allow multi-select."
                        .to_string(),
                ),
                default: None,
            },
        );

        ToolSchema {
            name: "question".to_string(),
            description: "Ask the user questions during execution.".to_string(),
            parameters: params,
            required: vec!["questions".to_string()],
        }
    }

    fn execute(&self, args: serde_json::Value) -> ToolResult<serde_json::Value> {
        let args_obj = args
            .as_object()
            .ok_or_else(|| ToolError::new("Invalid arguments: expected object"))?;

        let questions_val = args_obj
            .get("questions")
            .ok_or_else(|| ToolError::new("Missing required parameter: questions"))?;

        let questions: Vec<QuestionPrompt> = serde_json::from_value(questions_val.clone())
            .map_err(|e| ToolError::new(format!("Invalid questions format: {}", e)))?;

        if questions.is_empty() {
            return Err(ToolError::new("questions array must not be empty"));
        }

        // Validate each question has a question text.
        for (i, q) in questions.iter().enumerate() {
            if q.question.trim().is_empty() {
                return Err(ToolError::new(format!(
                    "Question at index {} has empty question text",
                    i
                )));
            }
        }

        // Try the registered handler; fall back to skip-all.
        let answers = {
            let handler = QUESTION_HANDLER.lock().unwrap();
            match handler.as_ref() {
                Some(h) => h.handle(&questions),
                None => {
                    tracing::warn!(
                        "question tool invoked but no handler registered; returning skip-all fallback"
                    );
                    fallback_answers(&questions)
                }
            }
        };

        // Sanity: answers must match questions count.
        if answers.len() != questions.len() {
            return Err(ToolError::new(format!(
                "Handler returned {} answers for {} questions",
                answers.len(),
                questions.len()
            )));
        }

        let skipped_count = answers.iter().filter(|a| a.skipped).count();

        Ok(serde_json::json!({
            "answers": answers,
            "total": questions.len(),
            "skipped": skipped_count,
            "answered": questions.len() - skipped_count,
        }))
    }

    /// `question` is read-only — it doesn't modify any files or run
    /// commands. But it's interactive (blocks on user input), so it
    /// should not be parallelized with other tools.
    fn is_read_only(&self) -> bool {
        true
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_empty_questions_array() {
        let tool = QuestionTool::new();
        let err = tool
            .execute(serde_json::json!({"questions": []}))
            .expect_err("should reject empty");
        assert!(err.to_string().contains("must not be empty"));
    }

    #[test]
    fn rejects_empty_question_text() {
        let tool = QuestionTool::new();
        let err = tool
            .execute(serde_json::json!({"questions": [{"question": "  "}]}))
            .expect_err("should reject empty text");
        assert!(err.to_string().contains("empty question text"));
    }

    #[test]
    fn returns_skip_all_when_no_handler() {
        // Make sure no handler is registered.
        clear_question_handler();

        let tool = QuestionTool::new();
        let result = tool
            .execute(serde_json::json!({
                "questions": [
                    {"question": "What framework?", "options": ["react", "vue"]},
                    {"question": "TypeScript?"}
                ]
            }))
            .expect("should succeed");

        assert_eq!(result["total"], 2);
        assert_eq!(result["skipped"], 2);
        assert_eq!(result["answered"], 0);

        let answers = result["answers"].as_array().unwrap();
        assert!(answers[0]["skipped"].as_bool().unwrap());
        assert!(answers[1]["skipped"].as_bool().unwrap());
    }

    #[test]
    fn invokes_registered_handler() {
        struct TestHandler;
        impl QuestionHandler for TestHandler {
            fn handle(&self, questions: &[QuestionPrompt]) -> Vec<QuestionAnswer> {
                questions
                    .iter()
                    .enumerate()
                    .map(|(i, q)| QuestionAnswer {
                        question: q.question.clone(),
                        answer: vec![format!("answer-{}", i)],
                        skipped: false,
                    })
                    .collect()
            }
        }

        set_question_handler(Box::new(TestHandler));

        let tool = QuestionTool::new();
        let result = tool
            .execute(serde_json::json!({
                "questions": [
                    {"question": "Color?", "options": ["red", "blue"]},
                    {"question": "Name?"}
                ]
            }))
            .expect("should succeed");

        assert_eq!(result["total"], 2);
        assert_eq!(result["skipped"], 0);
        assert_eq!(result["answered"], 2);

        let answers = result["answers"].as_array().unwrap();
        assert_eq!(answers[0]["answer"][0], "answer-0");
        assert_eq!(answers[1]["answer"][0], "answer-1");

        // Clean up.
        clear_question_handler();
    }

    #[test]
    fn parses_all_question_fields() {
        let qp: QuestionPrompt = serde_json::from_value(serde_json::json!({
            "question": "Pick colors",
            "header": "Colors",
            "options": ["red", "green", "blue"],
            "custom": true,
            "multiple": true
        }))
        .unwrap();

        assert_eq!(qp.question, "Pick colors");
        assert_eq!(qp.header.as_deref(), Some("Colors"));
        assert_eq!(qp.options, vec!["red", "green", "blue"]);
        assert!(qp.custom);
        assert!(qp.multiple);
    }

    #[test]
    fn defaults_optional_fields() {
        let qp: QuestionPrompt = serde_json::from_value(serde_json::json!({
            "question": "Simple?"
        }))
        .unwrap();

        assert!(qp.header.is_none());
        assert!(qp.options.is_empty());
        assert!(!qp.custom);
        assert!(!qp.multiple);
    }
}
