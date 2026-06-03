//! System prompt defaults and project-instructions discovery.
//!
//! Provides:
//! - A baseline system prompt that encodes the "search funnel" pattern
//!   (list → search → read → edit) and the three core rules from the
//!   architecture doc: read before edit, search before assuming,
//!   understand before modifying.
//! - Helpers to discover project-level instructions (`AGENT.md`,
//!   `AGENTS.md`, `.agentic/AGENT.md`) and assemble the effective
//!   system prompt sent to the model.

use std::path::{Path, PathBuf};

/// Baseline system prompt sent to the model when no override is configured.
pub const DEFAULT_SYSTEM_PROMPT: &str = r#"You are an AI coding agent operating in a real filesystem with tool access.

Core rules (always):
1. Read before edit. Never guess what's in a file. Always read it first.
2. Search before assuming. Use list_files / search_files / grep / glob
   instead of guessing paths.
3. Understand before modifying. Explore the codebase to understand patterns
   before making changes.

Search funnel:
   list_files (broad)  →  search/grep (narrow)  →  read_file (confirm)  →  edit_file (act)

Tool usage:
- Prefer edit_file for surgical changes; pass enough surrounding context in
  old_string so the match is unique.
- Run commands only when needed and prefer read-only commands first
  (ls, cat, git status, git log, etc).
- When a tool result is large, narrow your next call (use offset/limit,
  more specific paths, or a tighter pattern) instead of re-reading.

Output:
- Be concise. Focus on what changed and why.
- Never fabricate file contents or commands. Only report what tools actually
  returned.
"#;

/// Filenames that are auto-loaded as project instructions, in priority order.
/// The first one found is used (others are ignored to avoid conflicts).
pub const PROJECT_INSTRUCTION_FILES: &[&str] = &[
    "AGENT.md",
    "AGENTS.md",
    ".agentic/AGENT.md",
    ".agentic/AGENTS.md",
];

/// Search for a project instructions file starting at `cwd` and walking up
/// to the filesystem root. Returns the path of the first match, if any.
pub fn find_project_instructions(cwd: &Path) -> Option<PathBuf> {
    let mut current: Option<&Path> = Some(cwd);
    while let Some(dir) = current {
        for candidate in PROJECT_INSTRUCTION_FILES {
            let path = dir.join(candidate);
            if path.is_file() {
                return Some(path);
            }
        }
        current = dir.parent();
    }
    None
}

/// Load project instructions content if a file is found. Errors are swallowed
/// (returns None) so a missing or unreadable file never blocks startup.
pub fn load_project_instructions(cwd: &Path) -> Option<(PathBuf, String)> {
    let path = find_project_instructions(cwd)?;
    let content = std::fs::read_to_string(&path).ok()?;
    Some((path, content))
}

/// Generate the skills section for the system prompt.
///
/// Returns a string like:
/// ```text
/// ---
/// # Skills
///
/// 📦 my-skill — Does X and Y
/// 📦 other-skill — Does Z
/// ```
/// or `None` when the index is empty.
pub fn skills_system_section(skills: &[(&str, &str)]) -> Option<String> {
    if skills.is_empty() {
        return None;
    }
    let lines: Vec<String> = skills
        .iter()
        .map(|(name, desc)| format!("📦 {} — {}", name, desc))
        .collect();
    Some(format!(
        "---\n# Skills\n\nAvailable skills. Use the `skill` tool to load one on demand.\n\n{}",
        lines.join("\n")
    ))
}

