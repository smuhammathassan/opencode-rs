//! Integration tests for project detection/attachment (Project.fromDirectory).
mod common;

use common::*;
use oc_project::runtime::Runtime;
use oc_project::util::config::Config;

#[tokio::test]
async fn from_directory_resolves_global_and_git_projects() {
    // A directory without git resolves to the global project with worktree "/".
    let plain = fresh_dir("plain-dir");
    let runtime = Runtime::new(Config::default());
    let result = runtime
        .project
        .from_directory(plain.to_str().unwrap())
        .await
        .expect("fromDirectory");
    assert_eq!(result.project.id.0, "global");
    assert_eq!(result.project.vcs, None);
    assert_eq!(result.project.worktree, "/");
    assert_eq!(result.sandbox, "/");

    // A git repo resolves to a project keyed by its root commit.
    let repo = init_repo("proj-repo", "main.ts", "export const a = 1;\n");
    let canonical_repo = repo.canonicalize().expect("canonical repo path");
    let project_id = root_commit(&repo);
    let result = runtime
        .project
        .from_directory(repo.to_str().unwrap())
        .await
        .expect("fromDirectory");
    assert_eq!(result.project.id.0, project_id);
    assert_eq!(result.project.vcs.as_deref(), Some("git"));
    let norm_worktree = result.project.worktree.replace('\\', "/").to_lowercase();
    let norm_canonical = canonical_repo
        .to_string_lossy()
        .replace('\\', "/")
        .to_lowercase();
    assert!(
        norm_canonical.ends_with(&norm_worktree)
            || norm_worktree.ends_with(&norm_canonical)
            || norm_worktree == norm_canonical
    );
    let norm_sandbox = result.sandbox.replace('\\', "/").to_lowercase();
    assert!(
        norm_canonical.ends_with(&norm_sandbox)
            || norm_sandbox.ends_with(&norm_canonical)
            || norm_sandbox == norm_canonical
    );

    // get/list round-trips the persisted row.
    let fetched = runtime.project.get(&result.project.id).await.expect("get");
    assert_eq!(fetched.id.0, project_id);
    let list = runtime.project.list().await;
    assert!(list.iter().any(|item| item.id.0 == project_id));
    assert!(list.iter().any(|item| item.id.0 == "global"));
}

#[tokio::test]
async fn from_directory_stamps_initialized_time_via_init_command() {
    let repo = init_repo("init-repo", "a.txt", "a\n");
    let project_id = root_commit(&repo);
    let runtime = Runtime::new(Config::default());
    let ctx = runtime
        .load(repo.to_str().unwrap())
        .await
        .expect("instance loads");
    assert!(ctx.project.time.initialized.is_none());

    // Emit the /init command event for the instance directory.
    let bus = runtime.bus.clone();
    bus.emit(oc_project::util::bus::BusEvent {
        directory: ctx.directory.clone(),
        project: Some(ctx.project.id.0.clone()),
        workspace: None,
        payload: oc_project::util::bus::EventPayload {
            r#type: "command.executed".to_string(),
            properties: None,
            data: Some(serde_json::json!({ "name": "init" })),
            location: Some(oc_project::util::bus::EventLocation {
                directory: ctx.directory.clone(),
            }),
        },
    });

    // Wait for the watcher task to observe the event.
    for _ in 0..50 {
        if runtime
            .project
            .get(&ctx.project.id)
            .await
            .unwrap()
            .time
            .initialized
            .is_some()
        {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    let info = runtime.project.get(&ctx.project.id).await.expect("get");
    assert!(
        info.time.initialized.is_some(),
        "init command should stamp initialized"
    );
    assert!(info.time.initialized.unwrap() > 0);
    let _ = project_id;
}
