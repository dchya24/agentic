//! Dropdown widget for command and file completion

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
#[allow(dead_code)]
pub struct Dropdown {
    pub dropdown_type: DropdownType,
    pub query: String,
    pub items: Vec<String>,
    pub selected: usize,
    pub visible_count: usize,
}

/// Available slash commands
const SLASH_COMMANDS: &[(&str, &str)] = &[
    ("help", "Show help message"),
    ("clear", "Clear conversation"),
    ("config", "Show configuration"),
    ("tools", "List available tools"),
    ("history", "Show command history"),
    ("save", "Save conversation to file"),
    ("load", "Load conversation from file"),
    ("mcp", "Show MCP server status"),
    ("plan", "Create a plan for a goal"),
    ("model", "Switch model"),
    ("provider", "Switch provider"),
    ("quit", "Exit TUI"),
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

    /// Filter slash commands by query
    fn filter_commands(query: &str) -> Vec<String> {
        let query_lower = query.to_lowercase();
        SLASH_COMMANDS
            .iter()
            .filter(|(cmd, _)| cmd.starts_with(&query_lower))
            .map(|(cmd, _)| cmd.to_string())
            .collect()
    }

    /// Filter files by query
    fn filter_files(query: &str) -> Vec<String> {
        let mut results = Vec::new();
        
        // Determine base path and search pattern
        let (base_path, search_pattern) = if query.contains('/') {
            let path = PathBuf::from(query);
            if query.ends_with('/') {
                (path, String::new())
            } else {
                let parent = path.parent().map(|p| p.to_path_buf()).unwrap_or_default();
                let file_part = path.file_name()
                    .and_then(|f| f.to_str())
                    .unwrap_or("")
                    .to_string();
                (parent, file_part)
            }
        } else {
            (PathBuf::from("."), query.to_string())
        };

        // Read directory
        if let Ok(entries) = std::fs::read_dir(&base_path) {
            for entry in entries.filter_map(|e| e.ok()) {
                let file_name = entry.file_name().to_string_lossy().to_string();
                
                // Skip hidden files unless query starts with .
                if file_name.starts_with('.') && !search_pattern.starts_with('.') {
                    continue;
                }

                // Filter by search pattern
                if file_name.to_lowercase().starts_with(&search_pattern.to_lowercase()) {
                    // Build full path properly
                    let base_str = base_path.to_string_lossy();
                    let full_path = if base_str == "." {
                        file_name.clone()
                    } else {
                        // Remove trailing slash from base if present, then join
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

    /// Select previous item
    pub fn select_prev(&mut self) {
        if self.selected > 0 {
            self.selected -= 1;
        } else if !self.items.is_empty() {
            self.selected = self.items.len() - 1;
        }
    }

    /// Select next item
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
                .find(|(c, _)| *c == cmd)
                .map(|(_, desc)| *desc)
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
}
