//! Golden schema test.
//!
//! Runs the crate's DDL on an in-memory database and asserts
//! `SELECT sql FROM sqlite_master` matches the reference DDL from
//! `reference/packages/core/src/database/schema.gen.ts`, persisted in
//! `fixtures/schema.sql` in the exact form SQLite stores (leading whitespace
//! trimmed, trailing semicolon removed).

use std::collections::BTreeMap;

use oc_database::schema;
use oc_database::sqlite::{Config, Sqlite};

fn fixture_statements() -> Vec<String> {
    let raw = include_str!("fixtures/schema.sql");
    let text = raw.replace("\r\n", "\n");
    let mut out = Vec::new();
    for chunk in text.split("\n;\n") {
        let stmt: String = chunk
            .lines()
            .filter(|line| !line.starts_with("--"))
            .collect::<Vec<_>>()
            .join("\n")
            .trim()
            .to_string();
        if !stmt.is_empty() {
            out.push(stmt);
        }
    }
    out
}

fn statement_name(stmt: &str) -> (&str, String) {
    if let Some(rest) = stmt.strip_prefix("CREATE TABLE") {
        let name = rest
            .trim()
            .trim_start_matches('`')
            .split('`')
            .next()
            .unwrap();
        ("table", name.to_string())
    } else {
        let rest = stmt
            .strip_prefix("CREATE UNIQUE INDEX")
            .or_else(|| stmt.strip_prefix("CREATE INDEX"))
            .unwrap_or_default();
        let name = rest
            .trim()
            .trim_start_matches('`')
            .split('`')
            .next()
            .unwrap();
        ("index", name.to_string())
    }
}

#[test]
fn schema_matches_reference_ddl() {
    let expected = fixture_statements();
    assert_eq!(expected.len(), schema::TABLES.len() + schema::INDEXES.len());
    let expected_by_name: BTreeMap<(String, String), String> = expected
        .iter()
        .map(|stmt| {
            let (kind, name) = statement_name(stmt);
            ((kind.to_string(), name), stmt.clone())
        })
        .collect();

    let db = Sqlite::open(Config {
        filename: ":memory:".to_string(),
        disable_wal: true,
        ..Default::default()
    })
    .unwrap();
    db.transaction(schema::schema_up).unwrap();

    let actual_by_name: BTreeMap<(String, String), String> = db
        .run("SELECT type, name, sql FROM sqlite_master WHERE sql IS NOT NULL")
        .unwrap()
        .iter()
        .map(|row| {
            (
                (
                    row.get_by_name::<String>("type").unwrap(),
                    row.get_by_name::<String>("name").unwrap(),
                ),
                row.get_by_name::<String>("sql").unwrap(),
            )
        })
        .collect();

    assert_eq!(
        actual_by_name.len(),
        expected_by_name.len(),
        "object count mismatch (actual = {actual_by_name:?})"
    );
    for ((kind, name), expected_sql) in &expected_by_name {
        let actual_sql = actual_by_name
            .get(&(kind.clone(), name.clone()))
            .unwrap_or_else(|| panic!("missing {kind} `{name}` in sqlite_master"));
        assert_eq!(
            actual_sql, expected_sql,
            "{kind} `{name}` DDL differs from reference"
        );
    }
}
