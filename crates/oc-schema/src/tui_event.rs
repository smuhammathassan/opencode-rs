//! From reference/packages/schema/src/tui-event.ts

use crate::define_event;
use crate::event::Definition;
use crate::schema::PositiveInt;
use crate::session_id::SessionID;

/// `TuiEvent.ToastShow.variant`.
#[derive(serde::Serialize, serde::Deserialize, Clone, Debug, PartialEq, Eq)]
pub enum ToastVariant {
    #[serde(rename = "info")]
    Info,
    #[serde(rename = "success")]
    Success,
    #[serde(rename = "warning")]
    Warning,
    #[serde(rename = "error")]
    Error,
}

const DEFAULT_TOAST_DURATION: u64 = 5000;

fn default_duration() -> PositiveInt {
    DEFAULT_TOAST_DURATION
}

define_event! {
    /// `tui.prompt.append`.
    pub struct PromptAppend {
        tag: PromptAppendTag,
        r#type: "tui.prompt.append",
        data: PromptAppendData,
    }
}

define_event! {
    /// `tui.command.execute`.
    pub struct CommandExecute {
        tag: CommandExecuteTag,
        r#type: "tui.command.execute",
        data: CommandExecuteData,
    }
}

define_event! {
    /// `tui.toast.show`.
    pub struct ToastShow {
        tag: ToastShowTag,
        r#type: "tui.toast.show",
        data: ToastShowData,
    }
}

define_event! {
    /// `tui.session.select`.
    pub struct SessionSelect {
        tag: SessionSelectTag,
        r#type: "tui.session.select",
        data: SessionSelectData,
    }
}

/// Payload of `tui.prompt.append`.
#[derive(serde::Serialize, serde::Deserialize, Clone, Debug, PartialEq)]
pub struct PromptAppendData {
    pub text: String,
}

/// Payload of `tui.command.execute` — a literal command or any string.
#[derive(serde::Serialize, serde::Deserialize, Clone, Debug, PartialEq)]
pub struct CommandExecuteData {
    pub command: String,
}

/// Payload of `tui.toast.show`.
#[derive(serde::Serialize, serde::Deserialize, Clone, Debug, PartialEq)]
pub struct ToastShowData {
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub title: Option<String>,
    pub message: String,
    pub variant: ToastVariant,
    #[serde(default = "default_duration")]
    pub duration: PositiveInt,
}

/// Payload of `tui.session.select`.
#[derive(serde::Serialize, serde::Deserialize, Clone, Debug, PartialEq)]
pub struct SessionSelectData {
    #[serde(rename = "sessionID")]
    pub session_id: SessionID,
}

/// `TuiEvent.Definitions`.
pub const DEFINITIONS: &[Definition] = &[
    Definition {
        r#type: "tui.prompt.append",
        durable: None,
    },
    Definition {
        r#type: "tui.command.execute",
        durable: None,
    },
    Definition {
        r#type: "tui.toast.show",
        durable: None,
    },
    Definition {
        r#type: "tui.session.select",
        durable: None,
    },
];
