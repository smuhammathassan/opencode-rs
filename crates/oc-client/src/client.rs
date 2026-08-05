//! The typed RPC client.
//!
//! Mirrors `reference/packages/client/src/generated/client.ts` (the promise
//! client): every group and endpoint is ported with identical HTTP methods,
//! URL paths, query encoding, and JSON bodies. Path parameters are encoded with
//! `encodeURIComponent` semantics; query parameters mirror the recursive
//! `appendQuery` helper (objects encode as `key[child]`).
//!
//! `OpenCode::make` mirrors `OpenCode.make(options)` from the reference.

use crate::error::Error;
use crate::sse::sse_stream;
use crate::transport::{ClientOptions, RequestDescriptor, RequestOptions, Transport};
use crate::types::agent::AgentInfo;
use crate::types::command::CommandInfo;
use crate::types::event::SessionDurableEvent;
use crate::types::filesystem::{FileSystemEntry, FilesFindInput, FilesListInput};
use crate::types::health::Health;
use crate::types::integration::{
    IntegrationAttempt, IntegrationAttemptStatus, IntegrationInfo, IntegrationsAttemptCancelInput,
    IntegrationsAttemptCompleteInput, IntegrationsAttemptStatusInput, IntegrationsConnectKeyInput,
    IntegrationsConnectOauthInput, IntegrationsGetInput,
};
use crate::types::location::{LocationData, LocationInfo, LocationInput};
use crate::types::model::ModelInfo;
use crate::types::permission::{
    PermissionDecision, PermissionRequest, PermissionsCreateInput, PermissionsGetInput,
    PermissionsListInput, PermissionsReplyInput,
};
use crate::types::permission_saved::{
    PermissionSavedInfo, PermissionsListSavedInput, PermissionsRemoveSavedInput,
};
use crate::types::project_copy::{
    ProjectCopy, ProjectCopyCreateInput, ProjectCopyRefreshInput, ProjectCopyRemoveInput,
};
use crate::types::provider::{ProviderInfo, ProvidersGetInput};
use crate::types::pty::{PtyCreateInput, PtyGetInput, PtyInfo, PtyRemoveInput, PtyUpdateInput};
use crate::types::question::{
    QuestionReply, QuestionRequest, QuestionsListInput, QuestionsRejectInput, QuestionsReplyInput,
};
use crate::types::reference::ReferenceInfo;
use crate::types::schema::JsonValue;
use crate::types::session::{
    SessionActive, SessionIDInput, SessionInfo, SessionsCreateInput, SessionsEventsInput,
    SessionsGetInput, SessionsHistory, SessionsHistoryInput, SessionsListInput,
    SessionsMessageInput, SessionsPromptInput, SessionsResponse, SessionsStageInput,
    SessionsSwitchAgentInput, SessionsSwitchModelInput,
};
use crate::types::session_input::SessionInputAdmitted;
use crate::types::session_message::{SessionMessage, SessionMessagesResponse};
use crate::types::skill::SkillInfo;
use futures::Stream;
use percent_encoding::{utf8_percent_encode, AsciiSet, NON_ALPHANUMERIC};
use reqwest::Method;
use serde::de::DeserializeOwned;
use std::collections::HashMap;
use url::Url;

/// Characters left unescaped by `encodeURIComponent`.
const ENCODE_COMPONENT: &AsciiSet = &NON_ALPHANUMERIC
    .remove(b'-')
    .remove(b'_')
    .remove(b'.')
    .remove(b'!')
    .remove(b'~')
    .remove(b'*')
    .remove(b'\'')
    .remove(b'(')
    .remove(b')');

fn encode_component(value: &str) -> String {
    utf8_percent_encode(value, ENCODE_COMPONENT).to_string()
}

/// Append a query parameter when present, preserving the descriptor's key order.
fn q(out: &mut Vec<(String, JsonValue)>, key: &str, value: Option<JsonValue>) {
    if let Some(value) = value {
        out.push((key.to_string(), value));
    }
}

fn location_query(value: Option<&crate::types::location::LocationQueryRef>) -> Option<JsonValue> {
    value.map(|location| serde_json::to_value(location).unwrap_or(JsonValue::Null))
}

fn opt_string(value: Option<&String>) -> Option<JsonValue> {
    value.map(|value| JsonValue::from(value.as_str()))
}

/// Build a JSON object body from optional fields, omitting `None` fields.
/// Mirrors the body object literals in `reference/packages/client/src/generated/client.ts`.
macro_rules! body {
    ($($key:expr => $value:expr),* $(,)?) => {{
        let mut map = serde_json::Map::new();
        $(if let Some(value) = $value {
            map.insert($key.to_string(), serde_json::to_value(value).unwrap_or(JsonValue::Null));
        })*
        serde_json::Value::Object(map)
    }};
}

/// Wire wrapper for endpoints that return `{ data: T }`.
#[derive(serde::Deserialize)]
struct Data<T> {
    data: T,
}

async fn unwrap_data<T: DeserializeOwned>(
    transport: &Transport,
    desc: &RequestDescriptor,
    options: Option<&RequestOptions>,
) -> Result<T, Error> {
    transport
        .execute::<Data<T>>(desc, options)
        .await
        .map(|wrapper| wrapper.data)
}

/// The opencode client. Mirrors `OpenCode.make(...)` in
/// `reference/packages/client/src/generated/client.ts`.
#[derive(Clone)]
pub struct OpenCode {
    transport: Transport,
    pub health: HealthGroup,
    pub location: LocationGroup,
    pub agents: AgentGroup,
    pub sessions: SessionGroup,
    pub messages: MessageGroup,
    pub models: ModelGroup,
    pub providers: ProviderGroup,
    pub integrations: IntegrationGroup,
    pub credentials: CredentialGroup,
    pub permissions: PermissionGroup,
    pub files: FileSystemGroup,
    pub commands: CommandGroup,
    pub skills: SkillGroup,
    pub events: EventGroup,
    pub ptys: PtyGroup,
    pub questions: QuestionGroup,
    pub references: ReferenceGroup,
    /// The `projectCopies` group.
    pub project_copies: ProjectCopyGroup,
}

