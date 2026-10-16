//! The MCP server spoken to over standard input and output, as Claude Code does.

#![allow(clippy::unwrap_used, clippy::panic)]

use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

use serde_json::{Value, json};

struct Session {
    child: Child,
    input: ChildStdin,
    output: BufReader<ChildStdout>,
}

impl Session {
    fn start(project: &std::path::Path) -> Self {
        let mut child = Command::new(env!("CARGO_BIN_EXE_wizard"))
            .args(["serve", "--project"])
            .arg(project)
            .env_remove("CLAUDE_PLUGIN_ROOT")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let input = child.stdin.take().unwrap();
        let output = BufReader::new(child.stdout.take().unwrap());
        let mut session = Self {
            child,
            input,
            output,
        };
        let initialised = session.request(
            1,
            "initialize",
            &json!({
                "protocolVersion": "2025-06-18",
                "capabilities": {},
                "clientInfo": { "name": "test", "version": "1.0.0" }
            }),
        );
        assert!(
            initialised["result"]["capabilities"]["tools"].is_object(),
            "{initialised}"
        );
        session.send(&json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }));
        session
    }

    fn send(&mut self, message: &Value) {
        writeln!(self.input, "{message}").unwrap();
        self.input.flush().unwrap();
    }

    fn request(&mut self, id: u64, method: &str, params: &Value) -> Value {
        self.send(&json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params }));
        loop {
            let mut line = String::new();
            assert!(
                self.output.read_line(&mut line).unwrap() > 0,
                "server closed"
            );
            let message: Value = serde_json::from_str(&line).unwrap();
            if message["id"] == json!(id) {
                return message;
            }
        }
    }

    fn call(&mut self, id: u64, tool: &str, arguments: &Value) -> String {
        let response = self.request(
            id,
            "tools/call",
            &json!({ "name": tool, "arguments": arguments }),
        );
        response["result"]["content"][0]["text"]
            .as_str()
            .unwrap_or_else(|| panic!("{response}"))
            .to_owned()
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[test]
fn serves_the_tools_of_an_enabled_project() {
    let project = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(project.path().join("src")).unwrap();
    std::fs::write(
        project.path().join("src/billing.py"),
        "def compute_invoice(total):\n    return round(total, 2)\n",
    )
    .unwrap();
    std::fs::write(
        project.path().join("src/api.py"),
        "from billing import compute_invoice\n\ndef handler():\n    return compute_invoice(3)\n",
    )
    .unwrap();
    let decided = Command::new(env!("CARGO_BIN_EXE_wizard"))
        .args(["decide", "on", "--project"])
        .arg(project.path())
        .output()
        .unwrap();
    assert!(decided.status.success());

    let mut session = Session::start(project.path());
    let tools = session.request(2, "tools/list", &json!({}));
    let mut names: Vec<&str> = tools["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|tool| tool["name"].as_str().unwrap())
        .collect();
    names.sort_unstable();
    assert_eq!(
        names,
        [
            "exact_refs",
            "explain",
            "facts",
            "find_symbol",
            "impact",
            "map",
            "overview",
            "refs",
            "slice"
        ]
    );
    let found = session.call(3, "find_symbol", &json!({ "name": "compute_invoice" }));
    assert!(
        found.contains("src/billing.py:1 def compute_invoice(total):"),
        "{found}"
    );
    let used = session.call(4, "refs", &json!({ "name": "compute_invoice" }));
    assert!(used.contains("- src/api.py: lines"), "{used}");
    let overview = session.call(5, "overview", &json!({}));
    assert!(
        overview.starts_with("# Project overview (Context Wizard)"),
        "{overview}"
    );
}

#[test]
fn stays_inert_on_a_project_that_is_not_enabled() {
    let project = tempfile::tempdir().unwrap();
    let mut session = Session::start(project.path());
    let answer = session.call(2, "map", &json!({}));
    assert!(answer.contains("not enabled"), "{answer}");
    assert!(!project.path().join(".wizard").exists());
}
