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
use crate::orchestrator::progress::DeltaThrottler;

use std::sync::Arc;

use super::messages::{build_tool_call_responses, truncate_tool_result};
use super::Orchestrator;

/// Hasil eksekusi satu tool + metadata untuk `Event::ToolOutput`.
struct SlotOutcome {
    name: String,
    id: String,
    result: String, // sudah di-truncate
    duration_ms: u64,
    success: bool,
    truncated: bool,
}

/// Budget live output per tool (chars).
const DELTA_BUDGET_CHARS: usize = 8_000;

impl Orchestrator {
    /// Extract the most relevant target string from tool args (for safety scoring).
    pub(super) fn extract_target(args: &serde_json::Value) -> Option<&str> {
        args.get("command")
            .or(args.get("path"))
            .or(args.get("file_path"))
            .and_then(|v| v.as_str())
    }

    /// Execute a tool, streaming progress deltas through `on_progress`.
    /// Returns the truncated result plus duration/success/truncated flags
    /// so callers can emit an enriched `Event::ToolOutput`.
    fn execute_tool_streaming(
        &self,
        name: &str,
        args: &serde_json::Value,
        on_progress: &dyn Fn(&str),
    ) -> SlotOutcome {
        let start = std::time::Instant::now();
        let raw = match self
            .tools
            .execute_streaming_by_name(name, args, on_progress)
        {
            Ok(result) => {
                serde_json::to_string_pretty(&result).unwrap_or_else(|_| result.to_string())
            }
            Err(e) => format!("Tool error: {}", e),
        };
        let truncated = raw.len() > self.tool_result_max_chars;
        let result = truncate_tool_result(&raw, self.tool_result_max_chars);
        let success = !result.starts_with("Tool error");
        SlotOutcome {
            name: name.to_string(),
            id: String::new(), // diisi pemanggil
            result,
            duration_ms: start.elapsed().as_millis() as u64,
            success,
            truncated,
        }
    }

    /// Execute a tool with live `ToolDelta` emission (sync path).
    fn execute_tool_live(&self, id: &str, name: &str, args: &serde_json::Value) -> SlotOutcome {
        self.events.emit(Event::ToolStart {
            tool_call_id: id.to_string(),
            tool_name: name.to_string(),
            arguments: args.clone(),
        });
        let throttler = DeltaThrottler::new(DELTA_BUDGET_CHARS);
        let oid = id.to_string();
        let oname = name.to_string();
        let mut outcome = self.execute_tool_streaming(name, args, &|delta| {
            if throttler.accept(delta) {
                self.events.emit(Event::ToolDelta {
                    tool_call_id: oid.clone(),
                    tool_name: oname.clone(),
                    delta: delta.to_string(),
                });
            }
        });
        outcome.id = id.to_string();
        outcome
    }