impl OpenCode {
    /// Create a client. Mirrors `make(options)` in
    /// `reference/packages/client/src/generated/client.ts`.
    pub fn make(options: ClientOptions) -> Result<Self, reqwest::Error> {
        let transport = Transport::new(&options)?;
        Ok(OpenCode {
            transport: transport.clone(),
            health: HealthGroup {
                transport: transport.clone(),
            },
            location: LocationGroup {
                transport: transport.clone(),
            },
            agents: AgentGroup {
                transport: transport.clone(),
            },
            sessions: SessionGroup {
                transport: transport.clone(),
            },
            messages: MessageGroup {
                transport: transport.clone(),
            },
            models: ModelGroup {
                transport: transport.clone(),
            },
            providers: ProviderGroup {
                transport: transport.clone(),
            },
            integrations: IntegrationGroup {
                transport: transport.clone(),
            },
            credentials: CredentialGroup {
                transport: transport.clone(),
            },
            permissions: PermissionGroup {
                transport: transport.clone(),
            },
            files: FileSystemGroup {
                transport: transport.clone(),
            },
            commands: CommandGroup {
                transport: transport.clone(),
            },
            skills: SkillGroup {
                transport: transport.clone(),
            },
            events: EventGroup {
                transport: transport.clone(),
            },
            ptys: PtyGroup {
                transport: transport.clone(),
            },
            questions: QuestionGroup {
                transport: transport.clone(),
            },
            references: ReferenceGroup {
                transport: transport.clone(),
            },
            project_copies: ProjectCopyGroup {
                transport: transport.clone(),
            },
        })
    }

    /// Alias for [`OpenCode::make`].
    pub fn new(options: ClientOptions) -> Result<Self, reqwest::Error> {
        Self::make(options)
    }

    /// The base URL this client targets.
    pub fn base_url(&self) -> &Url {
        &self.transport.base_url
    }
}

/// `server.health` group.
#[derive(Clone)]
pub struct HealthGroup {
    transport: Transport,
}

impl HealthGroup {
    /// `GET /api/health` — check server health.
    pub async fn get(&self, options: Option<&RequestOptions>) -> Result<Health, Error> {
        self.transport
            .execute(
                &RequestDescriptor {
                    method: Method::GET,
                    path: "/api/health".into(),
                    query: Vec::new(),
                    body: None,
                    success_status: 200,
                    declared_statuses: &[401, 400],
                    empty: false,
                },
                options,
            )
            .await
    }
}

/// `server.location` group.
#[derive(Clone)]
pub struct LocationGroup {
    transport: Transport,
}

impl LocationGroup {
    /// `GET /api/location` — resolve the requested or default location.
    pub async fn get(
        &self,
        input: Option<&LocationInput>,
        options: Option<&RequestOptions>,
    ) -> Result<LocationInfo, Error> {
        let mut query = Vec::new();
        q(
            &mut query,
            "location",
            location_query(input.and_then(|input| input.location.as_ref())),
        );
        self.transport
            .execute(
                &RequestDescriptor {
                    method: Method::GET,
                    path: "/api/location".into(),
                    query,
                    body: None,
                    success_status: 200,
                    declared_statuses: &[401, 400],
                    empty: false,
                },
                options,
            )
            .await
    }
}

/// `server.agent` group.
#[derive(Clone)]
pub struct AgentGroup {
    transport: Transport,
}

impl AgentGroup {
    /// `GET /api/agent` — list registered agents.
    pub async fn list(
        &self,
        input: Option<&LocationInput>,
        options: Option<&RequestOptions>,
    ) -> Result<LocationData<Vec<AgentInfo>>, Error> {
        let mut query = Vec::new();
        q(
            &mut query,
            "location",
            location_query(input.and_then(|input| input.location.as_ref())),
        );
        self.transport
            .execute(
                &RequestDescriptor {
                    method: Method::GET,
                    path: "/api/agent".into(),
                    query,
                    body: None,
                    success_status: 200,
                    declared_statuses: &[401, 400],
                    empty: false,
                },
                options,
            )
            .await
    }
}

/// `server.session` group.
#[derive(Clone)]
pub struct SessionGroup {
    transport: Transport,
}

impl SessionGroup {
    /// `GET /api/session` — list sessions.
    pub async fn list(
        &self,
        input: Option<&SessionsListInput>,
        options: Option<&RequestOptions>,
    ) -> Result<SessionsResponse, Error> {
        let mut query = Vec::new();
        if let Some(input) = input {
            q(
                &mut query,
                "workspace",
                opt_string(input.workspace.as_ref()),
            );
            q(&mut query, "limit", input.limit.map(JsonValue::from));
            q(
                &mut query,
                "order",
                input
                    .order
                    .map(|order| serde_json::to_value(order).unwrap_or(JsonValue::Null)),
            );
            q(&mut query, "search", opt_string(input.search.as_ref()));
            q(
                &mut query,
                "directory",
                opt_string(input.directory.as_ref()),
            );
            q(&mut query, "project", opt_string(input.project.as_ref()));
            q(&mut query, "subpath", opt_string(input.subpath.as_ref()));
            q(&mut query, "cursor", opt_string(input.cursor.as_ref()));
        }
        self.transport
            .execute(
                &RequestDescriptor {
                    method: Method::GET,
                    path: "/api/session".into(),
                    query,
                    body: None,
                    success_status: 200,
                    declared_statuses: &[400, 401],
                    empty: false,
                },
                options,
            )
            .await
    }

    /// `POST /api/session` — create a session.
    pub async fn create(
        &self,
        input: Option<&SessionsCreateInput>,
        options: Option<&RequestOptions>,
    ) -> Result<SessionInfo, Error> {
        let body =
            serde_json::to_value(input.cloned().unwrap_or_default()).unwrap_or(JsonValue::Null);
        unwrap_data(
            &self.transport,
            &RequestDescriptor {
                method: Method::POST,
                path: "/api/session".into(),
                query: Vec::new(),
                body: Some(body),
                success_status: 200,
                declared_statuses: &[401, 400],
                empty: false,
            },
            options,
        )
        .await
    }

    /// `GET /api/session/active` — list active sessions.
    pub async fn active(
        &self,
        options: Option<&RequestOptions>,
    ) -> Result<HashMap<String, SessionActive>, Error> {
        unwrap_data(
            &self.transport,
            &RequestDescriptor {
                method: Method::GET,
                path: "/api/session/active".into(),
                query: Vec::new(),
                body: None,
                success_status: 200,
                declared_statuses: &[401, 400],
                empty: false,
            },
            options,
        )
        .await
    }

    /// `GET /api/session/:sessionID` — get a session.
    pub async fn get(
        &self,
        input: &SessionsGetInput,
        options: Option<&RequestOptions>,
    ) -> Result<SessionInfo, Error> {
        unwrap_data(
            &self.transport,
            &RequestDescriptor {
                method: Method::GET,
                path: format!("/api/session/{}", encode_component(&input.session_id)),
                query: Vec::new(),
                body: None,
                success_status: 200,
                declared_statuses: &[404, 400, 401],
                empty: false,
            },
            options,
        )
        .await
    }

