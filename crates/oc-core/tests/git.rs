//! Integration tests for the git wrapper against a real repository.

use std::process::Command;
use std::sync::Arc;

use oc_core::fs_util::FSUtilService;
use oc_core::git::{GitService, IndexMode, UntrackedMode};
use oc_core::schema::AbsolutePath;

fn temp_dir(tag: &str) -> String {
    let dir = std::env::temp_dir().join(format!("oc-core-git-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create temp dir");
    dir.display().to_string()
}

fn git(dir: &str, args: &[&str]) {
    let status = Command::new("git")
        .args(args)
        .env("GIT_AUTHOR_NAME", "test")
        .env("GIT_AUTHOR_EMAIL", "test@example.com")
        .env("GIT_COMMITTER_NAME", "test")
        .env("GIT_COMMITTER_EMAIL", "test@example.com")
        .current_dir(dir)
        .status()
        .expect("run git");
    assert!(status.success(), "git {args:?} failed");
}

fn service() -> GitService {
    GitService::new(Arc::new(FSUtilService))
}

#[tokio::test]
async fn discover_and_history() {
    let dir = temp_dir("discover");
    git(&dir, &["init", "-q", "-b", "main"]);
    std::fs::write(format!("{dir}/file.txt"), "hello").unwrap();
    git(&dir, &["add", "file.txt"]);
    git(&dir, &["commit", "-q", "-m", "initial"]);

    let svc = service();
    let repo = svc
        .discover(&format!("{dir}/sub/dir"))
        .await
        .expect("discover walks up");
    assert!(repo.worktree.0.ends_with(&dir) || repo.worktree.0 == dir);

    let head = svc.history_head(&repo).await.expect("head");
    assert_eq!(head.len(), 40);
    assert_eq!(svc.history_branch(&repo).await.as_deref(), Some("main"));
    let roots = svc.history_root_commits(&repo).await;
    assert_eq!(roots.len(), 1);
    assert_eq!(roots[0], head);

    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn capture_discard_changes() {
    let dir = temp_dir("changes");
    git(&dir, &["init", "-q", "-b", "main"]);
    std::fs::write(format!("{dir}/file.txt"), "one").unwrap();
    git(&dir, &["add", "file.txt"]);
    git(&dir, &["commit", "-q", "-m", "initial"]);

    std::fs::write(format!("{dir}/file.txt"), "two").unwrap();
    let svc = service();
    let repo = svc.discover(&dir).await.expect("repo");

    let changes = svc
        .change_capture(&repo, &AbsolutePath(format!("{dir}/file.txt")))
        .await
        .expect("capture");
    assert!(changes.0.contains("-one") && changes.0.contains("+two"));

    svc.change_discard(
        &repo,
        &AbsolutePath(format!("{dir}/file.txt")),
        IndexMode::Preserve,
        UntrackedMode::Preserve,
    )
    .await
    .expect("discard");
    assert_eq!(
        std::fs::read_to_string(format!("{dir}/file.txt")).unwrap(),
        "one"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn index_ignored_paths() {
    let dir = temp_dir("ignored");
    git(&dir, &["init", "-q", "-b", "main"]);
    std::fs::write(format!("{dir}/.gitignore"), "target/\n").unwrap();
    git(&dir, &["add", ".gitignore"]);
    git(&dir, &["commit", "-q", "-m", "init"]);

    let svc = service();
    let repo = svc.discover(&dir).await.expect("repo");
    let ignored = svc
        .index_ignored(
            &repo,
            &[
                oc_core::schema::RelativePath("target/x".to_string()),
                oc_core::schema::RelativePath("file.txt".to_string()),
            ],
        )
        .await
        .expect("check-ignore");
    assert_eq!(
        ignored,
        vec![oc_core::schema::RelativePath("target/x".to_string())]
    );

    let _ = std::fs::remove_dir_all(&dir);
}
