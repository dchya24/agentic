//! The agent loop: `run` (sync) and `run_stream` (async).
//!
//! Both follow the same shape:
//! 1. push the user message into memory
//! 2. enter the loop with iteration cap + cancel check
//! 3. autocompact if needed
//! 4. build the request, call the provider, accumulate response
//! 5. if there are tool calls → execute them and continue
//! 6. otherwise → finalize the assistant message and return

use std::sync::atomic::Ordering;

use crate::memory::Message;
use crate::providers::ChatRequest;
use crate::AgenticError;

use super::{Orchestrator, OrchestratorState};

impl Orchestrator {
    pub fn run(&self, input: &str) -> Result<String, AgenticError> {
        self.run_with_attachments(input, Vec::new())
    }

    /// Same as [`Self::run`] but the user message carries image (or
    /// other) attachments. The orchestrator validates capabilities up
    /// front so a vision-incompatible model fails fast with a clear
    /// error before any provider call.
    pub fn run_with_attachments(
        &self,
        input: &str,
        attachments: Vec<crate::attachments::Attachment>,
    ) -> Result<String, AgenticError> {
        if !attachments.is_empty() {
            self.check_attachment_capability(&attachments)?;
        }

        {
            let mut state = self.state.lock().unwrap();
            *state = OrchestratorState::Planning;
        }

        self.memory
            .lock()
            .unwrap()
            .add_message(if attachments.is_empty() {
                Message::user(input)
            } else {
                Message::user_with_attachments(input, attachments)
            });

        let tool_defs = self.tools.tool_definitions();
        let mut iteration: u32 = 0;

        loop {
            iteration += 1;
            if iteration > self.max_iterations {
                tracing::warn!(
                    max = self.max_iterations,
                    "Agent loop hit max_iterations; aborting"
                );
                return Err(AgenticError::Provider(format!(
                    "Agent loop exceeded max_iterations ({}). Aborting to prevent runaway.",
                    self.max_iterations
                )));
            }

            // Warn when approaching the limit (80% threshold)
            if self.approaching_limit(iteration) && iteration < self.max_iterations {
                tracing::info!(
                    iteration,
                    max = self.max_iterations,
                    "Approaching max_iterations limit"
                );
                // Emit a warning event so the UI can display it
                self.events.emit(crate::events::Event::System {
                    message: format!(
                        "⚠️ Approaching iteration limit ({}/{})",
                        iteration, self.max_iterations
                    ),
                });
            }

            if self.cancelled() {
                tracing::info!("Agent loop cancelled by user");
                return Err(AgenticError::Cancelled);
            }

            self.maybe_autocompact();

            let messages = self.build_messages();
            let mut request =
                ChatRequest::new(&self.model, messages).with_tools(tool_defs.clone());
            if let Some(ref prompt) = self.system_prompt {
                request = request.with_system_prompt(prompt.clone());
            }

            let response = self
                .provider
                .chat(request)
                .map_err(|e| AgenticError::Provider(e.to_string()))?;

            let content = response.message.content.clone().unwrap_or_default();

            if !response.message.tool_calls.is_empty() {
                // Emit the LLM's text content as a Thought event so the user
                // can see what the model is thinking/planning before tool execution.
                if !content.is_empty() {
                    self.events.emit(crate::events::Event::Thought {
                        content: content.clone(),
                    });
                }

                // Check for loop detection on the first tool call
                if let Some(first_tc) = response.message.tool_calls.first() {
                    let tool_name = &first_tc.function.name;
                    if self.record_tool_call_for_loop_detection(tool_name) {
                        tracing::warn!(
                            tool = %tool_name,
                            threshold = super::LOOP_DETECTION_THRESHOLD,
                            "Loop detected: same tool called consecutively"
                        );
                        return Err(AgenticError::Provider(format!(
                            "Loop detected: '{}' called {} times consecutively. Aborting to prevent infinite loop.",
                            tool_name, super::LOOP_DETECTION_THRESHOLD
                        )));
                    }
                }

                let tool_calls: Vec<(String, String, String)> = response
                    .message
                    .tool_calls
                    .iter()
                    .map(|tc| {
                        (
                            tc.id.clone(),
                            tc.function.name.clone(),
                            tc.function.arguments.clone(),
                        )
                    })
                    .collect();
                self.handle_tool_calls(&content, &tool_calls);
            } else {
                // No tool calls - model produced a final answer
                // Clear loop detection history for the next user turn
                self.clear_loop_detection();

                self.memory
                    .lock()
                    .unwrap()
                    .add_message(Message::assistant(&content));

                {
                    let mut state = self.state.lock().unwrap();
                    *state = OrchestratorState::Completed;
                }
                return Ok(content);
            }
        }
    }

    pub async fn run_stream<F>(
        &self,
        input: &str,
        on_chunk: F,
    ) -> Result<String, AgenticError>
    where
        F: FnMut(String),
    {
        self.run_stream_with_attachments(input, Vec::new(), on_chunk).await
    }

