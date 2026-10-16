//! Data written by an older version survives the current one.
//!
//! The project holds what the first schema versions wrote: a decision, an index of an older
//! schema and a learning store of schema 1. The current binary must keep the decision, rebuild
//! the index and migrate the learning store without losing its sessions.

#![allow(clippy::unwrap_used)]

use std::io::Write;
use std::path::Path;
use std::process::{Command, Output, Stdio};

const LEARN_V1: &str = "
    CREATE TABLE sessions (id TEXT PRIMARY KEY NOT NULL, started INTEGER NOT NULL, ended INTEGER) STRICT;
    CREATE TABLE events (
        id INTEGER PRIMARY KEY,
        session TEXT NOT NULL REFERENCES sessions (id) ON DELETE CASCADE,
        at INTEGER NOT NULL, kind TEXT NOT NULL, path TEXT, pattern TEXT,
        start_line INTEGER, end_line INTEGER, count INTEGER, text TEXT
    ) STRICT;
    CREATE INDEX events_by_session ON events (session, id);
    CREATE INDEX events_by_path ON events (path);
    INSERT INTO sessions (id, started, ended) VALUES ('old', 1, 4);
    INSERT INTO events (session, at, kind, text) VALUES ('old', 1, 'prompt', 'fix the invoice total');
    INSERT INTO events (session, at, kind, path) VALUES ('old', 2, 'read', 'src/billing.py');
    INSERT INTO events (session, at, kind, path) VALUES ('old', 3, 'edit', 'src/billing.py');
    INSERT INTO events (session, at, kind) VALUES ('old', 4, 'stop');
    PRAGMA user_version = 1;
";

fn wizard(args: &[&str], project: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_wizard"))
        .args(args)
        .arg("--project")
        .arg(project)
        .output()
        .unwrap()
}

fn hook(project: &Path, input: &str) -> Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_wizard"))
        .arg("hook")
        .env("CLAUDE_PROJECT_DIR", project)
        .env_remove("CLAUDE_PLUGIN_ROOT")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(input.as_bytes())
        .unwrap();
    child.wait_with_output().unwrap()
}

#[test]
fn data_of_an_older_version_survives_an_upgrade() {
    let project = tempfile::tempdir().unwrap();
    let root = project.path();
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::write(
        root.join("src/billing.py"),
        "def compute_invoice(total):\n    return round(total, 2)\n",
    )
    .unwrap();
    std::fs::create_dir_all(root.join(".claude")).unwrap();
    std::fs::write(
        root.join(".claude/wizard.json"),
        r#"{"enabled":true,"version":1,"decidedAt":"2026-10-01","team":"core"}"#,
    )
    .unwrap();
    std::fs::create_dir_all(root.join(".wizard")).unwrap();
    std::fs::write(root.join(".wizard/.gitignore"), "*\n").unwrap();
    let index = rusqlite::Connection::open(root.join(".wizard/index.db")).unwrap();
    index
        .execute_batch(
            "CREATE TABLE files (path TEXT PRIMARY KEY) STRICT; PRAGMA user_version = 2;",
        )
        .unwrap();
    drop(index);
    let learn = rusqlite::Connection::open(root.join(".wizard/learn.db")).unwrap();
    learn.execute_batch(LEARN_V1).unwrap();
    drop(learn);

    let start = hook(
        root,
        r#"{"hook_event_name":"SessionStart","source":"startup"}"#,
    );
    let value: serde_json::Value = serde_json::from_slice(&start.stdout).unwrap();
    let overview = value["hookSpecificOutput"]["additionalContext"]
        .as_str()
        .unwrap();
    assert!(overview.contains("1 indexed files"), "{overview}");

    let prompt = hook(
        root,
        r#"{"hook_event_name":"UserPromptSubmit","session_id":"new","prompt":"the invoice total is wrong"}"#,
    );
    assert!(prompt.status.success());
    let stats = String::from_utf8(wizard(&["stats"], root).stdout).unwrap();
    assert!(stats.contains("Sessions recorded: 2"), "{stats}");

    let decision = std::fs::read_to_string(root.join(".claude/wizard.json")).unwrap();
    assert!(decision.contains("\"enabled\": true") || decision.contains("\"enabled\":true"));
    assert!(decision.contains("core"), "{decision}");
    assert!(root.join(".wizard/learn.db.v1.bak").is_file());
    let migrated = rusqlite::Connection::open(root.join(".wizard/learn.db")).unwrap();
    let events: i64 = migrated
        .query_row(
            "SELECT COUNT(*) FROM events WHERE session = 'old'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(events, 4);
}
