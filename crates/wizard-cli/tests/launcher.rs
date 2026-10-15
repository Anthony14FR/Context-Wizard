//! Tests of the plugin's polyglot `SessionStart` launcher in every shell Claude Code may use.

#![allow(clippy::unwrap_used)]

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

const PROMPT: &str = r#"{"hook_event_name":"UserPromptSubmit","prompt":"café ✓"}"#;

fn plugin_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../plugin")
}

fn launcher_command() -> String {
    let hooks = std::fs::read_to_string(plugin_root().join("hooks/hooks.json")).unwrap();
    let hooks: serde_json::Value = serde_json::from_str(&hooks).unwrap();
    let command = hooks["hooks"]["SessionStart"][0]["hooks"][0]["command"]
        .as_str()
        .unwrap();
    let root = plugin_root().canonicalize().unwrap();
    let root = root.to_string_lossy().replace('\\', "/");
    let root = root.strip_prefix("//?/").unwrap_or(&root).to_owned();
    command.replace("${CLAUDE_PLUGIN_ROOT}", &root)
}

fn git_bash() -> Option<PathBuf> {
    std::env::var_os("CLAUDE_CODE_GIT_BASH_PATH")
        .map(PathBuf::from)
        .into_iter()
        .chain([
            PathBuf::from("C:/Program Files/Git/bin/bash.exe"),
            PathBuf::from("C:/Program Files (x86)/Git/bin/bash.exe"),
        ])
        .find(|path| path.is_file())
}

fn shells() -> Vec<Vec<String>> {
    let owned = |parts: &[&str]| parts.iter().map(ToString::to_string).collect();
    if cfg!(windows) {
        let mut shells = vec![owned(&[
            "powershell.exe",
            "-NoProfile",
            "-NonInteractive",
            "-ExecutionPolicy",
            "Bypass",
            "-Command",
        ])];
        if let Some(bash) = git_bash() {
            shells.push(vec![bash.to_string_lossy().into_owned(), "-c".to_owned()]);
        }
        shells
    } else {
        vec![owned(&["sh", "-c"])]
    }
}

fn run(shell: &[String], data: &Path, project: &Path, input: &str) -> Output {
    let mut child = Command::new(&shell[0])
        .args(&shell[1..])
        .arg(launcher_command())
        .env("CLAUDE_PLUGIN_DATA", data)
        .env("CLAUDE_PROJECT_DIR", project)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
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

fn install_binary(data: &Path) {
    let name = if cfg!(windows) {
        "wizard.exe"
    } else {
        "wizard"
    };
    let bin = data.join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    std::fs::copy(env!("CARGO_BIN_EXE_wizard"), bin.join(name)).unwrap();
}

#[test]
fn launcher_is_silent_without_binary() {
    let data = tempfile::tempdir().unwrap();
    let project = tempfile::tempdir().unwrap();
    for shell in shells() {
        let output = run(&shell, data.path(), project.path(), PROMPT);
        assert!(output.status.success(), "{shell:?}: {output:?}");
        assert!(output.stdout.is_empty(), "{shell:?}: {output:?}");
        assert!(output.stderr.is_empty(), "{shell:?}: {output:?}");
    }
}

#[test]
fn launcher_hands_the_event_to_the_installed_binary() {
    let data = tempfile::tempdir().unwrap();
    let project = tempfile::tempdir().unwrap();
    install_binary(data.path());
    for shell in shells() {
        let output = run(&shell, data.path(), project.path(), PROMPT);
        assert!(output.status.success(), "{shell:?}: {output:?}");
        let value: serde_json::Value = serde_json::from_slice(&output.stdout)
            .unwrap_or_else(|error| panic!("{shell:?}: {error}: {output:?}"));
        let context = value["hookSpecificOutput"]["additionalContext"]
            .as_str()
            .unwrap();
        assert!(context.contains("decide on --project"), "{shell:?}");
    }
}
