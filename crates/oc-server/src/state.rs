//! Shared application state backing the handlers.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::Value;
use tokio::sync::RwLock;

use crate::auth::AuthConfig;
use crate::cors::CorsOptions;
use crate::event::EventBus;
use crate::location::Location;
use crate::schema::SessionInfo;

/// A stored PTY session. From reference/packages/schema/src/pty.ts (`Pty.Info`).
#[derive(Debug, Clone)]
pub struct PtyRecord {
    pub info: Value,
    pub running: bool,
    pub buffer: Vec<u8>,
}

/// In-memory projection store.
#[derive(Debug, Default)]
pub struct Stores {
    pub sessions: HashMap<String, SessionRecord>,
    pub questions: HashMap<String, Value>,
    pub permissions: HashMap<String, Value>,
    pub pty: HashMap<String, PtyRecord>,
    pub config: Value,
}

impl Stores {
    pub fn new(config: Value) -> Self {
        Stores {
            config,
            ..Default::default()
        }
    }
}

#[derive(Debug, Clone)]
pub struct SessionRecord {
    pub info: SessionInfo,
    pub messages: Vec<Value>,
    pub active: bool,
}

/// Global server state shared by all handlers via axum's `State` extractor.
#[derive(Debug, Clone)]
pub struct AppState {
    pub stores: Arc<RwLock<Stores>>,
    pub events: EventBus,
    pub auth: Arc<AuthConfig>,
    pub cors: Arc<CorsOptions>,
    pub location: Arc<Location>,
}

impl AppState {
    pub fn new(auth: AuthConfig, cors: CorsOptions, location: Location) -> Self {
        AppState {
            stores: Arc::new(RwLock::new(Stores::new(default_config()))),
            events: EventBus::new(256),
            auth: Arc::new(auth),
            cors: Arc::new(cors),
            location: Arc::new(location),
        }
    }
}

/// Empty ConfigV1.Info shape. From reference/packages/core/src/v1/config/config.ts.
pub fn default_config() -> Value {
    serde_json::json!({})
}

pub fn now_millis() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

pub fn timestamp() -> i64 {
    now_millis()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn state_bootstraps() {
        let state = AppState::new(
            AuthConfig::default(),
            CorsOptions::default(),
            Location::default_location(),
        );
        assert_eq!(state.auth.username, "opencode");
        assert!(!state.auth.required());
    }
}
