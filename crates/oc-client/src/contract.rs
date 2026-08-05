//! Client contract metadata.
//! Mirrors `reference/packages/client/src/contract.ts`: the group and endpoint
//! name mappings used by the HTTP API code generator and the set of endpoints
//! omitted from the generated client.

/// The `server.<group>` -> client namespace mapping. Mirrors `groupNames` in
/// `reference/packages/client/src/contract.ts`.
pub const GROUP_NAMES: &[(&str, &str)] = &[
    ("server.health", "health"),
    ("server.location", "location"),
    ("server.agent", "agents"),
    ("server.session", "sessions"),
    ("server.message", "messages"),
    ("server.model", "models"),
    ("server.provider", "providers"),
    ("server.integration", "integrations"),
    ("server.credential", "credentials"),
    ("server.permission", "permissions"),
    ("server.fs", "files"),
    ("server.command", "commands"),
    ("server.skill", "skills"),
    ("server.event", "events"),
    ("server.pty", "ptys"),
    ("server.question", "questions"),
    ("server.reference", "references"),
    ("server.projectCopy", "projectCopies"),
];

/// The endpoint-name overrides used by the generator. Mirrors `endpointNames` in
/// `reference/packages/client/src/contract.ts`.
pub const ENDPOINT_NAMES: &[(&str, &str)] = &[
    ("session.messages", "list"),
    ("integration.connect.key", "connectKey"),
    ("integration.connect.oauth", "connectOauth"),
    ("integration.attempt.status", "attemptStatus"),
    ("integration.attempt.complete", "attemptComplete"),
    ("integration.attempt.cancel", "attemptCancel"),
    ("permission.request.list", "listRequests"),
    ("permission.saved.list", "listSaved"),
    ("permission.saved.remove", "removeSaved"),
    ("question.request.list", "listRequests"),
];

/// Endpoints omitted from the generated client. Mirrors `omitEndpoints` in
/// `reference/packages/client/src/contract.ts`. `fs.read` serves raw bytes,
/// `pty.connect` is a WebSocket upgrade, and `pty.connectToken` is consumed by
/// the raw PTY connect handler.
pub const OMIT_ENDPOINTS: &[&str] = &["fs.read", "pty.connect", "pty.connectToken"];
