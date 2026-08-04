//! `skill` tool — lets the agent load skill instructions on demand.
//!
//! The tool is registered in the tool registry like any other tool. When
//! called, it looks up the named skill in the global [`SkillIndex`] and
//! returns its full content (SKILL.md body + any referenced files).

use std::sync::Arc;

use crate::skills::{SkillIndex, SkillLoader};
use crate::tool::{Tool, ToolError, ToolParam, ToolResult, ToolSchema};
use std::collections::HashMap;

/// Tool name exposed to the model.
const SKILL_TOOL_NAME: &str = "skill";

/// The `skill` tool implementation.
///
/// Allows the agent to load a skill by name and receive its instructions.
/// When `activate: true` (default), the skill is also activated for the
/// session duration, meaning its instructions remain available for
/// subsequent turns.
pub struct SkillTool {
    /// The skill index to look up skills from.
    index: Option<Arc<std::sync::RwLock<SkillIndex>>>,
    /// The global skill loader for activation.
    loader: Option<Box<dyn SkillLoader>>,
}

impl SkillTool {
    /// Create a new `SkillTool` backed by a shared skill index.
    ///
    /// The index is wrapped in `Arc<RwLock<>>` so it can be updated
    /// (e.g., when reloading skills at runtime).
    pub fn new(index: Arc<std::sync::RwLock<SkillIndex>>) -> Self {
        Self {
            index: Some(index),
            loader: None,
        }
    }

    /// Create a `SkillTool` that uses the global [`SkillLoader`] for
    /// resolution and activation.
    pub fn with_loader() -> Self {
        Self {
            index: None,
            loader: None,
        }
    }

    /// Attach a custom [`SkillLoader`] to this tool.
    pub fn with_skill_loader(mut self, loader: Box<dyn SkillLoader>) -> Self {
        self.loader = Some(loader);
        self
    }
}

impl Tool for SkillTool {
    fn name(&self) -> &str {
        SKILL_TOOL_NAME
    }

    fn description(&self) -> &str {
        "Load a skill's instructions by name. Skills package domain-specific \
         knowledge that the agent can load on demand. Use this tool when \
         the task requires expertise in a specific area (e.g., Rust, React, \
         security, deployment). Call `skill` with the skill name to load \
         its full instructions. Set `activate: true` (default) to keep the \
         instructions active for the session."
    }

    fn schema(&self) -> ToolSchema {
        let mut params = HashMap::new();
        params.insert(
            "name".to_string(),
            ToolParam {
                param_type: "string".to_string(),
                description: Some("The exact name of the skill to load.".to_string()),
                default: None,
            },
        );
        params.insert(
            "activate".to_string(),
            ToolParam {
                param_type: "boolean".to_string(),
                description: Some(
                    "Whether to keep the skill instructions active for the \
                     session (default: true)"
                        .to_string(),
                ),
                default: Some(serde_json::Value::Bool(true)),
            },
        );
        ToolSchema {
            name: SKILL_TOOL_NAME.to_string(),
            description: self.description().to_string(),
            parameters: params,
            required: vec!["name".to_string()],
        }
    }

    fn execute(&self, args: serde_json::Value) -> ToolResult<serde_json::Value> {
        let name = args
            .get("name")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::new("Missing required argument: 'name' (string)"))?;

        let activate = args
            .get("activate")
            .and_then(|v| v.as_bool())
            .unwrap_or(true);

        // Try to resolve via loader first, then via index.
        let result = if let Some(loader) = &self.loader {
            loader.resolve(name)
        } else if let Some(index) = &self.index {
            index.read().unwrap().get(name).map(|skill| {
                let mut content = skill.body.clone();
                // Collect referenced files relative to skill directory
                if let Ok(entries) = std::fs::read_dir(&skill.dir) {
                    for entry in entries.flatten() {
                        let path = entry.path();
                        if path.is_file() {
                            let fname = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
                            // Skip SKILL.md itself (already included as body)
                            if fname == "SKILL.md" {
                                continue;
                            }
                            if let Ok(file_content) = std::fs::read_to_string(&path) {
                                content.push_str(&format!(
                                    "\n\n---\n# Referenced file: {}\n\n{}",
                                    fname, file_content
                                ));
                            }
                        }
                    }
                }
                content
            })
        } else {
            // Fallback: resolve through the global loader
            crate::skills::resolve_skill(name)
        };

