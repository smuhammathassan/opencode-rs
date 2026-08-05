//! Workspace adapter types.
//!
//! From reference/packages/opencode/src/control-plane/types.ts.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// `WorkspaceInfo` from reference/packages/opencode/src/control-plane/types.ts.
///
/// `branch`, `directory`, and `extra` are `optional(NullOr(...))`: absent is
/// omitted, `null` is explicit. `fromRow` in workspace.ts always sets them to the
/// DB value, so rows serialize them as `null` when empty.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorkspaceInfo {
    pub id: String,
    #[serde(rename = "type")]
    pub ty: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub branch: Option<Option<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub directory: Option<Option<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub extra: Option<Option<Value>>,
    #[serde(rename = "projectID")]
    pub project_id: String,
}

impl WorkspaceInfo {
    /// Build the `fromRow` shape: nullable fields present (as `null` when unset).
    pub fn from_row(
        id: String,
        ty: String,
        name: String,
        branch: Option<String>,
        directory: Option<String>,
        extra: Option<Value>,
        project_id: String,
    ) -> Self {
        Self {
            id,
            ty,
            name,
            branch: Some(branch),
            directory: Some(directory),
            extra: Some(extra),
            project_id,
        }
    }
}

/// `WorkspaceListedInfo` from reference/packages/opencode/src/control-plane/types.ts
/// (`Struct.omit(WorkspaceInfo.fields, ["id"])`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorkspaceListedInfo {
    #[serde(rename = "type")]
    pub ty: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub branch: Option<Option<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub directory: Option<Option<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub extra: Option<Option<Value>>,
    #[serde(rename = "projectID")]
    pub project_id: String,
}

/// `WorkspaceAdapterEntry` from reference/packages/opencode/src/control-plane/types.ts.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorkspaceAdapterEntry {
    pub r#type: String,
    pub name: String,
    pub description: String,
}

/// `Target` from reference/packages/opencode/src/control-plane/types.ts.
#[derive(Debug, Clone, PartialEq)]
pub enum Target {
    Local {
        directory: String,
    },
    Remote {
        url: String,
        headers: Vec<(String, String)>,
    },
}

/// `WorkspaceAdapterContext` from reference/packages/opencode/src/control-plane/types.ts.
///
/// The reference passes the whole `InstanceContext`; the port only carries the
/// subset the adapters need, notably the project id (`context.instance.project.id`).
#[derive(Debug, Clone, Default)]
pub struct WorkspaceAdapterContext {
    pub workspace_id: Option<String>,
    pub project_id: Option<String>,
}

/// `WorkspaceAdapter` from reference/packages/opencode/src/control-plane/types.ts.
///
/// `configure` is synchronous in the reference interface but the builtin adapters
/// implement it as `async`; the trait mirrors that (all methods async).
#[async_trait::async_trait]
pub trait WorkspaceAdapter: Send + Sync {
    fn name(&self) -> &'static str;
    fn description(&self) -> &'static str;

    async fn configure(
        &self,
        info: WorkspaceInfo,
        context: &WorkspaceAdapterContext,
    ) -> anyhow::Result<WorkspaceInfo>;

    async fn create(
        &self,
        info: &WorkspaceInfo,
        env: &std::collections::BTreeMap<String, Option<String>>,
        from: Option<&WorkspaceInfo>,
        context: &WorkspaceAdapterContext,
    ) -> anyhow::Result<()>;

    async fn list(
        &self,
        context: &WorkspaceAdapterContext,
    ) -> anyhow::Result<Vec<WorkspaceListedInfo>>;

    async fn remove(
        &self,
        info: &WorkspaceInfo,
        context: &WorkspaceAdapterContext,
    ) -> anyhow::Result<()>;

    async fn target(
        &self,
        info: &WorkspaceInfo,
        context: &WorkspaceAdapterContext,
    ) -> anyhow::Result<Target>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn workspace_info_serializes_with_project_id_camel_case() {
        let info = WorkspaceInfo {
            id: "wrk_1".into(),
            ty: "worktree".into(),
            name: "crisp-planet".into(),
            branch: Some(None),
            directory: Some(Some("/tmp/worktrees/crisp-planet".into())),
            extra: Some(Some(serde_json::json!({ "key": "value" }))),
            project_id: "global".into(),
        };
        let json = serde_json::to_string(&info).unwrap();
        assert_eq!(
            json,
            r#"{"id":"wrk_1","type":"worktree","name":"crisp-planet","branch":null,"directory":"/tmp/worktrees/crisp-planet","extra":{"key":"value"},"projectID":"global"}"#
        );
    }

    #[test]
    fn workspace_info_omits_absent_nullable_fields() {
        let info = WorkspaceInfo {
            id: "wrk_1".into(),
            ty: "remote".into(),
            name: "foo".into(),
            branch: None,
            directory: None,
            extra: None,
            project_id: "global".into(),
        };
        let json = serde_json::to_string(&info).unwrap();
        assert_eq!(
            json,
            r#"{"id":"wrk_1","type":"remote","name":"foo","projectID":"global"}"#
        );
    }

    #[test]
    fn from_row_produces_explicit_nulls() {
        let info = WorkspaceInfo::from_row(
            "wrk_1".into(),
            "worktree".into(),
            "crisp-planet".into(),
            None,
            Some("/tmp/x".into()),
            None,
            "global".into(),
        );
        let json = serde_json::to_string(&info).unwrap();
        assert_eq!(
            json,
            r#"{"id":"wrk_1","type":"worktree","name":"crisp-planet","branch":null,"directory":"/tmp/x","extra":null,"projectID":"global"}"#
        );
    }

    #[test]
    fn listed_info_omits_id() {
        let info = WorkspaceListedInfo {
            ty: "worktree".into(),
            name: "crisp-planet".into(),
            branch: Some(None),
            directory: Some(None),
            extra: Some(None),
            project_id: "global".into(),
        };
        let json = serde_json::to_string(&info).unwrap();
        assert_eq!(
            json,
            r#"{"type":"worktree","name":"crisp-planet","branch":null,"directory":null,"extra":null,"projectID":"global"}"#
        );
    }

    #[test]
    fn adapter_entry_json_shape() {
        let entry = WorkspaceAdapterEntry {
            r#type: "worktree".into(),
            name: "Worktree".into(),
            description: "Create a git worktree".into(),
        };
        let json = serde_json::to_string(&entry).unwrap();
        assert_eq!(
            json,
            r#"{"type":"worktree","name":"Worktree","description":"Create a git worktree"}"#
        );
    }
}
