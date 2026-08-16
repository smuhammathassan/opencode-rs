use std::fs;
use std::process::Command;

#[test]
fn print_logs_routes_startup_event_to_stderr_and_log_file() {
    let home = std::env::temp_dir().join(format!(
        "opencode-cli-logging-test-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system clock should be after unix epoch")
            .as_nanos()
    ));
    fs::create_dir_all(&home).expect("test home should be created");

    let output = Command::new(env!("CARGO_BIN_EXE_opencode"))
        .args(["--print-logs", "--log-level", "DEBUG", "debug", "paths"])
        .env("OPENCODE_TEST_HOME", &home)
        .env_remove("XDG_DATA_HOME")
        .env_remove("XDG_CACHE_HOME")
        .env_remove("XDG_CONFIG_HOME")
        .env_remove("XDG_STATE_HOME")
        .output()
        .expect("opencode should run");

    assert!(
        output.status.success(),
        "opencode failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("level=Debug"));
    assert!(stderr.contains("message=\"opencode starting\""));

    let log_path = home.join(".local/share/opencode/log/opencode.log");
    let log = fs::read_to_string(&log_path).expect("startup log should be created");
    assert!(log.contains("message=\"opencode starting\""));

    fs::remove_dir_all(home).expect("test home should be removed");
}
