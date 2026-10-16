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

fn installed_binary(data_dir_name: &str) -> (tempfile::TempDir, std::path::PathBuf) {
    let root = tempfile::tempdir().unwrap();
    let bin = root.path().join(data_dir_name).join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    let name = if cfg!(windows) {
        "wizard.exe"
    } else {
        "wizard"
    };
    std::fs::copy(env!("CARGO_BIN_EXE_wizard"), bin.join(name)).unwrap();
    (root, bin.join(name))
}

fn decide_with(binary: &Path, project: &Path, answer: &str) -> Output {
    let mut attempt = 0;
    loop {
        let result = Command::new(binary)
            .args(["decide", answer, "--project"])
            .arg(project)
            .output();
        match result {
            Err(error)
                if error.kind() == std::io::ErrorKind::ExecutableFileBusy && attempt < 20 =>
            {
                attempt += 1;
                std::thread::sleep(std::time::Duration::from_millis(50));
            }
            result => return result.unwrap(),
        }
    }
}

#[test]
fn disabling_from_a_plugin_install_adds_the_settings_override() {
    for (data_dir, id) in [
        (
            "context-wizard-context-wizard",
            "context-wizard@context-wizard",
        ),
        (
            "context-wizard-context-wizard-beta",
            "context-wizard@context-wizard-beta",
        ),
    ] {
        let (_root, binary) = installed_binary(data_dir);
        let project = tempfile::tempdir().unwrap();
        let settings = project.path().join(".claude/settings.local.json");
        assert!(decide_with(&binary, project.path(), "off").status.success());
        let value: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&settings).unwrap()).unwrap();
        assert_eq!(value["enabledPlugins"][id], false);
        assert!(decide_with(&binary, project.path(), "on").status.success());
        assert_eq!(
            std::fs::read_to_string(&settings).unwrap(),
            "{}
"
        );
    }
}

#[test]
fn disabling_outside_a_plugin_install_leaves_settings_alone() {
    let project = tempfile::tempdir().unwrap();
    let output = decide(project.path(), "off");
    assert!(output.status.success());
    assert!(!project.path().join(".claude/settings.local.json").exists());
    assert!(String::from_utf8_lossy(&output.stderr).contains("left untouched"));
}

