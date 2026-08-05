//! Migration runner tests, ported from
//! `reference/packages/core/test/database-migration.test.ts`.

use oc_database::migration;
use oc_database::sqlite::{Config, Sqlite, Value};

fn memory() -> Sqlite {
    Sqlite::open(Config {
        filename: ":memory:".to_string(),
        disable_wal: true,
        ..Default::default()
    })
    .unwrap()
}

fn apply(db: &Sqlite) -> oc_database::Result<()> {
    migration::apply(db)
}

fn apply_only(db: &Sqlite, id: &str) -> oc_database::Result<()> {
    let m = migration::by_id(id).unwrap();
    migration::apply_only(db, std::slice::from_ref(m))
}

#[test]
fn applies_tracked_migrations_to_an_empty_database() {
    let db = memory();
    apply(&db).unwrap();

    let session = db
        .get(
            "SELECT name FROM sqlite_master WHERE type = 'table' AND name = 'session'",
            &[],
        )
        .unwrap()
        .is_some();
    assert!(session);
    assert!(db
        .get(
            "SELECT name FROM sqlite_master WHERE type = 'table' AND name = 'session_input'",
            &[]
        )
        .unwrap()
        .is_some());
    assert!(db
        .get("SELECT name FROM sqlite_master WHERE type = 'table' AND name = 'session_context_epoch'", &[])
        .unwrap()
        .is_some());

    // Dropped columns from 20260622142730_simplify_session_context_epoch.
    let cols: Vec<String> = db
        .all("SELECT name FROM pragma_table_info('session_context_epoch') WHERE name IN ('agent', 'replacement_seq', 'revision')", &[])
        .unwrap()
        .iter()
        .map(|row| row.get_by_name::<String>("name").unwrap())
        .collect();
    assert!(cols.is_empty());

    let count: i64 = db
        .get("SELECT count(*) AS count FROM migration", &[])
        .unwrap()
        .unwrap()
        .get_by_name("count")
        .unwrap();
    assert_eq!(count as usize, migration::migrations().len());

    let expected: Vec<&str> = vec![
        "event_aggregate_seq_idx",
        "event_aggregate_type_seq_idx",
        "session_input_session_admitted_seq_idx",
        "session_input_session_pending_delivery_seq_idx",
        "session_input_session_promoted_seq_idx",
        "session_message_session_seq_idx",
        "session_message_session_time_created_id_idx",
        "session_message_session_type_seq_idx",
    ];
    let actual: Vec<String> = db
        .all(
            "SELECT name FROM sqlite_master WHERE type = 'index' AND name IN ('event_aggregate_seq_idx', 'event_aggregate_type_seq_idx', 'session_input_session_pending_seq_idx', 'session_input_session_pending_delivery_seq_idx', 'session_input_session_admitted_seq_idx', 'session_input_session_promoted_seq_idx', 'session_message_session_idx', 'session_message_session_type_idx', 'session_message_session_seq_idx', 'session_message_session_type_seq_idx', 'session_message_session_time_created_id_idx') ORDER BY name",
            &[],
        )
        .unwrap()
        .iter()
        .map(|row| row.get_by_name::<String>("name").unwrap())
        .collect();
    assert_eq!(actual, expected);
}

#[test]
fn rejects_a_non_empty_database_without_a_session_table() {
    let db = memory();
    db.execute("CREATE TABLE unrelated (id text PRIMARY KEY)")
        .unwrap();
    let error = apply(&db).unwrap_err();
    assert!(error
        .to_string()
        .contains("Database is not empty and has no session table"));
}

#[test]
fn imports_existing_drizzle_migration_state() {
    let db = memory();
    db.execute(
        "CREATE TABLE __drizzle_migrations (id INTEGER PRIMARY KEY, hash text NOT NULL, created_at numeric, name text, applied_at TEXT)",
    )
    .unwrap();
    db.execute(
        "INSERT INTO __drizzle_migrations (hash, created_at, name, applied_at) VALUES ('hash', 1, '20260127222353_familiar_lady_ursula', 'now')",
    )
    .unwrap();
    migration::apply_only(&db, &[]).unwrap();
    let id: String = db
        .get("SELECT id FROM migration", &[])
        .unwrap()
        .unwrap()
        .get_by_name("id")
        .unwrap();
    assert_eq!(id, "20260127222353_familiar_lady_ursula");
}

