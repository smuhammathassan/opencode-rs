//! Skills: discovery, loading, and rendering the available-skills context.
//!
//! From reference/packages/opencode/src/skill/index.ts. `Info` mirrors the
//! `Skill.Info` schema; discovery mirrors `discoverSkills`/`loadSkills`/`add`.

pub mod discovery;

use crate::frontmatter;
use crate::util::{config_directories, env_flag, escape_html, scan, up, ScanOptions};
use indexmap::IndexMap;
use serde_json::Value;
use std::path::{Path, PathBuf};

const CLAUDE_EXTERNAL_DIR: &str = ".claude";
const AGENTS_EXTERNAL_DIR: &str = ".agents";
const EXTERNAL_SKILL_PATTERN: &str = "skills/**/SKILL.md";
const OPENCODE_SKILL_PATTERN: &str = "{skill,skills}/**/SKILL.md";
const SKILL_PATTERN: &str = "**/SKILL.md";

const CUSTOMIZE_OPENCODE_SKILL_NAME: &str = "customize-opencode";
const CUSTOMIZE_OPENCODE_SKILL_DESCRIPTION: &str = "Use ONLY when the user is editing or creating opencode's own configuration: opencode.json, opencode.jsonc, files under .opencode/, or files under ~/.config/opencode/. Also use when creating or fixing opencode agents, subagents, skills, plugins, MCP servers, or permission rules. Do not use for the user's own application code, or for any project that is not configuring opencode itself.";
const CUSTOMIZE_OPENCODE_SKILL_BODY: &str = include_str!("customize-opencode.md");

/// Skill info, mirroring `Skill.Info` from
/// reference/packages/opencode/src/skill/index.ts.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Info {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub location: String,
    pub content: String,
}

/// Skill-related errors, mirroring the error classes in
/// reference/packages/opencode/src/skill/index.ts.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Skill \"{name}\" not found. Available skills: {available}")]
    NotFound { name: String, available: String },
    #[error("Skill name mismatch at {path}: expected \"{expected}\", found \"{actual}\"")]
    NameMismatch {
        path: String,
        expected: String,
        actual: String,
    },
    #[error("Invalid skill at {path}: {message:?}")]
    Invalid {
        path: String,
        message: Option<String>,
        issues: Option<Vec<Issue>>,
    },
}

/// A single validation issue (mirrors `Issue` in `Skill.InvalidError`).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Issue {
    pub message: String,
    pub path: Vec<String>,
}

/// Discovery settings mirroring the inputs of `discoverSkills`.
#[derive(Debug, Clone)]
pub struct Settings {
    pub home: PathBuf,
    pub directory: PathBuf,
    pub worktree: PathBuf,
    pub disable_external_skills: bool,
    pub disable_claude_code_skills: bool,
    /// `cfg.skills?.paths`.
    pub paths: Vec<String>,
    /// Directories already pulled from `cfg.skills?.urls` by
    /// [`crate::skill::discovery::Discovery::pull`].
    pub pulled_dirs: Vec<PathBuf>,
    /// Overrides the config directories to scan. When `None`, computed the
    /// same way as `ConfigPaths.directories`.
    pub config_dirs: Option<Vec<PathBuf>>,
}

impl Settings {
    /// The config search directories for skill scanning.
    /// From reference/packages/opencode/src/config/paths.ts.
    pub fn directories(&self) -> Vec<PathBuf> {
        match &self.config_dirs {
            Some(dirs) => dirs.clone(),
            None => config_directories(&self.home, &self.directory, Some(&self.worktree)),
        }
    }
}

/// Loaded skill registry, mirroring the `State` in
/// reference/packages/opencode/src/skill/index.ts.
#[derive(Debug, Default)]
pub struct SkillService {
    skills: IndexMap<String, Info>,
    dirs: Vec<PathBuf>,
}

