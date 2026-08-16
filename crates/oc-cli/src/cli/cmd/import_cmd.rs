//! `opencode import <file>`
//! From reference/packages/opencode/src/cli/cmd/import.ts.

use crate::cli::args::{Cli, ImportArgs};
use anyhow::{bail, Context};
use oc_database::tables::{json_columns, MessageRow, PartRow, ProjectRow, SessionRow};
use oc_database::Database;
use serde_json::Value;

/// Keep a malformed or unexpectedly large public share response from being
/// buffered without limit by the CLI. Share transcripts are JSON, so ten MiB
/// leaves room for normal sessions while making the limit explicit.
const MAX_SHARE_PAYLOAD_BYTES: u64 = 10 * 1024 * 1024;

#[derive(Debug, Clone)]
struct ShareFetchResponse {
    status: u16,
    content_type: Option<String>,
    content_length: Option<u64>,
    body: Vec<u8>,
}

/// Extract a share id from a share URL like `https://opncd.ai/share/abc123`.
/// Mirrors `parseShareUrl` in import.ts.
pub fn parse_share_url(url: &str) -> Option<&str> {
    let prefix = url
        .strip_prefix("http://")
        .or_else(|| url.strip_prefix("https://"))?;
    let slug = prefix.splitn(2, '/').nth(1)?;
    slug.strip_prefix("share/").filter(|s| {
        s.chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    })
}

/// Mirrors `formatImportFileError` in import.ts.
pub fn format_import_file_error(file: &str, err: &std::io::Error) -> String {
    match err.kind() {
        std::io::ErrorKind::NotFound => format!("File not found: {file}"),
        std::io::ErrorKind::PermissionDenied => {
            "Failed to read file: Permission denied".to_string()
        }
        _ => format!("Failed to read file: {err}"),
    }
}

struct ImportData {
    session: SessionRow,
    messages: Vec<Value>,
    parts: Vec<Value>,
}

pub async fn run(_cli: &Cli, args: &ImportArgs) -> anyhow::Result<i32> {
    let file = &args.file;
    let document: Value = if file.starts_with("http://") || file.starts_with("https://") {
        if parse_share_url(file).is_none() {
            println!("Invalid URL format. Expected: <baseUrl>/share/<slug>");
            return Ok(0);
        }
        fetch_share_payload(file).await?
    } else {
        let bytes = tokio::fs::read(file)
            .await
            .map_err(|err| anyhow::anyhow!("{}", format_import_file_error(file, &err)))?;
        serde_json::from_slice(&bytes)?
    };
    let import = decode_import(&document)?;
    let database = Database::open(oc_database::database::path())?;
    let (message_count, part_count) = persist_import(&database, &import)?;
    println!(
        "Imported session: {} ({} messages, {} parts)",
        import.session.id, message_count, part_count
    );
    Ok(0)
}

/// Fetch a public share transcript from the account-backed endpoint, with the
/// legacy endpoint as the compatibility fallback. This mirrors the reference
/// import command's `/api/shares/:id/data` then `/api/share/:id/data` order.
async fn fetch_share_payload(share_url: &str) -> anyhow::Result<Value> {
    let client = reqwest::Client::new();
    fetch_share_payload_with(share_url, move |url| {
        let client = client.clone();
        async move {
            let response = client
                .get(&url)
                .send()
                .await
                .with_context(|| format!("Failed to fetch share data from {url}"))?;
            let status = response.status().as_u16();
            let content_type = response
                .headers()
                .get(reqwest::header::CONTENT_TYPE)
                .and_then(|value| value.to_str().ok())
                .map(str::to_string);
            let content_length = response.content_length();
            let body = response
                .bytes()
                .await
                .with_context(|| format!("Failed to read share data from {url}"))?
                .to_vec();
            Ok(ShareFetchResponse {
                status,
                content_type,
                content_length,
                body,
            })
        }
    })
    .await
}

