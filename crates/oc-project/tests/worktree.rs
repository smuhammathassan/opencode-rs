//! Golden integration tests for the worktree path-creation rules and the
//! create/list/remove lifecycle against a real git repository.
mod common;

use common::*;
use oc_project::runtime::Runtime;
use oc_project::schema::WorktreeRemoveInput;
use oc_project::util::config::Config;
use oc_project::worktree::WorktreeInfoOptions;

#[tokio::test]
async fn worktree_path_rule_and_lifecycle() {
    let home = test_home().clone();
    let repo = init_repo("wt-repo", "file.txt", "hello\n");
    let canonical_repo = repo.canonicalize().expect("canonical repo path");
    let project_id = root_commit(&repo);

    let runtime = Runtime::new(Config::default());
    let ctx = runtime
        .load(repo.to_str().unwrap())
        .await
        .expect("instance loads");

    assert_eq!(ctx.project.vcs.as_deref(), Some("git"));
    assert_eq!(ctx.project.id.0, project_id);
    let norm_worktree = ctx.worktree.replace('\\', "/").to_lowercase();
    let norm_canonical = canonical_repo
        .to_string_lossy()
        .replace('\\', "/")
        .to_lowercase();
    assert!(
        norm_canonical.ends_with(&norm_worktree)
            || norm_worktree.ends_with(&norm_canonical)
            || norm_worktree == norm_canonical
    );

    // Path-creation rules: name is slugified, directory = {data}/worktree/{id}/{name},
    // branch = opencode/{name}.
    let info = runtime
        .worktree
        .make_worktree_info(
            &ctx,
            &WorktreeInfoOptions {
                name: Some("My Feature".to_string()),
                detached: false,
            },
        )
        .await
        .expect("info generated");
    assert_eq!(info.name, "my-feature");
    assert_eq!(info.branch.as_deref(), Some("opencode/my-feature"));
    assert_eq!(
        info.directory.replace('\\', "/"),
        data_dir()
            .join("worktree")
            .join(&project_id)
            .join("my-feature")
            .to_str()
            .unwrap()
            .replace('\\', "/")
    );

    // Create the worktree.
    let created = runtime
        .worktree
        .create(&ctx, None)
        .await
        .expect("worktree created");
    assert!(std::path::Path::new(&created.directory).exists());
    assert!(created.name.starts_with("opencode-") || created.name.contains('-'));
    assert!(created
        .branch
        .as_deref()
        .unwrap_or_default()
        .starts_with("opencode/"));
    let canonical_created = std::path::Path::new(&created.directory)
        .canonicalize()
        .expect("canonical created worktree");

    // Golden: worktree Info serializes with the exact zod shape.
    let json = serde_json::to_value(&created).unwrap();
    assert_eq!(
        json,
        serde_json::json!({
            "name": created.name,
            "branch": created.branch,
            "directory": created.directory,
        })
    );

    // The created worktree shows up in list (primary repo is excluded).
    let list = runtime.worktree.list(&ctx).await.expect("worktree list");
    let norm_canonical = canonical_created
        .to_string_lossy()
        .replace('\\', "/")
        .to_lowercase()
        .replace("///?/", "");
    let listed = list
        .iter()
        .find(|item| {
            let norm_item = item.directory.replace('\\', "/").to_lowercase();
            norm_item == norm_canonical
                || norm_item.ends_with(&norm_canonical)
                || norm_canonical.ends_with(&norm_item)
        })
        .expect("created worktree listed");
    assert_eq!(listed.branch.as_deref(), created.branch.as_deref());

    // Remove it.
    let removed = runtime
        .worktree
        .remove(
            &ctx,
            &WorktreeRemoveInput {
                directory: created.directory.clone(),
            },
        )
        .await
        .expect("worktree removed");
    assert!(removed);
    assert!(!std::path::Path::new(&created.directory).exists());

    let list = runtime
        .worktree
        .list(&ctx)
        .await
        .expect("worktree list after remove");
    assert!(!list.iter().any(|item| {
        let norm_item = item.directory.replace('\\', "/").to_lowercase();
        norm_item == norm_canonical
            || norm_item.ends_with(&norm_canonical)
            || norm_canonical.ends_with(&norm_item)
    }));

    let _ = home;
}
