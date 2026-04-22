//! Built-in tools for core-agentic

pub mod run_command;
pub mod read_file;
pub mod write_file;
pub mod list_files;

pub use run_command::RunCommandTool;
pub use read_file::ReadFileTool;
pub use write_file::WriteFileTool;
pub use list_files::ListFilesTool;

/// Returns all built-in tools
pub fn builtin_tools() -> Vec<Box<dyn crate::tool::Tool + Send + Sync>> {
    vec![
        Box::new(RunCommandTool::new()),
        Box::new(ReadFileTool::new()),
        Box::new(WriteFileTool::new()),
        Box::new(ListFilesTool::new()),
    ]
}