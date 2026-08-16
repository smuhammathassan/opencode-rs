//! `opencode export [sessionID]`
//! From reference/packages/opencode/src/cli/cmd/export.ts.

use crate::cli::args::{Cli, ExportArgs};
use oc_database::tables::{json_columns, MessageRow, PartRow, SessionRow};
use oc_database::Database;

pub async fn run(_cli: &Cli, args: &ExportArgs) -> anyhow::Result<i32> {
    let database = Database::open(oc_database::database::path())?;
    let sessions: Vec<SessionRow> =
        database.list::<SessionRow>("session", json_columns("session"))?;
    let session = if let Some(session_id) = &args.session_id {
        sessions
            .into_iter()
            .find(|session| &session.id == session_id)
            .ok_or_else(|| anyhow::anyhow!("Session not found: {session_id}"))?
    } else {
        sessions
            .into_iter()
            .max_by_key(|session| session.time_updated)
            .ok_or_else(|| anyhow::anyhow!("No sessions found"))?
    };
    let messages: Vec<MessageRow> = database
        .list::<MessageRow>("message", json_columns("message"))?
        .into_iter()
        .filter(|message| message.session_id == session.id)
        .collect();
    let parts: Vec<PartRow> = database
        .list::<PartRow>("part", json_columns("part"))?
        .into_iter()
        .filter(|part| part.session_id == session.id)
        .collect();
    let mut document = serde_json::json!({
        "format": "opencode.session",
        "version": 1,
        "sessionRow": session,
        "messages": messages.into_iter().map(|message| message.data).collect::<Vec<_>>(),
        "parts": parts.into_iter().map(|part| part.data).collect::<Vec<_>>(),
    });
    if args.sanitize {
        sanitize(&mut document);
    }
    println!("{}", serde_json::to_string_pretty(&document)?);
    Ok(0)
}

fn sanitize(value: &mut serde_json::Value) {
    match value {
        serde_json::Value::Object(object) => {
            for (key, child) in object.iter_mut() {
                if matches!(key.as_str(), "text" | "output" | "uri" | "path") {
                    *child = serde_json::Value::String("[REDACTED]".into());
                } else {
                    sanitize(child);
                }
            }
        }
        serde_json::Value::Array(items) => {
            for item in items {
                sanitize(item);
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::sanitize;

    #[test]
    fn sanitize_redacts_transcript_and_file_fields_recursively() {
        let mut value = serde_json::json!({
            "text": "secret prompt",
            "nested": [{"output": "tool secret", "path": "/private/file.rs"}],
            "cost": 1.25
        });
        sanitize(&mut value);
        assert_eq!(value["text"], "[REDACTED]");
        assert_eq!(value["nested"][0]["output"], "[REDACTED]");
        assert_eq!(value["nested"][0]["path"], "[REDACTED]");
        assert_eq!(value["cost"], 1.25);
    }
}
