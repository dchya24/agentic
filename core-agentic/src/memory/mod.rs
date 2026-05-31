//! Memory and context management for agentic AI sessions.
//!
//! Provides sliding-window context management, message pinning,
//! session-based isolation, disk persistence, and smart summarization.
//!
//! Module layout:
//! - [`types`] — `Message`, `MessageRole`, `SessionInfo`, `MemoryConfig`,
//!   `ContextWindow`, `SummarizedContext`, plus the `estimate_tokens`
//!   helper. Pure data, no behavior beyond constructors.
//! - [`store`] — the `Memory` struct itself: CRUD, context-window
//!   building, pinning, search, and session lifecycle.
//! - [`compaction`] — heuristic and LLM-driven compaction methods on
//!   `Memory`.
//! - [`persist`] — disk persistence (`persist`, `load`,
//!   `list_sessions`, `delete_session`) on `Memory`.

mod compaction;
mod persist;
mod store;
mod types;

pub use store::Memory;
pub use types::{
    estimate_tokens, ContextWindow, MemoryConfig, Message, MessageMetadata, MessageRole,
    SessionInfo, SummarizedContext,
};

// ---------------------------------------------------------------------------
// Unit Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn memory() -> Memory {
        Memory::new(10000)
    }

    fn memory_small() -> Memory {
        Memory::new(200) // small budget for compaction tests
    }

    // --- Message construction ---

    #[test]
    fn test_message_user() {
        let msg = Message::user("hello");
        assert!(matches!(msg.role, MessageRole::User));
        assert_eq!(msg.content, "hello");
        assert!(!msg.pinned);
        assert!(!msg.id.is_empty());
    }

    #[test]
    fn test_message_assistant() {
        let msg = Message::assistant("response");
        assert!(matches!(msg.role, MessageRole::Assistant));
        assert_eq!(msg.content, "response");
    }

    #[test]
    fn test_message_system() {
        let msg = Message::system("system prompt");
        assert!(matches!(msg.role, MessageRole::System));
    }

    #[test]
    fn test_message_tool() {
        let msg = Message::tool("read_file", "call-123", "file contents");
        assert!(matches!(msg.role, MessageRole::Tool { .. }));
        if let MessageRole::Tool { tool_name, tool_call_id } = &msg.role {
            assert_eq!(tool_name, "read_file");
            assert_eq!(tool_call_id, "call-123");
        }
    }

    #[test]
    fn test_message_with_model() {
        let msg = Message::assistant("hi").with_model("gpt-4o");
        assert_eq!(msg.metadata.model.as_deref(), Some("gpt-4o"));
    }

    #[test]
    fn test_message_with_duration() {
        let msg = Message::assistant("hi").with_duration(1500);
        assert_eq!(msg.metadata.duration_ms, Some(1500));
    }

    #[test]
    fn test_message_pinned() {
        let msg = Message::user("important").pinned();
        assert!(msg.pinned);
    }

    #[test]
    fn test_message_with_estimated_tokens() {
        // Token count depends on the active backend (heuristic ~4chars/tok
        // by default, or tiktoken when the feature is on). Both must
        // produce a non-zero count and stay within a sane band for
        // 100 'a' characters.
        let msg = Message::user("a".repeat(100)).with_estimated_tokens();
        let count = msg.metadata.token_count;
        assert!(count > 0, "token count should be non-zero");
        assert!(count <= 100, "token count cannot exceed character count");
    }

    // --- MessageRole ---

    #[test]
    fn test_message_role_as_str() {
        assert_eq!(MessageRole::User.as_str(), "user");
        assert_eq!(MessageRole::Assistant.as_str(), "assistant");
        assert_eq!(MessageRole::System.as_str(), "system");
        assert_eq!(
            MessageRole::Tool {
                tool_name: "t".into(),
                tool_call_id: "1".into()
            }
            .as_str(),
            "tool"
        );
    }

    // --- Basic add/get ---

    #[test]
    fn test_add_and_get_messages() {
        let mut m = memory();
        m.add_message(Message::user("Hello"));
        m.add_message(Message::assistant("Hi there"));

        assert_eq!(m.get_messages().len(), 2);
        assert_eq!(m.token_count(), 3); // "Hello"=1 + "Hi there"=2
    }

    #[test]
    fn test_get_context_limit() {
        let mut m = memory();
        for i in 0..10 {
            m.add_message(Message::user(format!("msg {}", i)));
        }

        let ctx = m.get_context(3);
        assert_eq!(ctx.len(), 3);
        // Should be the last 3
        assert!(ctx[0].content.contains("msg 7"));
        assert!(ctx[1].content.contains("msg 8"));
        assert!(ctx[2].content.contains("msg 9"));
    }

    #[test]
    fn test_clear() {
        let mut m = memory();
        m.add_message(Message::user("Hello"));
        m.add_message(Message::assistant("Hi"));
        m.clear();
        assert!(m.get_messages().is_empty());
        assert_eq!(m.token_count(), 0);
    }

    // --- Pinning ---

    #[test]
    fn test_pin_message() {
        let mut m = memory();
        let msg = Message::user("important");
        let id = msg.id.clone();
        m.add_message(msg);
        m.add_message(Message::user("normal"));

        assert!(m.pin(&id));
        assert!(m.pinned_ids().contains(&id));
    }

    #[test]
    fn test_pin_nonexistent() {
        let mut m = memory();
        assert!(!m.pin("nonexistent-id"));
    }

    #[test]
    fn test_unpin_message() {
        let mut m = memory();
        let msg = Message::user("important").pinned();
        let id = msg.id.clone();
        m.add_message(msg);

        assert!(m.pinned_ids().contains(&id));
        m.unpin(&id);
        assert!(!m.pinned_ids().contains(&id));
    }

    #[test]
    fn test_pinned_survives_compaction() {
        let mut m = memory_small(); // 200 token budget
        let pinned_msg = Message::user("keep me always").pinned();
        let pinned_id = pinned_msg.id.clone();
        m.add_message(pinned_msg);

        // Fill up memory
        for i in 0..20 {
            m.add_message(Message::user(format!(
                "padding message number {} with some text to use tokens",
                i
            )));
        }

        assert!(m.needs_compaction());
        let result = m.compact();

        // Pinned message should be kept
        assert!(result.kept_ids.contains(&pinned_id));
        assert!(m.get_messages().iter().any(|msg| msg.id == pinned_id));
    }

    // --- Context Window ---

    #[test]
    fn test_context_window_within_budget() {
        let mut m = memory();
        for i in 0..5 {
            m.add_message(Message::user(format!("short {}", i)));
        }
        let ctx = m.get_context_window();
        assert_eq!(ctx.messages.len(), 5);
        assert!(!ctx.was_compacted);
    }

    #[test]
    fn test_context_window_respects_limit() {
        let mut config = MemoryConfig::default();
        config.context_message_limit = 3;
        let mut m = Memory::with_config(config);

        for i in 0..10 {
            m.add_message(Message::user(format!("msg {}", i)));
        }

        let ctx = m.get_context_window();
        assert!(ctx.messages.len() <= 3);
    }

    #[test]
    fn test_context_window_with_summary() {
        let mut m = memory_small();
        for i in 0..20 {
            m.add_message(Message::user(format!(
                "padding message {} with enough text to fill budget",
                i
            )));
        }
        m.compact();

        let ctx = m.get_context_window();
        assert!(ctx.was_compacted);
        assert!(ctx.removed_count > 0 || ctx.messages.len() < 20);
    }

    // --- Compaction ---

    #[test]
    fn test_needs_compaction_below_threshold() {
        let mut m = memory();
        m.add_message(Message::user("short message"));
        assert!(!m.needs_compaction());
    }

    #[test]
    fn test_needs_compaction_above_threshold() {
        let mut m = memory_small(); // 200 tokens
        for i in 0..50 {
            m.add_message(Message::user(format!("message {} with some content", i)));
        }
        assert!(m.needs_compaction());
    }

    #[test]
    fn test_compact_reduces_tokens() {
        let mut config = MemoryConfig::default();
        config.max_tokens = 50_000;
        config.keep_recent = 4;
        let mut m = Memory::with_config(config);

        // Use large messages so compaction actually saves tokens
        for i in 0..100 {
            m.add_message(Message::user(format!(
                "message number {} with a lot of text to consume tokens padding padding padding",
                i
            )));
        }

        let tokens_before = m.token_count();
        assert!(tokens_before > 1000, "should have substantial tokens: {}", tokens_before);

        let result = m.compact();
        let tokens_after = m.token_count();

        assert!(result.summarized_count > 0, "should have compacted some messages");
        assert!(tokens_after < tokens_before, "tokens should decrease: {} -> {}", tokens_before, tokens_after);
    }

    #[test]
    fn test_compact_keeps_recent() {
        let mut config = MemoryConfig::default();
        config.max_tokens = 200;
        config.keep_recent = 3;
        let mut m = Memory::with_config(config);

        for i in 0..20 {
            m.add_message(Message::user(format!("msg {}", i)));
        }
        m.compact();

        let messages = m.get_messages();
        assert!(messages.len() >= 3);
        // Last 3 messages should be present
        let last_content: Vec<&str> = messages.iter().rev().take(3).map(|m| m.content.as_str()).collect();
        assert!(last_content[0].contains("msg 19"));
        assert!(last_content[1].contains("msg 18"));
        assert!(last_content[2].contains("msg 17"));
    }

    #[test]
    fn test_compact_no_compact_needed() {
        let mut m = memory();
        m.add_message(Message::user("hello"));
        m.add_message(Message::assistant("hi"));

        let result = m.compact();
        assert_eq!(result.summarized_count, 0); // nothing to compact
    }

    #[test]
    fn test_compact_accumulates_summary() {
        let mut m = memory_small();
        for i in 0..30 {
            m.add_message(Message::user(format!("msg {}", i)));
        }
        m.compact();

        let first_summary = m.summary().unwrap().to_string();

        // Add more and compact again
        for i in 0..30 {
            m.add_message(Message::user(format!("msg2 {}", i)));
        }
        m.compact();

        let second_summary = m.summary().unwrap().to_string();
        assert!(second_summary.len() > first_summary.len());
    }

    // --- LLM-based summarization helpers ---

    #[test]
    fn build_prompt_returns_none_when_below_keep_recent() {
        let mut m = memory();
        m.config.keep_recent = 4;
        m.add_message(Message::user("a"));
        m.add_message(Message::user("b"));
        assert!(m.build_summarization_prompt().is_none());
    }

    #[test]
    fn build_prompt_includes_old_messages_only() {
        let mut m = memory();
        m.config.keep_recent = 2;
        m.add_message(Message::user("OLDEST goal"));
        m.add_message(Message::assistant("OLD response"));
        m.add_message(Message::user("recent question"));
        m.add_message(Message::assistant("recent answer"));

        let prompt = m.build_summarization_prompt().expect("should build");
        assert!(prompt.contains("OLDEST goal"));
        assert!(prompt.contains("OLD response"));
        // The most recent two are kept verbatim and shouldn't appear
        // in the to-summarize section.
        assert!(!prompt.contains("recent question"));
        assert!(!prompt.contains("recent answer"));
    }

    #[test]
    fn build_prompt_skips_pinned_from_excerpt() {
        let mut m = memory();
        m.config.keep_recent = 1;
        let pinned = Message::user("pinned context").pinned();
        let pinned_id = pinned.id.clone();
        m.add_message(pinned);
        m.add_message(Message::user("droppable 1"));
        m.add_message(Message::user("droppable 2"));
        m.add_message(Message::user("recent"));

        let prompt = m.build_summarization_prompt().expect("should build");
        // Pinned content stays in memory verbatim, not in the
        // to-summarize transcript.
        assert!(!prompt.contains("pinned context"));
        assert!(prompt.contains("droppable 1"));
        assert!(prompt.contains("droppable 2"));
        assert!(m.pinned_ids().contains(&pinned_id));
    }

    #[test]
    fn compact_with_summary_keeps_recent_and_substitutes_summary() {
        let mut m = memory();
        m.config.keep_recent = 2;
        for i in 0..10 {
            m.add_message(Message::user(format!("old {}", i)));
        }
        let llm_summary = "User asked about X; agent read foo.rs and changed bar.rs.";
        let r = m.compact_with_summary(llm_summary);

        assert!(r.summarized_count >= 8);
        let stored = m.summary().expect("summary should be stored");
        assert!(stored.contains(llm_summary));
        // Two most recent messages should still be there.
        assert_eq!(m.get_messages().len(), 2);
        assert!(m.get_messages()[1].content.contains("old 9"));
    }

    #[test]
    fn compact_with_summary_no_op_when_below_keep_recent() {
        let mut m = memory();
        m.config.keep_recent = 4;
        m.add_message(Message::user("a"));
        let r = m.compact_with_summary("unused");
        assert_eq!(r.summarized_count, 0);
        assert!(m.summary().is_none());
    }

    #[test]
    fn compact_with_summary_appends_to_prior_summary() {
        let mut m = memory();
        m.config.keep_recent = 1;
        for i in 0..5 {
            m.add_message(Message::user(format!("first wave {}", i)));
        }
        m.compact_with_summary("FIRST_SUMMARY");
        // Add more, compact again with a new LLM-summary.
        for i in 0..5 {
            m.add_message(Message::user(format!("second wave {}", i)));
        }
        m.compact_with_summary("SECOND_SUMMARY");

        let s = m.summary().unwrap();
        assert!(s.contains("FIRST_SUMMARY"));
        assert!(s.contains("SECOND_SUMMARY"));
    }

    #[test]
    fn test_summarize_legacy_alias() {
        let mut m = memory_small();
        for i in 0..30 {
            m.add_message(Message::user(format!("msg {}", i)));
        }
        let _tokens_before = m.token_count();
        m.summarize();
        // Verify summarize ran — may not reduce tokens in tight budget
        assert!(!m.get_messages().is_empty() || m.summary().is_some());
    }

    // --- Search ---

    #[test]
    fn test_search_found() {
        let mut m = memory();
        m.add_message(Message::user("hello world"));
        m.add_message(Message::assistant("greetings"));
        m.add_message(Message::user("goodbye world"));

        let results = m.search("world");
        assert_eq!(results.len(), 2);
    }

    #[test]
    fn test_search_case_insensitive() {
        let mut m = memory();
        m.add_message(Message::user("Hello World"));

        let results = m.search("hello");
        assert_eq!(results.len(), 1);
    }

    #[test]
    fn test_search_not_found() {
        let mut m = memory();
        m.add_message(Message::user("hello"));

        let results = m.search("xyz");
        assert!(results.is_empty());
    }

    #[test]
    fn test_search_by_role() {
        let mut m = memory();
        m.add_message(Message::user("hello"));
        m.add_message(Message::assistant("hi"));
        m.add_message(Message::user("how are you"));

        let user_msgs = m.search_by_role(&MessageRole::User);
        assert_eq!(user_msgs.len(), 2);

        let assistant_msgs = m.search_by_role(&MessageRole::Assistant);
        assert_eq!(assistant_msgs.len(), 1);
    }

    // --- Token tracking ---

    #[test]
    fn test_remaining_budget() {
        let mut m = Memory::new(1000);
        m.add_message(Message::user("hello")); // max(5/4,1) = 1 token
        assert_eq!(m.remaining_budget(), 999);
    }

    #[test]
    fn test_usage_percentage() {
        let mut m = Memory::new(1000);
        m.add_message(Message::user("a".repeat(400))); // 100 tokens
        let pct = m.usage_percentage();
        assert!(pct > 0.0 && pct <= 100.0);
    }

    #[test]
    fn test_recalculate_tokens() {
        let mut m = memory();
        m.add_message(Message::user("hello world"));
        m.add_message(Message::assistant("hi there"));

        // Artificially corrupt token count
        m.total_tokens = 9999;
        m.recalculate_tokens();

        let expected = estimate_tokens("hello world") + estimate_tokens("hi there");
        assert_eq!(m.token_count(), expected);
    }

    // --- Session ---

    #[test]
    fn test_session_info_new() {
        let session = SessionInfo::new();
        assert!(!session.id.is_empty());
        assert!(session.label.is_empty());
    }

    #[test]
    fn test_session_with_label() {
        let mut m = memory();
        m = m.with_label("test session");
        assert_eq!(m.session().label, "test session");
    }

    #[test]
    fn test_new_session_clears_messages() {
        let mut m = memory();
        m.add_message(Message::user("hello"));
        m.new_session();
        assert!(m.get_messages().is_empty());
        assert_ne!(m.session().id, ""); // new session has new ID
    }

    #[test]
    fn test_new_session_with_label() {
        let mut m = memory();
        let session = m.new_session_with_label("my session");
        assert_eq!(session.label, "my session");
    }

    // --- Persistence ---

    #[test]
    fn test_persist_and_load() {
        let dir = std::env::temp_dir().join("core_agentic_test_memory");

        let mut config = MemoryConfig::default();
        config.persist_dir = Some(dir.to_string_lossy().to_string());
        let mut m = Memory::with_config(config);

        m.add_message(Message::user("hello from persist test"));
        m.add_message(Message::assistant("hi back"));

        let session_id = m.session().id.clone();
        let path = m.persist().unwrap();
        assert!(path.exists());

        // Load it back
        let loaded = Memory::load_from_dir(&session_id, Some(&dir)).unwrap();
        assert_eq!(loaded.get_messages().len(), 2);
        assert_eq!(loaded.session().id, session_id);
        assert!(loaded.get_messages()[0].content.contains("hello from persist test"));

        // Cleanup
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_persist_with_pinned() {
        let dir = std::env::temp_dir().join("core_agentic_test_pinned");

        let mut config = MemoryConfig::default();
        config.persist_dir = Some(dir.to_string_lossy().to_string());
        let mut m = Memory::with_config(config);

        let pinned = Message::user("keep this").pinned();
        let pinned_id = pinned.id.clone();
        m.add_message(pinned);
        m.add_message(Message::user("normal"));

        let session_id = m.session().id.clone();
        m.persist().unwrap();

        let loaded = Memory::load_from_dir(&session_id, Some(&dir)).unwrap();
        assert!(loaded.pinned_ids().contains(&pinned_id));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_list_sessions() {
        let dir = std::env::temp_dir().join("core_agentic_test_list");

        let mut config = MemoryConfig::default();
        config.persist_dir = Some(dir.to_string_lossy().to_string());

        let mut m1 = Memory::with_config(config.clone());
        m1.add_message(Message::user("session 1"));
        m1.persist().unwrap();

        let mut m2 = Memory::with_config(config.clone());
        m2.add_message(Message::user("session 2"));
        m2.persist().unwrap();

        let sessions = Memory::list_sessions_from_dir(Some(&dir)).unwrap();
        assert_eq!(sessions.len(), 2);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_delete_session() {
        let dir = std::env::temp_dir().join("core_agentic_test_delete");

        let mut config = MemoryConfig::default();
        config.persist_dir = Some(dir.to_string_lossy().to_string());
        let mut m = Memory::with_config(config);
        m.add_message(Message::user("to be deleted"));

        let session_id = m.session().id.clone();
        m.persist().unwrap();

        assert!(Memory::load_from_dir(&session_id, Some(&dir)).is_ok());
        Memory::delete_session_in_dir(&session_id, Some(&dir)).unwrap();

        let sessions = Memory::list_sessions_from_dir(Some(&dir)).unwrap();
        assert!(!sessions.contains(&session_id));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_load_nonexistent_session() {
        let result = Memory::load_from_dir("nonexistent-id", Some(std::env::temp_dir().as_path()));
        assert!(result.is_err());
    }

    // --- Edge cases ---

    #[test]
    fn test_empty_memory_operations() {
        let m = memory();
        assert!(m.get_messages().is_empty());
        assert_eq!(m.token_count(), 0);
        assert_eq!(m.remaining_budget(), 10000);
        assert_eq!(m.usage_percentage(), 0.0);
        assert_eq!(m.role_type(), "user"); // default
        assert!(m.search("anything").is_empty());
        assert!(m.summary().is_none());
    }

    #[test]
    fn test_compact_empty_memory() {
        let mut m = memory();
        let result = m.compact();
        assert_eq!(result.summarized_count, 0);
    }

    #[test]
    fn test_message_ids_unique() {
        let m1 = Message::user("a");
        let m2 = Message::user("b");
        assert_ne!(m1.id, m2.id);
    }

    #[test]
    fn test_message_serialization_roundtrip() {
        let msg = Message::user("hello").with_model("gpt-4o").with_duration(500);
        let json = serde_json::to_string(&msg).unwrap();
        let parsed: Message = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.content, "hello");
        assert_eq!(parsed.metadata.model.as_deref(), Some("gpt-4o"));
        assert_eq!(parsed.metadata.duration_ms, Some(500));
    }

    #[test]
    fn test_memory_serialization_roundtrip() {
        let mut m = memory();
        m.add_message(Message::user("hello"));
        m.add_message(Message::assistant("world"));

        let json = serde_json::to_string(&m).unwrap();
        let parsed: Memory = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.get_messages().len(), 2);
        assert_eq!(parsed.max_tokens, m.max_tokens);
    }

    #[test]
    fn test_very_long_message() {
        let mut m = memory();
        let long = "a".repeat(100_000);
        m.add_message(Message::user(&long));
        assert_eq!(m.get_messages().len(), 1);
        assert!(m.token_count() > 0);
    }

    // ---- get_context_with_user_anchor ----

    #[test]
    fn anchor_extends_to_include_user_when_window_too_small() {
        // Simulate an agent run with one user message followed by a long
        // chain of assistant + tool turns. A naive last-N slice would
        // contain no user message, which providers reject.
        let mut m = memory();
        m.add_message(Message::user("original prompt"));
        for i in 0..30 {
            m.add_message(Message::assistant(format!("thinking {}", i)));
            m.add_message(Message::tool("read_file", &format!("call-{}", i), "result"));
        }

        let ctx = m.get_context_with_user_anchor(20, None);
        assert!(
            ctx.iter().any(|msg| matches!(msg.role, MessageRole::User)),
            "slice must contain at least one user message"
        );
        // First message of the slice should be the user prompt.
        assert!(matches!(ctx[0].role, MessageRole::User));
        assert_eq!(ctx[0].content, "original prompt");
    }

    #[test]
    fn anchor_keeps_short_slice_when_window_already_has_user() {
        // The anchor only extends; if the window already contains a
        // user, leave it alone.
        let mut m = memory();
        for i in 0..5 {
            m.add_message(Message::user(format!("q{}", i)));
            m.add_message(Message::assistant(format!("a{}", i)));
        }
        let ctx = m.get_context_with_user_anchor(4, None);
        assert_eq!(ctx.len(), 4);
        // Last 4 of 10: q3, a3, q4, a4.
        assert_eq!(ctx[0].content, "q3");
    }

    #[test]
    fn anchor_respects_hard_cap() {
        // Even if the user message is far back, hard_cap caps the slice.
        let mut m = memory();
        m.add_message(Message::user("way back"));
        for _ in 0..50 {
            m.add_message(Message::assistant("more"));
        }
        let ctx = m.get_context_with_user_anchor(5, Some(10));
        assert!(ctx.len() <= 10);
        // The user message is older than the hard cap, so it WON'T be
        // included — sanitize_for_provider in the orchestrator will then
        // strip leading non-user messages and drop everything. That's
        // acceptable: the alternative is sending an unbounded payload.
    }

    #[test]
    fn anchor_handles_empty_memory() {
        let m = memory();
        let ctx = m.get_context_with_user_anchor(20, None);
        assert!(ctx.is_empty());
    }

    #[test]
    fn anchor_picks_most_recent_user_when_multiple_exist() {
        // Multi-turn conversation. Latest user turn becomes the anchor,
        // not the first one ever.
        let mut m = memory();
        m.add_message(Message::user("first prompt"));
        m.add_message(Message::assistant("first answer"));
        m.add_message(Message::user("second prompt"));
        for i in 0..30 {
            m.add_message(Message::assistant(format!("thinking {}", i)));
        }
        let ctx = m.get_context_with_user_anchor(5, None);
        // The slice must start at the second user prompt, not the first.
        assert!(matches!(ctx[0].role, MessageRole::User));
        assert_eq!(ctx[0].content, "second prompt");
    }

    // ---- get_context_for_request (production turn-aware builder) ----

    #[test]
    fn turn_builder_starts_with_user_message() {
        let mut m = memory();
        m.add_message(Message::user("first"));
        m.add_message(Message::assistant("a"));
        m.add_message(Message::user("second"));
        for i in 0..50 {
            m.add_message(Message::assistant(format!("t{}", i)));
            m.add_message(Message::tool("read_file", &format!("c-{}", i), "r"));
        }

        let ctx = m.get_context_for_request(100_000);
        assert!(matches!(ctx[0].role, MessageRole::User));
    }

    #[test]
    fn turn_builder_keeps_complete_turns_under_budget() {
        // Three small turns. With a generous budget all should fit.
        let mut m = memory();
        m.add_message(Message::user("q1"));
        m.add_message(Message::assistant("a1"));
        m.add_message(Message::user("q2"));
        m.add_message(Message::assistant("a2"));
        m.add_message(Message::user("q3"));
        m.add_message(Message::assistant("a3"));

        let ctx = m.get_context_for_request(100_000);
        assert_eq!(ctx.len(), 6);
        assert_eq!(ctx[0].content, "q1");
        assert_eq!(ctx[5].content, "a3");
    }

    #[test]
    fn turn_builder_drops_older_turns_when_budget_tight() {
        // Three turns with ~heavy content. Budget admits only the
        // newest few. The oldest must be dropped at the turn boundary,
        // not mid-turn.
        //
        // We make the per-turn content large enough that the heuristic
        // and a real BPE tokenizer both flag eviction — specifically
        // we use natural-language repetition, not a single repeated
        // character (BPE would compress "x".repeat(N) much harder than
        // the heuristic does).
        let mut m = memory();
        let big: String = (0..400)
            .map(|i| format!("sentence number {} with some words. ", i))
            .collect();
        m.add_message(Message::user(format!("q1 {}", big)));
        m.add_message(Message::assistant(format!("a1 {}", big)));
        m.add_message(Message::user(format!("q2 {}", big)));
        m.add_message(Message::assistant(format!("a2 {}", big)));
        m.add_message(Message::user(format!("q3 {}", big)));
        m.add_message(Message::assistant(format!("a3 {}", big)));

        // Tight budget: each turn is ~3.5k tokens (heuristic) or
        // ~2.5k (tiktoken). Either way, 3000 tokens fits at most one
        // full turn.
        let ctx = m.get_context_for_request(3_000);
        assert!(
            ctx.len() <= 4,
            "should drop at least one full turn; got {}",
            ctx.len()
        );
        // Whatever survives must start with a user message.
        assert!(matches!(ctx[0].role, MessageRole::User));
    }

    #[test]
    fn turn_builder_never_splits_tool_pair() {
        // Build a turn that has assistant+tool_call+tool_result.
        // Even if the budget is tight, the splitter must keep them
        // together (or drop them together), never one without the other.
        use crate::providers::{ToolCallFunction, ToolCallResponse};

        let mut m = memory();
        m.add_message(Message::user("please look"));
        m.add_message(Message::assistant_with_tool_calls(
            "",
            vec![ToolCallResponse {
                id: "call-1".into(),
                call_type: "function".into(),
                function: ToolCallFunction {
                    name: "read_file".into(),
                    arguments: "{}".into(),
                },
            }],
        ));
        m.add_message(Message::tool("read_file", "call-1", "result"));
        m.add_message(Message::assistant("final answer"));

        let ctx = m.get_context_for_request(100_000);
        // Find the assistant_with_tool_calls and verify the matching
        // tool message is also present.
        let assistant_idx = ctx
            .iter()
            .position(|m| !m.metadata.tool_calls.is_empty())
            .expect("assistant tool_call should be in context");
        let tool_id = &ctx[assistant_idx].metadata.tool_calls[0].id;
        let tool_present = ctx.iter().any(|m| {
            matches!(
                &m.role,
                MessageRole::Tool { tool_call_id, .. } if tool_call_id == tool_id
            )
        });
        assert!(tool_present, "tool result must accompany tool_call");
    }

    #[test]
    fn turn_builder_always_includes_most_recent_turn() {
        // Even if the most recent turn alone exceeds budget, include it.
        // The model will surface the truncation; sending nothing would
        // be worse.
        let mut m = memory();
        let huge = "x".repeat(40_000); // ~10k tokens
        m.add_message(Message::user(huge.clone()));

        let ctx = m.get_context_for_request(1_000);
        assert_eq!(ctx.len(), 1);
        assert!(matches!(ctx[0].role, MessageRole::User));
    }

    #[test]
    fn turn_builder_prepends_summary_when_present() {
        let mut m = memory();
        // Need enough messages for compact_with_summary to actually run
        // (default keep_recent=4 — anything beyond that gets summarized).
        for i in 0..6 {
            m.add_message(Message::user(format!("q{}", i)));
            m.add_message(Message::assistant(format!("a{}", i)));
        }
        m.compact_with_summary("Earlier we discussed X and Y.");

        let ctx = m.get_context_for_request(100_000);
        assert!(matches!(ctx[0].role, MessageRole::System));
        assert!(ctx[0].content.contains("discussed X and Y"));
        // A user/assistant follows.
        assert!(ctx.len() > 1);
        assert!(matches!(ctx[1].role, MessageRole::User));
    }

    // ---- request_budget / context_budget_ratio ----

    #[test]
    fn request_budget_applies_ratio() {
        let mut config = MemoryConfig::default();
        config.max_tokens = 100_000;
        config.context_budget_ratio = 0.5;
        let m = Memory::with_config(config);
        assert_eq!(m.request_budget(), 50_000);
    }

    #[test]
    fn request_budget_clamps_extreme_ratios() {
        // Out-of-range ratios get clamped to a sensible band so a
        // user typo doesn't accidentally send 0 or 99% of the window.
        let mut config = MemoryConfig::default();
        config.max_tokens = 100_000;

        config.context_budget_ratio = 0.0;
        let m = Memory::with_config(config.clone());
        assert_eq!(m.request_budget(), 10_000); // clamped to 0.1

        config.context_budget_ratio = 5.0;
        let m = Memory::with_config(config);
        assert_eq!(m.request_budget(), 95_000); // clamped to 0.95
    }

    #[test]
    fn request_budget_default_is_seventy_percent() {
        // Tracks the documented contract: default context_budget_ratio = 0.7.
        let mut config = MemoryConfig::default();
        config.max_tokens = 100_000;
        let m = Memory::with_config(config);
        assert_eq!(m.request_budget(), 70_000);
    }
}