fn hook_with_plugin_version(project: &Path, version: &str) -> Output {
    let plugin = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(plugin.path().join(".claude-plugin")).unwrap();
    std::fs::write(
        plugin.path().join(".claude-plugin/plugin.json"),
        format!(r#"{{"name": "context-wizard", "version": "{version}"}}"#),
    )
    .unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_wizard"))
        .arg("hook")
        .env("CLAUDE_PROJECT_DIR", project)
        .env("CLAUDE_PLUGIN_ROOT", plugin.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(br#"{"hook_event_name":"UserPromptSubmit"}"#)
        .unwrap();
    child.wait_with_output().unwrap()
}

#[test]
fn stays_inert_when_the_plugin_version_differs() {
    let project = tempfile::tempdir().unwrap();
    let mismatched = hook_with_plugin_version(project.path(), "999.0.0");
    assert!(mismatched.status.success());
    assert_eq!(String::from_utf8_lossy(&mismatched.stdout), "");
    let matching = hook_with_plugin_version(project.path(), env!("CARGO_PKG_VERSION"));
    assert!(context(&matching).contains("decide on --project"));
}

#[test]
fn enabled_projects_get_the_overview_at_session_start() {
    let project = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(project.path().join("src")).unwrap();
    std::fs::write(
        project.path().join("Cargo.toml"),
        "[package]\nname = \"demo\"\n",
    )
    .unwrap();
    std::fs::write(project.path().join("src/main.rs"), "fn main() {}\n").unwrap();
    let decided = decide(project.path(), "on");
    assert!(decided.status.success());
    assert!(String::from_utf8_lossy(&decided.stdout).contains("# Project overview"));
    let output = hook(project.path(), "SessionStart");
    assert!(output.status.success());
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["hookSpecificOutput"]["hookEventName"], "SessionStart");
    let overview = value["hookSpecificOutput"]["additionalContext"]
        .as_str()
        .unwrap();
    assert!(
        overview.contains("# Project overview (Context Wizard)"),
        "{overview}"
    );
    assert!(overview.contains("- src/main.rs"), "{overview}");
    assert!(overview.len() < 10_000);
}

fn hook_with(project: &Path, input: &str) -> Output {
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
fn grep_for_a_known_symbol_gets_its_definition() {
    let project = tempfile::tempdir().unwrap();
    std::fs::write(
        project.path().join("billing.py"),
        "def compute_invoice(total):\n    return total\n",
    )
    .unwrap();
    assert!(decide(project.path(), "on").status.success());
    let grep = |pattern: &str| {
        let input = serde_json::json!({
            "hook_event_name": "PreToolUse",
            "tool_name": "Grep",
            "tool_input": { "pattern": pattern }
        });
        hook_with(project.path(), &input.to_string())
    };
    let output = grep(r"def compute_invoice");
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["hookSpecificOutput"]["hookEventName"], "PreToolUse");
    let context = value["hookSpecificOutput"]["additionalContext"]
        .as_str()
        .unwrap();
    assert!(
        context.contains("- billing.py:1 def compute_invoice(total):"),
        "{context}"
    );
    assert!(
        value["hookSpecificOutput"]
            .get("permissionDecision")
            .is_none()
    );
    assert_eq!(String::from_utf8_lossy(&grep("TODO: later").stdout), "");
}

fn session_start_with(binary: &Path, project: &Path, opt_out: Option<&str>) -> serde_json::Value {
    let mut command = Command::new(binary);
    command
        .arg("hook")
        .env("CLAUDE_PROJECT_DIR", project)
        .env_remove("CLAUDE_PLUGIN_ROOT")
        .env_remove("DISABLE_UPDATES")
        .env_remove("DISABLE_AUTOUPDATER")
        .env_remove("CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped());
    if let Some(variable) = opt_out {
        command.env(variable, "1");
    }
    let mut child = command.spawn().unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(br#"{"hook_event_name":"SessionStart","source":"startup"}"#)
        .unwrap();
    serde_json::from_slice(&child.wait_with_output().unwrap().stdout).unwrap()
}

#[test]
fn enabling_shows_the_auto_update_tip_once() {
    let (_root, binary) = installed_binary("context-wizard-context-wizard");
    let first = tempfile::tempdir().unwrap();
    let second = tempfile::tempdir().unwrap();
    let shown = decide_with(&binary, first.path(), "on");
    assert!(String::from_utf8_lossy(&shown.stdout).contains("Enable auto-update"));
    let again = decide_with(&binary, second.path(), "on");
    assert!(!String::from_utf8_lossy(&again.stdout).contains("Enable auto-update"));
}

#[test]
fn session_start_announces_a_newer_release_unless_updates_are_off() {
    let (_root, binary) = installed_binary("context-wizard-context-wizard");
    let project = tempfile::tempdir().unwrap();
    assert!(decide_with(&binary, project.path(), "on").status.success());
    let checked = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    std::fs::write(
        binary
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .join("update-check.json"),
        format!(r#"{{"checked":{checked},"latest":"999.0.0"}}"#),
    )
    .unwrap();
    let output = session_start_with(&binary, project.path(), None);
    let notice = output["systemMessage"].as_str().unwrap();
    assert!(notice.contains("999.0.0 is available"), "{notice}");
    assert!(output["hookSpecificOutput"]["additionalContext"].is_string());
    for variable in [
        "DISABLE_UPDATES",
        "DISABLE_AUTOUPDATER",
        "CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC",
    ] {
        let output = session_start_with(&binary, project.path(), Some(variable));
        assert!(output.get("systemMessage").is_none(), "{variable}");
    }
}

#[test]
fn enabled_projects_record_the_session_in_the_learning_store() {
    let project = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(project.path().join("src")).unwrap();
    let file = project.path().join("src/auth.rs");
    std::fs::write(&file, "fn login() {}\n").unwrap();
    let file = serde_json::to_string(file.to_str().unwrap()).unwrap();
    let events = [
        r#"{"hook_event_name":"UserPromptSubmit","session_id":"s1","prompt":"fix the login bug"}"#
            .to_owned(),
        format!(
            r#"{{"hook_event_name":"PostToolUse","session_id":"s1","tool_name":"Read","tool_input":{{"file_path":{file}}}}}"#
        ),
        format!(
            r#"{{"hook_event_name":"PostToolUse","session_id":"s1","tool_name":"Edit","tool_input":{{"file_path":{file},"old_string":"a","new_string":"b"}}}}"#
        ),
        r#"{"hook_event_name":"Stop","session_id":"s1"}"#.to_owned(),
    ];
    for event in &events {
        assert!(hook_with(project.path(), event).status.success());
    }
    assert!(!project.path().join(".wizard/learn.db").exists());

    assert!(decide(project.path(), "on").status.success());
    for event in &events {
        assert!(hook_with(project.path(), event).status.success());
    }
    let learn = wizard_learn::Learn::open_project(project.path()).unwrap();
    let recorded: Vec<(wizard_learn::EventKind, Option<String>)> = learn
        .events("s1")
        .unwrap()
        .into_iter()
        .map(|event| (event.kind, event.path))
        .collect();
    assert_eq!(
        recorded,
        [
            (wizard_learn::EventKind::Prompt, None),
            (
                wizard_learn::EventKind::Read,
                Some("src/auth.rs".to_owned())
            ),
            (
                wizard_learn::EventKind::Edit,
                Some("src/auth.rs".to_owned())
            ),
            (wizard_learn::EventKind::Stop, None),
        ]
    );
    assert!(learn.session("s1").unwrap().unwrap().ended.is_some());
}
