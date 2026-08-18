//! Headless tests of the real `./opencode` binary's exit-status conventions
//! and error-formatting behavior (F147 / F148).
//!
//! Reference: `packages/opencode/src/index.ts` sets `process.exitCode = 1` on
//! any thrown error and prints "Unexpected error" + the cause chain when the
//! error is not a recognised `FormatError`; yargs parse failures exit 1.

use std::fs;
use std::path::PathBuf;
use std::process::{Command, Output};

const BIN: &str = env!("CARGO_BIN_EXE_opencode");

/// A fresh, isolated home directory so the CLI never touches real user data.
fn test_home(name: &str) -> PathBuf {
    let home = std::env::temp_dir().join(format!(
        "opencode-cli-exitcodes-{}-{}-{}",
        name,
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock")
            .as_nanos()
    ));
    fs::create_dir_all(&home).expect("test home should be created");
    home
}

fn run_in(home: &PathBuf, args: &[&str]) -> Output {
    Command::new(BIN)
        .args(args)
        .env("OPENCODE_TEST_HOME", home)
        .env_remove("XDG_DATA_HOME")
        .env_remove("XDG_CACHE_HOME")
        .env_remove("XDG_CONFIG_HOME")
        .env_remove("XDG_STATE_HOME")
        .output()
        .expect("opencode should run")
}

fn code(output: &Output) -> i32 {
    output.status.code().unwrap_or_else(|| {
        use std::os::unix::process::ExitStatusExt;
        let signal = output
            .status
            .signal()
            .expect("exit status has code or signal");
        -signal
    })
}

#[test]
fn version_exits_zero_and_prints_reference_version() {
    let home = test_home("version");
    let output = run_in(&home, &["--version"]);
    let _ = fs::remove_dir_all(&home);

    assert_eq!(
        code(&output),
        0,
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).starts_with("1.18."));
}

#[test]
fn unknown_subcommand_exits_one() {
    let home = test_home("unknown");
    let output = run_in(&home, &["totally-bogus-subcommand-xyz"]);
    let _ = fs::remove_dir_all(&home);

    // The reference `.fail()` handler exits 1 for unknown arguments.
    assert_eq!(
        code(&output),
        1,
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn missing_required_argument_exits_one() {
    let home = test_home("missing-arg");
    let output = run_in(&home, &["session", "delete"]);
    let _ = fs::remove_dir_all(&home);

    // yargs reports missing required argument and exits 1.
    assert_eq!(
        code(&output),
        1,
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("required arguments"), "stderr: {stderr}");
}

#[test]
fn command_error_exits_one_with_unexpected_error_and_cause_chain() {
    let home = test_home("missing-session");
    let output = run_in(&home, &["session", "delete", "does-not-exist-session-xyz"]);
    let _ = fs::remove_dir_all(&home);

    // Reference catch path: unformatted errors print "Unexpected error" then
    // the cause chain, and exit 1.
    assert_eq!(
        code(&output),
        1,
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("Unexpected error"), "stderr: {stderr}");
    assert!(stderr.contains("session not found"), "stderr: {stderr}");
}