    /// `POST /api/session/:sessionID/agent` — switch the session agent.
    pub async fn switch_agent(
        &self,
        input: &SessionsSwitchAgentInput,
        options: Option<&RequestOptions>,
    ) -> Result<(), Error> {
        self.transport
            .execute(
                &RequestDescriptor {
                    method: Method::POST,
                    path: format!("/api/session/{}/agent", encode_component(&input.session_id)),
                    query: Vec::new(),
                    body: Some(body! { "agent" => Some(&input.agent) }),
                    success_status: 204,
                    declared_statuses: &[404, 400, 401],
                    empty: true,
                },
                options,
            )
            .await
    }

    /// `POST /api/session/:sessionID/model` — switch the session model.
    pub async fn switch_model(
        &self,
        input: &SessionsSwitchModelInput,
        options: Option<&RequestOptions>,
    ) -> Result<(), Error> {
        self.transport
            .execute(
                &RequestDescriptor {
                    method: Method::POST,
                    path: format!("/api/session/{}/model", encode_component(&input.session_id)),
                    query: Vec::new(),
                    body: Some(body! { "model" => Some(&input.model) }),
                    success_status: 204,
                    declared_statuses: &[404, 400, 401],
                    empty: true,
                },
                options,
            )
            .await
    }

    /// `POST /api/session/:sessionID/prompt` — send a message.
    pub async fn prompt(
        &self,
        input: &SessionsPromptInput,
        options: Option<&RequestOptions>,
    ) -> Result<SessionInputAdmitted, Error> {
        let body = body! {
            "id" => input.id.as_ref(),
            "prompt" => Some(&input.prompt),
            "delivery" => input.delivery,
            "resume" => input.resume,
        };
        unwrap_data(
            &self.transport,
            &RequestDescriptor {
                method: Method::POST,
                path: format!(
                    "/api/session/{}/prompt",
                    encode_component(&input.session_id)
                ),
                query: Vec::new(),
                body: Some(body),
                success_status: 200,
                declared_statuses: &[409, 404, 400, 401],
                empty: false,
            },
            options,
        )
        .await
    }

    /// `POST /api/session/:sessionID/compact` — compact the session.
    pub async fn compact(
        &self,
        input: &SessionIDInput,
        options: Option<&RequestOptions>,
    ) -> Result<(), Error> {
        self.transport
            .execute(
                &RequestDescriptor {
                    method: Method::POST,
                    path: format!(
                        "/api/session/{}/compact",
                        encode_component(&input.session_id)
                    ),
                    query: Vec::new(),
                    body: None,
                    success_status: 204,
                    declared_statuses: &[404, 503, 400, 401],
                    empty: true,
                },
                options,
            )
            .await
    }

    /// `POST /api/session/:sessionID/wait` — wait for the agent loop to become idle.
    pub async fn wait(
        &self,
        input: &SessionIDInput,
        options: Option<&RequestOptions>,
    ) -> Result<(), Error> {
        self.transport
            .execute(
                &RequestDescriptor {
                    method: Method::POST,
                    path: format!("/api/session/{}/wait", encode_component(&input.session_id)),
                    query: Vec::new(),
                    body: None,
                    success_status: 204,
                    declared_statuses: &[404, 503, 400, 401],
                    empty: true,
                },
                options,
            )
            .await
    }

    /// `POST /api/session/:sessionID/revert/stage` — stage a reversible boundary.
    pub async fn stage(
        &self,
        input: &SessionsStageInput,
        options: Option<&RequestOptions>,
    ) -> Result<crate::types::revert::RevertState, Error> {
        let body = body! {
            "messageID" => Some(&input.message_id),
            "files" => input.files,
        };
        unwrap_data(
            &self.transport,
            &RequestDescriptor {
                method: Method::POST,
                path: format!(
                    "/api/session/{}/revert/stage",
                    encode_component(&input.session_id)
                ),
                query: Vec::new(),
                body: Some(body),
                success_status: 200,
                declared_statuses: &[404, 500, 400, 401],
                empty: false,
            },
            options,
        )
        .await
    }

    /// `POST /api/session/:sessionID/revert/clear` — clear the staged revert.
    pub async fn clear(
        &self,
        input: &SessionIDInput,
        options: Option<&RequestOptions>,
    ) -> Result<(), Error> {
        self.transport
            .execute(
                &RequestDescriptor {
                    method: Method::POST,
                    path: format!(
                        "/api/session/{}/revert/clear",
                        encode_component(&input.session_id)
                    ),
                    query: Vec::new(),
                    body: None,
                    success_status: 204,
                    declared_statuses: &[404, 500, 400, 401],
                    empty: true,
                },
                options,
            )
            .await
    }

    /// `POST /api/session/:sessionID/revert/commit` — commit the staged revert.
    pub async fn commit(
        &self,
        input: &SessionIDInput,
        options: Option<&RequestOptions>,
    ) -> Result<(), Error> {
        self.transport
            .execute(
                &RequestDescriptor {
                    method: Method::POST,
                    path: format!(
                        "/api/session/{}/revert/commit",
                        encode_component(&input.session_id)
                    ),
                    query: Vec::new(),
                    body: None,
                    success_status: 204,
                    declared_statuses: &[404, 400, 401],
                    empty: true,
                },
                options,
            )
            .await
    }

    /// `GET /api/session/:sessionID/context` — the active context messages.
    pub async fn context(
        &self,
        input: &SessionIDInput,
        options: Option<&RequestOptions>,
    ) -> Result<Vec<SessionMessage>, Error> {
        unwrap_data(
            &self.transport,
            &RequestDescriptor {
                method: Method::GET,
                path: format!(
                    "/api/session/{}/context",
                    encode_component(&input.session_id)
                ),
                query: Vec::new(),
                body: None,
                success_status: 200,
                declared_statuses: &[404, 500, 400, 401],
                empty: false,
            },
            options,
        )
        .await
    }

    /// `GET /api/session/:sessionID/history` — a page of durable session events.
    pub async fn history(
        &self,
        input: &SessionsHistoryInput,
        options: Option<&RequestOptions>,
    ) -> Result<SessionsHistory, Error> {
        let mut query = Vec::new();
        q(&mut query, "limit", input.limit.map(JsonValue::from));
        q(&mut query, "after", input.after.map(JsonValue::from));
        self.transport
            .execute(
                &RequestDescriptor {
                    method: Method::GET,
                    path: format!(
                        "/api/session/{}/history",
                        encode_component(&input.session_id)
                    ),
                    query,
                    body: None,
                    success_status: 200,
                    declared_statuses: &[404, 400, 401],
                    empty: false,
                },
                options,
            )
            .await
    }

