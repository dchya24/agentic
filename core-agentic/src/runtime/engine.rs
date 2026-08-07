use super::protocol::{ProtocolEvent, Request, PROTOCOL_NAME, PROTOCOL_VERSION};
use super::transport::Transport;
use crate::events::Event;
use crate::providers::{LLMProvider, OpenAIProvider};
use crate::{
    Config, Orchestrator, QuestionAnswer, QuestionHandler, QuestionPrompt, SkillTool,
    SpawnSubagentTool, TodoChangeHandler, TodoItem, ToolDeps, ToolRegistry,
};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Mutex};

pub struct RuntimeEngine<T: Transport> {
    transport: Arc<T>,
    orchestrator: Option<Arc<Orchestrator>>,
    running: Arc<AtomicBool>,
    current_request_id: Arc<Mutex<Option<String>>>,
    pending_question: Arc<Mutex<Option<mpsc::Sender<Vec<QuestionAnswer>>>>>,
    pending_confirmation: Arc<Mutex<Option<mpsc::Sender<bool>>>>,
    run_threads: Vec<std::thread::JoinHandle<()>>,
}

impl<T: Transport> RuntimeEngine<T> {
    pub fn new(transport: T) -> Self {
        Self {
            transport: Arc::new(transport),
            orchestrator: None,
            running: Arc::new(AtomicBool::new(false)),
            current_request_id: Arc::new(Mutex::new(None)),
            pending_question: Arc::new(Mutex::new(None)),
            pending_confirmation: Arc::new(Mutex::new(None)),
            run_threads: Vec::new(),
        }
    }

    pub fn with_session(transport: T, provider: Arc<dyn LLMProvider>, tools: ToolRegistry) -> Self {
        let mut engine = Self::new(transport);
        engine.orchestrator = Some(Arc::new(Orchestrator::new(provider, tools)));
        engine
    }

    pub fn with_provider(transport: T, provider: Arc<dyn LLMProvider>) -> Self {
        let mut engine = Self::new(transport);
        let question_handler = RuntimeQuestionHandler {
            transport: engine.transport.clone(),
            current_request_id: engine.current_request_id.clone(),
            pending: engine.pending_question.clone(),
        };
        let todo_handler = RuntimeTodoHandler {
            transport: engine.transport.clone(),
            current_request_id: engine.current_request_id.clone(),
        };
        let mut deps = ToolDeps::new();
        deps.question_handler = Some(Arc::new(question_handler));
        deps.todo_handler = Some(Box::new(todo_handler));
        let tools = ToolRegistry::new();
        for tool in crate::tools::builtin_tools_with_deps(deps) {
            tools.register(tool);
        }
        let mut orchestrator = Orchestrator::new(provider, tools);
        let transport = engine.transport.clone();
        let current_request_id = engine.current_request_id.clone();
        let pending_confirmation = engine.pending_confirmation.clone();
        orchestrator.set_confirmation_handler(move |request| {
            let (sender, receiver) = mpsc::channel();
            *pending_confirmation.lock().unwrap() = Some(sender);
            let request_id = current_request_id.lock().unwrap().clone();
            let _ = transport.write_event(&ProtocolEvent::new(
                request_id,
                Event::ConfirmationRequest {
                    action: request.action,
                    description: request.description,
                    risk_level: request.risk_level.as_str().to_string(),
                },
            ));
            receiver.recv().unwrap_or(false)
        });
        engine.orchestrator = Some(Arc::new(orchestrator));
        engine
    }

