mod common;

use common::TestHome;
use oc_config::{ConfigError, ConfigReloadWatcher, LoadOptions};
use std::time::{Duration, Instant};

fn options(home: &TestHome) -> LoadOptions {
    LoadOptions {
        directory: home.project.to_string_lossy().into_owned(),
        worktree: Some(home.home.to_string_lossy().into_owned()),
        env: Default::default(),
        username: Some("testuser".to_string()),
    }
}

#[test]
fn reloads_after_a_quiet_debounce_window() {
    let home = TestHome::new();
    let mut watcher = ConfigReloadWatcher::new(options(&home), Duration::from_secs(1))
        .expect("initial config loads");
    let start = Instant::now();

    assert!(watcher.poll_at(start).expect("poll").is_none());
    home.write_project(
        serde_json::json!({"model": "updated/model"}),
        "opencode.json",
    );
    assert!(watcher
        .poll_at(start + Duration::from_millis(500))
        .expect("debounced poll")
        .is_none());

    let state = watcher
        .poll_at(start + Duration::from_secs(2))
        .expect("reload")
        .expect("changed config should reload");
    assert_eq!(state.config.model.as_deref(), Some("updated/model"));
    assert_eq!(watcher.state().config.model, state.config.model);
}

#[test]
fn observes_creation_and_removal_of_a_candidate_file() {
    let home = TestHome::new();
    let mut watcher = ConfigReloadWatcher::new(options(&home), Duration::ZERO).expect("load");
    let start = Instant::now();

    home.write_project(
        serde_json::json!({"model": "created/model"}),
        "opencode.json",
    );
    let created = watcher
        .poll_at(start + Duration::from_millis(1))
        .expect("reload created file")
        .expect("creation should reload");
    assert_eq!(created.config.model.as_deref(), Some("created/model"));

    std::fs::remove_file(home.project.join("opencode.json")).expect("remove config");
    let removed = watcher
        .poll_at(start + Duration::from_millis(2))
        .expect("reload removed file")
        .expect("removal should reload");
    assert_eq!(removed.config.model, None);
}

#[test]
fn keeps_last_good_state_when_changed_config_is_invalid() {
    let home = TestHome::new();
    home.write_project(
        serde_json::json!({"model": "stable/model"}),
        "opencode.json",
    );
    let mut watcher = ConfigReloadWatcher::new(options(&home), Duration::ZERO).expect("load");
    let start = Instant::now();

    std::fs::write(home.project.join("opencode.json"), "{ invalid json").expect("write invalid");
    let error = watcher
        .poll_at(start + Duration::from_millis(1))
        .expect_err("invalid config should be reported");
    assert!(matches!(error, ConfigError::Json { .. }));
    assert_eq!(
        watcher.state().config.model.as_deref(),
        Some("stable/model")
    );

    home.write_project(serde_json::json!({"model": "fixed/model"}), "opencode.json");
    let fixed = watcher
        .poll_at(start + Duration::from_millis(2))
        .expect("reload fixed file")
        .expect("fixed config should reload");
    assert_eq!(fixed.config.model.as_deref(), Some("fixed/model"));
}
