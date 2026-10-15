//! Tests of the plugin's polyglot `SessionStart` launcher in every shell Claude Code may use.

#![allow(clippy::unwrap_used)]

use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

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

fn run(shell: &[String], data: &Path) -> Output {
    Command::new(&shell[0])
        .args(&shell[1..])
        .arg(launcher_command())
        .env("CLAUDE_PLUGIN_DATA", data)
        .stdin(Stdio::null())
        .output()
        .unwrap()
}

#[test]
fn launcher_is_silent_without_binary() {
    let data = tempfile::tempdir().unwrap();
    for shell in shells() {
        let output = run(&shell, data.path());
        assert!(output.status.success(), "{shell:?}: {output:?}");
        assert!(output.stdout.is_empty(), "{shell:?}: {output:?}");
        assert!(output.stderr.is_empty(), "{shell:?}: {output:?}");
    }
}
