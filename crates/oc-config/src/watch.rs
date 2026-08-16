//! Polling-based config reload support.
//!
//! The config loader is intentionally synchronous, so this module exposes a
//! small polling adapter instead of owning a background thread or runtime
//! task. Callers can invoke [`ConfigReloadWatcher::poll`] from their event
//! loop, or use [`ConfigReloadWatcher::poll_at`] when deterministic timing is
//! useful (for example, in tests).

use crate::load::{load_instance_state, managed_config_dir, Flags, InstanceState, LoadOptions};
use crate::managed;
use crate::paths;
use crate::{ConfigError, Result};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

/// The default quiet period after the last observed file change.
pub const DEFAULT_DEBOUNCE: Duration = Duration::from_millis(200);

/// A local config watcher which reloads after a debounced file change.
///
/// This is deliberately polling-based and does not start a thread. It is safe
/// to call from any synchronous event loop, and the watcher keeps the last
/// successfully loaded state when a changed file is temporarily invalid.
#[derive(Debug)]
pub struct ConfigReloadWatcher {
    options: LoadOptions,
    state: InstanceState,
    paths: Vec<PathBuf>,
    snapshot: Snapshot,
    observed: Snapshot,
    last_change_at: Option<Instant>,
    debounce: Duration,
}

impl ConfigReloadWatcher {
    /// Creates a watcher and loads the initial config state.
    pub fn new(options: LoadOptions, debounce: Duration) -> Result<Self> {
        let state = load_instance_state(&options)?;
        let paths = config_paths(&options);
        let snapshot = snapshot(&paths)?;
        Ok(Self {
            options,
            state,
            paths,
            snapshot: snapshot.clone(),
            observed: snapshot,
            last_change_at: None,
            debounce,
        })
    }

    /// Creates a watcher with [`DEFAULT_DEBOUNCE`].
    pub fn with_default_debounce(options: LoadOptions) -> Result<Self> {
        Self::new(options, DEFAULT_DEBOUNCE)
    }

    /// Returns the last successfully loaded state.
    pub fn state(&self) -> &InstanceState {
        &self.state
    }

    /// Returns the paths sampled by this watcher.
    ///
    /// The list includes absent candidate files so creating a previously
    /// missing `opencode.json`/`opencode.jsonc` is observable on the next
    /// poll.
    pub fn watched_paths(&self) -> &[PathBuf] {
        &self.paths
    }

    /// Returns the configured debounce period.
    pub fn debounce(&self) -> Duration {
        self.debounce
    }

    /// Polls using the current monotonic clock.
    ///
    /// `Ok(Some(state))` means a changed config was successfully reloaded.
    /// `Ok(None)` means no reload is ready yet. A config parse/validation
    /// failure is returned and the last good state remains installed.
    pub fn poll(&mut self) -> Result<Option<InstanceState>> {
        self.poll_at(Instant::now())
    }

    /// Polls at a caller-supplied instant, making debounce behavior testable
    /// without sleeping.
    pub fn poll_at(&mut self, now: Instant) -> Result<Option<InstanceState>> {
        let current = snapshot(&self.paths)?;
        if current != self.observed {
            self.observed = current.clone();
            self.last_change_at = Some(now);
        }

        let Some(last_change_at) = self.last_change_at else {
            return Ok(None);
        };
        if now.duration_since(last_change_at) < self.debounce || current == self.snapshot {
            return Ok(None);
        }

        let state = load_instance_state(&self.options)?;
        self.snapshot = current.clone();
        self.observed = current;
        self.last_change_at = None;
        self.state = state.clone();
        Ok(Some(state))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Fingerprint {
    Missing,
    Present {
        modified: Option<SystemTime>,
        len: u64,
        is_file: bool,
    },
}

type Snapshot = BTreeMap<PathBuf, Fingerprint>;

fn snapshot(paths: &[PathBuf]) -> Result<Snapshot> {
    paths
        .iter()
        .map(|path| {
            let fingerprint = match std::fs::metadata(path) {
                Ok(metadata) => Fingerprint::Present {
                    modified: metadata.modified().ok(),
                    len: metadata.len(),
                    is_file: metadata.is_file(),
                },
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => Fingerprint::Missing,
                Err(error) => {
                    return Err(ConfigError::Io {
                        path: path.to_string_lossy().into_owned(),
                        error: error.to_string(),
                    })
                }
            };
            Ok((path.clone(), fingerprint))
        })
        .collect()
}

fn config_paths(options: &LoadOptions) -> Vec<PathBuf> {
    let flags = Flags::from_env();
    let directory = PathBuf::from(&options.directory);
    let worktree = options.worktree.as_deref().map(Path::new);
    let mut paths = Vec::new();

    let global_dir = paths::config_dir();
    add_all_config_files(&mut paths, &global_dir);

    if let Some(path) = flags.config {
        add_unique(&mut paths, PathBuf::from(path));
    }

    if !flags.disable_project_config {
        for dir in ancestor_dirs(&directory, worktree) {
            add_opencode_files(&mut paths, &dir);
            add_opencode_files(&mut paths, &dir.join(".opencode"));
        }
    }

    // Include absent candidates as well, so a new home/project config file is
    // observed even when the directory did not exist at watcher creation.
    add_opencode_files(&mut paths, &paths::home_dir().join(".opencode"));

    if let Some(dir) = flags.config_dir {
        add_opencode_files(&mut paths, &PathBuf::from(dir));
    }
    if let Some(dir) = managed_config_dir() {
        for path in managed::config_files(&dir) {
            add_unique(&mut paths, path);
        }
    }

    paths
}

fn add_all_config_files(paths: &mut Vec<PathBuf>, directory: &Path) {
    for name in ["config", "config.json", "opencode.json", "opencode.jsonc"] {
        add_unique(paths, directory.join(name));
    }
}

fn add_opencode_files(paths: &mut Vec<PathBuf>, directory: &Path) {
    for name in ["opencode.json", "opencode.jsonc"] {
        add_unique(paths, directory.join(name));
    }
}

fn add_unique(paths: &mut Vec<PathBuf>, path: PathBuf) {
    if !paths.contains(&path) {
        paths.push(path);
    }
}

fn ancestor_dirs(start: &Path, stop: Option<&Path>) -> Vec<PathBuf> {
    let mut result = Vec::new();
    let mut current = Some(start.to_path_buf());
    while let Some(dir) = current {
        result.push(dir.clone());
        if stop == Some(dir.as_path()) {
            break;
        }
        current = dir.parent().map(Path::to_path_buf);
    }
    result
}
