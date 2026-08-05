//! Port of `reference/packages/opencode/src/tool/skill.ts`.

use crate::model::{ExecuteResult, PermissionRequest, ToolError};
use crate::prompts;
use crate::ripgrep;
use crate::schema::{prop, Schema};

/// `Parameters` from `reference/packages/opencode/src/tool/skill.ts:8`.
pub fn parameters() -> Schema {
    Schema::struct_(
        vec![prop(
            "name",
            Schema::string("The name of the skill from available_skills"),
        )],
        "skill",
    )
}

/// `SkillTool` from `reference/packages/opencode/src/tool/skill.ts:12`.
pub fn def() -> crate::tool::tool::Def {
    crate::tool::tool::def("skill", prompts::SKILL, parameters(), |args, ctx| {
        let name = args
            .get("name")
            .and_then(|value| value.as_str())
            .unwrap_or("")
            .to_string();
        let info = ctx
            .services
            .skill_require(&name)
            .map_err(|error| ToolError::Other(format!("Unable to load skill {name}: {error}")))?;

        ctx.ask(PermissionRequest {
            permission: "skill".to_string(),
            patterns: vec![name.clone()],
            always: vec![name.clone()],
            metadata: serde_json::json!({}),
        })?;

        let dir = std::path::Path::new(&info.location)
            .parent()
            .map(|parent| parent.to_string_lossy().to_string())
            .unwrap_or_default();
        let files = ripgrep::find(&ripgrep::FindInput {
            cwd: dir.clone(),
            pattern: "!**/SKILL.md".to_string(),
            limit: 10,
            hidden: true,
            follow: false,
        })
        .map_err(|error| ToolError::Other(format!("Unable to load skill {name}: {error}")))?;

        let file_blocks: Vec<String> = files
            .iter()
            .map(|file| {
                let absolute = std::path::Path::new(&dir).join(&file.path);
                format!("<file>{}</file>", absolute.to_string_lossy())
            })
            .collect();
        let output = [
                format!("<skill_content name=\"{}\">", info.name),
                format!("# Skill: {}", info.name),
                String::new(),
                info.content.trim().to_string(),
                String::new(),
                format!("Base directory for this skill: {dir}"),
                "Relative paths in this skill (e.g., scripts/, reference/) are relative to this base directory.".to_string(),
                "Note: file list is sampled.".to_string(),
                String::new(),
                "<skill_files>".to_string(),
                file_blocks.join("\n"),
                "</skill_files>".to_string(),
                "</skill_content>".to_string(),
            ]
            .join("\n");

        Ok(ExecuteResult {
            title: format!("Loaded skill: {}", info.name),
            output,
            metadata: serde_json::json!({ "name": info.name, "dir": dir }),
            attachments: None,
        })
    })
}
