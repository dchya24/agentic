mod support;

use core_agentic::events::Event;
use core_agentic::providers::{
    ChatChunk, ChatMessageResponse, ChatRequest, ChatResponse, LLMProvider, ProviderError,
    ProviderResult, StreamResult,
};
use core_agentic::runtime::engine::RuntimeEngine;
use core_agentic::runtime::protocol::{ProtocolRequest, Request};
use core_agentic::runtime::transport::MemoryTransport;
use core_agentic::{QuestionAnswer, ToolRegistry};

struct TextProvider;

impl LLMProvider for TextProvider {
    fn provider_type(&self) -> &str {
        "test"
    }
    fn provider_id(&self) -> &str {
        "test"
    }
    fn chat(&self, _request: ChatRequest) -> ProviderResult<ChatResponse> {
        Ok(ChatResponse {
            id: "response".into(),
            model: "test".into(),
            message: ChatMessageResponse {
                role: "assistant".into(),
                content: Some("unused".into()),
                tool_calls: vec![],
            },
            finish_reason: Some("stop".into()),
            usage: None,
        })
    }
    fn chat_stream(&self, _request: ChatRequest) -> StreamResult<ChatChunk, ProviderError> {
        Ok(Box::pin(futures::stream::iter(vec![Ok(ChatChunk {
            id: "chunk".into(),
            delta: "hello".into(),
            finish_reason: Some("stop".into()),
            tool_calls: vec![],
            usage: None,
        })])))
    }
}

#[test]
fn runtime_emits_ready_init_ok_delta_and_done() {
    let (request_tx, request_rx) = std::sync::mpsc::channel();
    let (event_tx, event_rx) = std::sync::mpsc::channel();
    let transport = MemoryTransport::new(request_rx, event_tx);
    let mut engine = RuntimeEngine::with_session(
        transport,
        std::sync::Arc::new(TextProvider),
        ToolRegistry::new(),
    );
    let worker = std::thread::spawn(move || engine.run());

    request_tx
        .send(ProtocolRequest::new(
            "init-1",
            Request::Init {
                overrides: Default::default(),
            },
        ))
        .unwrap();
    request_tx
        .send(ProtocolRequest::new(
            "run-1",
            Request::Run {
                task: "hello".into(),
                attachments: vec![],
            },
        ))
        .unwrap();
    drop(request_tx);
    worker.join().unwrap();

    let events: Vec<_> = event_rx.try_iter().collect();
    assert!(events
        .iter()
        .any(|event| matches!(event.event, Event::Ready { .. })));
    assert!(events
        .iter()
        .any(|event| matches!(event.event, Event::InitOk { .. })));
    assert!(events.iter().any(
        |event| matches!(&event.event, Event::AssistantDelta { content } if content == "hello")
    ));
    assert!(events
        .iter()
        .any(|event| matches!(&event.event, Event::Done { result } if result == "hello")));
}

#[test]
fn question_request_waits_for_client_response() {
    let provider = support::StreamingScriptedProvider::new(vec![
        support::tool_call_response(
            "q1",
            "question",
            &serde_json::json!({"questions":[{"question":"Pick?","options":["A","B"]}]}),
        ),
        support::text_response("thanks"),
    ]);
    let (request_tx, request_rx) = std::sync::mpsc::channel();
    let (event_tx, event_rx) = std::sync::mpsc::channel();
    let mut engine = RuntimeEngine::with_provider(
        MemoryTransport::new(request_rx, event_tx),
        std::sync::Arc::new(provider),
    );
    let worker = std::thread::spawn(move || engine.run());

    request_tx
        .send(ProtocolRequest::new(
            "run-q",
            Request::Run {
                task: "ask".into(),
                attachments: vec![],
            },
        ))
        .unwrap();
    let mut saw_question = false;
    loop {
        let event = event_rx
            .recv_timeout(std::time::Duration::from_secs(2))
            .unwrap();
        if matches!(event.event, Event::QuestionRequest { .. }) {
            saw_question = true;
            request_tx
                .send(ProtocolRequest::new(
                    "answer-q",
                    Request::QuestionResponse {
                        request_id: Some("run-q".into()),
                        answers: vec![QuestionAnswer {
                            question: "Pick?".into(),
                            answer: vec!["A".into()],
                            skipped: false,
                        }],
                    },
                ))
                .unwrap();
        }
        if matches!(event.event, Event::Done { .. }) {
            break;
        }
    }
    request_tx
        .send(ProtocolRequest::new("shutdown", Request::Shutdown))
        .unwrap();
    worker.join().unwrap();
    assert!(saw_question);
}