    pub(super) fn handle_tool_calls(&self, content: &str, tool_calls: &[(String, String, String)]) {
        let tool_call_responses = build_tool_call_responses(tool_calls);
        self.memory
            .lock()
            .unwrap()
            .add_message(Message::assistant_with_tool_calls(
                content,
                tool_call_responses,
            ));

        // Same Slot model as the async path: pre-pass for safety +
        // confirmation in the original tool-call order, then execute
        // consecutive read-only batches concurrently.
        enum Slot {
            PreResolved {
                name: String,
                id: String,
                message: String,
            },
            Pending {
                name: String,
                id: String,
                args: serde_json::Value,
                read_only: bool,
            },
        }

        let mut slots: Vec<Slot> = Vec::with_capacity(tool_calls.len());
        for (tc_id, tc_name, tc_args_str) in tool_calls {
            let args: serde_json::Value =
                serde_json::from_str(tc_args_str).unwrap_or(serde_json::json!({}));

            // Surface the call before safety so subscribers see denied
            // calls too (matches handle_tool_calls_parallel).
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
                    error: None,
                    tool_call_id: tc_id.clone(),
                    duration_ms: 0,
                    success: false,
                    truncated: false,
                });
                slots.push(Slot::PreResolved {
                    name: tc_name.clone(),
                    id: tc_id.clone(),
                    message: format!("Blocked: {}", reason),
                });
                continue;
            }

            if decision.needs_confirmation {
                let mut request = self.safety.create_request(tc_name, &format!("{:?}", args));
                request.preview_diff = preview_diff_for_tool(tc_name, &args);
                if !self.require_confirmation(request) {
                    println!("  -> [SKIPPED - Confirmation denied]");
                    self.events.emit(Event::ToolOutput {
                        tool_name: tc_name.clone(),
                        output: serde_json::Value::String(
                            "Skipped: Confirmation denied".to_string(),
                        ),
                        error: None,
                        tool_call_id: tc_id.clone(),
                        duration_ms: 0,
                        success: false,
                        truncated: false,
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

        // Execution pass: walk slots batching consecutive read-only
        // Pending entries. PreResolved slots and state-changing tools
        // keep the original sequential semantics (a write is not
        // batched with reads).
        let mut results: Vec<Option<SlotOutcome>> = (0..slots.len()).map(|_| None).collect();

        let mut i = 0;
        while i < slots.len() {
            match &slots[i] {
                Slot::PreResolved { name, id, message } => {
                    results[i] = Some(SlotOutcome {
                        name: name.clone(),
                        id: id.clone(),
                        result: message.clone(),
                        duration_ms: 0,
                        success: false,
                        truncated: false,
                    });
                    i += 1;
                    continue;
                }
                Slot::Pending {
                    read_only: false, ..
                } => {
                    // State-changing: run alone, sequentially, with live
                    // deltas (run_command/run_script stream).
                    if let Slot::Pending { name, id, args, .. } = &slots[i] {
                        let outcome = self.execute_tool_live(id, name, args);
                        results[i] = Some(outcome);
                    }
                    i += 1;
                    continue;
                }
                Slot::Pending {
                    read_only: true, ..
                } => {}
            }

            // Grow a run of consecutive read-only Pending slots.
            let start = i;
            let mut end = i + 1;
            while end < slots.len() {
                match &slots[end] {
                    Slot::Pending {
                        read_only: true, ..
                    } => end += 1,
                    _ => break,
                }
            }

            // Single-element batch: cheaper to run inline than spawn.
            if end - start == 1 {
                if let Slot::Pending { name, id, args, .. } = &slots[start] {
                    let outcome = self.execute_tool_live(id, name, args);
                    results[start] = Some(outcome);
                }
                i = end;
                continue;
            }

            // Multi-element batch: run threads in a scope so we can
            // borrow `self` without 'static. Each thread drops its
            // result into a slot keyed by index.
            let max_chars = self.tool_result_max_chars;
            let registry = self.tools.clone();
            let mut batch_results: Vec<Option<SlotOutcome>> = (start..end).map(|_| None).collect();

            // Emit ToolStart for every tool in the batch on the main
            // thread (read-only tools don't stream, so no deltas follow).
            for slot in slots.iter().skip(start).take(end - start) {
                if let Slot::Pending { name, id, args, .. } = slot {
                    self.events.emit(Event::ToolStart {
                        tool_call_id: id.clone(),
                        tool_name: name.clone(),
                        arguments: args.clone(),
                    });
                }
            }

            std::thread::scope(|s| {
                let mut handles = Vec::with_capacity(end - start);
                for (local_idx, slot) in slots.iter().skip(start).take(end - start).enumerate() {
                    if let Slot::Pending { name, id, args, .. } = slot {
                        let registry = registry.clone();
                        let name = name.clone();
                        let id = id.clone();
                        let args = args.clone();
                        let handle = s.spawn(move || {
                            let start = std::time::Instant::now();
                            let raw = match registry.execute_by_name(&name, &args) {
                                Ok(v) => serde_json::to_string_pretty(&v)
                                    .unwrap_or_else(|_| v.to_string()),
                                Err(e) => format!("Tool error: {}", e),
                            };
                            let duration_ms = start.elapsed().as_millis() as u64;
                            let truncated = raw.len() > max_chars;
                            let result = truncate_tool_result(&raw, max_chars);
                            (local_idx, name, id, result, duration_ms, truncated)
                        });
                        handles.push(handle);
                    }
                }
                for handle in handles {
                    match handle.join() {
                        Ok((local_idx, name, id, output, duration_ms, truncated)) => {
                            let success = !output.starts_with("Tool error");
                            batch_results[local_idx] = Some(SlotOutcome {
                                name,
                                id,
                                result: output,
                                duration_ms,
                                success,
                                truncated,
                            });
                        }
                        Err(_) => {
                            // A panic in a tool task: fill the matching
                            // slot with a synthetic error result so the
                            // model still gets a response.
                            // We can't recover the slot identity from a
                            // join error, so fill the next missing one.
                            for (local_idx, item) in batch_results.iter_mut().enumerate() {
                                if item.is_none() {
                                    if let Slot::Pending { name, id, .. } =
                                        &slots[start + local_idx]
                                    {
                                        *item = Some(SlotOutcome {
                                            name: name.clone(),
                                            id: id.clone(),
                                            result: "Tool error: task panicked".to_string(),
                                            duration_ms: 0,
                                            success: false,
                                            truncated: false,
                                        });
                                    }
                                    break;
                                }
                            }
                        }
                    }
                }
            });

            for (local_idx, entry) in batch_results.into_iter().enumerate() {
                results[start + local_idx] = entry;
            }
            i = end;
        }

        // Push results in original order. Emit ToolOutput events only
        // for Pending slots (PreResolved already emitted theirs above).
        let mut mem = self.memory.lock().unwrap();
        for (idx, entry) in results.into_iter().enumerate() {
            if let Some(outcome) = entry {
                if matches!(slots[idx], Slot::Pending { .. }) {
                    self.events.emit(Event::ToolOutput {
                        tool_name: outcome.name.clone(),
                        output: serde_json::Value::String(outcome.result.clone()),
                        error: None,
                        tool_call_id: outcome.id.clone(),
                        duration_ms: outcome.duration_ms,
                        success: outcome.success,
                        truncated: outcome.truncated,
                    });
                }
                mem.add_message(Message::tool(outcome.name, outcome.id, outcome.result));
            }
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
            .add_message(Message::assistant_with_tool_calls(
                content,
                tool_call_responses,
            ));

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
                    error: None,
                    tool_call_id: tc_id.clone(),
                    duration_ms: 0,
                    success: false,
                    truncated: false,
                });
                slots.push(Slot::PreResolved {
                    name: tc_name.clone(),
                    id: tc_id.clone(),
                    message: format!("Blocked: {}", reason),
                });
                continue;
            }

            if decision.needs_confirmation {
                let mut request = self.safety.create_request(tc_name, &format!("{:?}", args));
                request.preview_diff = preview_diff_for_tool(tc_name, &args);
                if !self.require_confirmation(request) {
                    println!("  -> [SKIPPED - Confirmation denied]");
                    self.events.emit(Event::ToolOutput {
                        tool_name: tc_name.clone(),
                        output: serde_json::Value::String(
                            "Skipped: Confirmation denied".to_string(),
                        ),
                        error: None,
                        tool_call_id: tc_id.clone(),
                        duration_ms: 0,
                        success: false,
                        truncated: false,
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
        let mut results: Vec<Option<SlotOutcome>> = (0..slots.len()).map(|_| None).collect();

        let mut i = 0;
        while i < slots.len() {
            // PreResolved slots are written directly without execution.
            if let Slot::PreResolved { name, id, message } = &slots[i] {
                results[i] = Some(SlotOutcome {
                    name: name.clone(),
                    id: id.clone(),
                    result: message.clone(),
                    duration_ms: 0,
                    success: false,
                    truncated: false,
                });
                i += 1;
                continue;
            }

            // Determine batch bounds.
            //   Pending + read_only     → grow batch while next is the same.
            //   Pending + !read_only    → batch of one.
            let start = i;
            let mut end = i + 1;
            if let Slot::Pending {
                read_only: true, ..
            } = &slots[i]
            {
                while end < slots.len() {
                    match &slots[end] {
                        Slot::Pending {
                            read_only: true, ..
                        } => end += 1,
                        _ => break,
                    }
                }
            }

            // One progress channel per batch + a forwarder thread that
            // converts deltas into ToolDelta events. Read-only tools never
            // call on_progress (default `execute`), so the forwarder only
            // ever sees deltas for streaming tools (run_command/run_script),
            // which are always batch-of-one.
            let (tx, rx) = std::sync::mpsc::channel::<(String, String, String)>();
            let emitter = self.events.clone();
            let throttler = Arc::new(DeltaThrottler::new(DELTA_BUDGET_CHARS));
            let t2 = throttler.clone();
            let fwd = std::thread::Builder::new()
                .name("agentic-delta-fwd".into())
                .spawn(move || {
                    for (id, name, delta) in rx {
                        if t2.accept(&delta) {
                            emitter.emit(Event::ToolDelta {
                                tool_call_id: id,
                                tool_name: name,
                                delta,
                            });
                        }
                    }
                })
                .expect("spawn delta forwarder");

            // Emit ToolStart + spawn one blocking task per call.
            let mut handles = Vec::with_capacity(end - start);
            for (slot_idx, slot) in slots.iter().enumerate().skip(start).take(end - start) {
                if let Slot::Pending { name, id, args, .. } = slot {
                    self.events.emit(Event::ToolStart {
                        tool_call_id: id.clone(),
                        tool_name: name.clone(),
                        arguments: args.clone(),
                    });

                    let registry = self.tools.clone();
                    let max_chars = self.tool_result_max_chars;
                    let name = name.clone();
                    let id = id.clone();
                    let args = args.clone();
                    let tx = tx.clone();
                    let handle = tokio::task::spawn_blocking(move || {
                        let start = std::time::Instant::now();
                        let raw = match registry.execute_streaming_by_name(&name, &args, &|delta| {
                            let _ = tx.send((id.clone(), name.clone(), delta.to_string()));
                        }) {
                            Ok(v) => {
                                serde_json::to_string_pretty(&v).unwrap_or_else(|_| v.to_string())
                            }
                            Err(e) => format!("Tool error: {}", e),
                        };
                        let duration_ms = start.elapsed().as_millis() as u64;
                        let truncated = raw.len() > max_chars;
                        let result = truncate_tool_result(&raw, max_chars);
                        (name, id, result, duration_ms, truncated)
                    });
                    handles.push((slot_idx, handle));
                }
            }

            for (slot_idx, handle) in handles {
                match handle.await {
                    Ok((name, id, output, duration_ms, truncated)) => {
                        let success = !output.starts_with("Tool error");
                        results[slot_idx] = Some(SlotOutcome {
                            name,
                            id,
                            result: output,
                            duration_ms,
                            success,
                            truncated,
                        });
                    }
                    Err(join_err) => {
                        // Recover slot identity for the error message.
                        if let Slot::Pending { name, id, .. } = &slots[slot_idx] {
                            results[slot_idx] = Some(SlotOutcome {
                                name: name.clone(),
                                id: id.clone(),
                                result: format!("Tool error: task panicked: {}", join_err),
                                duration_ms: 0,
                                success: false,
                                truncated: false,
                            });
                        }
                    }
                }
            }

            // Drop our Sender + drain the forwarder so every delta is
            // emitted BEFORE the final ToolOutput for this batch.
            drop(tx);
            fwd.join().expect("delta forwarder panicked");

            i = end;
        }

        // Push results in the original order so the model sees a coherent
        // tool/assistant/tool/assistant interleaving. Also emit ToolOutput
        // events for any executed (Pending) slots; PreResolved slots
        // already emitted their outcome above.
        let mut mem = self.memory.lock().unwrap();
        for (idx, entry) in results.into_iter().enumerate() {
            if let Some(outcome) = entry {
                // Only Pending slots produce real tool output; PreResolved
                // already emitted theirs.
                if matches!(slots[idx], Slot::Pending { .. }) {
                    self.events.emit(Event::ToolOutput {
                        tool_name: outcome.name.clone(),
                        output: serde_json::Value::String(outcome.result.clone()),
                        error: None,
                        tool_call_id: outcome.id.clone(),
                        duration_ms: outcome.duration_ms,
                        success: outcome.success,
                        truncated: outcome.truncated,
                    });
                }
                mem.add_message(Message::tool(outcome.name, outcome.id, outcome.result));
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
pub(super) fn preview_diff_for_tool(tool_name: &str, args: &serde_json::Value) -> Option<String> {
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
        let dir = std::env::temp_dir().join(format!("agentic-preview-{}-{}", pid, nanos));
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
        let preview =
            preview_diff_for_tool("apply_patch", &args).expect("apply_patch should pass through");
        assert_eq!(preview, patch);
    }

    #[test]
    fn apply_patch_returns_none_for_empty() {
        let args = serde_json::json!({"patch": "   "});
        assert!(preview_diff_for_tool("apply_patch", &args).is_none());
    }
}
