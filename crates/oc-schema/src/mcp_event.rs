//! From reference/packages/schema/src/mcp-event.ts

use crate::define_event;
use crate::event::Definition;

define_event! {
    /// `mcp.tools.changed`.
    pub struct ToolsChanged {
        tag: ToolsChangedTag,
        r#type: "mcp.tools.changed",
        data: ToolsChangedData,
    }
}

define_event! {
    /// `mcp.browser.open.failed`.
    pub struct BrowserOpenFailed {
        tag: BrowserOpenFailedTag,
        r#type: "mcp.browser.open.failed",
        data: BrowserOpenFailedData,
    }
}

/// Payload of `mcp.tools.changed`.
#[derive(serde::Serialize, serde::Deserialize, Clone, Debug, PartialEq)]
pub struct ToolsChangedData {
    pub server: String,
}

/// Payload of `mcp.browser.open.failed`.
#[derive(serde::Serialize, serde::Deserialize, Clone, Debug, PartialEq)]
pub struct BrowserOpenFailedData {
    #[serde(rename = "mcpName")]
    pub mcp_name: String,
    pub url: String,
}

/// `McpEvent.Definitions`.
pub const DEFINITIONS: &[Definition] = &[
    Definition {
        r#type: "mcp.tools.changed",
        durable: None,
    },
    Definition {
        r#type: "mcp.browser.open.failed",
        durable: None,
    },
];