impl SkillService {
    /// Discover skills using the process-wide external-skill environment
    /// switches in addition to the explicit settings.
    ///
    /// `SkillService::load` remains deterministic for callers that provide
    /// all policy explicitly; production entry points use this wrapper to
    /// honor the reference `OPENCODE_DISABLE_EXTERNAL_SKILLS` and
    /// `OPENCODE_DISABLE_CLAUDE_CODE_SKILLS` flags.
    pub fn load_with_environment(settings: &Settings) -> anyhow::Result<SkillService> {
        let mut settings = settings.clone();
        settings.disable_external_skills |= env_flag("OPENCODE_DISABLE_EXTERNAL_SKILLS");
        settings.disable_claude_code_skills |= env_flag("OPENCODE_DISABLE_CLAUDE_CODE_SKILLS");
        Self::load(&settings)
    }

    /// Discover and load all skills for a session.
    pub fn load(settings: &Settings) -> anyhow::Result<SkillService> {
        let mut service = SkillService::default();
        // Register the built-in skill BEFORE disk discovery so a user-disk
        // skill with the same name can override it.
        service.skills.insert(
            CUSTOMIZE_OPENCODE_SKILL_NAME.to_string(),
            Info {
                name: CUSTOMIZE_OPENCODE_SKILL_NAME.to_string(),
                description: Some(CUSTOMIZE_OPENCODE_SKILL_DESCRIPTION.to_string()),
                location: "<built-in>".to_string(),
                content: CUSTOMIZE_OPENCODE_SKILL_BODY.to_string(),
            },
        );

        let mut matches: Vec<PathBuf> = Vec::new();
        let mut dirs: Vec<PathBuf> = Vec::new();

        if !settings.disable_external_skills {
            let mut external_dirs: Vec<&str> = Vec::new();
            if !settings.disable_claude_code_skills {
                external_dirs.push(CLAUDE_EXTERNAL_DIR);
            }
            external_dirs.push(AGENTS_EXTERNAL_DIR);

            for dir in &external_dirs {
                let root = settings.home.join(dir);
                if !root.is_dir() {
                    continue;
                }
                scan_into(&mut matches, &mut dirs, &root, EXTERNAL_SKILL_PATTERN, true)?;
            }

            let up_dirs = up(
                &settings.directory,
                Some(&settings.worktree),
                &external_dirs,
            );
            for root in up_dirs {
                scan_into(&mut matches, &mut dirs, &root, EXTERNAL_SKILL_PATTERN, true)?;
            }
        }

        for dir in settings.directories() {
            scan_into(&mut matches, &mut dirs, &dir, OPENCODE_SKILL_PATTERN, false)?;
        }

        for item in &settings.paths {
            let expanded = match item.strip_prefix("~/") {
                Some(rest) => settings.home.join(rest),
                None => PathBuf::from(item),
            };
            let dir = if expanded.is_absolute() {
                expanded
            } else {
                settings.directory.join(expanded)
            };
            if !dir.is_dir() {
                tracing::warn!(path = %dir.display(), "skill path not found");
                continue;
            }
            scan_into(&mut matches, &mut dirs, &dir, SKILL_PATTERN, false)?;
        }

        for dir in &settings.pulled_dirs {
            scan_into(&mut matches, &mut dirs, dir, SKILL_PATTERN, false)?;
        }

        // `add` is ordered: each root's matches are sorted, and later scans
        // (config, paths, urls) override earlier ones (external dirs), which
        // mirrors the reference's scan order.
        for path in &matches {
            add(&mut service, path);
        }

        service.dirs = dedup(dirs);
        Ok(service)
    }

    pub fn get(&self, name: &str) -> Option<&Info> {
        self.skills.get(name)
    }

    /// Mirror of `Skill.require`: fails with the sorted available names when
    /// the skill is missing.
    pub fn require(&self, name: &str) -> Result<&Info, Error> {
        if let Some(info) = self.skills.get(name) {
            return Ok(info);
        }
        let mut available: Vec<&String> = self.skills.keys().collect();
        available.sort();
        let joined = if available.is_empty() {
            "none".to_string()
        } else {
            available
                .into_iter()
                .map(String::as_str)
                .collect::<Vec<_>>()
                .join(", ")
        };
        Err(Error::NotFound {
            name: name.to_string(),
            available: joined,
        })
    }