#[test]
fn skips_drizzle_import_when_migration_table_already_has_state() {
    let db = memory();
    db.execute("CREATE TABLE migration (id TEXT PRIMARY KEY, time_completed INTEGER NOT NULL)")
        .unwrap();
    db.execute("INSERT INTO migration (id, time_completed) VALUES ('existing', 1)")
        .unwrap();
    db.execute(
        "CREATE TABLE __drizzle_migrations (id INTEGER PRIMARY KEY, hash text NOT NULL, created_at numeric, name text, applied_at TEXT)",
    )
    .unwrap();
    db.execute(
        "INSERT INTO __drizzle_migrations (hash, created_at, name, applied_at) VALUES ('hash', 1, '20260127222353_familiar_lady_ursula', 'now')",
    )
    .unwrap();
    migration::apply_only(&db, &[]).unwrap();
    let ids: Vec<String> = db
        .all("SELECT id FROM migration ORDER BY id", &[])
        .unwrap()
        .iter()
        .map(|row| row.get_by_name::<String>("id").unwrap())
        .collect();
    assert_eq!(ids, vec!["existing"]);
}

#[test]
fn does_not_replay_a_migrated_session_metadata_column() {
    let db = memory();
    db.execute("CREATE TABLE session (id text PRIMARY KEY, metadata text)")
        .unwrap();
    db.execute(
        "CREATE TABLE __drizzle_migrations (id INTEGER PRIMARY KEY, hash text NOT NULL, created_at numeric, name text, applied_at TEXT)",
    )
    .unwrap();
    db.execute(
        "INSERT INTO __drizzle_migrations (hash, created_at, name, applied_at) VALUES ('hash', 1, '20260511173437_session-metadata', 'now')",
    )
    .unwrap();
    apply_only(&db, "20260511173437_session-metadata").unwrap();
    let ids: Vec<String> = db
        .all("SELECT id FROM migration", &[])
        .unwrap()
        .iter()
        .map(|row| row.get_by_name::<String>("id").unwrap())
        .collect();
    assert_eq!(ids, vec!["20260511173437_session-metadata"]);
}

#[test]
fn accepts_the_temporary_replacement_session_metadata_migration_id() {
    let db = memory();
    db.execute("CREATE TABLE session (id text PRIMARY KEY, metadata text)")
        .unwrap();
    db.execute("CREATE TABLE migration (id TEXT PRIMARY KEY, time_completed INTEGER NOT NULL)")
        .unwrap();
    db.execute(
        "INSERT INTO migration (id, time_completed) VALUES ('20260530232709_lovely_romulus', 1)",
    )
    .unwrap();
    apply_only(&db, "20260511173437_session-metadata").unwrap();
    let ids: Vec<String> = db
        .all("SELECT id FROM migration ORDER BY id", &[])
        .unwrap()
        .iter()
        .map(|row| row.get_by_name::<String>("id").unwrap())
        .collect();
    assert_eq!(
        ids,
        vec![
            "20260511173437_session-metadata",
            "20260530232709_lovely_romulus"
        ]
    );
}

#[test]
fn backfills_existing_context_epoch_rows_to_the_build_agent() {
    let db = memory();
    db.execute(
        "CREATE TABLE session_context_epoch (session_id text PRIMARY KEY, baseline text NOT NULL, snapshot text NOT NULL, baseline_seq integer NOT NULL, replacement_seq integer, revision integer DEFAULT 0 NOT NULL)",
    )
    .unwrap();
    db.execute(
        "INSERT INTO session_context_epoch (session_id, baseline, snapshot, baseline_seq) VALUES ('ses_existing', 'baseline', '{}', 0)",
    )
    .unwrap();
    apply_only(&db, "20260605042240_add_context_epoch_agent").unwrap();
    let agent: String = db
        .get(
            "SELECT agent FROM session_context_epoch WHERE session_id = 'ses_existing'",
            &[],
        )
        .unwrap()
        .unwrap()
        .get_by_name("agent")
        .unwrap();
    assert_eq!(agent, "build");
}