/// Injectable request seam for remote import tests. A non-successful account
/// endpoint response deliberately falls back to the legacy endpoint; malformed
/// successful responses do not, matching the upstream command.
async fn fetch_share_payload_with<F, Fut>(share_url: &str, mut fetch: F) -> anyhow::Result<Value>
where
    F: FnMut(String) -> Fut,
    Fut: std::future::Future<Output = anyhow::Result<ShareFetchResponse>>,
{
    let slug = parse_share_url(share_url)
        .ok_or_else(|| anyhow::anyhow!("Invalid URL format. Expected: <baseUrl>/share/<slug>"))?;
    let url = url::Url::parse(share_url)
        .context("Invalid URL format. Expected: <baseUrl>/share/<slug>")?;
    let origin = url.origin().ascii_serialization();
    let account_url = format!("{origin}/api/shares/{slug}/data");
    let legacy_url = format!("{origin}/api/share/{slug}/data");

    let account = fetch(account_url).await?;
    if is_success(account.status) {
        return decode_share_response(account);
    }

    let legacy = fetch(legacy_url).await?;
    if !is_success(legacy.status) {
        bail!(
            "Failed to fetch share data: HTTP {} (account endpoint returned HTTP {})",
            legacy.status,
            account.status
        );
    }
    decode_share_response(legacy)
}

fn is_success(status: u16) -> bool {
    (200..300).contains(&status)
}

fn decode_share_response(response: ShareFetchResponse) -> anyhow::Result<Value> {
    if let Some(length) = response.content_length {
        if length > MAX_SHARE_PAYLOAD_BYTES {
            bail!(
                "Share data is too large: {length} bytes (maximum {MAX_SHARE_PAYLOAD_BYTES} bytes)"
            );
        }
    }
    if response.body.len() as u64 > MAX_SHARE_PAYLOAD_BYTES {
        bail!(
            "Share data is too large: {} bytes (maximum {MAX_SHARE_PAYLOAD_BYTES} bytes)",
            response.body.len()
        );
    }
    if let Some(content_type) = response.content_type {
        let media_type = content_type
            .split(';')
            .next()
            .unwrap_or_default()
            .trim()
            .to_ascii_lowercase();
        if media_type != "application/json" && !media_type.ends_with("+json") {
            bail!("Share data was not JSON (received Content-Type: {content_type})");
        }
    }
    serde_json::from_slice(&response.body).context("Share data was not valid JSON")
}

fn decode_import(document: &Value) -> anyhow::Result<ImportData> {
    let mut session_value = document
        .get("sessionRow")
        .or_else(|| document.get("session"))
        .or_else(|| document.get("info"))
        .cloned();
    let mut messages = Vec::new();
    let mut parts = Vec::new();

    let share_records = document
        .as_array()
        .or_else(|| document.get("data").and_then(Value::as_array));
    if let Some(records) = share_records {
        for record in records {
            collect_share_record(record, &mut session_value, &mut messages, &mut parts);
        }
    } else if document.get("type").is_some() {
        collect_share_record(document, &mut session_value, &mut messages, &mut parts);
    } else {
        if let Some(items) = document.get("messages").and_then(Value::as_array) {
            for item in items {
                collect_message(item, &mut messages, &mut parts);
            }
        }
        if let Some(items) = document.get("parts").and_then(Value::as_array) {
            parts.extend(items.iter().map(payload_data).cloned());
        }
    }

    let session_value = session_value.unwrap_or_else(|| document.clone());
    let session = session_from_payload(&session_value)?;
    Ok(ImportData {
        session,
        messages,
        parts,
    })
}

fn collect_share_record(
    record: &Value,
    session: &mut Option<Value>,
    messages: &mut Vec<Value>,
    parts: &mut Vec<Value>,
) {
    let data = record.get("data").unwrap_or(record);
    match record.get("type").and_then(Value::as_str) {
        Some("session") => *session = Some(data.clone()),
        Some("message") => collect_message(data, messages, parts),
        Some("part") => parts.push(payload_data(data).clone()),
        _ => {}
    }
}

