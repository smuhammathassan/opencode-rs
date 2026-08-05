//! Port of `reference/packages/opencode/src/tool/external-directory.ts`.

use crate::model::{PermissionRequest, ToolContext, ToolError};

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Kind {
    File,
    Directory,
}

/// `assertExternalDirectoryEffect` from
/// `reference/packages/opencode/src/tool/external-directory.ts:15`.
pub fn assert_external_directory(
    ctx: &mut ToolContext,
    target: Option<&str>,
    bypass: bool,
    kind: Kind,
) -> Result<bool, ToolError> {
    let Some(target) = target else {
        return Ok(false);
    };
    if bypass {
        return Ok(false);
    }
    let Some(instance) = &ctx.instance else {
        return Ok(false);
    };
    if instance.contains_path(target) {
        return Ok(false);
    }

    let dir = match kind {
        Kind::Directory => target.to_string(),
        Kind::File => std::path::Path::new(target)
            .parent()
            .map(|parent| parent.to_string_lossy().to_string())
            .unwrap_or_else(|| target.to_string()),
    };
    let glob = std::path::Path::new(&dir)
        .join("*")
        .to_string_lossy()
        .replace('\\', "/");

    ctx.ask(PermissionRequest {
        permission: "external_directory".to_string(),
        patterns: vec![glob.clone()],
        always: vec![glob],
        metadata: serde_json::json!({
            "filepath": target,
            "parentDir": dir,
        }),
    })?;
    Ok(true)
}

/// Convenience wrapper used by tools targeting files.
pub fn assert_external_directory_file(
    ctx: &mut ToolContext,
    target: &str,
) -> Result<bool, ToolError> {
    assert_external_directory(ctx, Some(target), false, Kind::File)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn skips_internal_paths() {
        let dir = std::env::temp_dir().join("oc-tool-ext-test");
        let mut ctx = ToolContext::default();
        ctx.instance = Some(crate::model::InstanceContext {
            directory: dir.to_string_lossy().to_string(),
            worktree: dir.to_string_lossy().to_string(),
        });
        let inside = dir.join("sub/file.txt").to_string_lossy().to_string();
        let asserted = assert_external_directory_file(&mut ctx, &inside).unwrap();
        assert!(!asserted);
        assert!(ctx.asks.is_empty());
    }

    #[test]
    fn asks_for_external_paths() {
        let dir = std::env::temp_dir().join("oc-tool-ext-test");
        let mut ctx = ToolContext::default();
        ctx.instance = Some(crate::model::InstanceContext {
            directory: dir.to_string_lossy().to_string(),
            worktree: dir.to_string_lossy().to_string(),
        });
        let outside = std::env::temp_dir()
            .join("unrelated/file.txt")
            .to_string_lossy()
            .to_string();
        let asserted = assert_external_directory_file(&mut ctx, &outside).unwrap();
        assert!(asserted);
        assert_eq!(ctx.asks.len(), 1);
        assert_eq!(ctx.asks[0].permission, "external_directory");
    }
}
