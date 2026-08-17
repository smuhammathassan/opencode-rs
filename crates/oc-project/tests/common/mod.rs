#![allow(dead_code)]
//! Shared helpers for oc-project integration tests. Each test file runs in its
//! own process; `OPENCODE_TEST_HOME` is set once per file via `test_home`.
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;

pub static TEST_HOME: OnceLock<PathBuf> = OnceLock::new();

pub fn test_home() -> &'static PathBuf {
    TEST_HOME.get_or_init(|| {
        let dir = std::env::temp_dir().join(format!("oc-project-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::env::set_var("OPENCODE_TEST_HOME", &dir);
        dir
    })
}

pub fn fresh_dir(name: &str) -> PathBuf {
    let home = test_home();
    let dir = home.join(name);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn envs() -> Vec<(&'static str, &'static str)> {
    vec![
        ("GIT_AUTHOR_NAME", "test"),
        ("GIT_AUTHOR_EMAIL", "test@example.com"),
        ("GIT_COMMITTER_NAME", "test"),
        ("GIT_COMMITTER_EMAIL", "test@example.com"),
    ]
}

pub fn git(cwd: &Path, args: &[&str]) -> std::process::Output {
    Command::new("git")
        .args(args)
        .current_dir(cwd)
        .envs(envs())
        .output()
        .expect("git must be installed")
}

/// Initializes a git repo with one committed file, returning the repo dir.
pub fn init_repo(name: &str, file: &str, content: &str) -> PathBuf {
    let dir = fresh_dir(name);
    assert!(git(&dir, &["init", "-q"]).status.success());
    git(&dir, &["config", "user.name", "test"]);
    git(&dir, &["config", "user.email", "test@example.com"]);
    std::fs::write(dir.join(file), content).unwrap();
    assert!(git(&dir, &["add", "-A"]).status.success());
    assert!(git(&dir, &["commit", "-q", "-m", "initial"])
        .status
        .success());
    dir
}

#[allow(dead_code)]
pub fn root_commit(dir: &Path) -> String {
    let output = git(dir, &["rev-list", "--max-parents=0", "HEAD"]);
    String::from_utf8(output.stdout).unwrap().trim().to_string()
}

pub fn data_dir() -> PathBuf {
    test_home().join(".local/share/opencode")
}