    pub fn run(&mut self) {
        self.emit(
            None,
            Event::Ready {
                protocol: PROTOCOL_NAME.to_string(),
                version: PROTOCOL_VERSION,
            },
        );

        loop {
            let request = match self.transport.read_request() {
                Ok(Some(request)) => request,
                Ok(None) => break,
                Err(error) => {
                    self.emit(
                        None,
                        Event::Error {
                            message: format!("protocol_error: {error}"),
                        },
                    );
                    continue;
                }
            };
            if request.v != PROTOCOL_VERSION {
                self.emit(
                    Some(request.id),
                    Event::Error {
                        message: format!("unsupported protocol version: {}", request.v),
                    },
                );
                continue;
            }

            match request.request {
                Request::Init { overrides } => {
                    let result = if self.orchestrator.is_none() {
                        self.initialize(overrides)
                    } else {
                        Ok(())
                    };
                    match result {
                        Ok(()) => self.emit(
                            Some(request.id),
                            Event::InitOk {
                                protocol: PROTOCOL_NAME.to_string(),
                                version: PROTOCOL_VERSION,
                            },
                        ),
                        Err(message) => self.emit(Some(request.id), Event::Error { message }),
                    }
                }
                Request::Run { task, attachments } => {
                    if self.orchestrator.is_none() {
                        if let Err(message) = self.initialize(Default::default()) {
                            self.emit(Some(request.id), Event::Error { message });
                            continue;
                        }
                    }
                    self.start_run(request.id, task, attachments);
                }
                Request::Cancel => {
                    if let Some(orchestrator) = self.orchestrator.as_ref() {
                        orchestrator.cancel_handle().store(true, Ordering::SeqCst);
                    }
                }
                Request::ResetSession => {
                    if let Some(orchestrator) = self.orchestrator.as_ref() {
                        orchestrator.clear_memory();
                        orchestrator.reset_cancel();
                        orchestrator.clear_event_handlers();
                        self.emit(Some(request.id), Event::SessionReset);
                    } else {
                        self.emit(
                            Some(request.id),
                            Event::Error {
                                message: "runtime is not initialized".to_string(),
                            },
                        );
                    }
                }
                Request::SearchMemory { query } => {
                    let matches = self
                        .orchestrator
                        .as_ref()
                        .map(|orchestrator| {
                            orchestrator
                                .search_memory(&query)
                                .into_iter()
                                .map(|(role, content)| crate::events::MemorySearchMatch {
                                    role: role.as_str().to_string(),
                                    content,
                                })
                                .collect()
                        })
                        .unwrap_or_default();
                    self.emit(
                        Some(request.id),
                        Event::MemorySearchResult { query, matches },
                    );
                }
                Request::AddSystemMessage { content } => {
                    if let Some(orchestrator) = self.orchestrator.as_ref() {
                        orchestrator.add_system_message(content);
                    } else {
                        self.emit(
                            Some(request.id),
                            Event::Error {
                                message: "runtime is not initialized".to_string(),
                            },
                        );
                    }
                }
                Request::Plan { .. } => self.emit(
                    Some(request.id),
                    Event::Error {
                        message: "planning is not available through the runtime yet".to_string(),
                    },
                ),
                Request::Shutdown => break,
                Request::QuestionResponse { answers, .. } => {
                    if let Some(sender) = self.pending_question.lock().unwrap().take() {
                        let _ = sender.send(answers);
                    } else {
                        self.emit(
                            Some(request.id),
                            Event::Warning {
                                message: "no question response is pending".to_string(),
                            },
                        );
                    }
                }
                Request::ConfirmResponse { approved, .. } => {
                    if let Some(sender) = self.pending_confirmation.lock().unwrap().take() {
                        let _ = sender.send(approved);
                    } else {
                        self.emit(
                            Some(request.id),
                            Event::Warning {
                                message: "no confirmation response is pending".to_string(),
                            },
                        );
                    }
                }
            }
        }

        for handle in self.run_threads.drain(..) {
            let _ = handle.join();
        }
    }

