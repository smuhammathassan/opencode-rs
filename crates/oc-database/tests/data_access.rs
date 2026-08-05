//! Data access helper round-trip tests against a fully migrated in-memory
//! database. Mirrors `packages/opencode/src/session/message-v2.ts` and
//! `session.ts` query shapes.

use oc_database::tables::{MessageRow, PartRow, ProjectRow, SessionRow, TodoRow};
use oc_database::{Database, Result};

fn project() -> ProjectRow {
    ProjectRow {
        id: "global".to_string(),
        worktree: "/".to_string(),
        vcs: None,
        name: Some("global".to_string()),
        icon_url: None,
        icon_url_override: None,
        icon_color: None,
        time_created: 1,
        time_updated: 1,
        time_initialized: None,
        sandboxes: serde_json::json!([]),
        commands: None,
    }
}

fn session(id: &str, title: &str) -> SessionRow {
    SessionRow {
        id: id.to_string(),
        project_id: "global".to_string(),
        workspace_id: None,
        parent_id: None,
        slug: id.to_string(),
        directory: "/".to_string(),
        path: None,
        title: title.to_string(),
        version: "v1".to_string(),
        share_url: None,
        summary_additions: None,
        summary_deletions: None,
        summary_files: None,
        summary_diffs: None,
        metadata: None,
        cost: 1.25,
        tokens_input: 10,
        tokens_output: 20,
        tokens_reasoning: 0,
        tokens_cache_read: 0,
        tokens_cache_write: 0,
        revert: None,
        permission: None,
        agent: Some("build".to_string()),
        model: None,
        time_created: 1,
        time_updated: 2,
        time_compacting: None,
        time_archived: None,
    }
}

fn message(id: &str, session_id: &str, time: i64) -> MessageRow {
    MessageRow {
        id: id.to_string(),
        session_id: session_id.to_string(),
        time_created: time,
        time_updated: time,
        data: serde_json::json!({ "role": "user" }),
    }
}

fn part(id: &str, message_id: &str, session_id: &str) -> PartRow {
    PartRow {
        id: id.to_string(),
        message_id: message_id.to_string(),
        session_id: session_id.to_string(),
        time_created: 1,
        time_updated: 1,
        data: serde_json::json!({ "type": "text", "text": id }),
    }
}

#[test]
fn session_message_part_round_trip() -> Result<()> {
    let db = Database::open_memory()?;
    let json = oc_database::tables::json_columns;

    db.insert("project", &project(), json("project"))?;
    db.insert("session", &session("ses_1", "Hello"), json("session"))?;
    db.insert("session", &session("ses_2", "Archived"), json("session"))?;
    db.delete_by("session", "id", &oc_database::Value::Text("ses_2".into()))?;

    assert!(db.session_exists("ses_1")?);
    assert!(!db.session_exists("nope")?);

    let fetched = db.get_session("ses_1")?.unwrap();
    assert_eq!(fetched.title, "Hello");
    assert_eq!(fetched.cost, 1.25);
    assert_eq!(fetched.tokens_input, 10);
    assert_eq!(fetched.agent.as_deref(), Some("build"));

    let sessions = db.list_sessions(false)?;
    assert_eq!(sessions.len(), 1);

    db.insert("message", &message("msg_1", "ses_1", 100), json("message"))?;
    db.insert("message", &message("msg_2", "ses_1", 200), json("message"))?;
    db.insert("message", &message("msg_3", "ses_1", 300), json("message"))?;

    // page: newest first, limit + 1 to detect another page
    let page = db.list_messages_page("ses_1", 2, None)?;
    assert_eq!(page.len(), 3);
    assert_eq!(page[0].id, "msg_3");
    assert_eq!(page[1].id, "msg_2");

    // cursor page after msg_2
    let page2 = db.list_messages_page("ses_1", 2, Some(("msg_2", 200)))?;
    assert_eq!(page2.len(), 1);
    assert_eq!(page2[0].id, "msg_1");

    let msg = db.get_message("msg_1", "ses_1")?.unwrap();
    assert_eq!(msg.data["role"], "user");
    assert!(db.get_message("msg_1", "other")?.is_none());

    db.insert("part", &part("part_1", "msg_1", "ses_1"), json("part"))?;
    db.insert("part", &part("part_2", "msg_1", "ses_1"), json("part"))?;
    db.insert("part", &part("part_3", "msg_2", "ses_1"), json("part"))?;

    let parts = db.list_parts("msg_1")?;
    assert_eq!(parts.len(), 2);
    assert_eq!(parts[0].id, "part_1");
    assert_eq!(parts[0].data["text"], "part_1");

    let by_messages = db.list_parts_by_messages(&["msg_1", "msg_2"])?;
    assert_eq!(by_messages.len(), 3);
    assert!(db.list_parts_by_messages(&[])?.is_empty());

    let todo = TodoRow {
        session_id: "ses_1".to_string(),
        content: "do it".to_string(),
        status: "pending".to_string(),
        priority: "normal".to_string(),
        position: 0,
        time_created: 1,
        time_updated: 1,
    };
    db.insert("todo", &todo, json("todo"))?;
    let todos = db.list_todos("ses_1")?;
    assert_eq!(todos.len(), 1);
    assert_eq!(todos[0].content, "do it");

    Ok(())
}