#[test]
fn todo_changes_are_forwarded_as_events() {
    let provider = support::StreamingScriptedProvider::new(vec![
        support::tool_call_response(
            "todo1",
            "todowrite",
            &serde_json::json!({"todos":[{"content":"ship runtime"}]}),
        ),
        support::text_response("tracked"),
    ]);
    let (request_tx, request_rx) = std::sync::mpsc::channel();
    let (event_tx, event_rx) = std::sync::mpsc::channel();
    let mut engine = RuntimeEngine::with_provider(
        MemoryTransport::new(request_rx, event_tx),
        std::sync::Arc::new(provider),
    );
    let worker = std::thread::spawn(move || engine.run());
    request_tx
        .send(ProtocolRequest::new(
            "run-t",
            Request::Run {
                task: "track".into(),
                attachments: vec![],
            },
        ))
        .unwrap();

    let mut saw_todo = false;
    let mut saw_tool_finished = false;
    loop {
        let event = event_rx
            .recv_timeout(std::time::Duration::from_secs(2))
            .unwrap();
        if matches!(event.event, Event::TodoChanged { .. }) {
            saw_todo = true;
        }
        if matches!(event.event, Event::ToolFinished { .. }) {
            saw_tool_finished = true;
        }
        if matches!(event.event, Event::Done { .. }) {
            break;
        }
    }
    request_tx
        .send(ProtocolRequest::new("shutdown", Request::Shutdown))
        .unwrap();
    worker.join().unwrap();
    assert!(saw_todo);
    assert!(saw_tool_finished);
}

#[test]
fn confirmation_request_waits_for_client_approval() {
    let provider = support::StreamingScriptedProvider::new(vec![
        support::tool_call_response(
            "cmd1",
            "run_command",
            &serde_json::json!({"command":"curl --version"}),
        ),
        support::text_response("executed"),
    ]);
    let (request_tx, request_rx) = std::sync::mpsc::channel();
    let (event_tx, event_rx) = std::sync::mpsc::channel();
    let mut engine = RuntimeEngine::with_provider(
        MemoryTransport::new(request_rx, event_tx),
        std::sync::Arc::new(provider),
    );
    let worker = std::thread::spawn(move || engine.run());
    request_tx
        .send(ProtocolRequest::new(
            "run-c",
            Request::Run {
                task: "execute".into(),
                attachments: vec![],
            },
        ))
        .unwrap();

    let mut saw_confirmation = false;
    loop {
        let event = event_rx
            .recv_timeout(std::time::Duration::from_secs(2))
            .unwrap();
        if matches!(event.event, Event::ConfirmationRequest { .. }) {
            saw_confirmation = true;
            request_tx
                .send(ProtocolRequest::new(
                    "approve-c",
                    Request::ConfirmResponse {
                        request_id: Some("run-c".into()),
                        approved: true,
                    },
                ))
                .unwrap();
        }
        if matches!(event.event, Event::Done { .. }) {
            break;
        }
    }
    request_tx
        .send(ProtocolRequest::new("shutdown", Request::Shutdown))
        .unwrap();
    worker.join().unwrap();
    assert!(saw_confirmation);
}
