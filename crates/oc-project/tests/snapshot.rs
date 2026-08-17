//! Integration tests for snapshot track/diff/patch/restore against a real git
//! repository and the snapshot storage layout.
mod common;

use common::*;
use oc_project::runtime::Runtime;
use oc_project::util::config::Config;

#[tokio::test]
async fn snapshot_track_diff_and_restore() {
    let _ = tracing_subscriber::fmt()
        .with_max_level(tracing::Level::TRACE)
        .try_init();
    let repo = init_repo("snap-repo", "file.txt", "line one\n");

    let runtime = Runtime::new(Config::default());
    let ctx = runtime
        .load(repo.to_str().unwrap())
        .await
        .expect("instance loads");
    let expected_file = std::path::Path::new(&ctx.worktree)
        .join("file.txt")
        .to_string_lossy()
        .replace('\\', "/");

    // Snapshot storage layout: {data}/snapshot/{project.id}/{sha1(worktree)}.
    let expected = data_dir()
        .join("snapshot")
        .join(&ctx.project.id.0)
        .join(oc_project::util::hash::Hash::fast(ctx.worktree.as_bytes()));
    let first = runtime
        .snapshot
        .track(&ctx)
        .await
        .expect("first snapshot tracked");
    assert!(!first.is_empty());
    assert!(
        expected.exists(),
        "snapshot gitdir must exist at {expected:?}"
    );

    // Modify the file and snapshot again.
    std::fs::write(repo.join("file.txt"), "line one\nline two\n").unwrap();
    let second = runtime
        .snapshot
        .track(&ctx)
        .await
        .expect("second snapshot tracked");
    assert_ne!(first, second);

    // diffFull returns the per-file change.
    let diffs = runtime.snapshot.diff_full(&ctx, &first, &second).await;
    assert_eq!(diffs.len(), 1);
    let diff = &diffs[0];
    assert_eq!(diff.file.as_deref(), Some("file.txt"));
    assert_eq!(diff.status.as_deref(), Some("modified"));
    assert_eq!(diff.additions, 1);
    assert_eq!(diff.deletions, 0);
    let patch = diff.patch.as_deref().unwrap_or_default();
    assert!(
        patch.contains("+line two"),
        "patch should contain the added line: {patch}"
    );

    // patch(hash) lists the files that changed relative to the older snapshot,
    // with absolute worktree paths.
    let snapshot_patch = runtime.snapshot.patch(&ctx, &first).await;
    assert_eq!(snapshot_patch.hash, first);
    assert_eq!(snapshot_patch.files, vec![expected_file.clone()]);

    // Restore the first snapshot and verify content is back.
    runtime.snapshot.restore(&ctx, &first).await;
    let restored = std::fs::read_to_string(repo.join("file.txt")).unwrap();
    assert_eq!(restored, "line one\n");

    // Snapshot JSON serialization (golden).
    let json = serde_json::to_value(&snapshot_patch).unwrap();
    assert_eq!(
        json,
        serde_json::json!({
            "hash": first,
            "files": [expected_file],
        })
    );
}
