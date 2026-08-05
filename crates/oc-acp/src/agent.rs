//! The ACP agent facade.
//!
//! From reference/packages/opencode/src/acp/agent.ts. Wraps the [`Service`]
//! behind the agent-side method surface, converting service errors into ACP
//! `RequestError` values (JSON-RPC error responses).

use std::sync::Arc;

use crate::error::{to_request_error, ACPError};
use crate::service::Service;
use crate::types::{
    AuthenticateRequest, AuthenticateResponse, CancelNotification, CloseSessionRequest,
    CloseSessionResponse, ForkSessionRequest, ForkSessionResponse, InitializeRequest,
    InitializeResponse, ListSessionsRequest, ListSessionsResponse, LoadSessionRequest,
    LoadSessionResponse, NewSessionRequest, NewSessionResponse, PromptRequest, PromptResponse,
    RequestError, ResumeSessionRequest, ResumeSessionResponse, SetSessionConfigOptionRequest,
    SetSessionConfigOptionResponse, SetSessionModeRequest, SetSessionModeResponse,
    SetSessionModelRequest, SetSessionModelResponse,
};

/// The agent-side ACP method surface.
pub struct Agent {
    service: Arc<Service>,
}

impl Agent {
    pub fn new(service: Arc<Service>) -> Self {
        Self { service }
    }

    /// The underlying service.
    pub fn service(&self) -> &Service {
        &self.service
    }

    /// `initialize` from reference/packages/opencode/src/acp/agent.ts.
    pub async fn initialize(
        &self,
        params: &InitializeRequest,
    ) -> Result<InitializeResponse, RequestError> {
        run(self.service.initialize(params).await).await
    }

    /// `authenticate` from reference/packages/opencode/src/acp/agent.ts.
    pub async fn authenticate(
        &self,
        params: &AuthenticateRequest,
    ) -> Result<AuthenticateResponse, RequestError> {
        run(self.service.authenticate(params).await).await
    }

    /// `newSession` from reference/packages/opencode/src/acp/agent.ts.
    pub async fn new_session(
        &self,
        params: &NewSessionRequest,
    ) -> Result<NewSessionResponse, RequestError> {
        run(self.service.new_session(params).await).await
    }

    /// `loadSession` from reference/packages/opencode/src/acp/agent.ts.
    pub async fn load_session(
        &self,
        params: &LoadSessionRequest,
    ) -> Result<LoadSessionResponse, RequestError> {
        run(self.service.load_session(params).await).await
    }

    /// `listSessions` from reference/packages/opencode/src/acp/agent.ts.
    pub async fn list_sessions(
        &self,
        params: &ListSessionsRequest,
    ) -> Result<ListSessionsResponse, RequestError> {
        run(self.service.list_sessions(params).await).await
    }

    /// `resumeSession` from reference/packages/opencode/src/acp/agent.ts.
    pub async fn resume_session(
        &self,
        params: &ResumeSessionRequest,
    ) -> Result<ResumeSessionResponse, RequestError> {
        run(self.service.resume_session(params).await).await
    }

    /// `closeSession` from reference/packages/opencode/src/acp/agent.ts.
    pub async fn close_session(
        &self,
        params: &CloseSessionRequest,
    ) -> Result<CloseSessionResponse, RequestError> {
        run(self.service.close_session(params).await).await
    }

    /// `unstable_forkSession` from reference/packages/opencode/src/acp/agent.ts.
    pub async fn fork_session(
        &self,
        params: &ForkSessionRequest,
    ) -> Result<ForkSessionResponse, RequestError> {
        run(self.service.fork_session(params).await).await
    }

    /// `setSessionConfigOption` from reference/packages/opencode/src/acp/agent.ts.
    pub async fn set_session_config_option(
        &self,
        params: &SetSessionConfigOptionRequest,
    ) -> Result<SetSessionConfigOptionResponse, RequestError> {
        run(self.service.set_session_config_option(params).await).await
    }

    /// `setSessionMode` from reference/packages/opencode/src/acp/agent.ts.
    pub async fn set_session_mode(
        &self,
        params: &SetSessionModeRequest,
    ) -> Result<SetSessionModeResponse, RequestError> {
        run(self.service.set_session_mode(params).await).await
    }

    /// `unstable_setSessionModel` from reference/packages/opencode/src/acp/agent.ts.
    pub async fn set_session_model(
        &self,
        params: &SetSessionModelRequest,
    ) -> Result<SetSessionModelResponse, RequestError> {
        run(self.service.set_session_model(params).await).await
    }

    /// `prompt` from reference/packages/opencode/src/acp/agent.ts.
    pub async fn prompt(&self, params: &PromptRequest) -> Result<PromptResponse, RequestError> {
        run(self.service.prompt(params).await).await
    }

    /// `cancel` from reference/packages/opencode/src/acp/agent.ts.
    pub async fn cancel(&self, params: &CancelNotification) -> Result<(), RequestError> {
        run(self.service.cancel(params).await).await
    }
}

/// `run` from reference/packages/opencode/src/acp/agent.ts. Maps service errors
/// to `RequestError`; unexpected failures become a generic service failure.
///
/// TODO(integration): the reference's `catch` branch converts defects (unexpected
/// thrown values) into `ServiceFailureError { safeMessage: "Internal service
/// failure" }`. Rust service methods only produce [`ACPError`], so this path is
/// not reachable here.
async fn run<T>(result: Result<T, ACPError>) -> Result<T, RequestError> {
    match result {
        Ok(value) => Ok(value),
        Err(error) => Err(to_request_error(&error)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::ACPError;

    #[tokio::test]
    async fn error_mapping() {
        let err = run::<()>(Err(ACPError::SessionNotFound {
            session_id: "s1".into(),
        }))
        .await
        .unwrap_err();
        assert_eq!(err.code, -32602);
        assert_eq!(err.message, "Invalid params: session not found: s1");
    }
}
