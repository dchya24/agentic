//! Dropdown widget for command and file completion
//!
//! Handles:
//! - `/` command dropdown: shows available slash commands
//! - `@` file dropdown: shows all project files recursively (flat list)
//! - Respects `.gitignore` rules (skips node_modules, target, .git, etc.)
//! - Type `@` → full recursive file tree
//! - Type `@src/` → all files under src/
//! - Type `@chat` → fuzzy filter all files matching "chat"

use std::path::Path;

/// Type of dropdown
#[derive(Clone, Debug, PartialEq)]
pub enum DropdownType {
    /// Slash commands (/help, /config, etc.)
    Command,
    /// File paths (@src/main.rs, etc.)
    File,
}

/// Dropdown state
#[derive(Clone, Debug)]
pub struct Dropdown {
    pub dropdown_type: DropdownType,
    pub items: Vec<String>,
    pub selected: usize,
    pub visible_count: usize,
}

/// Available slash commands with aliases and descriptions
const SLASH_COMMANDS: &[(&str, &[&str], &str)] = &[
    ("help", &["h", "?"], "Show help message"),
    ("clear", &["cls", "c"], "Clear conversation"),
    ("config", &["cfg"], "Show configuration"),
    ("tools", &["t"], "List available tools"),
    ("history", &["hist"], "Show command history"),
    ("save", &["s"], "Save conversation to file"),
    ("load", &["l"], "Load conversation from file"),
    ("mcp", &[], "Show MCP server status"),
    ("plan", &["p"], "Create a plan for a goal"),
    ("model", &["m"], "Switch model"),
    ("provider", &["prov"], "Switch provider"),
    ("stats", &[], "Show session statistics"),
    ("quit", &["q", "exit"], "Exit TUI"),
];

impl Dropdown {
    pub fn new(dropdown_type: DropdownType, query: String) -> Self {
        let items = match dropdown_type {
            DropdownType::Command => Self::filter_commands(&query),
            DropdownType::File => Self::filter_files(&query),
        };

        Self {
            dropdown_type,
            items,
            selected: 0,
            visible_count: 8,
        }
    }

    /// Filter slash commands by query (supports aliases)
    fn filter_commands(query: &str) -> Vec<String> {
        let query_lower = query.to_lowercase();
        SLASH_COMMANDS
            .iter()
            .filter(|(cmd, aliases, _)| {
                cmd.starts_with(&query_lower)
                    || aliases.iter().any(|a| a.starts_with(&query_lower))
            })
            .map(|(cmd, _, _)| cmd.to_string())
            .collect()
    }

    /// Filter files by query — always recursive, respects .gitignore.
    ///
    /// Behavior based on query:
    /// - `""`     → all files in project (recursive flat list)
    /// - `"src/"` → all files under `src/` (recursive)
    /// - `"src/ma"` → all files under `src/` matching `ma`
    /// - `"chat"` → all files in project matching `chat` (by name or path)
    fn filter_files(query: &str) -> Vec<String> {
        let mut results = Vec::new();

        // Parse query into (path_prefix, name_filter)
        // e.g. "src/components/cha" → prefix="src/components/", filter="cha"
        // e.g. "src/"              → prefix="src/",         filter=""
        // e.g. "chat"             → prefix="",              filter="chat"
        // e.g. ""                 → prefix="",              filter=""
        let (path_prefix, name_filter) = if query.is_empty() {
            (String::new(), String::new())
        } else if query.ends_with('/') {
            // Pure path browse: "src/" → show all under src/
            (query.to_string(), String::new())
        } else if query.contains('/') {
            // Path + filter: "src/components/cha"
            let last_slash = query.rfind('/').unwrap();
            (
                query[..=last_slash].to_string(),
                query[last_slash + 1..].to_string(),
            )
        } else {
            // Just a filter, no path
            (String::new(), query.to_string())
        };

        let filter_lower = name_filter.to_lowercase();

        // Walk the entire project recursively, respecting .gitignore
        let mut builder = ignore::WalkBuilder::new(".");
        builder
            .hidden(false)
            .git_ignore(true)
            .git_global(true)
            .git_exclude(true)
            .require_git(false);

        for entry in builder.build().filter_map(|e| e.ok()) {
            let path = entry.path();
            let path_str = path.to_string_lossy();

            // Skip `.` itself
            if path_str == "." || path_str == "./" {
                continue;
            }

            // Normalize: backslashes to forward slashes, strip leading "./"
            let normalized = path_str.replace('\\', "/");
            let clean = normalized.strip_prefix("./").unwrap_or(&normalized);

            // If a path prefix was given, only include files under that prefix
            if !path_prefix.is_empty() {
                // Normalize: ensure prefix comparison works
                if !clean.starts_with(&path_prefix) && !clean.starts_with(&path_prefix.trim_end_matches('/')) {
                    continue;
                }
            }

            // If a name filter was given, match against filename and full path
            if !filter_lower.is_empty() {
                let fname = Path::new(&clean)
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy();
                let fname_lower = fname.to_lowercase();
                let clean_lower = clean.to_lowercase();

                // Match: filename starts with, filename contains, or path contains
                if !fname_lower.starts_with(&filter_lower)
                    && !fname_lower.contains(&filter_lower)
                    && !clean_lower.contains(&filter_lower)
                {
                    continue;
                }
            }

            // Format display: directories get trailing `/`
            if path.is_dir() {
                results.push(format!("{}/", clean));
            } else {
                results.push(clean.to_string());
            }
        }

        // Sort: directories first, then files — both alphabetically
        results.sort_by(|a, b| {
            let a_is_dir = a.ends_with('/');
            let b_is_dir = b.ends_with('/');
            match (a_is_dir, b_is_dir) {
                (true, false) => std::cmp::Ordering::Less,
                (false, true) => std::cmp::Ordering::Greater,
                _ => a.to_lowercase().cmp(&b.to_lowercase()),
            }
        });

        // Cap results for performance
        results.truncate(50);
        results
    }