    /// `GET /api/session/:sessionID/event` — subscribe to durable session events.
    pub fn events(
        &self,
        input: &SessionsEventsInput,
        options: Option<RequestOptions>,
    ) -> std::pin::Pin<Box<dyn Stream<Item = Result<SessionDurableEvent, Error>> + Send + 'static>>
    {
        let mut query = Vec::new();
        q(&mut query, "after", input.after.map(JsonValue::from));
        sse_stream(
            self.transport.clone(),
            RequestDescriptor {
                method: Method::GET,
                path: format!("/api/session/{}/event", encode_component(&input.session_id)),
                query,
                body: None,
                success_status: 200,
                declared_statuses: &[404, 400, 401],
                empty: false,
            },
            options,
        )
    }

    /// `POST /api/session/:sessionID/interrupt` — interrupt active execution.
    pub async fn interrupt(
        &self,
        input: &SessionIDInput,
        options: Option<&RequestOptions>,
    ) -> Result<(), Error> {
        self.transport
            .execute(
                &RequestDescriptor {
                    method: Method::POST,
                    path: format!(
                        "/api/session/{}/interrupt",
                        encode_component(&input.session_id)
                    ),
                    query: Vec::new(),
                    body: None,
                    success_status: 204,
                    declared_statuses: &[404, 400, 401],
                    empty: true,
                },
                options,
            )
            .await
    }

    /// `GET /api/session/:sessionID/message/:messageID` — one projected message.
    pub async fn message(
        &self,
        input: &SessionsMessageInput,
        options: Option<&RequestOptions>,
    ) -> Result<SessionMessage, Error> {
        unwrap_data(
            &self.transport,
            &RequestDescriptor {
                method: Method::GET,
                path: format!(
                    "/api/session/{}/message/{}",
                    encode_component(&input.session_id),
                    encode_component(&input.message_id)
                ),
                query: Vec::new(),
                body: None,
                success_status: 200,
                declared_statuses: &[404, 400, 401],
                empty: false,
            },
            options,
        )
        .await
    }
}

/// `server.message` group.
#[derive(Clone)]
pub struct MessageGroup {
    transport: Transport,
}

impl MessageGroup {
    /// `GET /api/session/:sessionID/message` — list projected messages.
    pub async fn list(
        &self,
        input: &crate::types::session_message::MessagesListInput,
        options: Option<&RequestOptions>,
    ) -> Result<SessionMessagesResponse, Error> {
        let mut query = Vec::new();
        q(&mut query, "limit", input.limit.map(JsonValue::from));
        q(
            &mut query,
            "order",
            input
                .order
                .map(|order| serde_json::to_value(order).unwrap_or(JsonValue::Null)),
        );
        q(&mut query, "cursor", opt_string(input.cursor.as_ref()));
        self.transport
            .execute(
                &RequestDescriptor {
                    method: Method::GET,
                    path: format!(
                        "/api/session/{}/message",
                        encode_component(&input.session_id)
                    ),
                    query,
                    body: None,
                    success_status: 200,
                    declared_statuses: &[400, 404, 500, 401],
                    empty: false,
                },
                options,
            )
            .await
    }
}

/// `server.model` group.
#[derive(Clone)]
pub struct ModelGroup {
    transport: Transport,
}

impl ModelGroup {
    /// `GET /api/model` — list available models.
    pub async fn list(
        &self,
        input: Option<&LocationInput>,
        options: Option<&RequestOptions>,
    ) -> Result<LocationData<Vec<ModelInfo>>, Error> {
        let mut query = Vec::new();
        q(
            &mut query,
            "location",
            location_query(input.and_then(|input| input.location.as_ref())),
        );
        self.transport
            .execute(
                &RequestDescriptor {
                    method: Method::GET,
                    path: "/api/model".into(),
                    query,
                    body: None,
                    success_status: 200,
                    declared_statuses: &[503, 401, 400],
                    empty: false,
                },
                options,
            )
            .await
    }
}

/// `server.provider` group.
#[derive(Clone)]
pub struct ProviderGroup {
    transport: Transport,
}

impl ProviderGroup {
    /// `GET /api/provider` — list providers.
    pub async fn list(
        &self,
        input: Option<&LocationInput>,
        options: Option<&RequestOptions>,
    ) -> Result<LocationData<Vec<ProviderInfo>>, Error> {
        let mut query = Vec::new();
        q(
            &mut query,
            "location",
            location_query(input.and_then(|input| input.location.as_ref())),
        );
        self.transport
            .execute(
                &RequestDescriptor {
                    method: Method::GET,
                    path: "/api/provider".into(),
                    query,
                    body: None,
                    success_status: 200,
                    declared_statuses: &[503, 401, 400],
                    empty: false,
                },
                options,
            )
            .await
    }

    /// `GET /api/provider/:providerID` — get one provider.
    pub async fn get(
        &self,
        input: &ProvidersGetInput,
        options: Option<&RequestOptions>,
    ) -> Result<LocationData<ProviderInfo>, Error> {
        let mut query = Vec::new();
        q(
            &mut query,
            "location",
            location_query(input.location.as_ref()),
        );
        self.transport
            .execute(
                &RequestDescriptor {
                    method: Method::GET,
                    path: format!("/api/provider/{}", encode_component(&input.provider_id)),
                    query,
                    body: None,
                    success_status: 200,
                    declared_statuses: &[404, 503, 401, 400],
                    empty: false,
                },
                options,
            )
            .await
    }
}

/// `server.integration` group.
#[derive(Clone)]
pub struct IntegrationGroup {
    transport: Transport,
}

impl IntegrationGroup {
    /// `GET /api/integration` — list integrations.
    pub async fn list(
        &self,
        input: Option<&LocationInput>,
        options: Option<&RequestOptions>,
    ) -> Result<LocationData<Vec<IntegrationInfo>>, Error> {
        let mut query = Vec::new();
        q(
            &mut query,
            "location",
            location_query(input.and_then(|input| input.location.as_ref())),
        );
        self.transport
            .execute(
                &RequestDescriptor {
                    method: Method::GET,
                    path: "/api/integration".into(),
                    query,
                    body: None,
                    success_status: 200,
                    declared_statuses: &[401, 400],
                    empty: false,
                },
                options,
            )
            .await
    }

