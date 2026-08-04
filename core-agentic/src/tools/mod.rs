pub mod apply_patch;
pub mod edit_file;
pub mod fetch;
pub mod git_query;
pub mod glob;
pub mod grep;
pub mod list_files;
pub mod question;
pub mod read_file;
pub mod run_command;
pub mod run_script;
pub mod run_tests;
pub mod search_files;
pub mod spawn_subagent;
pub mod todowrite;
pub mod update_memory;
pub mod web_search;
pub mod write_file;

pub use apply_patch::ApplyPatchTool;
pub use edit_file::EditFileTool;
pub use fetch::FetchTool;
pub use git_query::{GitDiffTool, GitStatusTool};
pub use glob::GlobTool;
pub use grep::GrepTool;
pub use list_files::ListFilesTool;
pub use question::{
    clear_question_handler, set_question_handler, QuestionAnswer, QuestionHandler, QuestionPrompt,
    QuestionTool,
};
pub use read_file::ReadFileTool;
pub use run_command::RunCommandTool;
pub use run_script::RunScriptTool;
pub use run_tests::RunTestsTool;
pub use search_files::SearchFilesTool;
pub use spawn_subagent::{SpawnSubagentTool, DEFAULT_SUBAGENT_MAX_ITERATIONS};
pub use todowrite::{
    clear_todo_change_handler, clear_todos, current_todos, set_todo_change_handler,
    TodoChangeHandler, TodoItem, TodoPriority, TodoStatus, TodowriteTool,
};
pub use update_memory::UpdateMemoryTool;
pub use web_search::WebSearchTool;
pub use write_file::WriteFileTool;

use crate::file_tracker::FileTracker;
use crate::safety::UrlPolicy;
use std::sync::Arc;

pub fn builtin_tools() -> Vec<Box<dyn crate::tool::Tool + Send + Sync>> {
    builtin_tools_with_tracker(Arc::new(FileTracker::new()))
}

/// Build the standard tool set, sharing a single [`FileTracker`] instance
/// between read_file and edit_file so staleness detection works end-to-end.
pub fn builtin_tools_with_tracker(
    tracker: Arc<FileTracker>,
) -> Vec<Box<dyn crate::tool::Tool + Send + Sync>> {
    builtin_tools_with(tracker, UrlPolicy::default())
}

/// Build the standard tool set with both a shared [`FileTracker`] and a
/// URL allowlist policy applied to URL-taking tools (`fetch`, `web_search`).
///
/// When `url_policy.is_unrestricted()` returns `true` (the default), this
/// is equivalent to `builtin_tools_with_tracker`.
pub fn builtin_tools_with(
    tracker: Arc<FileTracker>,
    url_policy: UrlPolicy,
) -> Vec<Box<dyn crate::tool::Tool + Send + Sync>> {
    vec![
        Box::new(RunCommandTool::new()),
        Box::new(ReadFileTool::with_tracker(tracker.clone())),
        Box::new(WriteFileTool::new()),
        Box::new(EditFileTool::with_tracker(tracker.clone())),
        Box::new(ApplyPatchTool::with_tracker(tracker.clone())),
        Box::new(ListFilesTool::new()),
        Box::new(GlobTool::new()),
        Box::new(GrepTool::new()),
        Box::new(SearchFilesTool::new()),
        Box::new(RunScriptTool::new()),
        Box::new(RunTestsTool::new()),
        Box::new(GitStatusTool::new()),
        Box::new(GitDiffTool::new()),
        Box::new(UpdateMemoryTool::new()),
        Box::new(QuestionTool::new()),
        Box::new(TodowriteTool::new()),
        Box::new(FetchTool::new().with_url_policy(url_policy.clone())),
        Box::new(WebSearchTool::new().with_url_policy(url_policy)),
    ]
}
