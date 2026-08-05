//! Plugin metadata store.
//!
//! Mirrors reference/packages/opencode/src/plugin/meta.ts: a JSON store at
//! `<state>/plugin-meta.json` tracking load counts, fingerprints and theme
//! files per plugin.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// A theme file shipped by a plugin.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Theme {
    pub src: String,
    pub dest: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mtime: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size: Option<u64>,
}

/// A single plugin's metadata entry. Field names match meta.ts exactly
/// (snake_case in the JSON store).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Entry {
    pub id: String,
    pub source: String,
    pub spec: String,
    pub target: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub requested: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub modified: Option<u64>,
    pub first_time: u64,
    pub last_time: u64,
    pub time_changed: u64,
    pub load_count: u64,
    pub fingerprint: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub themes: Option<HashMap<String, Theme>>,
}

/// Plugin load state relative to the previous run.
pub type State = &'static str;
pub const STATE_FIRST: State = "first";
pub const STATE_UPDATED: State = "updated";
pub const STATE_SAME: State = "same";

pub type Store = HashMap<String, Entry>;

/// The default metadata store path.
pub fn store_path(state_dir: &Path) -> PathBuf {
    state_dir.join("plugin-meta.json")
}

/// Mirrors `Entry` minus the runtime counters in meta.ts.
#[derive(Debug, Clone)]
struct Core {
    id: String,
    source: String,
    spec: String,
    target: String,
    requested: Option<String>,
    version: Option<String>,
    modified: Option<u64>,
}

fn file_target(spec: &str, target: &str) -> Option<PathBuf> {
    if let Some(rest) = spec.strip_prefix("file://") {
        return Some(PathBuf::from(rest));
    }
    if let Some(rest) = target.strip_prefix("file://") {
        return Some(PathBuf::from(rest));
    }
    // Plain absolute paths (e.g. a test or a direct path spec).
    let path = Path::new(spec);
    if path.is_absolute() {
        return Some(path.to_path_buf());
    }
    None
}

fn modified_at(file: &Path) -> Option<u64> {
    std::fs::metadata(file)
        .ok()
        .and_then(|m| m.modified().ok())
        .map(|t| {
            t.duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis() as u64)
                .unwrap_or(0)
        })
}

fn npm_version(target: &str) -> Option<String> {
    let file = target.strip_prefix("file://").unwrap_or(target);
    let path = PathBuf::from(file);
    let stat = std::fs::metadata(&path).ok()?;
    let dir = if stat.is_dir() {
        path
    } else {
        path.parent()?.to_path_buf()
    };
    let json: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(dir.join("package.json")).ok()?).ok()?;
    json.get("version")
        .and_then(serde_json::Value::as_str)
        .map(str::to_string)
}

fn entry_core(item: &Touch) -> Core {
    let source = if item.spec.starts_with("file://")
        || item.spec.starts_with('.')
        || Path::new(&item.spec).is_absolute()
    {
        "file".to_string()
    } else {
        "npm".to_string()
    };
    if source == "file" {
        Core {
            id: item.id.clone(),
            source,
            spec: item.spec.clone(),
            target: item.target.clone(),
            modified: file_target(&item.spec, &item.target)
                .as_deref()
                .and_then(modified_at),
            requested: None,
            version: None,
        }
    } else {
        Core {
            id: item.id.clone(),
            source,
            spec: item.spec.clone(),
            target: item.target.clone(),
            modified: None,
            requested: Some(crate::loader::parse_plugin_specifier(&item.spec).1),
            version: npm_version(&item.target),
        }
    }
}

fn fingerprint(core: &Core) -> String {
    if core.source == "file" {
        format!("{}|{}", core.target, core.modified.unwrap_or(0))
    } else {
        format!(
            "{}|{}|{}",
            core.target,
            core.requested.as_deref().unwrap_or(""),
            core.version.as_deref().unwrap_or("")
        )
    }
}

fn next(prev: Option<&Entry>, core: &Core, now: u64) -> (State, Entry) {
    let mut entry = Entry {
        id: core.id.clone(),
        source: core.source.clone(),
        spec: core.spec.clone(),
        target: core.target.clone(),
        requested: core.requested.clone(),
        version: core.version.clone(),
        modified: core.modified,
        first_time: prev.map(|p| p.first_time).unwrap_or(now),
        last_time: now,
        time_changed: prev.map(|p| p.time_changed).unwrap_or(now),
        load_count: prev.map(|p| p.load_count).unwrap_or(0) + 1,
        fingerprint: fingerprint(core),
        themes: prev.and_then(|p| p.themes.clone()),
    };
    let state = match prev {
        None => STATE_FIRST,
        Some(p) if p.fingerprint == entry.fingerprint => STATE_SAME,
        Some(_) => STATE_UPDATED,
    };
    if state == STATE_UPDATED {
        entry.time_changed = now;
    }
    (state, entry)
}

/// A plugin to be touched in the metadata store.
#[derive(Debug, Clone)]
pub struct Touch {
    pub spec: String,
    pub target: String,
    pub id: String,
}

fn read(file: &Path) -> Store {
    std::fs::read_to_string(file)
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

fn with_lock<R>(key: &str, f: impl FnOnce() -> R) -> R {
    // A lightweight advisory lock via a lock file. The reference uses Flock;
    // TODO(integration): replace with the shared oc-util flock when it lands.
    let sanitized: String = key
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '.' {
                c
            } else {
                '_'
            }
        })
        .collect();
    let lock_path = std::env::temp_dir().join(format!("opencode-{sanitized}.lock"));
    let _handle = FileLock::acquire(&lock_path);
    f()
}