    /// `GET /api/integration/:integrationID` — get one integration.
    pub async fn get(
        &self,
        input: &IntegrationsGetInput,
        options: Option<&RequestOptions>,
    ) -> Result<LocationData<Option<IntegrationInfo>>, Error> {
        let mut query = Vec::new();
        q(
            &mut query,
            "location",
            location_query(input.location.as_ref()),
        );
        self.transport
            .execute(
                &RequestDescriptor {
                    method: Method::GET,
                    path: format!(
                        "/api/integration/{}",
                        encode_component(&input.integration_id)
                    ),
                    query,
                    body: None,
                    success_status: 200,
                    declared_statuses: &[401, 400],
                    empty: false,
                },
                options,
            )
            .await
    }

    /// `POST /api/integration/:integrationID/connect/key` — connect with a key.
    pub async fn connect_key(
        &self,
        input: &IntegrationsConnectKeyInput,
        options: Option<&RequestOptions>,
    ) -> Result<(), Error> {
        let mut query = Vec::new();
        q(
            &mut query,
            "location",
            location_query(input.location.as_ref()),
        );
        let body = body! {
            "key" => Some(&input.key),
            "label" => input.label.as_ref(),
        };
        self.transport
            .execute(
                &RequestDescriptor {
                    method: Method::POST,
                    path: format!(
                        "/api/integration/{}/connect/key",
                        encode_component(&input.integration_id)
                    ),
                    query,
                    body: Some(body),
                    success_status: 204,
                    declared_statuses: &[400, 401],
                    empty: true,
                },
                options,
            )
            .await
    }

    /// `POST /api/integration/:integrationID/connect/oauth` — begin an OAuth attempt.
    pub async fn connect_oauth(
        &self,
        input: &IntegrationsConnectOauthInput,
        options: Option<&RequestOptions>,
    ) -> Result<LocationData<IntegrationAttempt>, Error> {
        let mut query = Vec::new();
        q(
            &mut query,
            "location",
            location_query(input.location.as_ref()),
        );
        let body = body! {
            "methodID" => Some(&input.method_id),
            "inputs" => Some(&input.inputs),
            "label" => input.label.as_ref(),
        };
        self.transport
            .execute(
                &RequestDescriptor {
                    method: Method::POST,
                    path: format!(
                        "/api/integration/{}/connect/oauth",
                        encode_component(&input.integration_id)
                    ),
                    query,
                    body: Some(body),
                    success_status: 200,
                    declared_statuses: &[400, 401],
                    empty: false,
                },
                options,
            )
            .await
    }

    /// `GET /api/integration/attempt/:attemptID` — poll an OAuth attempt.
    pub async fn attempt_status(
        &self,
        input: &IntegrationsAttemptStatusInput,
        options: Option<&RequestOptions>,
    ) -> Result<LocationData<IntegrationAttemptStatus>, Error> {
        let mut query = Vec::new();
        q(
            &mut query,
            "location",
            location_query(input.location.as_ref()),
        );
        self.transport
            .execute(
                &RequestDescriptor {
                    method: Method::GET,
                    path: format!(
                        "/api/integration/attempt/{}",
                        encode_component(&input.attempt_id)
                    ),
                    query,
                    body: None,
                    success_status: 200,
                    declared_statuses: &[401, 400],
                    empty: false,
                },
                options,
            )
            .await
    }

    /// `POST /api/integration/attempt/:attemptID/complete` — complete an OAuth attempt.
    pub async fn attempt_complete(
        &self,
        input: &IntegrationsAttemptCompleteInput,
        options: Option<&RequestOptions>,
    ) -> Result<(), Error> {
        let mut query = Vec::new();
        q(
            &mut query,
            "location",
            location_query(input.location.as_ref()),
        );
        let body = body! {
            "code" => input.code.as_ref(),
        };
        self.transport
            .execute(
                &RequestDescriptor {
                    method: Method::POST,
                    path: format!(
                        "/api/integration/attempt/{}/complete",
                        encode_component(&input.attempt_id)
                    ),
                    query,
                    body: Some(body),
                    success_status: 204,
                    declared_statuses: &[400, 401],
                    empty: true,
                },
                options,
            )
            .await
    }

    /// `DELETE /api/integration/attempt/:attemptID` — cancel an OAuth attempt.
    pub async fn attempt_cancel(
        &self,
        input: &IntegrationsAttemptCancelInput,
        options: Option<&RequestOptions>,
    ) -> Result<(), Error> {
        let mut query = Vec::new();
        q(
            &mut query,
            "location",
            location_query(input.location.as_ref()),
        );
        self.transport
            .execute(
                &RequestDescriptor {
                    method: Method::DELETE,
                    path: format!(
                        "/api/integration/attempt/{}",
                        encode_component(&input.attempt_id)
                    ),
                    query,
                    body: None,
                    success_status: 204,
                    declared_statuses: &[401, 400],
                    empty: true,
                },
                options,
            )
            .await
    }
}

/// `server.credential` group.
#[derive(Clone)]
pub struct CredentialGroup {
    transport: Transport,
}

impl CredentialGroup {
    /// `PATCH /api/credential/:credentialID` — update a credential label.
    pub async fn update(
        &self,
        input: &crate::types::credential::CredentialsUpdateInput,
        options: Option<&RequestOptions>,
    ) -> Result<(), Error> {
        let mut query = Vec::new();
        q(
            &mut query,
            "location",
            location_query(input.location.as_ref()),
        );
        let body = body! {
            "label" => Some(&input.label),
        };
        self.transport
            .execute(
                &RequestDescriptor {
                    method: Method::PATCH,
                    path: format!("/api/credential/{}", encode_component(&input.credential_id)),
                    query,
                    body: Some(body),
                    success_status: 204,
                    declared_statuses: &[401, 400],
                    empty: true,
                },
                options,
            )
            .await
    }

    /// `DELETE /api/credential/:credentialID` — remove a credential.
    pub async fn remove(
        &self,
        input: &crate::types::credential::CredentialsRemoveInput,
        options: Option<&RequestOptions>,
    ) -> Result<(), Error> {
        let mut query = Vec::new();
        q(
            &mut query,
            "location",
            location_query(input.location.as_ref()),
        );
        self.transport
            .execute(
                &RequestDescriptor {
                    method: Method::DELETE,
                    path: format!("/api/credential/{}", encode_component(&input.credential_id)),
                    query,
                    body: None,
                    success_status: 204,
                    declared_statuses: &[401, 400],
                    empty: true,
                },
                options,
            )
            .await
    }
}

/// `server.permission` group.
#[derive(Clone)]
pub struct PermissionGroup {
    transport: Transport,
}

