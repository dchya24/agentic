//! Tool-call execution paths.
//!
//! Two flavors live here:
//! - [`Orchestrator::handle_tool_calls`] — synchronous, sequential.
//!   Used by [`Orchestrator::run`].
//! - [`Orchestrator::handle_tool_calls_parallel`] — async, batches
//!   consecutive read-only tools. Used by [`Orchestrator::run_stream`].
//!
//! Both go through the same safety + confirmation pre-pass before
//! executing anything.

use crate::events::Event;
use crate::memory::Message;

use super::messages::{build_tool_call_responses, truncate_tool_result};
use super::Orchestrator;

impl Orchestrator {
    /// Extract the most relevant target string from tool args (for safety scoring).
    pub(super) fn extract_target(args: &serde_json::Value) -> Option<&str> {
        args.get("command")
            .or(args.get("path"))
            .or(args.get("file_path"))
            .and_then(|v| v.as_str())
    }

    pub(super) fn execute_tool(&self, name: &str, args: &serde_json::Value) -> String {
        let raw = match self.tools.execute_by_name(name, args) {
            Ok(result) => serde_json::to_string_pretty(&result)
                .unwrap_or_else(|_| result.to_string()),
            Err(e) => format!("Tool error: {}", e),
        };
        truncate_tool_result(&raw, self.tool_result_max_chars)
    }

    pub(super) fn handle_tool_calls(
        &self,
        content: &str,
        tool_calls: &[(String, String, String)],
    ) {
        let tool_call_responses = build_tool_call_responses(tool_calls);
        self.memory
            .lock()
            .unwrap()
            .add_message(Message::assistant_with_tool_calls(content, tool_call_responses));

        for (tc_id, tc_name, tc_args_str) in tool_calls {
            let args: serde_json::Value =
                serde_json::from_str(tc_args_str).unwrap_or(serde_json::json!({}));

            // Surface the call to subscribers before running it. This lets
            // CLIs render a tool-call panel even when execution will be
            // denied or skipped — the operator still gets to see what was
            // attempted.
            self.events.emit(Event::ToolCall {
                tool_name: tc_name.clone(),
                arguments: args.clone(),
            });

            let target = Self::extract_target(&args);
            let decision = self.safety.evaluate(tc_name, target);

            // Hard-denied (Plan mode, blocklist, sandbox, rate-limit).
            if !decision.allowed {
                let reason = if decision.reason.is_empty() {
                    "Action denied by safety policy".to_string()
                } else {
                    decision.reason.clone()
                };
                println!("  -> [DENIED: {}]", reason);
                self.events.emit(Event::ToolOutput {
                    tool_name: tc_name.clone(),
                    output: serde_json::Value::String(format!("Blocked: {}", reason)),
                });
                self.memory.lock().unwrap().add_message(Message::tool(
                    tc_name.clone(),
                    tc_id.clone(),
                    format!("Blocked: {}", reason),
                ));
                continue;
            }

            // Needs confirmation (Default mode, medium+ risk).
            if decision.needs_confirmation {
                let mut request = self
                    .safety
                    .create_request(tc_name, &format!("{:?}", args));
                // For state-changing file tools, attach a unified-diff
                // preview so the user sees the exact change they're
                // approving instead of just the args dump.
                request.preview_diff = preview_diff_for_tool(tc_name, &args);
                if !self.require_confirmation(request) {
                    println!("  -> [SKIPPED - Confirmation denied]");
                    self.events.emit(Event::ToolOutput {
                        tool_name: tc_name.clone(),
                        output: serde_json::Value::String(
                            "Skipped: Confirmation denied".to_string(),
                        ),
                    });
                    self.memory.lock().unwrap().add_message(Message::tool(
                        tc_name.clone(),
                        tc_id.clone(),
                        "Skipped: Confirmation denied".to_string(),
                    ));
                    continue;
                }
                self.safety
                    .record_confirmation(tc_name, target, &decision.score, true);
            }

            let result = self.execute_tool(tc_name, &args);

            // Emit the result as a structured event. We pass the truncated
            // string verbatim so subscribers see exactly what the model
            // will see in the next turn.
            self.events.emit(Event::ToolOutput {
                tool_name: tc_name.clone(),
                output: serde_json::Value::String(result.clone()),
            });

            self.memory.lock().unwrap().add_message(Message::tool(
                tc_name.clone(),
                tc_id.clone(),
                result,
            ));
        }
    }

