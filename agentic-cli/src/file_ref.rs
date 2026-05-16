//! `@` file reference expansion
//!
//! Expands `@path/to/file` references in user input by reading the file contents
//! and inlining them into the prompt sent to the AI.
//!
//! Example:
//!   "explain @src/main.rs" → "explain <file path=\"src/main.rs\">\n<file contents>\n</file>"
//!
//! Supports:
//! - Multiple `@` references in one message
//! - Directories: `@src/` reads all files in the directory (non-recursive)
//! - Respects .gitignore via the `ignore` crate

use std::path::Path;

/// Expand all `@path` references in the input string, replacing them with
/// the actual file contents wrapped in XML-style tags.
///
/// Returns the expanded string. If a file doesn't exist or can't be read,
/// includes an error message inline instead.
pub fn expand_file_refs(input: &str) -> String {
    let mut result = String::new();
    let mut last_end = 0;

    for (at_pos, path) in find_file_refs(input) {
        // Push text before this @ref
        result.push_str(&input[last_end..at_pos]);

        // Read and expand the file/directory
        let expanded = read_file_ref(&path);
        result.push_str(&expanded);

        // Skip past the @path in the original input
        last_end = at_pos + 1 + path.len();
    }

    // Push remaining text after last @ref
    if last_end < input.len() {
        result.push_str(&input[last_end..]);
    }

    result
}

/// Find all `@path` references in the input.
/// Returns a vec of (position_of_@, path_string).
fn find_file_refs(input: &str) -> Vec<(usize, String)> {
    let mut refs = Vec::new();
    let chars: Vec<char> = input.chars().collect();
    let mut i = 0;

    while i < chars.len() {
        if chars[i] == '@' && (i == 0 || chars[i - 1].is_whitespace()) {
            // Start collecting the path
            let at_pos = i;
            let mut path_end = i + 1;

            // Collect characters that are valid in a file path
            for j in (i + 1)..chars.len() {
                let c = chars[j];
                if c.is_whitespace() {
                    break;
                }
                path_end = j + 1;
            }

            if path_end > i + 1 {
                let path: String = chars[i + 1..path_end].iter().collect();
                // Only consider it a file ref if it looks like a path
                // (contains /, \, ., or common extensions)
                if looks_like_path(&path) {
                    refs.push((at_pos, path));
                    i = path_end;
                    continue;
                }
            }
        }
        i += 1;
    }

    refs
}

/// Check if a string looks like a file path (not just a social media @mention).
fn looks_like_path(s: &str) -> bool {
    // Contains path separators
    if s.contains('/') || s.contains('\\') {
        return true;
    }
    // Has a file extension
    if let Some(dot_pos) = s.rfind('.') {
        if dot_pos < s.len() - 1 {
            let ext = &s[dot_pos + 1..];
            // Common extensions
            return matches!(
                ext,
                "rs"
                | "ts"
                | "tsx"
                | "js"
                | "jsx"
                | "py"
                | "go"
                | "java"
                | "c"
                | "cpp"
                | "h"
                | "hpp"
                | "rb"
                | "php"
                | "sh"
                | "bash"
                | "zsh"
                | "fish"
                | "css"
                | "scss"
                | "html"
                | "htm"
                | "xml"
                | "json"
                | "yaml"
                | "yml"
                | "toml"
                | "ini"
                | "cfg"
                | "conf"
                | "md"
                | "txt"
                | "csv"
                | "sql"
                | "lock"
                | "log"
                | "env"
                | "gitignore"
                | "dockerignore"
                | "editorconfig"
                | "Makefile"
                | "Dockerfile"
            );
        }
    }
    // Matches known config/build files without extensions
    matches!(
        s,
        "Makefile"
            | "Dockerfile"
            | "Cargo.toml"
            | "Cargo.lock"
            | "package.json"
            | "tsconfig.json"
            | ".gitignore"
            | ".env"
            | ".env.local"
            | ".env.production"
            | "README"
            | "README.md"
            | "LICENSE"
            | "LICENSE.md"
    )
}

/// Read a file or directory and return its contents formatted for the AI prompt.
fn read_file_ref(path_str: &str) -> String {
    let path_str = path_str.trim_end_matches('/').trim_end_matches('\\');
    let path = Path::new(path_str);

    if !path.exists() {
        return format!("<file path=\"{}\">[File not found: {}]</file>", path_str, path_str);
    }

    if path.is_dir() {
        read_directory(path_str, path)
    } else {
        read_single_file(path_str, path)
    }
}

/// Read a single file and format it.
fn read_single_file(path_str: &str, path: &Path) -> String {
    match std::fs::read_to_string(path) {
        Ok(content) => {
            let line_count = content.lines().count();
            let ext = path
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("");

            // Truncate very large files
            const MAX_LINES: usize = 500;
            if line_count > MAX_LINES {
                let truncated: String = content.lines().take(MAX_LINES).collect::<Vec<_>>().join("\n");
                return format!(
                    "<file path=\"{}\" lang=\"{}\" lines=\"{}\" truncated=\"true\">\n{}\n... ({} of {} lines shown)\n</file>",
                    path_str, ext, line_count, truncated, MAX_LINES, line_count
                );
            }

            format!(
                "<file path=\"{}\" lang=\"{}\" lines=\"{}\">\n{}\n</file>",
                path_str, ext, line_count, content
            )
        }
        Err(e) => {
            format!(
                "<file path=\"{}\">[Error reading file: {}]</file>",
                path_str, e
            )
        }
    }
}