    fn initialize(&mut self, overrides: super::protocol::InitOverrides) -> Result<(), String> {
        let config = match overrides.config_path.as_deref() {
            Some(path) => Config::load_from_path(path)
                .ok_or_else(|| format!("failed to load config from: {path}"))?,
            None => Config::load().unwrap_or_else(Config::fallback),
        };
        let provider_config = config
            .to_provider_config()
            .ok_or_else(|| "no provider configured".to_string())?;
        let model = overrides
            .model
            .clone()
            .unwrap_or_else(|| provider_config.default_model.clone());
        let provider: Arc<dyn LLMProvider> = Arc::new(OpenAIProvider::new(provider_config));

        let mut deps = self.runtime_tool_deps();
        deps.url_policy = config.url_policy();
        let tools = ToolRegistry::new();
        for tool in crate::tools::builtin_tools_with_deps(deps) {
            tools.register(tool);
        }
        tools.register(Box::new(
            SpawnSubagentTool::new(provider.clone(), tools.clone(), model.clone())
                .with_mode(overrides.permission_mode.unwrap_or_default()),
        ));

        let cwd = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
        let discovery_config: crate::DiscoveryConfig = (&config.skills).into();
        let skill_index = Arc::new(std::sync::RwLock::new(crate::discover_skills(
            &discovery_config,
        )));
        tools.register(Box::new(SkillTool::new(skill_index.clone())));

        let project_instructions = crate::load_project_instructions(&cwd).map(|(_, text)| text);
        let skills_section = {
            let index = skill_index.read().unwrap();
            let pairs: Vec<(&str, &str)> = index
                .all()
                .iter()
                .map(|skill| (skill.name(), skill.description()))
                .collect();
            crate::skills_system_section(&pairs)
        };
        let prompt_override = overrides
            .system_prompt
            .as_deref()
            .or(config.system_prompt.as_deref());
        let mut system_prompt = crate::assemble_system_prompt(
            None,
            project_instructions.as_deref(),
            skills_section.as_deref(),
            prompt_override,
        );
        if let Some(memory) = crate::assemble_memory_section(&cwd) {
            system_prompt.push_str("\n\n---\n# Persistent Memory\n\n");
            system_prompt.push_str(&memory);
        }

        let mut orchestrator = Orchestrator::new(provider, tools);
        orchestrator.set_model(model);
        orchestrator.set_system_prompt(system_prompt);
        orchestrator.set_permission_mode(overrides.permission_mode.unwrap_or_default());
        if config.agent.auto_compact_with_llm {
            orchestrator.set_auto_compact_with_llm(true);
        }
        if let Some(summarizer) = config.agent.summarizer_model {
            orchestrator.set_summarizer_model(summarizer);
        }
        if let Some(max_iterations) = config.agent.max_iterations {
            orchestrator.set_max_iterations(max_iterations);
        }
        self.install_confirmation_handler(&mut orchestrator);
        self.orchestrator = Some(Arc::new(orchestrator));
        Ok(())
    }

    fn runtime_tool_deps(&self) -> ToolDeps {
        let mut deps = ToolDeps::new();
        deps.question_handler = Some(Arc::new(RuntimeQuestionHandler {
            transport: self.transport.clone(),
            current_request_id: self.current_request_id.clone(),
            pending: self.pending_question.clone(),
        }));
        deps.todo_handler = Some(Box::new(RuntimeTodoHandler {
            transport: self.transport.clone(),
            current_request_id: self.current_request_id.clone(),
        }));
        deps
    }

    fn install_confirmation_handler(&self, orchestrator: &mut Orchestrator) {
        let transport = self.transport.clone();
        let current_request_id = self.current_request_id.clone();
        let pending_confirmation = self.pending_confirmation.clone();
        orchestrator.set_confirmation_handler(move |request| {
            let (sender, receiver) = mpsc::channel();
            *pending_confirmation.lock().unwrap() = Some(sender);
            let request_id = current_request_id.lock().unwrap().clone();
            let _ = transport.write_event(&ProtocolEvent::new(
                request_id,
                Event::ConfirmationRequest {
                    action: request.action,
                    description: request.description,
                    risk_level: request.risk_level.as_str().to_string(),
                },
            ));
            receiver.recv().unwrap_or(false)
        });
    }

