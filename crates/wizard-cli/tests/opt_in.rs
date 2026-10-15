//! End-to-end tests of the opt-in flow: question, decision, and silence afterwards.

#![allow(clippy::unwrap_used)]

use std::io::Write;
use std::path::Path;
use std::process::{Command, Output, Stdio};

fn hook(project: &Path, event: &str) -> Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_wizard"))
        .arg("hook")
        .env("CLAUDE_PROJECT_DIR", project)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let input = format!(r#"{{"hook_event_name":"{event}","prompt":"fix the login bug"}}"#);
    child
        .stdin
        .take()
        .unwrap()
        .write_all(input.as_bytes())
        .unwrap();
    child.wait_with_output().unwrap()
}

fn decide(project: &Path, answer: &str) -> Output {
    Command::new(env!("CARGO_BIN_EXE_wizard"))
        .args(["decide", answer, "--project"])
        .arg(project)
        .output()
        .unwrap()
}

fn context(output: &Output) -> String {
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(
        value["hookSpecificOutput"]["hookEventName"],
        "UserPromptSubmit"
    );
    value["hookSpecificOutput"]["additionalContext"]
        .as_str()
        .unwrap()
        .to_owned()
}

#[test]
fn undecided_project_gets_the_question_on_every_prompt() {
    let project = tempfile::tempdir().unwrap();
    for _ in 0..2 {
        let output = hook(project.path(), "UserPromptSubmit");
        assert!(output.status.success());
        let text = context(&output);
        assert!(text.contains("AskUserQuestion"));
        assert!(text.contains("decide on --project"));
        assert!(text.contains("decide off --project"));
    }
}

#[test]
fn other_events_stay_silent_while_undecided() {
    let project = tempfile::tempdir().unwrap();
    for event in [
        "SessionStart",
        "PreToolUse",
        "PostToolUse",
        "Stop",
        "Notification",
    ] {
        let output = hook(project.path(), event);
        assert!(output.status.success());
        assert_eq!(String::from_utf8_lossy(&output.stdout), "", "{event}");
    }
}

#[test]
fn answering_stops_the_question() {
    for answer in ["on", "off"] {
        let project = tempfile::tempdir().unwrap();
        let decided = decide(project.path(), answer);
        assert!(decided.status.success());
        let file = std::fs::read_to_string(project.path().join(".claude/wizard.json")).unwrap();
        let file: serde_json::Value = serde_json::from_str(&file).unwrap();
        assert_eq!(file["enabled"], answer == "on");
        assert_eq!(file["version"], 1);
        let output = hook(project.path(), "UserPromptSubmit");
        assert_eq!(String::from_utf8_lossy(&output.stdout), "");
    }
}

#[test]
fn unreadable_decision_stays_silent() {
    let project = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(project.path().join(".claude")).unwrap();
    std::fs::write(project.path().join(".claude/wizard.json"), "{ broken").unwrap();
    let output = hook(project.path(), "UserPromptSubmit");
    assert!(output.status.success());
    assert_eq!(String::from_utf8_lossy(&output.stdout), "");
}