    /// Streaming variant of [`Self::run_with_attachments`]. The same
    /// capability check runs up front; on failure the loop returns
    /// `AgenticError::Provider` before opening a stream.
    pub async fn run_stream_with_attachments<F>(
        &self,
        input: &str,
        attachments: Vec<crate::attachments::Attachment>,
        mut on_chunk: F,
    ) -> Result<String, AgenticError>
    where
        F: FnMut(String),
    {
        use std::collections::HashMap;

        use futures::stream::StreamExt;

        if !attachments.is_empty() {
            self.check_attachment_capability(&attachments)?;
        }

        {
            let mut state = self.state.lock().unwrap();
            *state = OrchestratorState::Planning;
        }

        self.memory
            .lock()
            .unwrap()
            .add_message(if attachments.is_empty() {
                Message::user(input)
            } else {
                Message::user_with_attachments(input, attachments)
            });

        let tool_defs = self.tools.tool_definitions();
        let mut iteration: u32 = 0;

        loop {
            iteration += 1;
            if iteration > self.max_iterations {
                tracing::warn!(
                    max = self.max_iterations,
                    "Agent stream loop hit max_iterations; aborting"
                );
                return Err(AgenticError::Provider(format!(
                    "Agent loop exceeded max_iterations ({}). Aborting to prevent runaway.",
                    self.max_iterations
                )));
            }

            // Warn when approaching the limit (80% threshold)
            if self.approaching_limit(iteration) && iteration < self.max_iterations {
                tracing::info!(
                    iteration,
                    max = self.max_iterations,
                    "Approaching max_iterations limit"
                );
                self.events.emit(crate::events::Event::System {
                    message: format!(
                        "⚠️ Approaching iteration limit ({}/{})",
                        iteration, self.max_iterations
                    ),
                });
            }

            if self.cancelled() {
                tracing::info!("Agent stream loop cancelled by user");
                return Err(AgenticError::Cancelled);
            }

            self.maybe_autocompact();

            let messages = self.build_messages();
            let mut request = ChatRequest::new(&self.model, messages)
                .with_tools(tool_defs.clone())
                .stream();
            if let Some(ref prompt) = self.system_prompt {
                request = request.with_system_prompt(prompt.clone());
            }

            let mut content_buf = String::new();
            let mut tool_calls_map: HashMap<u32, (String, String, String)> = HashMap::new();

            match self.provider.chat_stream(request) {
                Ok(mut stream) => {
                    while let Some(chunk_result) = stream.next().await {
                        match chunk_result {
                            Ok(chunk) => {
                                if !chunk.delta.is_empty() {
                                    on_chunk(chunk.delta.clone());
                                    content_buf.push_str(&chunk.delta);
                                }
                                for tc in chunk.tool_calls {
                                    let entry = tool_calls_map
                                        .entry(tc.index)
                                        .or_insert_with(|| {
                                            (String::new(), String::new(), String::new())
                                        });
                                    if let Some(id) = tc.id {
                                        entry.0 = id;
                                    }
                                    if let Some(name) = tc.function_name {
                                        entry.1 = name;
                                    }
                                    if let Some(args) = tc.function_arguments {
                                        entry.2.push_str(&args);
                                    }
                                }
                            }
                            Err(e) => {
                                return Err(AgenticError::Provider(e.to_string()));
                            }
                        }
                    }
                }
                Err(e) => {
                    return Err(AgenticError::Provider(e.to_string()));
                }
            }

            let accumulated_tool_calls: Vec<(String, String, String)> = {
                let mut indices: Vec<u32> = tool_calls_map.keys().copied().collect();
                indices.sort();
                indices
                    .into_iter()
                    .map(|i| {
                        let (id, name, args) = tool_calls_map.remove(&i).unwrap();
                        (id, name, args)
                    })
                    .collect()
            };

            if !accumulated_tool_calls.is_empty() {
                // Emit the LLM's text content as a Thought event so the user
                // can see what the model is thinking/planning before tool execution.
                if !content_buf.is_empty() {
                    self.events.emit(crate::events::Event::Thought {
                        content: content_buf.clone(),
                    });
                }

                // Check for loop detection on the first tool call
                if let Some(first_tc) = accumulated_tool_calls.first() {
                    let tool_name = &first_tc.1; // (id, name, args)
                    if self.record_tool_call_for_loop_detection(tool_name) {
                        tracing::warn!(
                            tool = %tool_name,
                            threshold = super::LOOP_DETECTION_THRESHOLD,
                            "Loop detected: same tool called consecutively"
                        );
                        return Err(AgenticError::Provider(format!(
                            "Loop detected: '{}' called {} times consecutively. Aborting to prevent infinite loop.",
                            tool_name, super::LOOP_DETECTION_THRESHOLD
                        )));
                    }
                }

                self.handle_tool_calls_parallel(&content_buf, &accumulated_tool_calls)
                    .await;
                continue;
            }

            // No tool calls - model produced a final answer
            // Clear loop detection history for the next user turn
            self.clear_loop_detection();

            self.memory
                .lock()
                .unwrap()
                .add_message(Message::assistant(&content_buf));

            {
                let mut state = self.state.lock().unwrap();
                *state = OrchestratorState::Completed;
            }

            return Ok(content_buf);
        }
    }

    /// Internal helper: returns true when cancel was requested.
    pub(super) fn cancelled(&self) -> bool {
        self.cancel.load(Ordering::SeqCst)
    }

    /// Pre-flight check: every attachment must be supported by the
    /// active model. Today only image attachments are checked, against
    /// `ModelCapabilities::vision`. Lookup falls back to the conservative
    /// default (`vision: false`) for unknown models, so the safe
    /// behavior is to reject rather than silently send.
    pub(super) fn check_attachment_capability(
        &self,
        attachments: &[crate::attachments::Attachment],
    ) -> Result<(), AgenticError> {
        let needs_vision = attachments
            .iter()
            .any(|a| matches!(a.kind, crate::attachments::AttachmentKind::Image));
        if !needs_vision {
            return Ok(());
        }
        let caps = crate::capabilities::resolve(&self.model);
        if caps.vision {
            return Ok(());
        }
        Err(AgenticError::Provider(format!(
            "Model '{}' does not support image input. Switch to a vision-capable model \
             (e.g. gpt-4o, gpt-4o-mini, claude-3-5-sonnet, claude-3-5-haiku) via `/models`.",
            self.model
        )))
    }
}
