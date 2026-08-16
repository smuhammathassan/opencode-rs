//! Emits build metadata consumed by `oc_util::version` (RELEASE-002/018).
//!
//! `OC_UTIL_GIT_COMMIT` precedence: the `GIT_COMMIT` environment variable
//! (set by the release pipeline) > `git rev-parse --short HEAD` in the build
//! directory > `"unknown"` (release tarballs, `cargo install` outside a git
//! checkout). The build never fails when git is unavailable.

fn main() {
    println!("cargo:rerun-if-env-changed=GIT_COMMIT");
    println!(
        "cargo:rustc-env=OC_UTIL_BUILD_PROFILE={}",
        std::env::var("PROFILE").unwrap_or_else(|_| "unknown".to_string())
    );

    let commit = std::env::var("GIT_COMMIT")
        .ok()
        .filter(|s| !s.is_empty())
        .or_else(git_commit)
        .unwrap_or_else(|| "unknown".to_string());

    println!("cargo:rustc-env=OC_UTIL_GIT_COMMIT={commit}");
}

fn git_commit() -> Option<String> {
    let out = std::process::Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let sha = String::from_utf8(out.stdout).ok()?;
    let sha = sha.trim();
    if sha.is_empty() {
        None
    } else {
        Some(sha.to_string())
    }
}
