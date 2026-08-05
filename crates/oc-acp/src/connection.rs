//! The agent-side connection to an ACP client.
//!
//! Mirrors `Pick<AgentSideConnection, "sessionUpdate"> & Partial<Pick<...,
//! "requestPermission" | "writeTextFile">>` from reference/packages/opencode/src/acp/service.ts.
//! The concrete implementation is provided by the transport layer
//! (TODO(integration): oc-server / oc-cli stdio connection).

use async_trait::async_trait;

use crate::types::{
    RequestPermissionRequest, RequestPermissionResponse, SessionUpdate, WriteTextFileRequest,
};

/// The agent-side connection to the connected ACP client.
#[async_trait]
pub trait AgentSideConnection: Send + Sync {
    /// `sessionUpdate` from the ACP SDK.
    async fn session_update(&self, session_id: &str, update: SessionUpdate) -> Result<(), ()>;

    /// `requestPermission` from the ACP SDK.
    async fn request_permission(
        &self,
        request: RequestPermissionRequest,
    ) -> Result<RequestPermissionResponse, ()>;

    /// `writeTextFile` from the ACP SDK.
    async fn write_text_file(&self, request: WriteTextFileRequest) -> Result<(), ()>;

    /// Whether the client advertises `requestPermission`.
    fn supports_request_permission(&self) -> bool {
        true
    }

    /// Whether the client advertises `writeTextFile`.
    fn supports_write_text_file(&self) -> bool {
        true
    }
}