    /// Get selected item
    pub fn selected_item(&self) -> Option<&str> {
        self.items.get(self.selected).map(|s| s.as_str())
    }

    /// Select previous item (wraps around)
    pub fn select_prev(&mut self) {
        if self.selected > 0 {
            self.selected -= 1;
        } else if !self.items.is_empty() {
            self.selected = self.items.len() - 1;
        }
    }

    /// Select next item (wraps around)
    pub fn select_next(&mut self) {
        if !self.items.is_empty() {
            self.selected = (self.selected + 1) % self.items.len();
        }
    }

    /// Get visible items with selection info
    pub fn visible_items(&self) -> Vec<(usize, &str, bool)> {
        let start = if self.selected >= self.visible_count {
            self.selected - self.visible_count + 1
        } else {
            0
        };

        self.items
            .iter()
            .enumerate()
            .skip(start)
            .take(self.visible_count)
            .map(|(i, item)| (i, item.as_str(), i == self.selected))
            .collect()
    }

    /// Get description for command (if command dropdown)
    pub fn get_description(&self, cmd: &str) -> Option<&'static str> {
        if self.dropdown_type == DropdownType::Command {
            SLASH_COMMANDS
                .iter()
                .find(|(c, _, _)| *c == cmd)
                .map(|(_, _, desc)| *desc)
        } else {
            None
        }
    }

    /// Check if dropdown has items
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// Get icon for dropdown type
    pub fn icon(&self) -> &'static str {
        match self.dropdown_type {
            DropdownType::Command => "⌘",
            DropdownType::File => "📁",
        }
    }

    /// Get title for dropdown
    pub fn title(&self) -> &'static str {
        match self.dropdown_type {
            DropdownType::Command => "Commands",
            DropdownType::File => "Files",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_command_filter() {
        let dropdown = Dropdown::new(DropdownType::Command, "he".to_string());
        assert!(dropdown.items.contains(&"help".to_string()));
    }

    #[test]
    fn test_command_filter_by_alias() {
        let dropdown = Dropdown::new(DropdownType::Command, "h".to_string());
        assert!(dropdown.items.contains(&"help".to_string()));
    }

    #[test]
    fn test_command_filter_empty() {
        let dropdown = Dropdown::new(DropdownType::Command, "".to_string());
        assert_eq!(dropdown.items.len(), SLASH_COMMANDS.len());
    }

    #[test]
    fn test_select_navigation() {
        let mut dropdown = Dropdown::new(DropdownType::Command, "".to_string());
        assert_eq!(dropdown.selected, 0);

        dropdown.select_next();
        assert_eq!(dropdown.selected, 1);

        dropdown.select_prev();
        assert_eq!(dropdown.selected, 0);

        // Wrap around
        dropdown.select_prev();
        assert_eq!(dropdown.selected, dropdown.items.len() - 1);
    }

    #[test]
    fn test_file_filter_shows_nested() {
        // Empty query should show ALL files recursively
        let dropdown = Dropdown::new(DropdownType::File, String::new());
        if std::path::Path::new("src").exists() {
            // Should contain nested paths, not just top-level
            let has_nested = dropdown
                .items
                .iter()
                .any(|i| i.matches('/').count() >= 2 && !i.ends_with('/'));
            assert!(has_nested, "expected nested file paths in results, got: {:?}", dropdown.items);
        }
    }

    #[test]
    fn test_file_filter_path_prefix() {
        // "src/" should only show files under src/
        let dropdown = Dropdown::new(DropdownType::File, "src/".to_string());
        for item in &dropdown.items {
            assert!(
                item.starts_with("src/"),
                "expected item to start with 'src/', got: {}",
                item
            );
        }
    }

    #[test]
    fn test_file_filter_name_search() {
        // "main" should find files with "main" in name or path
        let dropdown = Dropdown::new(DropdownType::File, "main".to_string());
        // Just verify it doesn't crash and filters correctly
        for item in &dropdown.items {
            let lower = item.to_lowercase();
            assert!(
                lower.contains("main"),
                "expected item to contain 'main', got: {}",
                item
            );
        }
    }

    #[test]
    fn test_file_filter_path_and_name() {
        // "src/ma" should find files under src/ matching "ma"
        let dropdown = Dropdown::new(DropdownType::File, "src/ma".to_string());
        for item in &dropdown.items {
            assert!(
                item.starts_with("src/"),
                "expected item to start with 'src/', got: {}",
                item
            );
        }
    }
}
