//! Debug workspace plugin: simulate a remote environment locally.
//!
//! From reference/packages/opencode/src/control-plane/dev/README.md and
//! reference/packages/opencode/src/control-plane/dev/debug-workspace-plugin.ts.
//!
//! The reference is a JS plugin (`experimental_workspace.register("debug", ...)`).
//! This port keeps the same wire protocol (a dev data file the workspace server
//! watches) so the debug workflow survives, but the plugin registration happens
//! in Rust.
//!
//! TODO(integration): register the `debug` adapter when oc-plugin / oc-cli load
//! experimental workspace plugins.

use std::collections::BTreeMap;

use super::adapters::{self, AdapterRef};
use super::types::{
    Target, WorkspaceAdapter, WorkspaceAdapterContext, WorkspaceInfo, WorkspaceListedInfo,
};

/// `DEV_DATA_FILE` from the reference.
pub const DEV_DATA_FILE: &str = "/tmp/opencode-workspace-dev-data.json";
/// `DEV_DATA_TEMP_FILE` from the reference.
pub const DEV_DATA_TEMP_FILE: &str = "/tmp/opencode-workspace-dev-data.json.tmp";

/// The JSON written to the dev data file: `{ port, id, env }`.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct DevData {
    pub port: u16,
    pub id: String,
    pub env: BTreeMap<String, Option<String>>,
}

/// The `debug` workspace adapter, mirroring `DebugWorkspacePlugin.register` in
/// the reference: create writes the dev data file and waits for the workspace
/// server health check; the target is the remote server.
pub struct DebugWorkspaceAdapter {
    port: u16,
}

impl DebugWorkspaceAdapter {
    pub fn new(port: u16) -> Self {
        Self { port }
    }

    pub fn register(port: u16) -> AdapterRef {
        adapters::register_adapter("global", "debug", std::sync::Arc::new(Self::new(port)));
        // The reference returns the adapter; keep the default debug adapter path.
        adapters::get_adapter("global", "debug").expect("debug adapter just registered")
    }
}

#[async_trait::async_trait]
impl WorkspaceAdapter for DebugWorkspaceAdapter {
    fn name(&self) -> &'static str {
        "Debug"
    }

    fn description(&self) -> &'static str {
        "Create a debugging server"
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
        info: &WorkspaceInfo,
        env: &BTreeMap<String, Option<String>>,
        _from: Option<&WorkspaceInfo>,
        _context: &WorkspaceAdapterContext,
    ) -> anyhow::Result<()> {
        let data = DevData {
            port: self.port,
            id: info.id.clone(),
            env: env.clone(),
        };
        // `writeDebugData` in the reference: write temp file then rename for
        // atomicity so the watching server picks up complete data.
        let temp = format!("{DEV_DATA_TEMP_FILE}.{}", self.port);
        let json = serde_json::to_string_pretty(&data)?;
        std::fs::write(&temp, json)?;
        std::fs::rename(&temp, DEV_DATA_FILE)?;
        // `waitForHealth`: poll `/global/health` until it responds or 30s pass.
        wait_for_health(self.port).await?;
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
        Ok(Target::Remote {
            url: format!("http://localhost:{}/", self.port),
            headers: Vec::new(),
        })
    }
}

/// `waitForHealth` from the reference: poll `http://127.0.0.1:{port}/global/health`
/// for up to 30 seconds.
pub async fn wait_for_health(port: u16) -> anyhow::Result<()> {
    let url = format!("http://127.0.0.1:{port}/global/health");
    let started = std::time::Instant::now();
    loop {
        match reqwest::get(&url).await {
            Ok(response) if response.status().is_success() => return Ok(()),
            _ => {}
        }
        if started.elapsed() > std::time::Duration::from_secs(30) {
            anyhow::bail!("Timed out waiting for debug server health check at {url}");
        }
        tokio::time::sleep(std::time::Duration::from_millis(250)).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dev_data_serializes() {
        let data = DevData {
            port: 5000,
            id: "wrk_1".into(),
            env: BTreeMap::new(),
        };
        let json = serde_json::to_value(&data).unwrap();
        assert_eq!(json["port"], serde_json::json!(5000));
        assert_eq!(json["id"], serde_json::json!("wrk_1"));
    }

    #[tokio::test]
    async fn debug_target_is_remote() {
        let adapter = DebugWorkspaceAdapter::new(7000);
        let info = WorkspaceInfo {
            id: "wrk_1".into(),
            ty: "debug".into(),
            name: "debug".into(),
            branch: None,
            directory: None,
            extra: None,
            project_id: "global".into(),
        };
        let target = adapter
            .target(&info, &WorkspaceAdapterContext::default())
            .await
            .unwrap();
        assert_eq!(
            target,
            Target::Remote {
                url: "http://localhost:7000/".into(),
                headers: vec![]
            }
        );
    }
}