/// Read a directory: list all files (non-recursive, .gitignore-aware) and include their contents.
fn read_directory(path_str: &str, path: &Path) -> String {
    let mut parts = Vec::new();

    // List directory contents
    let mut file_count = 0;
    let mut dir_count = 0;

    // Use ignore crate for .gitignore-aware listing
    let mut builder = ignore::WalkBuilder::new(path);
    builder
        .max_depth(Some(2))  // Shallow: just the dir and one level down
        .hidden(false)
        .git_ignore(true)
        .git_global(true)
        .git_exclude(true)
        .require_git(false);

    let mut entries: Vec<(String, bool)> = Vec::new(); // (path, is_dir)

    for entry in builder.build().filter_map(|e| e.ok()) {
        let entry_path = entry.path();
        let entry_str = entry_path.to_string_lossy();

        // Skip the directory itself
        if entry_path == path {
            continue;
        }

        // Normalize path
        let normalized = entry_str.replace('\\', "/");
        let is_dir = entry_path.is_dir();

        if is_dir {
            dir_count += 1;
        } else {
            file_count += 1;
        }

        entries.push((normalized, is_dir));
    }

    // Sort: dirs first, then files
    entries.sort_by(|a, b| {
        match (a.1, b.1) {
            (true, false) => std::cmp::Ordering::Less,
            (false, true) => std::cmp::Ordering::Greater,
            _ => a.0.to_lowercase().cmp(&b.0.to_lowercase()),
        }
    });

    // Build listing
    parts.push(format!(
        "<directory path=\"{}\" files=\"{}\" dirs=\"{}\">",
        path_str, file_count, dir_count
    ));

    for (entry_path, is_dir) in &entries {
        let icon = if *is_dir { "📁" } else { "📄" };
        let suffix = if *is_dir { "/" } else { "" };
        parts.push(format!("  {} {}{}", icon, entry_path, suffix));
    }

    parts.push("</directory>".to_string());

    // Also include contents of files (limit to 10 files to avoid huge prompts)
    let mut files_included = 0;
    for (entry_path, is_dir) in &entries {
        if *is_dir {
            continue;
        }
        if files_included >= 10 {
            parts.push(format!(
                "\n<!-- ... and {} more files not shown -->",
                file_count - files_included
            ));
            break;
        }

        if let Ok(content) = std::fs::read_to_string(entry_path) {
            let ext = Path::new(entry_path)
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("");
            let line_count = content.lines().count();

            const MAX_LINES: usize = 200;
            if line_count <= MAX_LINES {
                parts.push(format!(
                    "\n<file path=\"{}\" lang=\"{}\" lines=\"{}\">\n{}\n</file>",
                    entry_path, ext, line_count, content
                ));
            } else {
                let truncated: String = content.lines().take(MAX_LINES).collect::<Vec<_>>().join("\n");
                parts.push(format!(
                    "\n<file path=\"{}\" lang=\"{}\" lines=\"{}\" truncated=\"true\">\n{}\n... ({} of {} lines shown)\n</file>",
                    entry_path, ext, line_count, truncated, MAX_LINES, line_count
                ));
            }
            files_included += 1;
        }
    }

    parts.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_no_refs() {
        assert_eq!(expand_file_refs("hello world"), "hello world");
    }

    #[test]
    fn test_email_not_expanded() {
        // @ in email should NOT be treated as file ref
        let result = expand_file_refs("send to user@example.com");
        assert_eq!(result, "send to user@example.com");
    }

    #[test]
    fn test_mention_not_expanded() {
        // @mention without path-like structure should NOT be expanded
        let result = expand_file_refs("hey @john can you help");
        assert_eq!(result, "hey @john can you help");
    }

    #[test]
    fn test_path_with_slash_detected() {
        let refs = find_file_refs("look at @src/main.rs please");
        assert_eq!(refs.len(), 1);
        assert_eq!(refs[0].1, "src/main.rs");
    }

    #[test]
    fn test_path_with_extension_detected() {
        let refs = find_file_refs("check @Cargo.toml for deps");
        assert_eq!(refs.len(), 1);
        assert_eq!(refs[0].1, "Cargo.toml");
    }

    #[test]
    fn test_multiple_refs() {
        let refs = find_file_refs("compare @src/a.rs and @src/b.rs");
        assert_eq!(refs.len(), 2);
        assert_eq!(refs[0].1, "src/a.rs");
        assert_eq!(refs[1].1, "src/b.rs");
    }

    #[test]
    fn test_nonexistent_file() {
        let result = expand_file_refs("read @nonexistent/file.txt");
        assert!(result.contains("[File not found:"));
        assert!(result.contains("nonexistent/file.txt"));
    }

    #[test]
    fn test_existing_file() {
        // Use Cargo.toml which exists in the project
        let result = expand_file_refs("check @Cargo.toml");
        assert!(result.contains("<file path=\"Cargo.toml\""));
        assert!(result.contains("[package]"));
    }

    #[test]
    fn test_existing_directory() {
        let result = expand_file_refs("list @src/");
        assert!(result.contains("<directory path=\"src\""));
    }

    #[test]
    fn test_looks_like_path() {
        assert!(looks_like_path("src/main.rs"));
        assert!(looks_like_path("Cargo.toml"));
        assert!(looks_like_path(".gitignore"));
        assert!(looks_like_path("config/app.json"));
        assert!(!looks_like_path("john"));
        assert!(!looks_like_path("everyone"));
    }

    #[test]
    fn test_ref_at_start_of_input() {
        let refs = find_file_refs("@Cargo.toml is the config");
        assert_eq!(refs.len(), 1);
        assert_eq!(refs[0].1, "Cargo.toml");
    }

    #[test]
    fn test_ref_at_end_of_input() {
        let refs = find_file_refs("check @Cargo.toml");
        assert_eq!(refs.len(), 1);
        assert_eq!(refs[0].1, "Cargo.toml");
    }
}