#[test]
fn keeps_legacy_credential_fields_nullable() {
    let db = memory();
    db.execute(
        "CREATE TABLE credential (id text PRIMARY KEY, connector_id text NOT NULL, method_id text NOT NULL, label text NOT NULL, value text NOT NULL, active integer DEFAULT false NOT NULL, time_created integer NOT NULL, time_updated integer NOT NULL)",
    )
    .unwrap();
    db.execute("CREATE UNIQUE INDEX credential_connector_active_idx ON credential (connector_id) WHERE active = 1")
        .unwrap();
    apply_only(&db, "20260611192811_lush_chimera").unwrap();
    db.execute(
        "INSERT INTO credential (id, integration_id, label, value, time_created, time_updated) VALUES ('current', 'anthropic', 'Current', '{}', 2, 2)",
    )
    .unwrap();
    let row = db
        .get(
            "SELECT connector_id, method_id, active FROM credential WHERE id = 'current'",
            &[],
        )
        .unwrap()
        .unwrap();
    assert!(row.is_null_by_name("connector_id"));
    assert!(row.is_null_by_name("method_id"));
    assert!(row.is_null_by_name("active"));
}

#[test]
fn runs_session_usage_backfill_in_order_with_schema_changes() {
    let db = memory();
    db.execute("CREATE TABLE session (id text PRIMARY KEY, time_updated integer NOT NULL)")
        .unwrap();
    db.execute(
        "CREATE TABLE message (id text PRIMARY KEY, session_id text NOT NULL, data text NOT NULL)",
    )
    .unwrap();
    db.execute("INSERT INTO session (id, time_updated) VALUES ('session_1', 1)")
        .unwrap();
    db.execute(
        "INSERT INTO message (id, session_id, data) VALUES ('message_1', 'session_1', '{\"role\":\"assistant\",\"cost\":1.25,\"tokens\":{\"input\":2,\"output\":3,\"reasoning\":4,\"cache\":{\"read\":5,\"write\":6}}}')",
    )
    .unwrap();
    apply_only(&db, "20260510033149_session_usage").unwrap();
    let row = db
        .get(
            "SELECT cost, tokens_input, tokens_output, tokens_reasoning, tokens_cache_read, tokens_cache_write FROM session WHERE id = 'session_1'",
            &[],
        )
        .unwrap()
        .unwrap();
    assert_eq!(row.get_by_name::<f64>("cost").unwrap(), 1.25);
    assert_eq!(row.get_by_name::<i64>("tokens_input").unwrap(), 2);
    assert_eq!(row.get_by_name::<i64>("tokens_output").unwrap(), 3);
    assert_eq!(row.get_by_name::<i64>("tokens_reasoning").unwrap(), 4);
    assert_eq!(row.get_by_name::<i64>("tokens_cache_read").unwrap(), 5);
    assert_eq!(row.get_by_name::<i64>("tokens_cache_write").unwrap(), 6);
}