struct FileLock {
    path: PathBuf,
}

impl FileLock {
    fn acquire(path: &Path) -> Option<Self> {
        // Best-effort exclusive create.
        let mut attempt = 0;
        loop {
            match std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(path)
            {
                Ok(_) => {
                    return Some(Self {
                        path: path.to_path_buf(),
                    })
                }
                Err(_) if attempt < 200 => {
                    std::thread::sleep(std::time::Duration::from_millis(10));
                    attempt += 1;
                }
                Err(_) => return None,
            }
        }
    }
}

impl Drop for FileLock {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

/// Touch multiple plugin entries, mirroring `touchMany` in meta.ts.
pub fn touch_many(state_dir: &Path, items: &[Touch]) -> Vec<(State, Entry)> {
    if items.is_empty() {
        return Vec::new();
    }
    let file = store_path(state_dir);
    let cores: Vec<Core> = items.iter().map(entry_core).collect();
    with_lock(&file.to_string_lossy(), || {
        let mut store = read(&file);
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);
        let mut out = Vec::new();
        for (item, core) in items.iter().zip(cores.iter()) {
            let prev = store.get(&item.id);
            let (state, entry) = next(prev, core, now);
            store.insert(item.id.clone(), entry.clone());
            out.push((state, entry));
        }
        if let Ok(text) = serde_json::to_string_pretty(&store) {
            let _ = std::fs::create_dir_all(state_dir);
            let _ = std::fs::write(&file, text);
        }
        out
    })
}

/// Touch a single plugin entry.
pub fn touch(state_dir: &Path, spec: &str, target: &str, id: &str) -> Option<(State, Entry)> {
    touch_many(
        state_dir,
        &[Touch {
            spec: spec.to_string(),
            target: target.to_string(),
            id: id.to_string(),
        }],
    )
    .into_iter()
    .next()
}

/// Store a theme file for a plugin.
pub fn set_theme(state_dir: &Path, id: &str, name: &str, theme: Theme) {
    let file = store_path(state_dir);
    with_lock(&file.to_string_lossy(), || {
        let mut store = read(&file);
        let Some(entry) = store.get_mut(id) else {
            return;
        };
        let themes = entry.themes.get_or_insert_with(HashMap::new);
        themes.insert(name.to_string(), theme);
        if let Ok(text) = serde_json::to_string_pretty(&store) {
            let _ = std::fs::write(&file, text);
        }
    });
}

/// List the full metadata store.
pub fn list(state_dir: &Path) -> Store {
    read(&store_path(state_dir))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_state(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("oc-meta-test-{}-{name}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        dir
    }

    #[test]
    fn touch_tracks_state_lifecycle() {
        let state = temp_state("touch");
        let spec = "/tmp/plugin.ts";
        let (state1, entry1) = touch(&state, spec, "/tmp/plugin.ts", "my-plugin").unwrap();
        assert_eq!(state1, STATE_FIRST);
        assert_eq!(entry1.load_count, 1);
        assert_eq!(entry1.spec, spec);

        let (state2, entry2) = touch(&state, spec, "/tmp/plugin.ts", "my-plugin").unwrap();
        assert_eq!(state2, STATE_SAME);
        assert_eq!(entry2.load_count, 2);
        assert!(entry2.first_time == entry1.first_time);

        let (state3, entry3) = touch(&state, spec, "/tmp/plugin-modified.ts", "my-plugin").unwrap();
        assert_eq!(state3, STATE_UPDATED);
        assert!(entry3.time_changed >= entry1.time_changed);
        let _ = entry3;
    }

    #[test]
    fn store_serializes_exact_shape() {
        let state = temp_state("shape");
        touch(
            &state,
            "my-plugin",
            "/cache/packages/my-plugin",
            "my-plugin",
        )
        .unwrap();
        let text = std::fs::read_to_string(store_path(&state)).unwrap();
        let value: serde_json::Value = serde_json::from_str(&text).unwrap();
        let entry = &value["my-plugin"];
        // Field names mirror meta.ts exactly (`version` is optional when the
        // installed package.json cannot be read).
        for field in [
            "id",
            "source",
            "spec",
            "target",
            "requested",
            "first_time",
            "last_time",
            "time_changed",
            "load_count",
            "fingerprint",
        ] {
            assert!(entry.get(field).is_some(), "missing field {field}");
        }
        assert_eq!(entry["source"], "npm");
        assert_eq!(entry["requested"], "latest");
    }

    #[test]
    fn file_fingerprint_uses_modified() {
        let state = temp_state("file");
        let file = std::env::temp_dir().join(format!("oc-meta-file-{}.ts", std::process::id()));
        std::fs::write(&file, "export default {}").unwrap();
        let spec = file.to_string_lossy().into_owned();
        let (_, entry) = touch(&state, &spec, &spec, "file-plugin").unwrap();
        assert_eq!(entry.source, "file");
        assert!(entry.modified.is_some());
        std::fs::remove_file(&file).ok();
    }

    #[test]
    fn set_theme_persists() {
        let state = temp_state("theme");
        touch(&state, "my-plugin", "/tmp/target", "my-plugin").unwrap();
        set_theme(
            &state,
            "my-plugin",
            "dark",
            Theme {
                src: "/a.json".into(),
                dest: "/b.json".into(),
                mtime: Some(1),
                size: Some(2),
            },
        );
        let store = list(&state);
        let themes = store["my-plugin"].themes.as_ref().unwrap();
        assert_eq!(themes["dark"].dest, "/b.json");
    }
}
