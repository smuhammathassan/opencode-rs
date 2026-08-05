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
    let project_id = root_commit(&repo);

    let runtime = Runtime::new(Config::default());
    let ctx = runtime
        .load(repo.to_str().unwrap())
        .await
        .expect("instance loads");

    assert_eq!(ctx.project.vcs.as_deref(), Some("git"));
    assert_eq!(ctx.project.id.0, project_id);
    assert_eq!(ctx.worktree, repo.to_str().unwrap());

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
        info.directory,
        data_dir()
            .join("worktree")
            .join(&project_id)
            .join("my-feature")
            .to_str()
            .unwrap()
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

    // The created worktree shows up in list (primary repo is excluded).
    let list = runtime.worktree.list(&ctx).await.expect("worktree list");
    let listed = list
        .iter()
        .find(|item| item.directory == created.directory)
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
    assert!(!list.iter().any(|item| item.directory == created.directory));

    let _ = home;
}