impl PermissionGroup {
    /// `GET /api/permission/request` — list pending permission requests.
    pub async fn list_requests(
        &self,
        input: Option<&LocationInput>,
        options: Option<&RequestOptions>,
    ) -> Result<LocationData<Vec<PermissionRequest>>, Error> {
        let mut query = Vec::new();
        q(
            &mut query,
            "location",
            location_query(input.and_then(|input| input.location.as_ref())),
        );
        self.transport
            .execute(
                &RequestDescriptor {
                    method: Method::GET,
                    path: "/api/permission/request".into(),
                    query,
                    body: None,
                    success_status: 200,
                    declared_statuses: &[401, 400],
                    empty: false,
                },
                options,
            )
            .await
    }

    /// `GET /api/permission/saved` — list saved permissions.
    pub async fn list_saved(
        &self,
        input: Option<&PermissionsListSavedInput>,
        options: Option<&RequestOptions>,
    ) -> Result<Vec<PermissionSavedInfo>, Error> {
        let mut query = Vec::new();
        q(
            &mut query,
            "projectID",
            input.and_then(|input| opt_string(input.project_id.as_ref())),
        );
        unwrap_data(
            &self.transport,
            &RequestDescriptor {
                method: Method::GET,
                path: "/api/permission/saved".into(),
                query,
                body: None,
                success_status: 200,
                declared_statuses: &[401, 400],
                empty: false,
            },
            options,
        )
        .await
    }

    /// `DELETE /api/permission/saved/:id` — remove a saved permission.
    pub async fn remove_saved(
        &self,
        input: &PermissionsRemoveSavedInput,
        options: Option<&RequestOptions>,
    ) -> Result<(), Error> {
        self.transport
            .execute(
                &RequestDescriptor {
                    method: Method::DELETE,
                    path: format!("/api/permission/saved/{}", encode_component(&input.id)),
                    query: Vec::new(),
                    body: None,
                    success_status: 204,
                    declared_statuses: &[401, 400],
                    empty: true,
                },
                options,
            )
            .await
    }

    /// `POST /api/session/:sessionID/permission` — evaluate and create a permission request.
    pub async fn create(
        &self,
        input: &PermissionsCreateInput,
        options: Option<&RequestOptions>,
    ) -> Result<PermissionDecision, Error> {
        let body = body! {
            "id" => input.id.as_ref(),
            "action" => Some(&input.action),
            "resources" => Some(&input.resources),
            "save" => input.save.as_ref(),
            "metadata" => input.metadata.as_ref(),
            "source" => input.source.as_ref(),
            "agent" => input.agent.as_ref(),
        };
        unwrap_data(
            &self.transport,
            &RequestDescriptor {
                method: Method::POST,
                path: format!(
                    "/api/session/{}/permission",
                    encode_component(&input.session_id)
                ),
                query: Vec::new(),
                body: Some(body),
                success_status: 200,
                declared_statuses: &[404, 400, 401],
                empty: false,
            },
            options,
        )
        .await
    }

    /// `GET /api/session/:sessionID/permission` — list session permission requests.
    pub async fn list(
        &self,
        input: &PermissionsListInput,
        options: Option<&RequestOptions>,
    ) -> Result<Vec<PermissionRequest>, Error> {
        unwrap_data(
            &self.transport,
            &RequestDescriptor {
                method: Method::GET,
                path: format!(
                    "/api/session/{}/permission",
                    encode_component(&input.session_id)
                ),
                query: Vec::new(),
                body: None,
                success_status: 200,
                declared_statuses: &[404, 400, 401],
                empty: false,
            },
            options,
        )
        .await
    }

    /// `GET /api/session/:sessionID/permission/:requestID` — one permission request.
    pub async fn get(
        &self,
        input: &PermissionsGetInput,
        options: Option<&RequestOptions>,
    ) -> Result<PermissionRequest, Error> {
        unwrap_data(
            &self.transport,
            &RequestDescriptor {
                method: Method::GET,
                path: format!(
                    "/api/session/{}/permission/{}",
                    encode_component(&input.session_id),
                    encode_component(&input.request_id)
                ),
                query: Vec::new(),
                body: None,
                success_status: 200,
                declared_statuses: &[404, 400, 401],
                empty: false,
            },
            options,
        )
        .await
    }

    /// `POST /api/session/:sessionID/permission/:requestID/reply` — reply to a permission request.
    pub async fn reply(
        &self,
        input: &PermissionsReplyInput,
        options: Option<&RequestOptions>,
    ) -> Result<(), Error> {
        let body = body! {
            "reply" => Some(&input.reply),
            "message" => input.message.as_ref(),
        };
        self.transport
            .execute(
                &RequestDescriptor {
                    method: Method::POST,
                    path: format!(
                        "/api/session/{}/permission/{}/reply",
                        encode_component(&input.session_id),
                        encode_component(&input.request_id)
                    ),
                    query: Vec::new(),
                    body: Some(body),
                    success_status: 204,
                    declared_statuses: &[404, 400, 401],
                    empty: true,
                },
                options,
            )
            .await
    }
}

/// `server.fs` group.
#[derive(Clone)]
pub struct FileSystemGroup {
    transport: Transport,
}

impl FileSystemGroup {
    /// `GET /api/fs/list` — list a directory.
    pub async fn list(
        &self,
        input: Option<&FilesListInput>,
        options: Option<&RequestOptions>,
    ) -> Result<LocationData<Vec<FileSystemEntry>>, Error> {
        let mut query = Vec::new();
        if let Some(input) = input {
            q(
                &mut query,
                "location",
                location_query(input.location.as_ref()),
            );
            q(&mut query, "path", opt_string(input.path.as_ref()));
        }
        self.transport
            .execute(
                &RequestDescriptor {
                    method: Method::GET,
                    path: "/api/fs/list".into(),
                    query,
                    body: None,
                    success_status: 200,
                    declared_statuses: &[401, 400],
                    empty: false,
                },
                options,
            )
            .await
    }

    /// `GET /api/fs/find` — find files recursively.
    pub async fn find(
        &self,
        input: &FilesFindInput,
        options: Option<&RequestOptions>,
    ) -> Result<LocationData<Vec<FileSystemEntry>>, Error> {
        let mut query = Vec::new();
        q(
            &mut query,
            "location",
            location_query(input.location.as_ref()),
        );
        q(
            &mut query,
            "query",
            Some(JsonValue::from(input.query.as_str())),
        );
        q(
            &mut query,
            "type",
            input
                .kind
                .map(|kind| serde_json::to_value(kind).unwrap_or(JsonValue::Null)),
        );
        q(&mut query, "limit", input.limit.map(JsonValue::from));
        self.transport
            .execute(
                &RequestDescriptor {
                    method: Method::GET,
                    path: "/api/fs/find".into(),
                    query,
                    body: None,
                    success_status: 200,
                    declared_statuses: &[401, 400],
                    empty: false,
                },
                options,
            )
            .await
    }
}