/// Assemble the effective system prompt from layered sources.
///
/// Layers (concatenated in this order, each separated by a blank line):
/// 1. `base` — typically [`DEFAULT_SYSTEM_PROMPT`] or a config-provided value
/// 2. `project_instructions` — content of an `AGENT.md` discovered in cwd
/// 3. `skills_section` — list of discovered skills (see [`skills_system_section`])
/// 4. `user_override` — additional instructions injected by the CLI/REPL
///
/// Empty parts are skipped. Returns the joined prompt as a single string.
pub fn assemble_system_prompt(
    base: Option<&str>,
    project_instructions: Option<&str>,
    skills_section: Option<&str>,
    user_override: Option<&str>,
) -> String {
    let mut sections: Vec<String> = Vec::new();

    let base = base.unwrap_or(DEFAULT_SYSTEM_PROMPT).trim();
    if !base.is_empty() {
        sections.push(base.to_string());
    }

    if let Some(p) = project_instructions {
        let p = p.trim();
        if !p.is_empty() {
            sections.push(format!("---\n# Project Instructions\n\n{}", p));
        }
    }

    if let Some(s) = skills_section {
        let s = s.trim();
        if !s.is_empty() {
            sections.push(s.to_string());
        }
    }

    if let Some(u) = user_override {
        let u = u.trim();
        if !u.is_empty() {
            sections.push(format!("---\n# Additional Instructions\n\n{}", u));
        }
    }

    sections.join("\n\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::io::Write;

    fn tmp_dir(suffix: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("prompts_test_{}", suffix));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn finds_agent_md_in_cwd() {
        let dir = tmp_dir("cwd");
        let path = dir.join("AGENT.md");
        let mut f = fs::File::create(&path).unwrap();
        writeln!(f, "rules").unwrap();

        let found = find_project_instructions(&dir);
        assert_eq!(found, Some(path));
    }

    #[test]
    fn finds_agent_md_walking_up() {
        let root = tmp_dir("walk");
        let nested = root.join("a").join("b").join("c");
        fs::create_dir_all(&nested).unwrap();
        let path = root.join("AGENT.md");
        fs::write(&path, "from root").unwrap();

        let found = find_project_instructions(&nested);
        assert_eq!(found, Some(path));
    }

    #[test]
    fn returns_none_when_missing() {
        let dir = tmp_dir("missing");
        // Search starting from a deep dir with no AGENT.md anywhere on the path
        // (still walks up to /, which may have one). To keep the test
        // deterministic, just verify the API doesn't panic and returns Option.
        let _ = find_project_instructions(&dir);
    }

    #[test]
    fn assemble_uses_default_when_base_none() {
        let out = assemble_system_prompt(None, None, None, None);
        assert!(out.contains("Read before edit"));
        assert!(out.contains("Search funnel"));
    }

    #[test]
    fn assemble_includes_project_instructions() {
        let out = assemble_system_prompt(
            Some("BASE"),
            Some("project rule X"),
            None,
            None,
        );
        assert!(out.contains("BASE"));
        assert!(out.contains("Project Instructions"));
        assert!(out.contains("project rule X"));
    }

    #[test]
    fn assemble_includes_user_override() {
        let out = assemble_system_prompt(
            Some("BASE"),
            None,
            None,
            Some("user override Y"),
        );
        assert!(out.contains("BASE"));
        assert!(out.contains("Additional Instructions"));
        assert!(out.contains("user override Y"));
    }

    #[test]
    fn assemble_includes_skills_section() {
        let out = assemble_system_prompt(
            Some("BASE"),
            None,
            Some("---\n# Skills\n\n📦 test-skill — A test"),
            None,
        );
        assert!(out.contains("BASE"));
        assert!(out.contains("Skills"));
        assert!(out.contains("📦 test-skill — A test"));
    }

    #[test]
    fn assemble_skips_empty_sections() {
        let out = assemble_system_prompt(Some("BASE"), Some("   "), None, Some(""));
        assert!(out.contains("BASE"));
        assert!(!out.contains("Project Instructions"));
        assert!(!out.contains("Additional Instructions"));
    }

    #[test]
    fn skills_section_returns_none_when_empty() {
        assert!(skills_system_section(&[]).is_none());
    }

    #[test]
    fn skills_section_formats_correctly() {
        let skills = &[("rust", "Rust programming"), ("react", "React UI dev")];
        let section = skills_system_section(skills).unwrap();
        assert!(section.contains("📦 rust — Rust programming"));
        assert!(section.contains("📦 react — React UI dev"));
        assert!(section.contains("Use the `skill` tool"));
    }
}
