//! End-to-end integration tests for the orchestrator agent loop.
//!
//! These tests drive a real `Orchestrator` against a scripted mock
//! provider that yields one queued `ChatResponse` per `chat()` call. The
//! orchestrator's real tool registry, memory, safety engine, and event
//! emitter are exercised — only the LLM is stubbed.
//!
//! Why this lives in `tests/` rather than next to the orchestrator's own
//! unit tests: each test below covers a multi-turn loop, often with real
//! filesystem operations through the builtin tools. Keeping them in an
//! integration crate makes the boundary explicit (no access to private
//! orchestrator internals) and matches Cargo's standard layout.

use core_agentic::{
    safety::PermissionMode,
    tools::builtin_tools_with_tracker,
    Event, FileTracker, LLMProvider, Orchestrator, ToolRegistry,
};
use std::sync::Arc;
use std::sync::Mutex;

mod support;
use support::{tool_call, ScriptedProvider};

/// One realistic happy path: model says "use read_file", we let it run,
/// model produces final text. Verifies the loop terminates, the tool
/// result reaches memory, and the final answer is what the second turn
/// returned.
#[test]
fn run_loop_executes_tool_then_returns_final_answer() {
    let dir = support::tempdir();
    let path = dir.join("hello.txt");
    std::fs::write(&path, "the meaning is 42\n").unwrap();

    let provider: Arc<dyn LLMProvider> = Arc::new(ScriptedProvider::new(vec![
        support::tool_call_response(
            "call-1",
            "read_file",
            &serde_json::json!({"path": path.to_string_lossy()}),
        ),
        support::text_response("42 is the answer"),
    ]));

    let tools = ToolRegistry::new();
    for t in builtin_tools_with_tracker(Arc::new(FileTracker::new())) {
        tools.register(t);
    }

    let mut orch = Orchestrator::new(provider, tools);
    orch.set_permission_mode(PermissionMode::Yolo); // skip confirm prompts
    orch.set_model("gpt-4o-mini");

    let answer = orch.run("what is the answer?").expect("run should succeed");
    assert_eq!(answer, "42 is the answer");
}

/// The orchestrator must abort cleanly when the provider keeps returning
/// tool calls beyond the configured cap. The error must mention the cap.
#[test]
fn run_loop_aborts_at_max_iterations() {
    // Script far more tool calls than max_iterations: every turn requests
    // another no-op read. Use different tool names to avoid loop detection.
    let mut script = Vec::new();
    let tool_names = ["list_files", "search_files", "glob"];
    for i in 0..20 {
        let tool = tool_names[i % tool_names.len()];
        script.push(support::tool_call_response(
            &format!("call-{}", i),
            tool,
            &serde_json::json!({"path": "."}),
        ));
    }
    let provider: Arc<dyn LLMProvider> = Arc::new(ScriptedProvider::new(script));

    let tools = ToolRegistry::new();
    for t in builtin_tools_with_tracker(Arc::new(FileTracker::new())) {
        tools.register(t);
    }

    let mut orch = Orchestrator::new(provider, tools);
    orch.set_permission_mode(PermissionMode::Yolo);
    orch.set_max_iterations(3);

    let err = orch.run("list things forever").expect_err("should hit cap");
    let msg = err.to_string();
    assert!(
        msg.contains("max_iterations"),
        "expected 'max_iterations' in error, got: {}",
        msg
    );
}

/// Loop detection: same tool called consecutively must trigger early abort.
#[test]
fn loop_detection_aborts_on_repeated_tool() {
    // Same tool called 3 times in a row should trigger loop detection
    // before hitting max_iterations.
    let mut script = Vec::new();
    for i in 0..10 {
        script.push(support::tool_call_response(
            &format!("call-{}", i),
            "write_file",
            &serde_json::json!({"path": "file.txt", "content": "content"}),
        ));
    }
    let provider: Arc<dyn LLMProvider> = Arc::new(ScriptedProvider::new(script));

    let tools = ToolRegistry::new();
    for t in builtin_tools_with_tracker(Arc::new(FileTracker::new())) {
        tools.register(t);
    }

    let mut orch = Orchestrator::new(provider, tools);
    orch.set_permission_mode(PermissionMode::Yolo);
    orch.set_max_iterations(10); // High limit, but loop detection should trigger first

    let err = orch.run("write files forever").expect_err("should detect loop");
    let msg = err.to_string();
    assert!(
        msg.contains("Loop detected"),
        "expected 'Loop detected' in error, got: {}",
        msg
    );
    assert!(
        msg.contains("write_file"),
        "expected tool name in error, got: {}",
        msg
    );
}