#[test]
fn resets_incompatible_projected_session_messages_before_adding_sequence_order() {
    let db = memory();
    db.execute("CREATE TABLE session (id text PRIMARY KEY)")
        .unwrap();
    db.execute(
        "CREATE TABLE message (id text PRIMARY KEY, session_id text NOT NULL, time_created integer NOT NULL, time_updated integer NOT NULL, data text NOT NULL)",
    )
    .unwrap();
    db.execute(
        "CREATE TABLE part (id text PRIMARY KEY, message_id text NOT NULL, session_id text NOT NULL, time_created integer NOT NULL, time_updated integer NOT NULL, data text NOT NULL)",
    )
    .unwrap();
    db.execute("CREATE TABLE event (id text PRIMARY KEY, seq integer NOT NULL)")
        .unwrap();
    db.execute(
        "CREATE TABLE session_message (id text PRIMARY KEY, session_id text NOT NULL, type text NOT NULL, time_created integer NOT NULL, time_updated integer NOT NULL, data text NOT NULL)",
    )
    .unwrap();
    db.execute(
        "CREATE INDEX session_message_session_time_created_id_idx ON session_message (session_id, time_created, id)",
    )
    .unwrap();
    db.execute(
        "CREATE INDEX session_message_session_type_time_created_id_idx ON session_message (session_id, type, time_created, id)",
    )
    .unwrap();
    db.execute("INSERT INTO session (id) VALUES ('session')")
        .unwrap();
    db.execute(
        "INSERT INTO message (id, session_id, time_created, time_updated, data) VALUES ('legacy_message', 'session', 1, 1, '{\"role\":\"user\"}')",
    )
    .unwrap();
    db.execute(
        "INSERT INTO part (id, message_id, session_id, time_created, time_updated, data) VALUES ('legacy_part', 'legacy_message', 'session', 1, 1, '{\"type\":\"text\",\"text\":\"hello\"}')",
    )
    .unwrap();
    db.execute(
        "INSERT INTO session_message (id, session_id, type, time_created, time_updated, data) VALUES ('stale_projection', 'session', 'user', 1, 1, '{}')",
    )
    .unwrap();

    apply_only(&db, "20260603040000_session_message_projection_order").unwrap();

    assert_eq!(
        db.all("SELECT id FROM session_message", &[]).unwrap().len(),
        0
    );
    assert_eq!(db.all("SELECT id FROM message", &[]).unwrap().len(), 1);
    assert_eq!(db.all("SELECT id FROM part", &[]).unwrap().len(), 1);

    db.execute(
        "INSERT INTO session_message (id, session_id, type, seq, time_created, time_updated, data) VALUES ('fresh_projection', 'session', 'user', 7, 2, 2, '{}')",
    )
    .unwrap();
    let row = db
        .get("SELECT id, seq FROM session_message", &[])
        .unwrap()
        .unwrap();
    assert_eq!(row.get_by_name::<String>("id").unwrap(), "fresh_projection");
    assert_eq!(row.get_by_name::<i64>("seq").unwrap(), 7);
}

