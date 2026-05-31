pub mod edit_file;
pub mod fetch;
pub mod glob;
pub mod grep;
pub mod list_files;
pub mod read_file;
pub mod run_command;
pub mod run_script;
pub mod search_files;
pub mod spawn_subagent;
pub mod update_memory;
pub mod web_search;
pub mod write_file;

pub use edit_file::EditFileTool;
pub use fetch::FetchTool;
pub use glob::GlobTool;
pub use grep::GrepTool;
pub use list_files::ListFilesTool;
pub use read_file::ReadFileTool;
pub use run_command::RunCommandTool;
pub use run_script::RunScriptTool;
pub use search_files::SearchFilesTool;
pub use spawn_subagent::{SpawnSubagentTool, DEFAULT_SUBAGENT_MAX_ITERATIONS};
pub use update_memory::UpdateMemoryTool;
pub use web_search::WebSearchTool;
pub use write_file::WriteFileTool;

use std::sync::Arc;
use crate::file_tracker::FileTracker;

pub fn builtin_tools() -> Vec<Box<dyn crate::tool::Tool + Send + Sync>> {
    builtin_tools_with_tracker(Arc::new(FileTracker::new()))
}

/// Build the standard tool set, sharing a single [`FileTracker`] instance
/// between read_file and edit_file so staleness detection works end-to-end.
pub fn builtin_tools_with_tracker(
    tracker: Arc<FileTracker>,
) -> Vec<Box<dyn crate::tool::Tool + Send + Sync>> {
    vec![
        Box::new(RunCommandTool::new()),
        Box::new(ReadFileTool::with_tracker(tracker.clone())),
        Box::new(WriteFileTool::new()),
        Box::new(EditFileTool::with_tracker(tracker.clone())),
        Box::new(ListFilesTool::new()),
        Box::new(GlobTool::new()),
        Box::new(GrepTool::new()),
        Box::new(SearchFilesTool::new()),
        Box::new(RunScriptTool::new()),
        Box::new(UpdateMemoryTool::new()),
        Box::new(FetchTool::new()),
        Box::new(WebSearchTool::new()),
    ]
}