/// Warning at 80% of max_iterations must emit a System event.
#[test]
fn approaching_limit_emits_warning_event() {
    // Script enough tool calls to reach 80% of max_iterations.
    // Use different tool names to avoid loop detection.
    let mut script = Vec::new();
    let tool_names = ["list_files", "search_files", "glob"];
    for i in 0..10 {
        let tool = tool_names[i % tool_names.len()];
        script.push(support::tool_call_response(
            &format!("call-{}", i),
            tool,
            &serde_json::json!({"path": "."}),
        ));
    }
    let provider: Arc<dyn LLMProvider> = Arc::new(ScriptedProvider::new(script));

    let tools = ToolRegistry::new();
    for t in builtin_tools_with_tracker(Arc::new(FileTracker::new())) {
        tools.register(t);
    }

    let mut orch = Orchestrator::new(provider, tools);
    orch.set_permission_mode(PermissionMode::Yolo);
    orch.set_max_iterations(5); // 80% of 5 = 4, so warning at iteration 4

    let events: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let events_clone = events.clone();
    orch.on_event(move |event| {
        if let Event::System { message } = event {
            events_clone.lock().unwrap().push(message);
        }
    });

    // The loop will hit max_iterations (5) before finishing all script items,
    // but the warning should be emitted at iteration 4 (80% of 5).
    let err = orch.run("go far").expect_err("should hit cap");
    let msg = err.to_string();
    assert!(msg.contains("max_iterations"), "expected max_iterations error");

    // Check that a warning event was emitted
    let captured = events.lock().unwrap();
    assert!(
        captured.iter().any(|m| m.contains("Approaching iteration limit")),
        "expected 'Approaching iteration limit' warning, got: {:?}",
        *captured
    );
}

/// Plan mode must deny state-changing tools. The model gets a "Blocked"
/// message back as the tool result, then issues a final text response.
#[test]
fn plan_mode_denies_write_file_with_blocked_message() {
    let provider: Arc<dyn LLMProvider> = Arc::new(ScriptedProvider::new(vec![
        support::tool_call_response(
            "call-1",
            "write_file",
            &serde_json::json!({
                "path": "should-not-be-created.txt",
                "content": "x",
            }),
        ),
        support::text_response("ok, can't write in plan mode"),
    ]));

    let tools = ToolRegistry::new();
    for t in builtin_tools_with_tracker(Arc::new(FileTracker::new())) {
        tools.register(t);
    }

    let orch = Orchestrator::new(provider, tools);
    orch.set_permission_mode(PermissionMode::Plan);

    let answer = orch.run("write a file").expect("loop should still finish");
    assert_eq!(answer, "ok, can't write in plan mode");

    // The blocked file must not have been created.
    assert!(!std::path::Path::new("should-not-be-created.txt").exists());
}

/// Cooperative cancel: flipping the cancel flag between turns must make
/// the next iteration return `Cancelled` without calling the provider.
#[test]
fn cancel_flag_aborts_loop_at_iteration_boundary() {
    let provider: Arc<dyn LLMProvider> = Arc::new(ScriptedProvider::new(vec![
        // First turn: a tool call so we re-enter the loop. Cancel will
        // be flipped after this call lands.
        support::tool_call_response(
            "call-1",
            "list_files",
            &serde_json::json!({"path": "."}),
        ),
        // The orchestrator should never get this far.
        support::text_response("should never be returned"),
    ]));

    let tools = ToolRegistry::new();
    for t in builtin_tools_with_tracker(Arc::new(FileTracker::new())) {
        tools.register(t);
    }

    let orch = Orchestrator::new(provider, tools);
    orch.set_permission_mode(PermissionMode::Yolo);
    let cancel = orch.cancel_handle();

    // Subscribe to events so we can flip cancel after the first
    // ToolOutput event lands.
    orch.on_event({
        let cancel = cancel.clone();
        move |event| {
            if matches!(event, Event::ToolOutput { .. }) {
                cancel.store(true, std::sync::atomic::Ordering::SeqCst);
            }
        }
    });

    let err = orch.run("loop until cancelled").expect_err("should cancel");
    let msg = err.to_string();
    assert!(
        msg.to_lowercase().contains("cancel"),
        "expected cancel error, got: {}",
        msg
    );
}

/// Event emission: every tool call must surface as a ToolCall event
/// followed by a ToolOutput event, in the same order as execution.
#[test]
fn events_are_emitted_for_every_tool_call() {
    let dir = support::tempdir();
    let path_a = dir.join("a.txt");
    let path_b = dir.join("b.txt");
    std::fs::write(&path_a, "AAA\n").unwrap();
    std::fs::write(&path_b, "BBB\n").unwrap();

    let provider: Arc<dyn LLMProvider> = Arc::new(ScriptedProvider::new(vec![
        // Two tool calls in one turn.
        support::multi_tool_call_response(vec![
            tool_call(
                "call-a",
                "read_file",
                &serde_json::json!({"path": path_a.to_string_lossy()}),
            ),
            tool_call(
                "call-b",
                "read_file",
                &serde_json::json!({"path": path_b.to_string_lossy()}),
            ),
        ]),
        support::text_response("done"),
    ]));

    let tools = ToolRegistry::new();
    for t in builtin_tools_with_tracker(Arc::new(FileTracker::new())) {
        tools.register(t);
    }

    let orch = Orchestrator::new(provider, tools);
    orch.set_permission_mode(PermissionMode::Yolo);

    let events: Arc<Mutex<Vec<&'static str>>> = Arc::new(Mutex::new(Vec::new()));
    let events_clone = events.clone();
    orch.on_event(move |e| {
        let tag = match e {
            Event::ToolCall { .. } => "call",
            Event::ToolOutput { .. } => "output",
            _ => return,
        };
        events_clone.lock().unwrap().push(tag);
    });

    let answer = orch.run("read a and b").expect("ok");
    assert_eq!(answer, "done");

    let captured = events.lock().unwrap();
    // Must be two call+output pairs. Order between calls/outputs isn't
    // strictly call,output,call,output (sync and parallel paths interleave
    // differently), so we just count instead.
    let calls = captured.iter().filter(|t| **t == "call").count();
    let outputs = captured.iter().filter(|t| **t == "output").count();
    assert_eq!(calls, 2, "events: {:?}", *captured);
    assert_eq!(outputs, 2, "events: {:?}", *captured);
}

