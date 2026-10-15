//! End-to-end tests of the `wizard` binary.

use std::process::Command;

fn wizard() -> Command {
    Command::new(env!("CARGO_BIN_EXE_wizard"))
}

#[test]
fn prints_version() {
    let output = wizard().arg("--version").output().unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert_eq!(
        stdout.trim(),
        format!("wizard {}", env!("CARGO_PKG_VERSION"))
    );
}

#[test]
fn prints_help() {
    let output = wizard().arg("--help").output().unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("Usage: wizard"));
}

#[test]
fn rejects_unknown_arguments() {
    let output = wizard().arg("--unknown").output().unwrap();
    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains("unexpected argument"));
}

#[test]
fn hook_is_silent_on_any_input() {
    for input in [
        &b"{\"hook_event_name\":\"SessionStart\"}"[..],
        b"not json",
        b"",
    ] {
        let mut child = wizard()
            .arg("hook")
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        std::io::Write::write_all(&mut child.stdin.take().unwrap(), input).unwrap();
        let output = child.wait_with_output().unwrap();
        assert!(output.status.success());
        assert_eq!(String::from_utf8_lossy(&output.stdout), "");
    }
}
