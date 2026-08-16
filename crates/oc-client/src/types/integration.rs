//! Integration types.
//! From reference/packages/schema/src/integration.ts.
//!
//! Canonical home: `oc_schema::integration`.

use crate::types::location::LocationQueryRef;

// Re-export shim: `oc_schema::integration` is the single canonical definition.
pub use oc_schema::integration::{
    Attempt as IntegrationAttempt, AttemptMode as IntegrationAttemptMode,
    AttemptStatus as IntegrationAttemptStatus, AttemptTime as IntegrationAttemptTime,
    Info as IntegrationInfo, Inputs as IntegrationInputs, Method as IntegrationMethod,
    Prompt as IntegrationPrompt, Ref as IntegrationRef, SelectOption as IntegrationOption,
    When as IntegrationWhen, WhenOp as IntegrationWhenOp,
};

/// `IntegrationsGetInput`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct IntegrationsGetInput {
    pub integration_id: String,
    pub location: Option<LocationQueryRef>,
}

/// `IntegrationsConnectKeyInput`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct IntegrationsConnectKeyInput {
    pub integration_id: String,
    pub location: Option<LocationQueryRef>,
    pub key: String,
    pub label: Option<String>,
}

/// `IntegrationsConnectOauthInput`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct IntegrationsConnectOauthInput {
    pub integration_id: String,
    pub location: Option<LocationQueryRef>,
    pub method_id: String,
    pub inputs: IntegrationInputs,
    pub label: Option<String>,
}

/// `IntegrationsAttemptStatusInput`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct IntegrationsAttemptStatusInput {
    pub attempt_id: String,
    pub location: Option<LocationQueryRef>,
}

/// `IntegrationsAttemptCompleteInput`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct IntegrationsAttemptCompleteInput {
    pub attempt_id: String,
    pub location: Option<LocationQueryRef>,
    pub code: Option<String>,
}

/// `IntegrationsAttemptCancelInput`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct IntegrationsAttemptCancelInput {
    pub attempt_id: String,
    pub location: Option<LocationQueryRef>,
}