    pub fn all(&self) -> Vec<&Info> {
        self.skills.values().collect()
    }

    pub fn dirs(&self) -> &[PathBuf] {
        &self.dirs
    }

    /// Skills sorted by name, optionally filtered by a permission predicate.
    /// From reference/packages/opencode/src/skill/index.ts (`Skill.available`).
    pub fn available(&self, allow: Option<&dyn Fn(&str) -> bool>) -> Vec<&Info> {
        let mut list: Vec<&Info> = self
            .skills
            .values()
            .filter(|skill| allow.map_or(true, |allow| allow(&skill.name)))
            .collect();
        list.sort_by(|a, b| a.name.cmp(&b.name));
        list
    }
}

/// Render the available-skills system context block.
/// From reference/packages/opencode/src/skill/index.ts (`fmt`).
pub fn fmt(list: &[Info], verbose: bool) -> String {
    let mut described: Vec<&Info> = list
        .iter()
        .filter(|skill| skill.description.is_some())
        .collect();
    if described.is_empty() {
        return "No skills are currently available.".to_string();
    }
    described.sort_by(|a, b| a.name.cmp(&b.name));
    if verbose {
        let mut lines = vec!["<available_skills>".to_string()];
        for skill in described {
            lines.push("  <skill>".to_string());
            lines.push(format!("    <name>{}</name>", skill.name));
            lines.push(format!(
                "    <description>{}</description>",
                skill.description.as_deref().expect("filtered above")
            ));
            lines.push(format!(
                "    <location>{}</location>",
                escape_html(&skill.location)
            ));
            lines.push("  </skill>".to_string());
        }
        lines.push("</available_skills>".to_string());
        return lines.join("\n");
    }
    let mut lines = vec!["## Available Skills".to_string()];
    for skill in described {
        lines.push(format!(
            "- **{}**: {}",
            skill.name,
            skill.description.as_deref().expect("filtered above")
        ));
    }
    lines.join("\n")
}

fn scan_into(
    matches: &mut Vec<PathBuf>,
    dirs: &mut Vec<PathBuf>,
    root: &Path,
    pattern: &str,
    dot: bool,
) -> anyhow::Result<()> {
    for path in scan(root, pattern, &ScanOptions { dot, follow: false })? {
        matches.push(path.clone());
        if let Some(dir) = path.parent() {
            dirs.push(dir.to_path_buf());
        }
    }
    Ok(())
}

fn add(service: &mut SkillService, path: &Path) {
    let md = match frontmatter::parse_file(path) {
        Ok(md) => md,
        Err(error) => {
            // TODO(integration): publish Session.Event.Error via the oc-core
            // event bus, mirroring the reference's error path.
            tracing::error!(skill = %path.display(), error = %error, "failed to load skill");
            return;
        }
    };
    let Some(data) = md.data.as_object() else {
        return;
    };
    if !is_skill_frontmatter(data) {
        return;
    }
    let name = data
        .get("name")
        .and_then(Value::as_str)
        .expect("checked by is_skill_frontmatter")
        .to_string();
    if let Some(existing) = service.skills.get(&name) {
        tracing::warn!(
            name = %name,
            existing = %existing.location,
            duplicate = %path.display(),
            "duplicate skill name"
        );
    }
    service.skills.insert(
        name.clone(),
        Info {
            name,
            description: data
                .get("description")
                .and_then(Value::as_str)
                .map(str::to_string),
            location: path.display().to_string(),
            content: md.content.clone(),
        },
    );
}

fn is_skill_frontmatter(data: &serde_json::Map<String, Value>) -> bool {
    data.get("name").is_some_and(Value::is_string)
        && data.get("description").is_none_or(Value::is_string)
}

fn dedup(dirs: Vec<PathBuf>) -> Vec<PathBuf> {
    let mut result: Vec<PathBuf> = Vec::new();
    for dir in dirs {
        if !result.contains(&dir) {
            result.push(dir);
        }
    }
    result
}
