//! Tests for the event manifests and definition inventory composition.

use oc_schema::event::{self, Definition, DurableVersion};

#[test]
fn definitions_inventory_is_ordered_and_complete() {
    let definitions = oc_schema::event_manifest::definitions();
    let types: Vec<&str> = definitions.iter().map(|d| d.r#type).collect();
    // First three entries mirror foundationDefinitions ordering.
    assert_eq!(types[0], "models-dev.refreshed");
    assert_eq!(types[1], "integration.updated");
    assert_eq!(types[2], "integration.connection.updated");
    assert_eq!(types[3], "catalog.updated");
    // Every canonical event type is present exactly once.
    for expected in [
        "session.created",
        "session.next.prompted",
        "session.next.step.ended",
        "session.next.text.delta",
        "message.part.updated",
        "message.part.delta",
        "session.diff",
        "installation.updated",
        "permission.asked",
        "todo.updated",
        "lsp.updated",
        "tui.toast.show",
        "mcp.tools.changed",
        "command.executed",
        "project.updated",
        "session.status",
        "question.replied",
        "session.compacted",
        "vcs.branch.updated",
        "workspace.ready",
        "worktree.failed",
        "server.connected",
        "global.disposed",
        "pty.created",
        "plugin.added",
        "file.edited",
        "file.watcher.updated",
        "reference.updated",
        "question.v2.asked",
    ] {
        assert_eq!(
            types.iter().filter(|t| **t == expected).count(),
            1,
            "type {expected} should appear exactly once"
        );
    }
}

#[test]
fn server_definitions_is_a_prefix_subset() {
    let server = oc_schema::event_manifest::server_definitions();
    let all = oc_schema::event_manifest::definitions();
    assert!(server.len() < all.len());
    for definition in &server {
        assert!(all.iter().any(|d| d.r#type == definition.r#type));
    }
}

#[test]
fn latest_prefers_newer_durable_version() {
    let latest = oc_schema::event_manifest::latest();
    let step_ended = latest
        .iter()
        .find(|d| d.r#type == "session.next.step.ended")
        .unwrap();
    assert_eq!(step_ended.durable.as_ref().unwrap().version, 2);
    let step_failed = latest
        .iter()
        .find(|d| d.r#type == "session.next.step.failed")
        .unwrap();
    assert_eq!(step_failed.durable.as_ref().unwrap().version, 2);
    let session_created = latest
        .iter()
        .find(|d| d.r#type == "session.created")
        .unwrap();
    assert_eq!(session_created.durable.as_ref().unwrap().version, 1);
}

#[test]
fn durable_manifest_keys() {
    let map = oc_schema::durable_event_manifest::session_durable_definitions();
    assert!(map.contains_key("session.next.prompted.1"));
    assert!(map.contains_key("session.next.step.ended.2"));
    assert!(!map.contains_key("session.next.text.delta.1"));
    let all = oc_schema::durable_event_manifest::durable_definitions();
    assert!(all.contains_key("session.created.1"));
    assert!(all.contains_key("session.next.tool.failed.1"));
}

#[test]
fn versioned_type_format() {
    assert_eq!(
        event::versioned_type("session.created", 1),
        "session.created.1"
    );
}

#[test]
fn event_latest_rejects_duplicate_non_durable() {
    // A durable definition followed by a non-durable one of the same type throws.
    let definitions = vec![
        Definition {
            r#type: "session.diff",
            durable: Some(DurableVersion {
                version: 1,
                aggregate: "sessionID",
            }),
        },
        Definition {
            r#type: "session.diff",
            durable: None,
        },
    ];
    let result = std::panic::catch_unwind(|| event::latest(&definitions));
    assert!(result.is_err());

    // Two non-durable definitions of the same type throw.
    let definitions = vec![
        Definition {
            r#type: "session.diff",
            durable: None,
        },
        Definition {
            r#type: "session.diff",
            durable: None,
        },
    ];
    let result = std::panic::catch_unwind(|| event::latest(&definitions));
    assert!(result.is_err());
}

#[test]
fn durable_map_rejects_duplicate_keys() {
    let definitions = vec![
        Definition {
            r#type: "session.created",
            durable: Some(DurableVersion {
                version: 1,
                aggregate: "sessionID",
            }),
        },
        Definition {
            r#type: "session.created",
            durable: Some(DurableVersion {
                version: 1,
                aggregate: "sessionID",
            }),
        },
    ];
    let result = std::panic::catch_unwind(|| event::durable(&definitions));
    assert!(result.is_err());
}

#[test]
fn event_definition_statics() {
    let definition = oc_schema::session_event::Prompted::definition();
    assert_eq!(definition.r#type, "session.next.prompted");
    assert_eq!(
        definition.durable,
        Some(DurableVersion {
            version: 1,
            aggregate: "sessionID",
        })
    );
    let delta = oc_schema::session_event::TextDelta::definition();
    assert_eq!(delta.r#type, "session.next.text.delta");
    assert_eq!(delta.durable, None);
}