    fn start_run(&mut self, request_id: String, task: String, attachments: Vec<crate::Attachment>) {
        if self.running.swap(true, Ordering::SeqCst) {
            self.emit(
                Some(request_id),
                Event::Error {
                    message: "busy: another run is active".to_string(),
                },
            );
            return;
        }

        let Some(orchestrator) = self.orchestrator.clone() else {
            self.running.store(false, Ordering::SeqCst);
            self.emit(
                Some(request_id),
                Event::Error {
                    message: "runtime is not initialized".to_string(),
                },
            );
            return;
        };

        *self.current_request_id.lock().unwrap() = Some(request_id.clone());
        orchestrator.reset_cancel();
        orchestrator.clear_event_handlers();

        let transport = self.transport.clone();
        let running = self.running.clone();
        let current_request_id = self.current_request_id.clone();
        let event_request_id = request_id.clone();
        orchestrator.on_event(move |event| {
            let finished = match &event {
                Event::ToolOutput {
                    tool_call_id,
                    tool_name,
                    success,
                    ..
                } => Some(Event::ToolFinished {
                    tool_call_id: tool_call_id.clone(),
                    tool_name: tool_name.clone(),
                    success: *success,
                }),
                _ => None,
            };
            let _ =
                transport.write_event(&ProtocolEvent::new(Some(event_request_id.clone()), event));
            if let Some(finished) = finished {
                let _ = transport.write_event(&ProtocolEvent::new(
                    Some(event_request_id.clone()),
                    finished,
                ));
            }
        });

        let transport = self.transport.clone();
        self.run_threads.push(std::thread::spawn(move || {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("runtime engine tokio runtime");
            let delta_transport = transport.clone();
            let delta_request_id = request_id.clone();
            let result = runtime.block_on(orchestrator.run_stream_with_attachments(
                &task,
                attachments,
                move |content| {
                    let _ = delta_transport.write_event(&ProtocolEvent::new(
                        Some(delta_request_id.clone()),
                        Event::AssistantDelta { content },
                    ));
                },
            ));

            let event = match result {
                Ok(result) => Event::Done { result },
                Err(error) => Event::Error {
                    message: error.to_string(),
                },
            };
            let _ = transport.write_event(&ProtocolEvent::new(Some(request_id), event));
            running.store(false, Ordering::SeqCst);
            *current_request_id.lock().unwrap() = None;
        }));
    }

    fn emit(&self, request_id: Option<String>, event: Event) {
        if let Err(error) = self
            .transport
            .write_event(&ProtocolEvent::new(request_id, event))
        {
            tracing::warn!(%error, "failed to write runtime event");
        }
    }
}

struct RuntimeQuestionHandler<T: Transport> {
    transport: Arc<T>,
    current_request_id: Arc<Mutex<Option<String>>>,
    pending: Arc<Mutex<Option<mpsc::Sender<Vec<QuestionAnswer>>>>>,
}

impl<T: Transport> QuestionHandler for RuntimeQuestionHandler<T> {
    fn handle(&self, questions: &[QuestionPrompt]) -> Vec<QuestionAnswer> {
        let request_id = self.current_request_id.lock().unwrap().clone();
        let (sender, receiver) = mpsc::channel();
        *self.pending.lock().unwrap() = Some(sender);
        let _ = self.transport.write_event(&ProtocolEvent::new(
            request_id,
            Event::QuestionRequest {
                questions: questions.to_vec(),
            },
        ));
        receiver.recv().unwrap_or_else(|_| {
            questions
                .iter()
                .map(|question| QuestionAnswer {
                    question: question.question.clone(),
                    answer: vec![],
                    skipped: true,
                })
                .collect()
        })
    }
}

struct RuntimeTodoHandler<T: Transport> {
    transport: Arc<T>,
    current_request_id: Arc<Mutex<Option<String>>>,
}

impl<T: Transport> TodoChangeHandler for RuntimeTodoHandler<T> {
    fn on_change(&self, todos: &[TodoItem]) {
        let request_id = self.current_request_id.lock().unwrap().clone();
        let _ = self.transport.write_event(&ProtocolEvent::new(
            request_id,
            Event::TodoChanged {
                todos: todos.to_vec(),
            },
        ));
    }
}
