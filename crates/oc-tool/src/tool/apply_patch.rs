//! Port of `reference/packages/opencode/src/tool/apply_patch.ts`.

use crate::diff::{create_two_files_patch, diff_lines, trim_diff};
use crate::model::{ExecuteResult, PermissionRequest, ToolError};
use crate::patch::{self, Hunk};
use crate::prompts;
use crate::schema::{prop, Schema};
use crate::tool::external_directory;
use crate::util::bom;

/// `Parameters` from `reference/packages/opencode/src/tool/apply_patch.ts:18`.
pub fn parameters() -> Schema {
    Schema::struct_(
        vec![prop(
            "patchText",
            Schema::string("The full patch text that describes all changes to be made"),
        )],
        "apply_patch",
    )
}

struct FileChange {
    file_path: String,
    #[allow(dead_code)]
    old_content: String,
    new_content: String,
    kind: &'static str,
    move_path: Option<String>,
    diff: String,
    additions: usize,
    deletions: usize,
    bom: bool,
}

/// `ApplyPatchTool` from `reference/packages/opencode/src/tool/apply_patch.ts:22`.
pub fn def() -> crate::tool::tool::Def {
    crate::tool::tool::def(
        "apply_patch",
        prompts::APPLY_PATCH,
        parameters(),
        |args, ctx| {
            let patch_text = args
                .get("patchText")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            if patch_text.is_empty() {
                return Err(ToolError::Other("patchText is required".to_string()));
            }

            let hunks = patch::parse_patch(&patch_text).map_err(|error| {
                ToolError::Other(format!("apply_patch verification failed: {error}"))
            })?;
            if hunks.is_empty() {
                let normalized = patch_text
                    .replace("\r\n", "\n")
                    .replace('\r', "\n")
                    .trim()
                    .to_string();
                if normalized == "*** Begin Patch\n*** End Patch" {
                    return Err(ToolError::Other("patch rejected: empty patch".to_string()));
                }
                return Err(ToolError::Other(
                    "apply_patch verification failed: no hunks found".to_string(),
                ));
            }

            let instance = ctx.instance.clone().ok_or_else(|| {
                ToolError::Other(
                    "InstanceState.context is required for the apply_patch tool".to_string(),
                )
            })?;
            let directory = &instance.directory;
            let worktree = &instance.worktree;

            let mut file_changes: Vec<FileChange> = Vec::new();
            let mut total_diff = String::new();

            for hunk in &hunks {
                let file_path = crate::util::path_resolve(directory, hunk.path());
                external_directory::assert_external_directory_file(ctx, &file_path)?;

                match hunk {
                    Hunk::Add { contents, .. } => {
                        let old_content = String::new();
                        let new_content = if contents.is_empty() || contents.ends_with('\n') {
                            contents.clone()
                        } else {
                            format!("{contents}\n")
                        };
                        let next = bom::split(&new_content);
                        let diff =
                            trim_diff(&create_two_files_patch(&file_path, &file_path, "", &next.1));
                        let mut additions = 0;
                        let mut deletions = 0;
                        for part in diff_lines("", &next.1) {
                            if part.added {
                                additions += part.count;
                            }
                            if part.removed {
                                deletions += part.count;
                            }
                        }
                        file_changes.push(FileChange {
                            file_path,
                            old_content,
                            new_content: next.1.clone(),
                            kind: "add",
                            move_path: None,
                            diff,
                            additions,
                            deletions,
                            bom: next.0,
                        });
                        total_diff.push_str(file_changes.last().unwrap().diff.as_str());
                        total_diff.push('\n');
                    }
                    Hunk::Update {
                        path: _,
                        move_path,
                        chunks,
                    } => {
                        let info = std::fs::symlink_metadata(&file_path).ok();
                        if info.as_ref().map(|meta| meta.is_dir()).unwrap_or(true) {
                            return Err(ToolError::Other(format!(
                                "apply_patch verification failed: Failed to read file to update: {file_path}"
                            )));
                        }
                        let (source_bom, source_text) =
                            bom::read_file(&file_path).map_err(|error| {
                                ToolError::Other(format!(
                                    "apply_patch verification failed: {error}"
                                ))
                            })?;
                        let old_content = source_text;
                        let update = patch::derive_new_contents_from_chunks(
                            &file_path,
                            chunks,
                            &bom::join(&old_content, source_bom),
                        )
                        .map_err(|error| {
                            ToolError::Other(format!("apply_patch verification failed: {error}"))
                        })?;
                        let new_content = update.content;
                        let diff = trim_diff(&create_two_files_patch(
                            &file_path,
                            &file_path,
                            &old_content,
                            &new_content,
                        ));
                        let mut additions = 0;
                        let mut deletions = 0;
                        for part in diff_lines(&old_content, &new_content) {
                            if part.added {
                                additions += part.count;
                            }
                            if part.removed {
                                deletions += part.count;
                            }
                        }
                        let resolved_move_path = move_path
                            .as_ref()
                            .map(|target| crate::util::path_resolve(directory, target));
                        if let Some(target) = &resolved_move_path {
                            external_directory::assert_external_directory_file(ctx, target)?;
                        }
                        file_changes.push(FileChange {
                            file_path: file_path.clone(),
                            old_content,
                            new_content: new_content.clone(),
                            kind: if move_path.is_some() {
                                "move"
                            } else {
                                "update"
                            },
                            move_path: resolved_move_path,
                            diff,
                            additions,
                            deletions,
                            bom: update.bom,
                        });
                        total_diff.push_str(file_changes.last().unwrap().diff.as_str());
                        total_diff.push('\n');
                    }
                    Hunk::Delete { .. } => {
                        let (source_bom, content_to_delete) =
                            bom::read_file(&file_path).map_err(|error| {
                                ToolError::Other(format!(
                                    "apply_patch verification failed: {}",
                                    error
                                ))
                            })?;
                        let delete_diff = trim_diff(&create_two_files_patch(
                            &file_path,
                            &file_path,
                            &content_to_delete,
                            "",
                        ));
                        let deletions = content_to_delete.split('\n').count();
                        file_changes.push(FileChange {
                            file_path,
                            old_content: content_to_delete,
                            new_content: String::new(),
                            kind: "delete",
                            move_path: None,
                            diff: delete_diff,
                            additions: 0,
                            deletions,
                            bom: source_bom,
                        });
                        total_diff.push_str(file_changes.last().unwrap().diff.as_str());
                        total_diff.push('\n');
                    }
                }
            }

            let files: Vec<serde_json::Value> = file_changes
                .iter()
                .map(|change| {
                    let target = change.move_path.clone().unwrap_or_else(|| change.file_path.clone());
                    serde_json::json!({
                        "filePath": change.file_path,
                        "relativePath": crate::util::path_relative(worktree, &target).replace('\\', "/"),
                        "type": change.kind,
                        "patch": change.diff,
                        "additions": change.additions,
                        "deletions": change.deletions,
                        "movePath": change.move_path,
                    })
                })
                .collect();

            let relative_paths: Vec<String> = file_changes
                .iter()
                .map(|change| {
                    crate::util::path_relative(worktree, &change.file_path).replace('\\', "/")
                })
                .collect();
            ctx.ask(PermissionRequest {
                permission: "edit".to_string(),
                patterns: relative_paths,
                always: vec!["*".to_string()],
                metadata: serde_json::json!({
                    "filepath": file_changes
                        .iter()
                        .map(|change| crate::util::path_relative(worktree, &change.file_path).replace('\\', "/"))
                        .collect::<Vec<_>>()
                        .join(", "),
                    "diff": total_diff,
                    "files": files,
                }),
            })?;

            for change in &file_changes {
                match change.kind {
                    "add" => {
                        crate::tool::write::write_with_dirs(
                            &change.file_path,
                            &bom::join(&change.new_content, change.bom),
                        )?;
                    }
                    "update" => {
                        crate::tool::write::write_with_dirs(
                            &change.file_path,
                            &bom::join(&change.new_content, change.bom),
                        )?;
                    }
                    "move" => {
                        if let Some(move_path) = &change.move_path {
                            crate::tool::write::write_with_dirs(
                                move_path,
                                &bom::join(&change.new_content, change.bom),
                            )?;
                            std::fs::remove_file(&change.file_path).map_err(|error| {
                                ToolError::Other(format!(
                                    "Unable to remove {}: {error}",
                                    change.file_path
                                ))
                            })?;
                        }
                    }
                    "delete" => {
                        std::fs::remove_file(&change.file_path).map_err(|error| {
                            ToolError::Other(format!(
                                "Unable to remove {}: {error}",
                                change.file_path
                            ))
                        })?;
                    }
                    _ => {}
                }
            }

            let summary_lines: Vec<String> = file_changes
                .iter()
                .map(|change| {
                    let target = change
                        .move_path
                        .clone()
                        .unwrap_or_else(|| change.file_path.clone());
                    let relative = crate::util::path_relative(worktree, &target).replace('\\', "/");
                    match change.kind {
                        "add" => format!("A {relative}"),
                        "delete" => format!("D {relative}"),
                        _ => format!("M {relative}"),
                    }
                })
                .collect();
            let output = format!(
                "Success. Updated the following files:\n{}",
                summary_lines.join("\n")
            );

            // TODO(integration): run formatter, publish watcher/edited events,
            // and append LSP diagnostic blocks.

            Ok(ExecuteResult {
                title: output.clone(),
                metadata: serde_json::json!({
                    "diff": total_diff,
                    "files": files,
                    "diagnostics": {},
                }),
                output,
                attachments: None,
            })
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::jsonschema;
    use crate::model::ToolContext;

    #[test]
    fn schema_matches_reference_snapshot() {
        let schema = jsonschema::from_schema(&parameters());
        assert_eq!(
            schema,
            serde_json::json!({
                "$schema": "https://json-schema.org/draft/2020-12/schema",
                "properties": {
                    "patchText": { "description": "The full patch text that describes all changes to be made", "type": "string" }
                },
                "required": ["patchText"],
                "type": "object"
            })
        );
    }

    #[tokio::test]
    async fn applies_add_patch() {
        let def = crate::tool::tool::wrap("apply_patch", def());
        let dir = tempfile::tempdir().unwrap();
        let mut ctx = ToolContext {
            instance: Some(crate::model::InstanceContext {
                directory: dir.path().to_string_lossy().to_string(),
                worktree: dir.path().to_string_lossy().to_string(),
            }),
            ..Default::default()
        };

        let result = def
            .execute(
                serde_json::json!({
                    "patchText": "*** Begin Patch\n*** Add File: hello.txt\n+Hello world\n*** End Patch"
                }),
                &mut ctx,
            )
            .await
            .unwrap();
        assert!(result
            .output
            .starts_with("Success. Updated the following files:"));
        assert!(result.output.contains("A hello.txt"));
        assert_eq!(
            std::fs::read_to_string(dir.path().join("hello.txt")).unwrap(),
            "Hello world\n"
        );
        assert_eq!(ctx.asks[0].permission, "edit");
    }

    #[tokio::test]
    async fn rejects_empty_patch() {
        let def = crate::tool::tool::wrap("apply_patch", def());
        let dir = tempfile::tempdir().unwrap();
        let mut ctx = ToolContext {
            instance: Some(crate::model::InstanceContext {
                directory: dir.path().to_string_lossy().to_string(),
                worktree: dir.path().to_string_lossy().to_string(),
            }),
            ..Default::default()
        };

        let error = def
            .execute(
                serde_json::json!({
                    "patchText": "*** Begin Patch\n*** End Patch"
                }),
                &mut ctx,
            )
            .await
            .unwrap_err();
        assert_eq!(error.message(), "patch rejected: empty patch");
    }
}