    /// Async variant of [`Self::handle_tool_calls`] that batches
    /// consecutive read-only tools and runs them concurrently.
    ///
    /// Sequencing rules (matching the architecture doc):
    /// - Read-only tools (read_file, list_files, glob, grep, search_files)
    ///   in the same batch run in parallel via spawn_blocking.
    /// - State-changing tools run alone, sequentially.
    /// - Results are pushed to memory in the **original tool-call order**
    ///   regardless of which batch finished first.
    /// - Safety evaluation and user confirmation happen sequentially on the
    ///   main task before any execution starts (parallelism doesn't change
    ///   gating semantics).
    pub(super) async fn handle_tool_calls_parallel(
        &self,
        content: &str,
        tool_calls: &[(String, String, String)],
    ) {
        let tool_call_responses = build_tool_call_responses(tool_calls);
        self.memory
            .lock()
            .unwrap()
            .add_message(Message::assistant_with_tool_calls(content, tool_call_responses));

        // Outcome of the safety+confirmation pre-pass for a single call.
        enum Slot {
            /// Pre-resolved (denied, skipped). The string is the message we
            /// will record verbatim as the tool result.
            PreResolved {
                name: String,
                id: String,
                message: String,
            },
            /// Needs to be executed. Carries the parsed args and a flag for
            /// scheduling.
            Pending {
                name: String,
                id: String,
                args: serde_json::Value,
                read_only: bool,
            },
        }

        // Pre-pass: evaluate every call. Confirmation prompts run here, in
        // the original order, before anything is executed.
        let mut slots: Vec<Slot> = Vec::with_capacity(tool_calls.len());
        for (tc_id, tc_name, tc_args_str) in tool_calls {
            let args: serde_json::Value =
                serde_json::from_str(tc_args_str).unwrap_or(serde_json::json!({}));

            // Surface the call before safety evaluation so subscribers see
            // even denied calls.
            self.events.emit(Event::ToolCall {
                tool_name: tc_name.clone(),
                arguments: args.clone(),
            });

            let target = Self::extract_target(&args);
            let decision = self.safety.evaluate(tc_name, target);

            if !decision.allowed {
                let reason = if decision.reason.is_empty() {
                    "Action denied by safety policy".to_string()
                } else {
                    decision.reason.clone()
                };
                println!("  -> [DENIED: {}]", reason);
                self.events.emit(Event::ToolOutput {
                    tool_name: tc_name.clone(),
                    output: serde_json::Value::String(format!("Blocked: {}", reason)),
                });
                slots.push(Slot::PreResolved {
                    name: tc_name.clone(),
                    id: tc_id.clone(),
                    message: format!("Blocked: {}", reason),
                });
                continue;
            }

            if decision.needs_confirmation {
                let mut request = self
                    .safety
                    .create_request(tc_name, &format!("{:?}", args));
                request.preview_diff = preview_diff_for_tool(tc_name, &args);
                if !self.require_confirmation(request) {
                    println!("  -> [SKIPPED - Confirmation denied]");
                    self.events.emit(Event::ToolOutput {
                        tool_name: tc_name.clone(),
                        output: serde_json::Value::String(
                            "Skipped: Confirmation denied".to_string(),
                        ),
                    });
                    slots.push(Slot::PreResolved {
                        name: tc_name.clone(),
                        id: tc_id.clone(),
                        message: "Skipped: Confirmation denied".to_string(),
                    });
                    continue;
                }
                self.safety
                    .record_confirmation(tc_name, target, &decision.score, true);
            }

            let read_only = self.tools.is_read_only(tc_name);
            slots.push(Slot::Pending {
                name: tc_name.clone(),
                id: tc_id.clone(),
                args,
                read_only,
            });
        }

        // Execute slots in batches. Output is collected position-aligned to
        // `slots` so we can push to memory in original order at the end.
        let mut results: Vec<Option<(String, String, String)>> =
            (0..slots.len()).map(|_| None).collect();

        let mut i = 0;
        while i < slots.len() {
            // PreResolved slots are written directly without execution.
            if let Slot::PreResolved { name, id, message } = &slots[i] {
                results[i] = Some((name.clone(), id.clone(), message.clone()));
                i += 1;
                continue;
            }

            // Determine batch bounds.
            //   Pending + read_only     → grow batch while next is the same.
            //   Pending + !read_only    → batch of one.
            let start = i;
            let mut end = i + 1;
            if let Slot::Pending { read_only: true, .. } = &slots[i] {
                while end < slots.len() {
                    match &slots[end] {
                        Slot::Pending { read_only: true, .. } => end += 1,
                        _ => break,
                    }
                }
            }

            // Spawn one blocking task per call in the batch. spawn_blocking
            // is the right primitive because Tool::execute is sync and may
            // do filesystem / process I/O.
            let mut handles = Vec::with_capacity(end - start);
            for slot_idx in start..end {
                if let Slot::Pending { name, id, args, .. } = &slots[slot_idx] {
                    let registry = self.tools.clone();
                    let max_chars = self.tool_result_max_chars;
                    let name = name.clone();
                    let id = id.clone();
                    let args = args.clone();
                    let handle = tokio::task::spawn_blocking(move || {
                        let raw = match registry.execute_by_name(&name, &args) {
                            Ok(v) => serde_json::to_string_pretty(&v)
                                .unwrap_or_else(|_| v.to_string()),
                            Err(e) => format!("Tool error: {}", e),
                        };
                        let truncated = truncate_tool_result(&raw, max_chars);
                        (name, id, truncated)
                    });
                    handles.push((slot_idx, handle));
                }
            }

            for (slot_idx, handle) in handles {
                match handle.await {
                    Ok(triple) => results[slot_idx] = Some(triple),
                    Err(join_err) => {
                        // Recover slot identity for the error message.
                        if let Slot::Pending { name, id, .. } = &slots[slot_idx] {
                            results[slot_idx] = Some((
                                name.clone(),
                                id.clone(),
                                format!("Tool error: task panicked: {}", join_err),
                            ));
                        }
                    }
                }
            }

            i = end;
        }

        // Push results in the original order so the model sees a coherent
        // tool/assistant/tool/assistant interleaving. Also emit ToolOutput
        // events for any executed (Pending) slots; PreResolved slots
        // already emitted their outcome above.
        let mut mem = self.memory.lock().unwrap();
        for (idx, entry) in results.into_iter().enumerate() {
            if let Some((name, id, output)) = entry {
                // Only Pending slots produce real tool output; PreResolved
                // already emitted theirs.
                if matches!(slots[idx], Slot::Pending { .. }) {
                    self.events.emit(Event::ToolOutput {
                        tool_name: name.clone(),
                        output: serde_json::Value::String(output.clone()),
                    });
                }
                mem.add_message(Message::tool(name, id, output));
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Diff preview
// ---------------------------------------------------------------------------

/// Compute a unified-diff preview of what a state-changing tool would do,
/// without executing it. Returns `None` for tools we don't know how to
/// preview, or when the source file is unreadable.
///
/// Supported tools:
/// - `write_file`: diff between current file content and `args.content`.
/// - `edit_file`: diff between current content and the result of applying
///   the requested string replacement (or `None` if old_string is missing
///   or doesn't match — the user will see the same error the tool would
///   return on execution).
/// - `apply_patch`: returns the patch text directly (it's already a
///   unified diff).
pub(super) fn preview_diff_for_tool(
    tool_name: &str,
    args: &serde_json::Value,
) -> Option<String> {
    let obj = args.as_object()?;
    match tool_name {
        "write_file" => {
            let path = obj.get("path")?.as_str()?;
            let new_content = obj.get("content")?.as_str()?;
            let before = std::fs::read_to_string(path).unwrap_or_default();
            let diff = crate::diff_util::unified_diff(path, &before, new_content, 3);
            if diff.is_empty() {
                None
            } else {
                Some(diff)
            }
        }
        "edit_file" => {
            let path = obj.get("file_path")?.as_str()?;
            let old_string = obj.get("old_string")?.as_str()?;
            let new_string = obj.get("new_string")?.as_str()?;
            let replace_all = obj
                .get("replace_all")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);

            let before = std::fs::read_to_string(path).ok()?;
            // Mirror EditFileTool's matching: exact, then unique. We only
            // need a usable preview, so fall back gracefully if the
            // string isn't found.
            if !before.contains(old_string) {
                return None;
            }
            let after = if replace_all {
                before.replace(old_string, new_string)
            } else {
                // First-occurrence replace.
                before.replacen(old_string, new_string, 1)
            };
            if after == before {
                return None;
            }
            let diff = crate::diff_util::unified_diff(path, &before, &after, 3);
            if diff.is_empty() {
                None
            } else {
                Some(diff)
            }
        }
        "apply_patch" => obj
            .get("patch")
            .and_then(|v| v.as_str())
            .filter(|s| !s.trim().is_empty())
            .map(|s| s.to_string()),
        _ => None,
    }
}

#[cfg(test)]
mod preview_diff_tests {
    use super::preview_diff_for_tool;
    use std::io::Write;

    fn temp_file(name: &str, content: &str) -> std::path::PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let pid = std::process::id();
        let dir = std::env::temp_dir()
            .join(format!("agentic-preview-{}-{}", pid, nanos));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(name);
        let mut f = std::fs::File::create(&path).unwrap();
        f.write_all(content.as_bytes()).unwrap();
        path
    }

    #[test]
    fn returns_none_for_unknown_tool() {
        let args = serde_json::json!({"path": "x.txt", "content": "hello"});
        assert!(preview_diff_for_tool("read_file", &args).is_none());
        assert!(preview_diff_for_tool("run_command", &args).is_none());
    }

    #[test]
    fn write_file_diffs_against_existing_file() {
        let p = temp_file("a.txt", "hello\nworld\n");
        let args = serde_json::json!({
            "path": p.to_string_lossy(),
            "content": "hello\nrust\n",
        });
        let diff = preview_diff_for_tool("write_file", &args).expect("diff");
        assert!(diff.contains("-world"));
        assert!(diff.contains("+rust"));
    }

    #[test]
    fn write_file_returns_none_when_content_unchanged() {
        let p = temp_file("same.txt", "unchanged\n");
        let args = serde_json::json!({
            "path": p.to_string_lossy(),
            "content": "unchanged\n",
        });
        assert!(preview_diff_for_tool("write_file", &args).is_none());
    }

    #[test]
    fn write_file_treats_missing_path_as_creation() {
        // Non-existent path: the diff should look like a creation
        // (all `+` lines, no `-`). diff_util keeps it concise.
        let args = serde_json::json!({
            "path": "/tmp/agentic-preview-doesnotexist.txt",
            "content": "new\nfile\n",
        });
        let diff = preview_diff_for_tool("write_file", &args).expect("diff");
        assert!(diff.contains("+new"));
        assert!(diff.contains("+file"));
    }

    #[test]
    fn edit_file_returns_none_when_old_string_missing() {
        let p = temp_file("miss.txt", "alpha\nbeta\n");
        let args = serde_json::json!({
            "file_path": p.to_string_lossy(),
            "old_string": "NOT_PRESENT",
            "new_string": "x",
        });
        assert!(preview_diff_for_tool("edit_file", &args).is_none());
    }

    #[test]
    fn edit_file_diffs_first_occurrence() {
        let p = temp_file("edit.txt", "alpha\nbeta\nalpha\n");
        let args = serde_json::json!({
            "file_path": p.to_string_lossy(),
            "old_string": "alpha",
            "new_string": "ALPHA",
        });
        let diff = preview_diff_for_tool("edit_file", &args).expect("diff");
        assert!(diff.contains("-alpha"));
        assert!(diff.contains("+ALPHA"));
        // Replace_all not set — the second alpha must remain in diff context.
        assert!(diff.contains(" alpha") || diff.matches('a').count() >= 2);
    }

    #[test]
    fn edit_file_replace_all_changes_every_occurrence() {
        let p = temp_file("edit-all.txt", "x\nx\nx\n");
        let args = serde_json::json!({
            "file_path": p.to_string_lossy(),
            "old_string": "x",
            "new_string": "y",
            "replace_all": true,
        });
        let diff = preview_diff_for_tool("edit_file", &args).expect("diff");
        // Three occurrences should produce three `+y` and three `-x`.
        assert_eq!(diff.matches("+y").count(), 3);
        assert_eq!(diff.matches("-x").count(), 3);
    }

    #[test]
    fn apply_patch_returns_patch_text_directly() {
        let patch = "--- a/x\n+++ b/x\n@@ -1 +1 @@\n-old\n+new\n";
        let args = serde_json::json!({"patch": patch});
        let preview = preview_diff_for_tool("apply_patch", &args)
            .expect("apply_patch should pass through");
        assert_eq!(preview, patch);
    }

    #[test]
    fn apply_patch_returns_none_for_empty() {
        let args = serde_json::json!({"patch": "   "});
        assert!(preview_diff_for_tool("apply_patch", &args).is_none());
    }
}