/// `server.command` group.
#[derive(Clone)]
pub struct CommandGroup {
    transport: Transport,
}

impl CommandGroup {
    /// `GET /api/command` — list commands.
    pub async fn list(
        &self,
        input: Option<&LocationInput>,
        options: Option<&RequestOptions>,
    ) -> Result<LocationData<Vec<CommandInfo>>, Error> {
        let mut query = Vec::new();
        q(
            &mut query,
            "location",
            location_query(input.and_then(|input| input.location.as_ref())),
        );
        self.transport
            .execute(
                &RequestDescriptor {
                    method: Method::GET,
                    path: "/api/command".into(),
                    query,
                    body: None,
                    success_status: 200,
                    declared_statuses: &[401, 400],
                    empty: false,
                },
                options,
            )
            .await
    }
}

/// `server.skill` group.
#[derive(Clone)]
pub struct SkillGroup {
    transport: Transport,
}

impl SkillGroup {
    /// `GET /api/skill` — list skills.
    pub async fn list(
        &self,
        input: Option<&LocationInput>,
        options: Option<&RequestOptions>,
    ) -> Result<LocationData<Vec<SkillInfo>>, Error> {
        let mut query = Vec::new();
        q(
            &mut query,
            "location",
            location_query(input.and_then(|input| input.location.as_ref())),
        );
        self.transport
            .execute(
                &RequestDescriptor {
                    method: Method::GET,
                    path: "/api/skill".into(),
                    query,
                    body: None,
                    success_status: 200,
                    declared_statuses: &[401, 400],
                    empty: false,
                },
                options,
            )
            .await
    }
}

/// `server.event` group.
#[derive(Clone)]
pub struct EventGroup {
    transport: Transport,
}

impl EventGroup {
    /// `GET /api/event` — subscribe to server events.
    pub fn subscribe(
        &self,
        options: Option<RequestOptions>,
    ) -> std::pin::Pin<
        Box<dyn Stream<Item = Result<crate::types::event::OpenCodeEvent, Error>> + Send + 'static>,
    > {
        sse_stream(
            self.transport.clone(),
            RequestDescriptor {
                method: Method::GET,
                path: "/api/event".into(),
                query: Vec::new(),
                body: None,
                success_status: 200,
                declared_statuses: &[401, 400],
                empty: false,
            },
            options,
        )
    }
}

/// `server.pty` group.
#[derive(Clone)]
pub struct PtyGroup {
    transport: Transport,
}

impl PtyGroup {
    /// `GET /api/pty` — list PTY sessions.
    pub async fn list(
        &self,
        input: Option<&LocationInput>,
        options: Option<&RequestOptions>,
    ) -> Result<LocationData<Vec<PtyInfo>>, Error> {
        let mut query = Vec::new();
        q(
            &mut query,
            "location",
            location_query(input.and_then(|input| input.location.as_ref())),
        );
        self.transport
            .execute(
                &RequestDescriptor {
                    method: Method::GET,
                    path: "/api/pty".into(),
                    query,
                    body: None,
                    success_status: 200,
                    declared_statuses: &[401, 400],
                    empty: false,
                },
                options,
            )
            .await
    }

    /// `POST /api/pty` — create a PTY session.
    pub async fn create(
        &self,
        input: Option<&PtyCreateInput>,
        options: Option<&RequestOptions>,
    ) -> Result<LocationData<PtyInfo>, Error> {
        let mut query = Vec::new();
        if let Some(input) = input {
            q(
                &mut query,
                "location",
                location_query(input.location.as_ref()),
            );
        }
        let body = match input {
            Some(input) => body! {
                "command" => input.command.as_ref(),
                "args" => input.args.as_ref(),
                "cwd" => input.cwd.as_ref(),
                "title" => input.title.as_ref(),
                "env" => input.env.as_ref(),
            },
            None => JsonValue::Object(serde_json::Map::new()),
        };
        self.transport
            .execute(
                &RequestDescriptor {
                    method: Method::POST,
                    path: "/api/pty".into(),
                    query,
                    body: Some(body),
                    success_status: 200,
                    declared_statuses: &[401, 400],
                    empty: false,
                },
                options,
            )
            .await
    }

    /// `GET /api/pty/:ptyID` — get one PTY session.
    pub async fn get(
        &self,
        input: &PtyGetInput,
        options: Option<&RequestOptions>,
    ) -> Result<LocationData<PtyInfo>, Error> {
        let mut query = Vec::new();
        q(
            &mut query,
            "location",
            location_query(input.location.as_ref()),
        );
        self.transport
            .execute(
                &RequestDescriptor {
                    method: Method::GET,
                    path: format!("/api/pty/{}", encode_component(&input.pty_id)),
                    query,
                    body: None,
                    success_status: 200,
                    declared_statuses: &[404, 401, 400],
                    empty: false,
                },
                options,
            )
            .await
    }

    /// `PUT /api/pty/:ptyID` — update a PTY session.
    pub async fn update(
        &self,
        input: &PtyUpdateInput,
        options: Option<&RequestOptions>,
    ) -> Result<LocationData<PtyInfo>, Error> {
        let mut query = Vec::new();
        q(
            &mut query,
            "location",
            location_query(input.location.as_ref()),
        );
        let body = body! {
            "title" => input.title.as_ref(),
            "size" => input.size.as_ref(),
        };
        self.transport
            .execute(
                &RequestDescriptor {
                    method: Method::PUT,
                    path: format!("/api/pty/{}", encode_component(&input.pty_id)),
                    query,
                    body: Some(body),
                    success_status: 200,
                    declared_statuses: &[404, 401, 400],
                    empty: false,
                },
                options,
            )
            .await
    }

    /// `DELETE /api/pty/:ptyID` — remove a PTY session.
    pub async fn remove(
        &self,
        input: &PtyRemoveInput,
        options: Option<&RequestOptions>,
    ) -> Result<(), Error> {
        let mut query = Vec::new();
        q(
            &mut query,
            "location",
            location_query(input.location.as_ref()),
        );
        self.transport
            .execute(
                &RequestDescriptor {
                    method: Method::DELETE,
                    path: format!("/api/pty/{}", encode_component(&input.pty_id)),
                    query,
                    body: None,
                    success_status: 204,
                    declared_statuses: &[404, 401, 400],
                    empty: true,
                },
                options,
            )
            .await
    }
}

