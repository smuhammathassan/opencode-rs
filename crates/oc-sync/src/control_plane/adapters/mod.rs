//! Workspace adapter registry.
//!
//! From reference/packages/opencode/src/control-plane/adapters/index.ts: a
//! per-project map of custom adapters layered over the builtin adapters.

pub mod worktree;

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

use super::types::{WorkspaceAdapter, WorkspaceAdapterEntry};

pub type AdapterRef = Arc<dyn WorkspaceAdapter>;

/// `getAdapter`/`registerAdapter` from reference/packages/opencode/src/control-plane/adapters/index.ts:
/// project id -> (type -> adapter).
static STATE: OnceLock<Mutex<HashMap<String, HashMap<String, AdapterRef>>>> = OnceLock::new();

fn state() -> &'static Mutex<HashMap<String, HashMap<String, AdapterRef>>> {
    STATE.get_or_init(|| Mutex::new(HashMap::new()))
}

/// The `BUILTIN` map from the reference: `{ worktree: WorktreeAdapter }`.
pub fn builtin() -> HashMap<String, AdapterRef> {
    let mut map = HashMap::new();
    map.insert("worktree".to_string(), worktree::worktree_adapter());
    map
}

/// `registeredAdapters` from the reference.
pub fn registered_adapters(project_id: &str) -> Vec<(String, AdapterRef)> {
    let mut adapters = builtin();
    if let Some(custom) = state()
        .lock()
        .expect("adapter registry poisoned")
        .get(project_id)
    {
        for (ty, adapter) in custom {
            adapters.insert(ty.clone(), adapter.clone());
        }
    }
    adapters.into_iter().collect()
}

/// `getAdapter` from the reference: custom first, then builtin, else error.
pub fn get_adapter(project_id: &str, ty: &str) -> anyhow::Result<AdapterRef> {
    if let Some(custom) = state()
        .lock()
        .expect("adapter registry poisoned")
        .get(project_id)
    {
        if let Some(adapter) = custom.get(ty) {
            return Ok(adapter.clone());
        }
    }
    if let Some(adapter) = builtin().get(ty) {
        return Ok(adapter.clone());
    }
    anyhow::bail!("Unknown workspace adapter: {ty}")
}

/// `listAdapters` from the reference.
pub fn list_adapters(project_id: &str) -> Vec<WorkspaceAdapterEntry> {
    registered_adapters(project_id)
        .into_iter()
        .map(|(ty, adapter)| WorkspaceAdapterEntry {
            r#type: ty,
            name: adapter.name().to_string(),
            description: adapter.description().to_string(),
        })
        .collect()
}

/// `registerAdapter` from the reference. Plugins load per-project; pass the
/// global project id to register globally.
pub fn register_adapter(project_id: &str, ty: &str, adapter: AdapterRef) {
    state()
        .lock()
        .expect("adapter registry poisoned")
        .entry(project_id.to_string())
        .or_default()
        .insert(ty.to_string(), adapter);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::control_plane::types::*;

    struct TestAdapter;

    #[async_trait::async_trait]
    impl WorkspaceAdapter for TestAdapter {
        fn name(&self) -> &'static str {
            "Test"
        }
        fn description(&self) -> &'static str {
            "Test adapter"
        }
        async fn configure(
            &self,
            info: WorkspaceInfo,
            _context: &WorkspaceAdapterContext,
        ) -> anyhow::Result<WorkspaceInfo> {
            Ok(info)
        }
        async fn create(
            &self,
            _info: &WorkspaceInfo,
            _env: &std::collections::BTreeMap<String, Option<String>>,
            _from: Option<&WorkspaceInfo>,
            _context: &WorkspaceAdapterContext,
        ) -> anyhow::Result<()> {
            Ok(())
        }
        async fn list(
            &self,
            _context: &WorkspaceAdapterContext,
        ) -> anyhow::Result<Vec<WorkspaceListedInfo>> {
            Ok(vec![])
        }
        async fn remove(
            &self,
            _info: &WorkspaceInfo,
            _context: &WorkspaceAdapterContext,
        ) -> anyhow::Result<()> {
            Ok(())
        }
        async fn target(
            &self,
            _info: &WorkspaceInfo,
            _context: &WorkspaceAdapterContext,
        ) -> anyhow::Result<Target> {
            Ok(Target::Local {
                directory: "/tmp".into(),
            })
        }
    }

    #[test]
    fn builtin_has_worktree_adapter() {
        let adapter = get_adapter("global", "worktree").unwrap();
        assert_eq!(adapter.name(), "Worktree");
    }

    #[test]
    fn unknown_adapter_errors() {
        assert!(get_adapter("global", "nope").is_err());
    }

    #[test]
    fn custom_adapter_overrides_builtin_per_project() {
        register_adapter("prj_custom", "worktree", Arc::new(TestAdapter));
        let adapter = get_adapter("prj_custom", "worktree").unwrap();
        assert_eq!(adapter.name(), "Test");
        // Other projects keep the builtin.
        let adapter = get_adapter("global", "worktree").unwrap();
        assert_eq!(adapter.name(), "Worktree");
    }

    #[test]
    fn list_adapters_returns_entries() {
        let entries = list_adapters("global");
        let worktree = entries.iter().find(|e| e.r#type == "worktree").unwrap();
        assert_eq!(worktree.name, "Worktree");
    }
}