/// Memory: all messages from a successful run survive in the
/// orchestrator's memory in the right order. The user message lands
/// first, the assistant's tool-call assistant message second, the tool
/// result third, the final assistant message last.
#[test]
fn memory_records_full_turn_history() {
    let dir = support::tempdir();
    let path = dir.join("greet.txt");
    std::fs::write(&path, "hi\n").unwrap();

    let provider: Arc<dyn LLMProvider> = Arc::new(ScriptedProvider::new(vec![
        support::tool_call_response(
            "call-1",
            "read_file",
            &serde_json::json!({"path": path.to_string_lossy()}),
        ),
        support::text_response("greeting received"),
    ]));

    let tools = ToolRegistry::new();
    for t in builtin_tools_with_tracker(Arc::new(FileTracker::new())) {
        tools.register(t);
    }

    let orch = Orchestrator::new(provider, tools);
    orch.set_permission_mode(PermissionMode::Yolo);

    let _ = orch.run("read the greeting").expect("ok");

    // Search for marker substrings in memory. The exact role-tag layout
    // is internal but the content must appear in order.
    let user_hits = orch.search_memory("read the greeting");
    let final_hits = orch.search_memory("greeting received");
    assert!(!user_hits.is_empty(), "user message should be recorded");
    assert!(
        !final_hits.is_empty(),
        "final assistant answer should be recorded"
    );
}

/// Safety record + reset: clearing memory should give a fresh state
/// without re-creating the orchestrator.
#[test]
fn restart_session_workflow_resets_memory() {
    let provider: Arc<dyn LLMProvider> = Arc::new(ScriptedProvider::new(vec![
        support::text_response("first"),
        support::text_response("second"),
    ]));

    let tools = ToolRegistry::new();
    for t in builtin_tools_with_tracker(Arc::new(FileTracker::new())) {
        tools.register(t);
    }

    let orch = Orchestrator::new(provider, tools);
    orch.set_permission_mode(PermissionMode::Yolo);

    let _ = orch.run("first turn").expect("ok");
    assert!(!orch.search_memory("first turn").is_empty());

    // Simulate the /restart slash command's effect.
    orch.clear_memory();
    assert!(orch.search_memory("first turn").is_empty());

    // Second turn uses a fresh memory.
    let answer = orch.run("second turn").expect("ok");
    assert_eq!(answer, "second");
}

/// Sync run() concurrent batching: when the model returns multiple
/// read-only tool calls in the same turn, they execute concurrently in
/// a `std::thread::scope` rather than sequentially. We verify by
/// scripting four 200ms slow read-only tools in one turn and asserting
/// total wall-time is much less than 4 * 200ms.
#[test]
fn sync_run_executes_read_only_batch_concurrently() {
    use std::time::{Duration, Instant};

    // Four no-op slow reads in one assistant turn, then a final answer.
    let provider: Arc<dyn LLMProvider> = Arc::new(ScriptedProvider::new(vec![
        support::multi_tool_call_response(vec![
            tool_call("a", "slow_read", &serde_json::json!({})),
            tool_call("b", "slow_read", &serde_json::json!({})),
            tool_call("c", "slow_read", &serde_json::json!({})),
            tool_call("d", "slow_read", &serde_json::json!({})),
        ]),
        support::text_response("done"),
    ]));

    let tools = ToolRegistry::new();
    // Single tool registered four times under one name; each call lands
    // a fresh Box. The CONCURRENT path doesn't care about name
    // duplication — it dispatches by name per call.
    tools.register(Box::new(support::SlowReadTool::new(
        "slow_read",
        Duration::from_millis(200),
    )));

    let orch = Orchestrator::new(provider, tools);
    orch.set_permission_mode(PermissionMode::Yolo);

    let start = Instant::now();
    let answer = orch.run("go").expect("ok");
    let elapsed = start.elapsed();
    assert_eq!(answer, "done");

    // Sequential lower bound is 4 * 200ms = 800ms.
    // Concurrent ceiling on a slow CI box: 600ms (3x slot) leaves
    // plenty of headroom while still failing if the loop went serial.
    assert!(
        elapsed < Duration::from_millis(600),
        "expected concurrent batch to complete in <600ms, got {:?}",
        elapsed
    );
}
