// Shared test helpers: isolated homes/config dirs and env guards.
#![allow(dead_code)]

use std::path::PathBuf;
use std::sync::Mutex;

/// Serializes every test that mutates process env (the test harness runs
/// tests in parallel threads).
pub static ENV_LOCK: Mutex<()> = Mutex::new(());

/// Restores env vars on drop.
pub struct EnvGuard(Vec<(String, Option<String>)>);

impl EnvGuard {
    pub fn set(key: &str, value: &str) -> Self {
        Self::set_all(&[(key, Some(value))])
    }

    pub fn unset(key: &str) -> Self {
        Self::set_all(&[(key, None)])
    }

    pub fn set_all(entries: &[(&str, Option<&str>)]) -> Self {
        let mut originals = Vec::new();
        for (key, value) in entries {
            originals.push((key.to_string(), std::env::var(key).ok()));
            match value {
                Some(value) => std::env::set_var(key, value),
                None => std::env::remove_var(key),
            }
        }
        EnvGuard(originals)
    }
}

impl Drop for EnvGuard {
    fn drop(&mut self) {
        for (key, value) in &self.0 {
            match value {
                Some(value) => std::env::set_var(key, value),
                None => std::env::remove_var(key),
            }
        }
    }
}

/// An isolated environment with a temp home, temp global config dir, and a
/// temp project directory. Locks the env mutex for the test's duration.
pub struct TestHome {
    _guard: std::sync::MutexGuard<'static, ()>,
    _env: EnvGuard,
    pub tmp: tempfile::TempDir,
    pub global_config: PathBuf,
    pub home: PathBuf,
    pub project: PathBuf,
    pub managed: PathBuf,
}

impl TestHome {
    pub fn new() -> Self {
        let guard = ENV_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let tmp = tempfile::tempdir().expect("temp dir");
        let home = tmp.path().join("home");
        let global_config = tmp.path().join("xdg-config").join("opencode");
        let project = tmp.path().join("project");
        let managed = tmp.path().join("managed");
        std::fs::create_dir_all(&home).expect("mkdir home");
        std::fs::create_dir_all(&global_config).expect("mkdir global config");
        std::fs::create_dir_all(&project).expect("mkdir project");
        std::fs::create_dir_all(&managed).expect("mkdir managed");
        let env = EnvGuard::set_all(&[
            (
                "XDG_CONFIG_HOME",
                Some(tmp.path().join("xdg-config").to_str().unwrap()),
            ),
            ("OPENCODE_TEST_HOME", Some(home.to_str().unwrap())),
            (
                "OPENCODE_TEST_MANAGED_CONFIG_DIR",
                Some(managed.to_str().unwrap()),
            ),
            ("OPENCODE_CONFIG", None),
            ("OPENCODE_CONFIG_DIR", None),
            ("OPENCODE_CONFIG_CONTENT", None),
            ("OPENCODE_PERMISSION", None),
            ("OPENCODE_DISABLE_PROJECT_CONFIG", None),
            ("OPENCODE_DISABLE_AUTOCOMPACT", None),
            ("OPENCODE_DISABLE_PRUNE", None),
        ]);
        Self {
            _guard: guard,
            _env: env,
            tmp,
            global_config,
            home,
            project,
            managed,
        }
    }

    /// Writes a global config file.
    pub fn write_global(&self, name: &str, config: serde_json::Value) {
        std::fs::write(
            self.global_config.join(name),
            serde_json::to_string_pretty(&config).expect("serialize"),
        )
        .expect("write");
    }

    /// Writes a project config file (default `opencode.json`).
    pub fn write_project(&self, config: serde_json::Value, name: &str) {
        std::fs::write(
            self.project.join(name),
            serde_json::to_string_pretty(&config).expect("serialize"),
        )
        .expect("write");
    }

    /// Writes a file inside the project directory.
    pub fn write_in_project(&self, relative: &str, content: &str) {
        let path = self.project.join(relative);
        std::fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
        std::fs::write(path, content).expect("write");
    }

    pub fn load(&self) -> oc_config::load::InstanceState {
        oc_config::load::load_instance_state(&oc_config::load::LoadOptions {
            directory: self.project.to_string_lossy().into_owned(),
            worktree: Some(self.home.to_string_lossy().into_owned()),
            env: Default::default(),
            username: Some("testuser".to_string()),
        })
        .expect("load config")
    }
}