        match result {
            Some(content) => {
                // Optionally activate for session duration.
                // Activation is BEST-EFFORT: if no `SkillLoader` is
                // registered (e.g. the CLI didn't call
                // `set_skill_loader`), the skill content is still
                // returned as the tool result so the model receives
                // the instructions immediately.  Activation failure
                // should never discard or prevent the content from
                // reaching the model.
                let mut activated = false;
                if activate {
                    let activation_result = if let Some(loader) = &self.loader {
                        loader.activate(name)
                    } else {
                        crate::skills::activate_skill(name)
                    };
                    activated = activation_result.is_ok();
                    if let Err(ref e) = activation_result {
                        tracing::warn!(
                            skill = name,
                            error = %e,
                            "Skill found but activation failed (best-effort)"
                        );
                    }
                }

                Ok(serde_json::json!({
                    "skill": name,
                    "content": content,
                    "activated": activated,
                }))
            }
            None => Err(ToolError::new(format!(
                "Skill '{}' not found. Use the /skills command to list \
                 available skills, or check that the skill directory exists \
                 and contains a valid SKILL.md file.",
                name
            ))),
        }
    }

    fn is_read_only(&self) -> bool {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::skills::{Skill, SkillMetadata};

    fn make_index_with_skills(skills: Vec<Skill>) -> Arc<std::sync::RwLock<SkillIndex>> {
        let mut index = SkillIndex::new();
        for skill in skills {
            index.insert(skill);
        }
        Arc::new(std::sync::RwLock::new(index))
    }

    #[test]
    fn skill_tool_returns_content() {
        // Since we can't create the temp file, we use the index but the dir
        // doesn't exist so execute should find the skill but fail on loading
        // files. Actually execute returns the body which is already loaded.
        // Let's test with a skill where the dir exists.
        let dir = std::env::temp_dir().join("skill_tool_test");
        let _ = std::fs::create_dir_all(&dir);
        std::fs::write(
            dir.join("SKILL.md"),
            "---\nname: test-skill\ndescription: A test\n---\n# Test\nDo stuff.",
        )
        .unwrap();

        let skill = Skill {
            metadata: SkillMetadata {
                name: "test-skill".to_string(),
                description: "A test".to_string(),
            },
            dir: dir.clone(),
            content: "---\nname: test-skill\ndescription: A test\n---\n# Test\nDo stuff."
                .to_string(),
            frontmatter: "name: test-skill\ndescription: A test".to_string(),
            body: "# Test\nDo stuff.".to_string(),
        };

        let index = make_index_with_skills(vec![skill]);
        let tool = SkillTool::new(index);

        let result = tool.execute(serde_json::json!({"name": "test-skill", "activate": false}));
        assert!(result.is_ok());
        let val = result.unwrap();
        assert_eq!(val["skill"], "test-skill");
        assert!(val["content"].as_str().unwrap().contains("Do stuff."));
        assert_eq!(val["activated"], false);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn skill_tool_missing_name() {
        let index = make_index_with_skills(Vec::new());
        let tool = SkillTool::new(index);
        let result = tool.execute(serde_json::json!({}));
        assert!(result.is_err());
    }

    #[test]
    fn skill_tool_unknown_skill() {
        let index = make_index_with_skills(Vec::new());
        let tool = SkillTool::new(index);
        let result = tool.execute(serde_json::json!({"name": "nonexistent"}));
        assert!(result.is_err());
        assert!(result.unwrap_err().0.contains("not found"));
    }

    #[test]
    fn skill_tool_includes_referenced_files() {
        let dir = std::env::temp_dir().join("skill_tool_refs");
        let _ = std::fs::create_dir_all(&dir);

        // Write SKILL.md
        std::fs::write(
            dir.join("SKILL.md"),
            "---\nname: refs-skill\ndescription: Skill with refs\n---\n# Main\nInstructions.",
        )
        .unwrap();

        // Write a referenced file
        std::fs::write(dir.join("config.json"), r#"{"setting": "value"}"#).unwrap();

        let skill = Skill {
            metadata: SkillMetadata {
                name: "refs-skill".to_string(),
                description: "Skill with refs".to_string(),
            },
            dir: dir.clone(),
            content: "---\n...\n---\n# Main\nInstructions.".to_string(),
            frontmatter: "...".to_string(),
            body: "# Main\nInstructions.".to_string(),
        };

        let index = make_index_with_skills(vec![skill]);
        let tool = SkillTool::new(index);

        let result = tool.execute(serde_json::json!({"name": "refs-skill", "activate": false}));
        assert!(result.is_ok());
        let val = result.unwrap();
        let content = val["content"].as_str().unwrap();
        assert!(content.contains("config.json"));
        assert!(content.contains(r#""setting": "value""#));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn skill_tool_is_read_only() {
        let index = make_index_with_skills(Vec::new());
        let tool = SkillTool::new(index);
        assert!(tool.is_read_only());
    }
}