fn collect_message(value: &Value, messages: &mut Vec<Value>, parts: &mut Vec<Value>) {
    let data = payload_data(value);
    let message = data
        .get("info")
        .filter(|info| info.get("id").is_some())
        .unwrap_or(data);
    let message_id = message.get("id").and_then(Value::as_str);
    messages.push(message.clone());

    let nested_parts = value
        .get("parts")
        .and_then(Value::as_array)
        .or_else(|| data.get("parts").and_then(Value::as_array));
    if let Some(nested_parts) = nested_parts {
        for part in nested_parts {
            collect_part(part, message_id, parts);
        }
    }
}

fn collect_part(value: &Value, message_id: Option<&str>, parts: &mut Vec<Value>) {
    let mut part = payload_data(value).clone();
    if message_id_for(&part).is_none() {
        if let Some(message_id) = message_id {
            if let Some(object) = part.as_object_mut() {
                object.insert(
                    "messageID".to_string(),
                    Value::String(message_id.to_string()),
                );
            }
        }
    }
    parts.push(part);
}

fn payload_data(value: &Value) -> &Value {
    value.get("data").unwrap_or(value)
}

fn session_from_payload(value: &Value) -> anyhow::Result<SessionRow> {
    match serde_json::from_value::<SessionRow>(value.clone()) {
        Ok(row) => Ok(row),
        Err(_) => Ok(fallback_session_row(value)?),
    }
}

fn fallback_session_row(info: &Value) -> anyhow::Result<SessionRow> {
    let location = info.get("location");
    let time = info.get("time");
    let id = string_field(info, &["id"])
        .ok_or_else(|| anyhow::anyhow!("import payload does not contain a session id"))?;
    let now = chrono::Utc::now().timestamp_millis();
    let directory = location
        .and_then(|location| string_field(location, &["directory"]))
        .or_else(|| string_field(info, &["directory"]))
        .unwrap_or_else(|| ".".to_string());
    let project_id = string_field(info, &["projectID", "project_id"])
        .unwrap_or_else(|| "prj_imported".to_string());
    let time_created = integer_field(time, &["created"])
        .or_else(|| integer_field(Some(info), &["created", "time_created", "timeCreated"]))
        .unwrap_or(now);
    let time_updated = integer_field(time, &["updated"])
        .or_else(|| integer_field(Some(info), &["updated", "time_updated", "timeUpdated"]))
        .unwrap_or(time_created);
    let tokens = info.get("tokens");
    let summary = info.get("summary");
    let model = info.get("model").cloned();
    let slug = string_field(info, &["slug"])
        .unwrap_or_else(|| id.rsplit('_').next().unwrap_or(&id).to_string());

    Ok(SessionRow {
        id,
        project_id,
        workspace_id: location
            .and_then(|location| string_field(location, &["workspaceID", "workspace_id"]))
            .or_else(|| string_field(info, &["workspaceID", "workspace_id"])),
        parent_id: string_field(info, &["parentID", "parent_id"]),
        slug,
        directory,
        path: string_field(info, &["subpath", "path"]),
        title: string_field(info, &["title"]).unwrap_or_else(|| "Imported Session".to_string()),
        version: string_field(info, &["version"]).unwrap_or_else(|| crate::VERSION.to_string()),
        share_url: info
            .get("share")
            .and_then(|share| string_field(share, &["url"]))
            .or_else(|| string_field(info, &["share_url", "shareUrl"])),
        summary_additions: summary.and_then(|summary| integer_field(Some(summary), &["additions"])),
        summary_deletions: summary.and_then(|summary| integer_field(Some(summary), &["deletions"])),
        summary_files: summary.and_then(|summary| integer_field(Some(summary), &["files"])),
        summary_diffs: summary.and_then(|summary| summary.get("diffs").cloned()),
        metadata: info.get("metadata").cloned(),
        cost: info.get("cost").and_then(Value::as_f64).unwrap_or(0.0),
        tokens_input: tokens
            .and_then(|tokens| integer_field(Some(tokens), &["input"]))
            .unwrap_or(0),
        tokens_output: tokens
            .and_then(|tokens| integer_field(Some(tokens), &["output"]))
            .unwrap_or(0),
        tokens_reasoning: tokens
            .and_then(|tokens| integer_field(Some(tokens), &["reasoning"]))
            .unwrap_or(0),
        tokens_cache_read: tokens
            .and_then(|tokens| tokens.get("cache"))
            .and_then(|cache| integer_field(Some(cache), &["read"]))
            .unwrap_or(0),
        tokens_cache_write: tokens
            .and_then(|tokens| tokens.get("cache"))
            .and_then(|cache| integer_field(Some(cache), &["write"]))
            .unwrap_or(0),
        revert: info.get("revert").cloned(),
        permission: info.get("permission").cloned(),
        agent: string_field(info, &["agent"]),
        model,
        time_created,
        time_updated,
        time_compacting: time.and_then(|time| integer_field(Some(time), &["compacting"])),
        time_archived: time.and_then(|time| integer_field(Some(time), &["archived"])),
    })
}

