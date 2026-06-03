//! Skill system — domain-specific instructions loaded on demand.
//!
//! Follows the [Agent Skills standard](https://agentskills.io/specification)
//! for cross-agent compatibility with pi, opencode, codex, and similar tools.
//!
//! ## Structure
//!
//! - [`SkillMetadata`]: Parsed frontmatter from a `SKILL.md` file.
//! - [`Skill`]: A discovered skill with its directory, metadata, and content.
//! - [`SkillIndex`]: Collection of all discovered skills with lookup.
//! - [`SkillLoader`]: Trait for resolving/activating skills at runtime.
//! - [`SkillTool`]: The `skill` tool registered in the tool registry.

pub mod discovery;
pub mod tool;

pub use discovery::{discover_skills, DiscoveryConfig};
pub use tool::SkillTool;

use std::collections::HashMap;
use std::path::PathBuf;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Frontmatter metadata parsed from a `SKILL.md` file.
#[derive(Debug, Clone, PartialEq)]
pub struct SkillMetadata {
    /// Machine name: lowercase a-z, 0-9, hyphens only, 1-64 chars.
    /// Must match the parent directory name.
    pub name: String,
    /// Human-readable description (max 1024 chars).
    pub description: String,
}

/// A single discovered skill.
#[derive(Debug, Clone)]
pub struct Skill {
    /// Parsed frontmatter.
    pub metadata: SkillMetadata,
    /// Absolute path to the skill directory.
    pub dir: PathBuf,
    /// Full content of the SKILL.md file (frontmatter + body).
    pub content: String,
    /// Raw frontmatter string (the `---\n...\n---` block).
    pub frontmatter: String,
    /// Body after the frontmatter block.
    pub body: String,
}

impl Skill {
    pub fn name(&self) -> &str {
        &self.metadata.name
    }

    pub fn description(&self) -> &str {
        &self.metadata.description
    }

    /// Path to the SKILL.md file.
    pub fn skill_md_path(&self) -> PathBuf {
        self.dir.join("SKILL.md")
    }
}

/// Index of all discovered skills.
#[derive(Debug, Clone, Default)]
pub struct SkillIndex {
    /// name → Skill (first-found wins on collision).
    skills: HashMap<String, Skill>,
    /// Names that were skipped due to the blocklist.
    blocked: Vec<String>,
}

impl SkillIndex {
    pub fn new() -> Self {
        Self::default()
    }

    /// Insert a skill. Returns `true` if this is the first skill with that name.
    pub fn insert(&mut self, skill: Skill) -> bool {
        let name = skill.name().to_string();
        if self.skills.contains_key(&name) {
            false
        } else {
            self.skills.insert(name, skill);
            true
        }
    }

    /// Look up a skill by name (exact match).
    pub fn get(&self, name: &str) -> Option<&Skill> {
        self.skills.get(name)
    }

    /// All skills (unordered).
    pub fn all(&self) -> Vec<&Skill> {
        self.skills.values().collect()
    }

    /// Number of skills in the index.
    pub fn len(&self) -> usize {
        self.skills.len()
    }

    pub fn is_empty(&self) -> bool {
        self.skills.is_empty()
    }

    /// Names that were blocked from the index.
    pub fn blocked(&self) -> &[String] {
        &self.blocked
    }

    pub fn add_blocked(&mut self, name: String) {
        self.blocked.push(name);
    }

    /// Remove a skill by name (returns the skill if it existed).
    pub fn remove(&mut self, name: &str) -> Option<Skill> {
        self.skills.remove(name)
    }
}

impl FromIterator<Skill> for SkillIndex {
    fn from_iter<I: IntoIterator<Item = Skill>>(iter: I) -> Self {
        let mut index = SkillIndex::new();
        for skill in iter {
            index.insert(skill);
        }
        index
    }
}

// ---------------------------------------------------------------------------
// Skill Loader trait (callback pattern, following QuestionHandler precedent)
// ---------------------------------------------------------------------------

/// Trait for resolving and activating skills at runtime.
///
/// Follows the same callback pattern as [`QuestionHandler`] and
/// [`TodoChangeHandler`] — the CLI registers its own implementation and
/// the core library delegates through it.
pub trait SkillLoader: Send + Sync {
    /// Resolve a skill by name and return its full content (SKILL.md +
    /// referenced files joined into one blob), or `None` if unknown.
    fn resolve(&self, name: &str) -> Option<String>;

    /// List all available skills as `(name, description)` pairs.
    fn list(&self) -> Vec<(String, String)>;

    /// Activate a skill for the session duration. May append instructions
    /// to the system prompt / active context.
    fn activate(&self, name: &str) -> Result<String, String>;

    /// Deactivate the currently active skill.
    fn deactivate(&self);

    /// Name of the currently active skill, if any.
    fn active_skill(&self) -> Option<String>;
}

// Global default: always returns empty / no-op.
static SKILL_LOADER: std::sync::OnceLock<std::sync::RwLock<Option<Box<dyn SkillLoader>>>> =
    std::sync::OnceLock::new();

fn skill_loader_rw() -> &'static std::sync::RwLock<Option<Box<dyn SkillLoader>>> {
    SKILL_LOADER.get_or_init(|| std::sync::RwLock::new(None))
}

/// Register the global [`SkillLoader`] implementation.
///
/// The CLI calls this at startup with an implementation that wraps the
/// orchestrator's skill index and system prompt.
pub fn set_skill_loader(loader: Box<dyn SkillLoader>) {
    *skill_loader_rw().write().unwrap() = Some(loader);
}

/// Remove the global [`SkillLoader`].
pub fn clear_skill_loader() {
    *skill_loader_rw().write().unwrap() = None;
}

/// Invoke the global [`SkillLoader::resolve`], or return `None` if unset.
pub fn resolve_skill(name: &str) -> Option<String> {
    skill_loader_rw()
        .read()
        .unwrap()
        .as_ref()
        .and_then(|l| l.resolve(name))
}

/// Invoke the global [`SkillLoader::list`], or return empty.
pub fn list_skills() -> Vec<(String, String)> {
    skill_loader_rw()
        .read()
        .unwrap()
        .as_ref()
        .map(|l| l.list())
        .unwrap_or_default()
}

/// Invoke the global [`SkillLoader::activate`], or return error.
pub fn activate_skill(name: &str) -> Result<String, String> {
    skill_loader_rw()
        .read()
        .unwrap()
        .as_ref()
        .ok_or_else(|| "No skill loader registered".to_string())
        .and_then(|l| l.activate(name))
}

/// Invoke the global [`SkillLoader::deactivate`].
pub fn deactivate_skill() {
    if let Some(loader) = skill_loader_rw().read().unwrap().as_ref() {
        loader.deactivate();
    }
}

/// Invoke the global [`SkillLoader::active_skill`].
pub fn active_skill() -> Option<String> {
    skill_loader_rw()
        .read()
        .unwrap()
        .as_ref()
        .and_then(|l| l.active_skill())
}
