//! Dropdown widget for command and file completion
//!
//! Handles:
//! - `/` command dropdown: shows available slash commands
//! - `@` file dropdown: shows files and directories with fuzzy matching

use std::path::PathBuf;

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
    pub query: String,
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
            query,
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

    /// Filter files by query — supports nested paths, directories, and fuzzy matching
    fn filter_files(query: &str) -> Vec<String> {
        let mut results = Vec::new();

        // Determine base path and search pattern
        let (base_path, search_pattern) = if query.contains('/') {
            let path = PathBuf::from(query);
            if query.ends_with('/') {
                // "src/" -> list contents of src/
                (path, String::new())
            } else {
                // "src/ma" -> list contents of src/, filter by "ma"
                let parent = path.parent().map(|p| p.to_path_buf()).unwrap_or_default();
                let file_part = path
                    .file_name()
                    .and_then(|f| f.to_str())
                    .unwrap_or("")
                    .to_string();
                (parent, file_part)
            }
        } else {
            // No slash -> search current directory
            (PathBuf::from("."), query.to_string())
        };

        // Read directory entries
        if let Ok(entries) = std::fs::read_dir(&base_path) {
            for entry in entries.filter_map(|e| e.ok()) {
                let file_name = entry.file_name().to_string_lossy().to_string();

                // Skip hidden files/dirs unless query explicitly starts with .
                if file_name.starts_with('.') && !search_pattern.starts_with('.') {
                    continue;
                }

                // Skip common non-useful dirs
                if file_name == "target" || file_name == "node_modules" || file_name == ".git" {
                    if !search_pattern.starts_with(&file_name[..2.min(file_name.len())]) {
                        continue;
                    }
                }

                // Filter by search pattern (prefix match + contains fallback)
                let matches = if search_pattern.is_empty() {
                    true
                } else {
                    let fname_lower = file_name.to_lowercase();
                    let pattern_lower = search_pattern.to_lowercase();
                    fname_lower.starts_with(&pattern_lower)
                        || fname_lower.contains(&pattern_lower)
                };

                if matches {
                    let base_str = base_path.to_string_lossy();
                    let full_path = if base_str == "." {
                        file_name.clone()
                    } else {
                        let base_clean = base_str.trim_end_matches('/');
                        format!("{}/{}", base_clean, file_name)
                    };

                    // Add trailing slash for directories
                    let display = if entry.path().is_dir() {
                        format!("{}/", full_path)
                    } else {
                        full_path
                    };

                    results.push(display);
                }
            }
        }

        // Sort: directories first, then alphabetically
        results.sort_by(|a, b| {
            let a_is_dir = a.ends_with('/');
            let b_is_dir = b.ends_with('/');
            match (a_is_dir, b_is_dir) {
                (true, false) => std::cmp::Ordering::Less,
                (false, true) => std::cmp::Ordering::Greater,
                _ => a.to_lowercase().cmp(&b.to_lowercase()),
            }
        });

        // Limit results
        results.truncate(20);
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
    fn test_file_filter_current_dir() {
        let dropdown = Dropdown::new(DropdownType::File, "src".to_string());
        // Should find src/ if it exists
        if std::path::Path::new("src").exists() {
            assert!(dropdown.items.iter().any(|i| i.contains("src")));
        }
    }

    #[test]
    fn test_file_filter_nested() {
        // This tests that nested path queries work
        let dropdown = Dropdown::new(DropdownType::File, "src/".to_string());
        // Just verify it doesn't crash
        let _ = dropdown.items;
    }
}
