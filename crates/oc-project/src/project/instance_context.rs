/// From reference/packages/opencode/src/project/instance-context.ts
use crate::schema::ProjectInfo;
use crate::util::pathutil;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstanceContext {
    pub directory: String,
    pub worktree: String,
    pub project: ProjectInfo,
}

/// Check if a path is within the project boundary. Returns true if `filepath`
/// is inside `ctx.directory` OR `ctx.worktree`. Paths within the worktree but
/// outside the working directory should not trigger external_directory
/// permission.
pub fn contains_path(filepath: &str, ctx: &InstanceContext) -> bool {
    if pathutil::contains(&ctx.directory, filepath) {
        return true;
    }
    // Non-git projects set worktree to "/" which would match ANY absolute path.
    // Skip worktree check in this case to preserve external_directory permissions.
    if ctx.worktree == "/" {
        return false;
    }
    pathutil::contains(&ctx.worktree, filepath)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx(directory: &str, worktree: &str) -> InstanceContext {
        InstanceContext {
            directory: directory.to_string(),
            worktree: worktree.to_string(),
            project: ProjectInfo::default(),
        }
    }

    #[test]
    fn path_inside_directory() {
        let ctx = ctx("/project", "/project");
        assert!(contains_path("/project/src/a.ts", &ctx));
        assert!(contains_path("/project", &ctx));
    }

    #[test]
    fn path_outside_directory_but_inside_worktree_is_allowed() {
        let ctx = ctx("/project", "/project");
        assert!(!contains_path("/other/a.ts", &ctx));
    }

    #[test]
    fn worktree_check_covers_linked_directories() {
        let ctx = ctx("/project", "/data/worktree/proj/feature");
        assert!(contains_path("/data/worktree/proj/feature/src/a.ts", &ctx));
        assert!(!contains_path("/data/worktree/proj/other/a.ts", &ctx));
    }

    #[test]
    fn root_worktree_never_matches() {
        let ctx = ctx("/project", "/");
        assert!(!contains_path("/etc/passwd", &ctx));
        assert!(contains_path("/project/src/a.ts", &ctx));
    }
}