/// `server.question` group.
#[derive(Clone)]
pub struct QuestionGroup {
    transport: Transport,
}

impl QuestionGroup {
    /// `GET /api/question/request` — list pending question requests.
    pub async fn list_requests(
        &self,
        input: Option<&LocationInput>,
        options: Option<&RequestOptions>,
    ) -> Result<LocationData<Vec<QuestionRequest>>, Error> {
        let mut query = Vec::new();
        q(
            &mut query,
            "location",
            location_query(input.and_then(|input| input.location.as_ref())),
        );
        self.transport
            .execute(
                &RequestDescriptor {
                    method: Method::GET,
                    path: "/api/question/request".into(),
                    query,
                    body: None,
                    success_status: 200,
                    declared_statuses: &[401, 400],
                    empty: false,
                },
                options,
            )
            .await
    }

    /// `GET /api/session/:sessionID/question` — list session question requests.
    pub async fn list(
        &self,
        input: &QuestionsListInput,
        options: Option<&RequestOptions>,
    ) -> Result<Vec<QuestionRequest>, Error> {
        unwrap_data(
            &self.transport,
            &RequestDescriptor {
                method: Method::GET,
                path: format!(
                    "/api/session/{}/question",
                    encode_component(&input.session_id)
                ),
                query: Vec::new(),
                body: None,
                success_status: 200,
                declared_statuses: &[404, 400, 401],
                empty: false,
            },
            options,
        )
        .await
    }

    /// `POST /api/session/:sessionID/question/:requestID/reply` — answer a question.
    pub async fn reply(
        &self,
        input: &QuestionsReplyInput,
        options: Option<&RequestOptions>,
    ) -> Result<(), Error> {
        let body = serde_json::to_value(QuestionReply {
            answers: input.answers.clone(),
        })
        .unwrap_or(JsonValue::Null);
        self.transport
            .execute(
                &RequestDescriptor {
                    method: Method::POST,
                    path: format!(
                        "/api/session/{}/question/{}/reply",
                        encode_component(&input.session_id),
                        encode_component(&input.request_id)
                    ),
                    query: Vec::new(),
                    body: Some(body),
                    success_status: 204,
                    declared_statuses: &[404, 400, 401],
                    empty: true,
                },
                options,
            )
            .await
    }

    /// `POST /api/session/:sessionID/question/:requestID/reject` — reject a question.
    pub async fn reject(
        &self,
        input: &QuestionsRejectInput,
        options: Option<&RequestOptions>,
    ) -> Result<(), Error> {
        self.transport
            .execute(
                &RequestDescriptor {
                    method: Method::POST,
                    path: format!(
                        "/api/session/{}/question/{}/reject",
                        encode_component(&input.session_id),
                        encode_component(&input.request_id)
                    ),
                    query: Vec::new(),
                    body: None,
                    success_status: 204,
                    declared_statuses: &[404, 400, 401],
                    empty: true,
                },
                options,
            )
            .await
    }
}

/// `server.reference` group.
#[derive(Clone)]
pub struct ReferenceGroup {
    transport: Transport,
}

impl ReferenceGroup {
    /// `GET /api/reference` — list references.
    pub async fn list(
        &self,
        input: Option<&LocationInput>,
        options: Option<&RequestOptions>,
    ) -> Result<LocationData<Vec<ReferenceInfo>>, Error> {
        let mut query = Vec::new();
        q(
            &mut query,
            "location",
            location_query(input.and_then(|input| input.location.as_ref())),
        );
        self.transport
            .execute(
                &RequestDescriptor {
                    method: Method::GET,
                    path: "/api/reference".into(),
                    query,
                    body: None,
                    success_status: 200,
                    declared_statuses: &[401, 400],
                    empty: false,
                },
                options,
            )
            .await
    }
}

/// `server.projectCopy` group.
#[derive(Clone)]
pub struct ProjectCopyGroup {
    transport: Transport,
}

impl ProjectCopyGroup {
    /// `POST /experimental/project/:projectID/copy` — create a project copy.
    pub async fn create(
        &self,
        input: &ProjectCopyCreateInput,
        options: Option<&RequestOptions>,
    ) -> Result<ProjectCopy, Error> {
        let mut query = Vec::new();
        q(
            &mut query,
            "location",
            location_query(input.location.as_ref()),
        );
        let body = body! {
            "strategy" => Some(&input.strategy),
            "directory" => Some(&input.directory),
            "name" => input.name.as_ref(),
        };
        self.transport
            .execute(
                &RequestDescriptor {
                    method: Method::POST,
                    path: format!(
                        "/experimental/project/{}/copy",
                        encode_component(&input.project_id)
                    ),
                    query,
                    body: Some(body),
                    success_status: 200,
                    declared_statuses: &[400, 401],
                    empty: false,
                },
                options,
            )
            .await
    }

    /// `DELETE /experimental/project/:projectID/copy` — remove a project copy.
    pub async fn remove(
        &self,
        input: &ProjectCopyRemoveInput,
        options: Option<&RequestOptions>,
    ) -> Result<(), Error> {
        let mut query = Vec::new();
        q(
            &mut query,
            "location",
            location_query(input.location.as_ref()),
        );
        let body = body! {
            "directory" => Some(&input.directory),
            "force" => Some(input.force),
        };
        self.transport
            .execute(
                &RequestDescriptor {
                    method: Method::DELETE,
                    path: format!(
                        "/experimental/project/{}/copy",
                        encode_component(&input.project_id)
                    ),
                    query,
                    body: Some(body),
                    success_status: 204,
                    declared_statuses: &[400, 401],
                    empty: true,
                },
                options,
            )
            .await
    }

    /// `POST /experimental/project/:projectID/copy/refresh` — refresh a project copy.
    pub async fn refresh(
        &self,
        input: &ProjectCopyRefreshInput,
        options: Option<&RequestOptions>,
    ) -> Result<(), Error> {
        let mut query = Vec::new();
        q(
            &mut query,
            "location",
            location_query(input.location.as_ref()),
        );
        self.transport
            .execute(
                &RequestDescriptor {
                    method: Method::POST,
                    path: format!(
                        "/experimental/project/{}/copy/refresh",
                        encode_component(&input.project_id)
                    ),
                    query,
                    body: None,
                    success_status: 204,
                    declared_statuses: &[400, 401],
                    empty: true,
                },
                options,
            )
            .await
    }
}
