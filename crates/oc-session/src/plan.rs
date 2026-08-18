//! Plan file lifecycle, from `reference/packages/opencode/src/session/session.ts`
//! (`Session.plan`) and the plan-mode reminders in
//! `reference/packages/opencode/src/session/reminders.ts`.

/// `Session.plan()` from `reference/.../session/session.ts:331` — the plan
/// file lives under `.opencode/plans` when the project has a VCS worktree,
/// otherwise under the global data directory. The filename is
/// `{created}-{slug}.md`.
pub fn plan_path(
    worktree: &str,
    data_dir: &str,
    has_vcs: bool,
    created: u64,
    slug: &str,
) -> String {
    let base = if has_vcs {
        format!("{worktree}/.opencode/plans")
    } else {
        format!("{data_dir}/plans")
    };
    format!("{base}/{created}-{slug}.md")
}

/// The resolved plan file plus whether it already exists.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanFileState {
    pub path: String,
    pub exists: bool,
}

/// Resolve the plan file state for a session. When the plan agent is active
/// and no plan exists yet, the parent directory is created — mirroring the
/// reference `reminders.ts` `fsys.ensureDir(path.dirname(plan))` call so the
/// plan agent can write the plan with the `write` tool.
pub fn ensure_plan_file(
    worktree: &str,
    data_dir: &str,
    has_vcs: bool,
    created: u64,
    slug: &str,
    ensure_dir: bool,
) -> PlanFileState {
    let path = plan_path(worktree, data_dir, has_vcs, created, slug);
    let exists = std::path::Path::new(&path).is_file();
    if ensure_dir && !exists {
        if let Some(parent) = std::path::Path::new(&path).parent() {
            let _ = std::fs::create_dir_all(parent);
        }
    }
    PlanFileState { path, exists }
}

/// A plan-file slug derived from a session id, mirroring the v1 projection
/// (`v1_info` in oc-server): the trailing `id` segment.
pub fn slug_from_session_id(session_id: &str) -> String {
    session_id
        .rsplit('_')
        .next()
        .filter(|part| !part.is_empty())
        .unwrap_or(session_id)
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vcs_worktree_plans_live_under_opencode_plans() {
        let path = plan_path("/work/repo", "/data", true, 1_700_000_000_000, "abc");
        assert_eq!(path, "/work/repo/.opencode/plans/1700000000000-abc.md");
    }

    #[test]
    fn non_vcs_plans_live_under_global_data() {
        let path = plan_path("/work/dir", "/data", false, 42, "xyz");
        assert_eq!(path, "/data/plans/42-xyz.md");
    }

    #[test]
    fn ensure_plan_file_creates_parent_directory() {
        let dir =
            std::env::temp_dir().join(format!("opencode-session-plan-{}", std::process::id()));
        let worktree = dir.join("repo");
        let data = dir.join("data");
        let created = 7;
        let slug = "ses_plan";

        // No VCS: directory created under the data dir.
        let state = ensure_plan_file(
            &worktree.to_string_lossy(),
            &data.to_string_lossy(),
            false,
            created,
            slug,
            true,
        );
        assert!(std::path::Path::new(&state.path).parent().unwrap().is_dir());
        assert!(!state.exists);

        // A second call sees the file once it exists.
        std::fs::write(&state.path, "# Plan").unwrap();
        let state = ensure_plan_file(
            &worktree.to_string_lossy(),
            &data.to_string_lossy(),
            false,
            created,
            slug,
            false,
        );
        assert!(state.exists);
        assert_eq!(
            state.path,
            format!("{}/plans/7-ses_plan.md", data.to_string_lossy())
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn slug_falls_back_to_full_id() {
        assert_eq!(slug_from_session_id("ses_abc123"), "abc123");
        assert_eq!(slug_from_session_id("no-underscore"), "no-underscore");
    }
}