fn persist_import(database: &Database, import: &ImportData) -> anyhow::Result<(usize, usize)> {
    let session = &import.session;
    let project = ProjectRow {
        id: session.project_id.clone(),
        worktree: session.directory.clone(),
        vcs: None,
        name: None,
        icon_url: None,
        icon_url_override: None,
        icon_color: None,
        time_created: session.time_created,
        time_updated: session.time_updated,
        time_initialized: None,
        sandboxes: Value::Array(Vec::new()),
        commands: None,
    };
    database.upsert(
        "project",
        &project,
        json_columns("project"),
        "id",
        &oc_database::Value::Text(project.id.clone()),
    )?;
    database.upsert(
        "session",
        session,
        json_columns("session"),
        "id",
        &oc_database::Value::Text(session.id.clone()),
    )?;
    let messages = import_messages(database, session, &import.messages)?;
    let parts = import_parts(database, session, &import.parts)?;
    Ok((messages, parts))
}

fn import_messages(
    database: &Database,
    session: &SessionRow,
    values: &[Value],
) -> anyhow::Result<usize> {
    let mut imported = 0;
    for value in values {
        let Some(id) = value.get("id").and_then(Value::as_str) else {
            continue;
        };
        let created = time_value(value, &["created", "time_created", "timeCreated"])
            .unwrap_or(session.time_created);
        let updated =
            time_value(value, &["updated", "time_updated", "timeUpdated"]).unwrap_or(created);
        let row = MessageRow {
            id: id.to_string(),
            session_id: session.id.clone(),
            time_created: created,
            time_updated: updated,
            data: value.clone(),
        };
        database.upsert(
            "message",
            &row,
            json_columns("message"),
            "id",
            &oc_database::Value::Text(row.id.clone()),
        )?;
        imported += 1;
    }
    Ok(imported)
}

fn import_parts(
    database: &Database,
    session: &SessionRow,
    values: &[Value],
) -> anyhow::Result<usize> {
    let mut imported = 0;
    for value in values {
        let Some(id) = value.get("id").and_then(Value::as_str) else {
            continue;
        };
        let Some(message_id) = message_id_for(value) else {
            continue;
        };
        if database.get_message(message_id, &session.id)?.is_none() {
            continue;
        }
        let created = time_value(value, &["start", "time_created", "timeCreated"])
            .unwrap_or(session.time_created);
        let updated = time_value(value, &["end", "time_updated", "timeUpdated"]).unwrap_or(created);
        let row = PartRow {
            id: id.to_string(),
            message_id: message_id.to_string(),
            session_id: session.id.clone(),
            time_created: created,
            time_updated: updated,
            data: value.clone(),
        };
        database.upsert(
            "part",
            &row,
            json_columns("part"),
            "id",
            &oc_database::Value::Text(row.id.clone()),
        )?;
        imported += 1;
    }
    Ok(imported)
}

fn message_id_for(value: &Value) -> Option<&str> {
    ["messageID", "message_id", "messageId"]
        .iter()
        .find_map(|field| value.get(*field).and_then(Value::as_str))
}

fn string_field(value: &Value, fields: &[&str]) -> Option<String> {
    fields.iter().find_map(|field| {
        value
            .get(*field)
            .and_then(Value::as_str)
            .map(str::to_string)
    })
}