#[test]
fn resets_beta_history_and_rebuilds_event_sourced_session_input_storage() {
    let db = memory();
    db.execute("CREATE TABLE session (id text PRIMARY KEY, workspace_id text)")
        .unwrap();
    db.execute("CREATE TABLE workspace (id text PRIMARY KEY)")
        .unwrap();
    db.execute("CREATE TABLE message (id text PRIMARY KEY)")
        .unwrap();
    db.execute("CREATE TABLE part (id text PRIMARY KEY)")
        .unwrap();
    db.execute("CREATE TABLE event_sequence (aggregate_id text PRIMARY KEY, seq integer NOT NULL)")
        .unwrap();
    db.execute(
        "CREATE TABLE event (id text PRIMARY KEY, aggregate_id text NOT NULL, seq integer NOT NULL, type text NOT NULL, data text NOT NULL)",
    )
    .unwrap();
    db.execute("CREATE INDEX event_aggregate_seq_idx ON event (aggregate_id, seq)")
        .unwrap();
    db.execute("CREATE INDEX event_aggregate_type_seq_idx ON event (aggregate_id, type, seq)")
        .unwrap();
    db.execute(
        "CREATE TABLE session_message (id text PRIMARY KEY, session_id text NOT NULL, type text NOT NULL, seq integer NOT NULL, time_created integer NOT NULL, time_updated integer NOT NULL, data text NOT NULL)",
    )
    .unwrap();
    db.execute("CREATE INDEX session_message_session_seq_idx ON session_message (session_id, seq)")
        .unwrap();
    db.execute(
        "CREATE TABLE session_input (seq integer PRIMARY KEY AUTOINCREMENT, id text NOT NULL UNIQUE, session_id text NOT NULL, prompt text NOT NULL, delivery text NOT NULL, promoted_seq integer, time_created integer NOT NULL)",
    )
    .unwrap();
    db.execute(
        "CREATE INDEX session_input_session_pending_delivery_seq_idx ON session_input (session_id, promoted_seq, delivery, seq)",
    )
    .unwrap();
    db.execute("INSERT INTO session (id, workspace_id) VALUES ('session', 'wrk_old')")
        .unwrap();
    db.execute("INSERT INTO workspace (id) VALUES ('wrk_old')")
        .unwrap();
    db.execute("INSERT INTO message (id) VALUES ('message')")
        .unwrap();
    db.execute("INSERT INTO part (id) VALUES ('part')").unwrap();
    db.execute("INSERT INTO event_sequence (aggregate_id, seq) VALUES ('session', 0)")
        .unwrap();
    db.execute("INSERT INTO event (id, aggregate_id, seq, type, data) VALUES ('evt_old', 'session', 0, 'old.1', '{}')")
        .unwrap();
    db.execute(
        "INSERT INTO session_message (id, session_id, type, seq, time_created, time_updated, data) VALUES ('msg_old', 'session', 'user', 0, 1, 1, '{}')",
    )
    .unwrap();
    db.execute(
        "INSERT INTO session_input (id, session_id, prompt, delivery, time_created) VALUES ('msg_pending', 'session', '{}', 'steer', 1)",
    )
    .unwrap();

    apply_only(&db, "20260604172448_event_sourced_session_input").unwrap();

    let session = db
        .get("SELECT id, workspace_id FROM session", &[])
        .unwrap()
        .unwrap();
    assert_eq!(session.get_by_name::<String>("id").unwrap(), "session");
    assert!(session.is_null_by_name("workspace_id"));
    assert_eq!(db.all("SELECT id FROM workspace", &[]).unwrap().len(), 0);
    assert_eq!(db.all("SELECT id FROM message", &[]).unwrap().len(), 1);
    assert_eq!(db.all("SELECT id FROM part", &[]).unwrap().len(), 1);
    assert_eq!(db.all("SELECT id FROM event", &[]).unwrap().len(), 0);
    assert_eq!(
        db.all("SELECT aggregate_id FROM event_sequence", &[])
            .unwrap()
            .len(),
        0
    );
    assert_eq!(
        db.all("SELECT id FROM session_message", &[]).unwrap().len(),
        0
    );
    assert_eq!(
        db.all("SELECT id FROM session_input", &[]).unwrap().len(),
        0
    );

    let input_cols: Vec<String> = db
        .all("SELECT name FROM pragma_table_info('session_input')", &[])
        .unwrap()
        .iter()
        .map(|row| row.get_by_name::<String>("name").unwrap())
        .collect();
    assert_eq!(
        input_cols,
        vec![
            "id",
            "session_id",
            "prompt",
            "delivery",
            "admitted_seq",
            "promoted_seq",
            "time_created"
        ]
    );

    let session_message_unique: i64 = db
        .get(
            "SELECT \"unique\" FROM pragma_index_list('session_message') WHERE name = 'session_message_session_seq_idx'",
            &[],
        )
        .unwrap()
        .unwrap()
        .get_by_name("unique")
        .unwrap();
    assert_eq!(session_message_unique, 1);
    let event_unique: i64 = db
        .get(
            "SELECT \"unique\" FROM pragma_index_list('event') WHERE name = 'event_aggregate_seq_idx'",
            &[],
        )
        .unwrap()
        .unwrap()
        .get_by_name("unique")
        .unwrap();
    assert_eq!(event_unique, 1);
}

