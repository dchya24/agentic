pub mod edit_file;
pub mod glob;
pub mod grep;
pub mod list_files;
pub mod read_file;
pub mod run_command;
pub mod write_file;

pub use edit_file::EditFileTool;
pub use glob::GlobTool;
pub use grep::GrepTool;
pub use list_files::ListFilesTool;
pub use read_file::ReadFileTool;
pub use run_command::RunCommandTool;
pub use write_file::WriteFileTool;

pub fn builtin_tools() -> Vec<Box<dyn crate::tool::Tool + Send + Sync>> {
    vec![
        Box::new(RunCommandTool::new()),
        Box::new(ReadFileTool::new()),
        Box::new(WriteFileTool::new()),
        Box::new(EditFileTool::new()),
        Box::new(ListFilesTool::new()),
        Box::new(GlobTool::new()),
        Box::new(GrepTool::new()),
    ]
}