fn integer_field(value: Option<&Value>, fields: &[&str]) -> Option<i64> {
    fields.iter().find_map(|field| {
        value.and_then(|value| {
            value.get(*field).and_then(|value| {
                value
                    .as_i64()
                    .or_else(|| value.as_u64().and_then(|value| i64::try_from(value).ok()))
                    .or_else(|| value.as_f64().map(|value| value as i64))
                    .or_else(|| value.as_str().and_then(|value| value.parse().ok()))
            })
        })
    })
}

fn time_value(value: &Value, fields: &[&str]) -> Option<i64> {
    integer_field(value.get("time"), fields).or_else(|| integer_field(Some(value), fields))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    fn share_payload() -> Value {
        serde_json::json!({
            "data": [
                {
                    "type": "session",
                    "data": {
                        "id": "ses_share",
                        "projectID": "prj_share",
                        "title": "Shared session",
                        "cost": 1.5,
                        "tokens": {
                            "input": 3,
                            "output": 5,
                            "reasoning": 0,
                            "cache": {"read": 1, "write": 2}
                        },
                        "time": {"created": 10, "updated": 20},
                        "location": {
                            "directory": "/tmp/shared",
                            "workspaceID": "wrk_share"
                        }
                    }
                },
                {
                    "type": "message",
                    "data": {
                        "id": "msg_share",
                        "time": {"created": 11},
                        "type": "user",
                        "parts": [{
                            "id": "part_share",
                            "type": "text",
                            "text": "hello",
                            "time": {"start": 12, "end": 13}
                        }]
                    }
                }
            ]
        })
    }

    fn session_row() -> SessionRow {
        SessionRow {
            id: "ses_export".into(),
            project_id: "prj_export".into(),
            workspace_id: None,
            parent_id: None,
            slug: "export".into(),
            directory: "/tmp/export".into(),
            path: None,
            title: "Exported session".into(),
            version: "test".into(),
            share_url: None,
            summary_additions: None,
            summary_deletions: None,
            summary_files: None,
            summary_diffs: None,
            metadata: None,
            cost: 0.0,
            tokens_input: 0,
            tokens_output: 0,
            tokens_reasoning: 0,
            tokens_cache_read: 0,
            tokens_cache_write: 0,
            revert: None,
            permission: None,
            agent: None,
            model: None,
            time_created: 1,
            time_updated: 2,
            time_compacting: None,
            time_archived: None,
        }
    }

    #[test]
    fn parses_share_urls() {
        assert_eq!(
            parse_share_url("https://opncd.ai/share/abc123"),
            Some("abc123")
        );
        assert_eq!(
            parse_share_url("https://opncd.ai/share/a_b-9"),
            Some("a_b-9")
        );
        assert_eq!(parse_share_url("https://example.com/other"), None);
        assert_eq!(parse_share_url("not-a-url"), None);
    }

    #[tokio::test]
    async fn fetches_account_share_data_from_the_share_origin() {
        let calls = Arc::new(Mutex::new(Vec::new()));
        let expected = share_payload();
        let expected_body = serde_json::to_vec(&expected).unwrap();
        let recorded_calls = calls.clone();
        let document =
            fetch_share_payload_with("https://shares.example.test/share/slug_123", move |url| {
                let calls = recorded_calls.clone();
                let body = expected_body.clone();
                async move {
                    calls.lock().unwrap().push(url);
                    Ok(ShareFetchResponse {
                        status: 200,
                        content_type: Some("application/json; charset=utf-8".into()),
                        content_length: Some(body.len() as u64),
                        body,
                    })
                }
            })
            .await
            .unwrap();

        assert_eq!(document, expected);
        assert_eq!(
            *calls.lock().unwrap(),
            vec!["https://shares.example.test/api/shares/slug_123/data"]
        );
    }

    #[tokio::test]
    async fn falls_back_to_legacy_share_data_after_account_endpoint_failure() {
        let calls = Arc::new(Mutex::new(Vec::new()));
        let expected = share_payload();
        let expected_body = serde_json::to_vec(&expected).unwrap();
        let recorded_calls = calls.clone();
        let document = fetch_share_payload_with("http://127.0.0.1:4096/share/legacy", move |url| {
            let calls = recorded_calls.clone();
            let body = expected_body.clone();
            async move {
                calls.lock().unwrap().push(url.clone());
                if url.contains("/api/shares/") {
                    Ok(ShareFetchResponse {
                        status: 404,
                        content_type: Some("application/json".into()),
                        content_length: Some(0),
                        body: Vec::new(),
                    })
                } else {
                    Ok(ShareFetchResponse {
                        status: 200,
                        content_type: Some("application/problem+json".into()),
                        content_length: Some(body.len() as u64),
                        body,
                    })
                }
            }
        })
        .await
        .unwrap();

        assert_eq!(document, expected);
        assert_eq!(
            *calls.lock().unwrap(),
            vec![
                "http://127.0.0.1:4096/api/shares/legacy/data",
                "http://127.0.0.1:4096/api/share/legacy/data",
            ]
        );
    }

    #[test]
    fn validates_share_content_type_size_and_json() {
        let wrong_type = decode_share_response(ShareFetchResponse {
            status: 200,
            content_type: Some("text/html".into()),
            content_length: Some(2),
            body: b"{}".to_vec(),
        })
        .unwrap_err();
        assert!(wrong_type.to_string().contains("Content-Type"));

        let too_large = decode_share_response(ShareFetchResponse {
            status: 200,
            content_type: Some("application/json".into()),
            content_length: Some(MAX_SHARE_PAYLOAD_BYTES + 1),
            body: Vec::new(),
        })
        .unwrap_err();
        assert!(too_large.to_string().contains("too large"));

        let invalid_json = decode_share_response(ShareFetchResponse {
            status: 200,
            content_type: None,
            content_length: None,
            body: b"not json".to_vec(),
        })
        .unwrap_err();
        assert!(invalid_json.to_string().contains("not valid JSON"));
    }

    #[test]
    fn decodes_share_payload_records_and_nested_parts() {
        let import = decode_import(&share_payload()).unwrap();

        assert_eq!(import.session.id, "ses_share");
        assert_eq!(import.session.project_id, "prj_share");
        assert_eq!(import.session.directory, "/tmp/shared");
        assert_eq!(import.messages.len(), 1);
        assert_eq!(import.parts.len(), 1);
        assert_eq!(import.parts[0]["messageID"], "msg_share");
    }

    #[test]
    fn persists_export_and_share_payloads_idempotently() {
        let database = Database::open_memory().unwrap();
        let export_session = session_row();
        let export_document = serde_json::json!({
            "format": "opencode.session",
            "version": 1,
            "sessionRow": export_session,
            "messages": [{"id": "msg_export", "time": {"created": 3}}],
            "parts": [{
                "id": "part_export",
                "messageID": "msg_export",
                "type": "text",
                "time": {"start": 4, "end": 5}
            }]
        });
        let export_import = decode_import(&export_document).unwrap();
        assert_eq!(persist_import(&database, &export_import).unwrap(), (1, 1));

        let share_import = decode_import(&share_payload()).unwrap();
        assert_eq!(persist_import(&database, &share_import).unwrap(), (1, 1));
        assert_eq!(persist_import(&database, &share_import).unwrap(), (1, 1));

        let sessions = database
            .list::<SessionRow>("session", json_columns("session"))
            .unwrap();
        let messages = database
            .list::<MessageRow>("message", json_columns("message"))
            .unwrap();
        let parts = database
            .list::<PartRow>("part", json_columns("part"))
            .unwrap();
        assert_eq!(sessions.len(), 2);
        assert_eq!(messages.len(), 2);
        assert_eq!(parts.len(), 2);
        assert!(sessions.iter().any(|session| session.id == "ses_share"));
        assert!(messages.iter().any(|message| message.id == "msg_share"));
        assert!(parts.iter().any(|part| part.id == "part_share"));
    }
}