#[test]
fn normalizes_windows_storage_paths_and_leaves_posix_paths_untouched() {
    let db = memory();
    db.execute("CREATE TABLE project (id text PRIMARY KEY, worktree text NOT NULL, sandboxes text NOT NULL)").unwrap();
    db.execute("CREATE TABLE session (id text PRIMARY KEY, directory text NOT NULL, path text)")
        .unwrap();
    db.execute_with(
        "INSERT INTO project (id, worktree, sandboxes) VALUES (?, ?, ?)",
        &[
            Value::Text("win".into()),
            Value::Text("C:\\Repo\\Thing".into()),
            Value::Text(r#"["C:\\Repo\\Thing\\sandbox"]"#.into()),
        ],
    )
    .unwrap();
    db.execute_with(
        "INSERT INTO project (id, worktree, sandboxes) VALUES (?, ?, ?)",
        &[
            Value::Text("unc".into()),
            Value::Text("\\\\server\\share".into()),
            Value::Text(r#"["\\\\server\\share\\sandbox"]"#.into()),
        ],
    )
    .unwrap();
    db.execute_with(
        "INSERT INTO project (id, worktree, sandboxes) VALUES (?, ?, ?)",
        &[
            Value::Text("global".into()),
            Value::Text("/".into()),
            Value::Text("[]".into()),
        ],
    )
    .unwrap();
    db.execute_with(
        "INSERT INTO session (id, directory, path) VALUES (?, ?, ?)",
        &[
            Value::Text("win".into()),
            Value::Text("C:\\Repo\\Thing\\packages\\api".into()),
            Value::Text("packages\\api".into()),
        ],
    )
    .unwrap();
    db.execute_with(
        "INSERT INTO session (id, directory, path) VALUES (?, ?, ?)",
        &[
            Value::Text("posix".into()),
            Value::Text("/home/me/we\\ird".into()),
            Value::Text("src\\weird".into()),
        ],
    )
    .unwrap();

    apply_only(&db, "20260601010001_normalize_storage_paths").unwrap();

    let row = db
        .get(
            "SELECT worktree, sandboxes FROM project WHERE id = 'win'",
            &[],
        )
        .unwrap()
        .unwrap();
    assert_eq!(
        row.get_by_name::<String>("worktree").unwrap(),
        "C:/Repo/Thing"
    );
    assert_eq!(
        row.get_by_name::<String>("sandboxes").unwrap(),
        r#"["C:/Repo/Thing/sandbox"]"#
    );
    let row = db
        .get("SELECT directory, path FROM session WHERE id = 'win'", &[])
        .unwrap()
        .unwrap();
    assert_eq!(
        row.get_by_name::<String>("directory").unwrap(),
        "C:/Repo/Thing/packages/api"
    );
    assert_eq!(row.get_by_name::<String>("path").unwrap(), "packages/api");
    let row = db
        .get(
            "SELECT worktree, sandboxes FROM project WHERE id = 'unc'",
            &[],
        )
        .unwrap()
        .unwrap();
    assert_eq!(
        row.get_by_name::<String>("worktree").unwrap(),
        "//server/share"
    );
    assert_eq!(
        row.get_by_name::<String>("sandboxes").unwrap(),
        r#"["//server/share/sandbox"]"#
    );
    let row = db
        .get("SELECT worktree FROM project WHERE id = 'global'", &[])
        .unwrap()
        .unwrap();
    assert_eq!(row.get_by_name::<String>("worktree").unwrap(), "/");
    let row = db
        .get(
            "SELECT directory, path FROM session WHERE id = 'posix'",
            &[],
        )
        .unwrap()
        .unwrap();
    assert_eq!(
        row.get_by_name::<String>("directory").unwrap(),
        "/home/me/we\\ird"
    );
    assert_eq!(row.get_by_name::<String>("path").unwrap(), "src\\weird");
}

#[test]
fn serializes_concurrent_embedded_initialization_for_one_database_path() {
    let dir = std::env::temp_dir().join(format!("ocdb-concurrent-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let filename = dir.join("embedded.sqlite");
    let handles: Vec<_> = (0..8)
        .map(|_| {
            let filename = filename.clone();
            std::thread::spawn(move || {
                let db = oc_database::Database::open(&filename).unwrap();
                let count: i64 = db
                    .db
                    .get("SELECT count(*) AS count FROM migration", &[])
                    .unwrap()
                    .unwrap()
                    .get_by_name("count")
                    .unwrap();
                assert_eq!(count as usize, migration::migrations().len());
            })
        })
        .collect();
    for handle in handles {
        handle.join().unwrap();
    }
    std::fs::remove_dir_all(&dir).ok();
}
