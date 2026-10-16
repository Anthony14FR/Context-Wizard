//! Tests of the plugin's polyglot `SessionStart` launcher in every shell Claude Code may use:
//! hand-off to the installed binary, first-run download, checksum check, proxy support and
//! retention of previous versions.
//!
//! Shell-form hook commands reach sh and bash unsubstituted, with `CLAUDE_PLUGIN_ROOT` in the
//! environment, and reach PowerShell with the path substituted, as Claude Code runs them.

#![allow(clippy::unwrap_used, clippy::panic)]

use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::thread;
use std::time::Duration;

const PROMPT: &str = r#"{"hook_event_name":"UserPromptSubmit","prompt":"café ✓"}"#;
const TARGETS: [&str; 6] = [
    "aarch64-apple-darwin",
    "aarch64-pc-windows-msvc",
    "aarch64-unknown-linux-musl",
    "x86_64-apple-darwin",
    "x86_64-pc-windows-msvc",
    "x86_64-unknown-linux-musl",
];

fn binary_name() -> &'static str {
    if cfg!(windows) {
        "wizard.exe"
    } else {
        "wizard"
    }
}

fn forward_slashes(path: &Path) -> String {
    let path = path.canonicalize().unwrap();
    let text = path.to_string_lossy().replace('\\', "/");
    text.strip_prefix("//?/").unwrap_or(&text).to_owned()
}

fn raw_launcher_command(root: &Path) -> String {
    let hooks = std::fs::read_to_string(root.join("hooks/hooks.json")).unwrap();
    let hooks: serde_json::Value = serde_json::from_str(&hooks).unwrap();
    hooks["hooks"]["SessionStart"][0]["hooks"][0]["command"]
        .as_str()
        .unwrap()
        .to_owned()
}

fn launcher_command(shell: &[String], root: &Path) -> String {
    let raw = raw_launcher_command(root);
    if shell[0].contains("powershell") || shell[0].contains("pwsh") {
        raw.replace("${CLAUDE_PLUGIN_ROOT}", &forward_slashes(root))
    } else {
        raw
    }
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
        if Command::new("pwsh.exe").arg("-Version").output().is_ok() {
            shells.push(owned(&[
                "pwsh.exe",
                "-NoProfile",
                "-NonInteractive",
                "-ExecutionPolicy",
                "Bypass",
                "-Command",
            ]));
        }
        if let Some(bash) = git_bash() {
            shells.push(vec![bash.to_string_lossy().into_owned(), "-c".to_owned()]);
        }
        shells
    } else {
        vec![owned(&["sh", "-c"])]
    }
}

struct Launch<'a> {
    root: &'a Path,
    data: &'a Path,
    project: &'a Path,
    env: Vec<(&'a str, String)>,
}

impl Launch<'_> {
    fn run(&self, shell: &[String]) -> Output {
        let mut command = Command::new(&shell[0]);
        command
            .args(&shell[1..])
            .arg(launcher_command(shell, self.root))
            .env("CLAUDE_PLUGIN_ROOT", forward_slashes(self.root))
            .env("CLAUDE_PLUGIN_DATA", self.data)
            .env("CLAUDE_PROJECT_DIR", self.project)
            .env_remove("HTTPS_PROXY")
            .env_remove("HTTP_PROXY")
            .env_remove("https_proxy")
            .env_remove("http_proxy")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        for (key, value) in &self.env {
            command.env(key, value);
        }
        let mut child = command.spawn().unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(PROMPT.as_bytes())
            .unwrap();
        child.wait_with_output().unwrap()
    }
}

fn sha256(path: &Path) -> String {
    use sha2::Digest;
    let digest = sha2::Sha256::digest(std::fs::read(path).unwrap());
    digest.iter().fold(String::new(), |mut hex, byte| {
        let _ = std::fmt::Write::write_fmt(&mut hex, format_args!("{byte:02x}"));
        hex
    })
}

fn plugin_copy(url: &str, sha: &str) -> tempfile::TempDir {
    let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../plugin");
    let root = tempfile::tempdir().unwrap();
    for directory in ["hooks", "scripts"] {
        std::fs::create_dir_all(root.path().join(directory)).unwrap();
        for entry in std::fs::read_dir(source.join(directory)).unwrap() {
            let entry = entry.unwrap();
            std::fs::copy(
                entry.path(),
                root.path().join(directory).join(entry.file_name()),
            )
            .unwrap();
        }
    }
    let lock = serde_json::json!({
        "version": env!("CARGO_PKG_VERSION"),
        "url": url,
        "sha256": TARGETS.iter().map(|target| (target.to_string(), sha.into())).collect::<serde_json::Map<_, _>>(),
    });
    std::fs::write(
        root.path().join("binaries.lock.json"),
        serde_json::to_string_pretty(&lock).unwrap(),
    )
    .unwrap();
    root
}

struct Server {
    address: String,
    hits: Arc<AtomicUsize>,
}

fn serve_binary() -> Server {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap().to_string();
    let body = std::fs::read(env!("CARGO_BIN_EXE_wizard")).unwrap();
    let hits = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&hits);
    thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut request_line = String::new();
            let _ = reader.read_line(&mut request_line);
            let mut header = String::new();
            while reader.read_line(&mut header).is_ok_and(|read| read > 2) {
                header.clear();
            }
            let path = request_line.split_whitespace().nth(1).unwrap_or_default();
            let known = TARGETS.iter().any(|target| {
                path.ends_with(&format!("/wizard-{target}"))
                    || path.ends_with(&format!("/wizard-{target}.exe"))
            });
            if known {
                counter.fetch_add(1, Ordering::SeqCst);
                let _ = write!(
                    stream,
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                );
                let _ = stream.write_all(&body);
            } else {
                let _ = stream.write_all(
                    b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                );
            }
        }
    });
    Server { address, hits }
}

fn assert_question(shell: &[String], output: &Output) {
    assert!(output.status.success(), "{shell:?}: {output:?}");
    let value: serde_json::Value = serde_json::from_slice(&output.stdout)
        .unwrap_or_else(|error| panic!("{shell:?}: {error}: {output:?}"));
    let context = value["hookSpecificOutput"]["additionalContext"]
        .as_str()
        .unwrap();
    assert!(context.contains("decide on --project"), "{shell:?}");
}

fn assert_silent(shell: &[String], output: &Output) {
    assert!(output.status.success(), "{shell:?}: {output:?}");
    assert!(output.stdout.is_empty(), "{shell:?}: {output:?}");
    assert!(output.stderr.is_empty(), "{shell:?}: {output:?}");
}

#[test]
fn launcher_is_silent_without_binary_or_checksum() {
    let root = plugin_copy("http://127.0.0.1:9", "");
    for shell in shells() {
        let data = tempfile::tempdir().unwrap();
        let project = tempfile::tempdir().unwrap();
        let launch = Launch {
            root: root.path(),
            data: data.path(),
            project: project.path(),
            env: Vec::new(),
        };
        assert_silent(&shell, &launch.run(&shell));
    }
}

#[test]
fn launcher_downloads_verifies_and_runs_the_binary() {
    let server = serve_binary();
    let sha = sha256(Path::new(env!("CARGO_BIN_EXE_wizard")));
    let root = plugin_copy(&format!("http://{}/v1", server.address), &sha);
    for shell in shells() {
        let data = tempfile::tempdir().unwrap();
        let project = tempfile::tempdir().unwrap();
        let launch = Launch {
            root: root.path(),
            data: data.path(),
            project: project.path(),
            env: Vec::new(),
        };
        let hits = server.hits.load(Ordering::SeqCst);
        let output = launch.run(&shell);
        let error =
            std::fs::read_to_string(data.path().join("bin/install-error")).unwrap_or_default();
        assert!(error.is_empty(), "{shell:?}: {error}");
        assert_question(&shell, &output);
        assert_eq!(server.hits.load(Ordering::SeqCst), hits + 1, "{shell:?}");
        let bin = data.path().join("bin");
        assert!(bin.join(binary_name()).is_file(), "{shell:?}");
        assert!(
            bin.join(env!("CARGO_PKG_VERSION"))
                .join(binary_name())
                .is_file()
        );
        let current = std::fs::read_to_string(bin.join("current")).unwrap();
        assert_eq!(current.trim(), env!("CARGO_PKG_VERSION"), "{shell:?}");
        assert_question(&shell, &launch.run(&shell));
        assert_eq!(server.hits.load(Ordering::SeqCst), hits + 1, "{shell:?}");
    }
}

#[test]
fn launcher_keeps_two_previous_versions() {
    let server = serve_binary();
    let sha = sha256(Path::new(env!("CARGO_BIN_EXE_wizard")));
    let root = plugin_copy(&format!("http://{}/v1", server.address), &sha);
    for shell in shells() {
        let data = tempfile::tempdir().unwrap();
        let project = tempfile::tempdir().unwrap();
        let bin = data.path().join("bin");
        for old in ["0.0.1", "0.0.2", "0.0.3", "0.0.4"] {
            std::fs::create_dir_all(bin.join(old)).unwrap();
            std::fs::write(bin.join(old).join(binary_name()), b"old").unwrap();
            thread::sleep(Duration::from_millis(1100));
        }
        let launch = Launch {
            root: root.path(),
            data: data.path(),
            project: project.path(),
            env: Vec::new(),
        };
        assert_question(&shell, &launch.run(&shell));
        let mut kept: Vec<String> = std::fs::read_dir(&bin)
            .unwrap()
            .filter_map(Result::ok)
            .filter(|entry| entry.path().is_dir())
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .collect();
        kept.sort_unstable();
        let mut expected = vec![
            "0.0.3".to_owned(),
            "0.0.4".to_owned(),
            env!("CARGO_PKG_VERSION").to_owned(),
        ];
        expected.sort_unstable();
        assert_eq!(kept, expected, "{shell:?}");
    }
}

#[test]
fn launcher_rejects_a_checksum_mismatch() {
    let server = serve_binary();
    let root = plugin_copy(&format!("http://{}/v1", server.address), &"0".repeat(64));
    for shell in shells() {
        let data = tempfile::tempdir().unwrap();
        let project = tempfile::tempdir().unwrap();
        let launch = Launch {
            root: root.path(),
            data: data.path(),
            project: project.path(),
            env: Vec::new(),
        };
        assert_silent(&shell, &launch.run(&shell));
        assert!(
            !data.path().join("bin").join(binary_name()).exists(),
            "{shell:?}"
        );
    }
}

#[test]
fn launcher_downloads_through_the_configured_proxy() {
    let proxy = serve_binary();
    let sha = sha256(Path::new(env!("CARGO_BIN_EXE_wizard")));
    let root = plugin_copy("http://releases.invalid/v1", &sha);
    for shell in shells() {
        let data = tempfile::tempdir().unwrap();
        let project = tempfile::tempdir().unwrap();
        let launch = Launch {
            root: root.path(),
            data: data.path(),
            project: project.path(),
            env: vec![
                ("HTTP_PROXY", format!("http://{}", proxy.address)),
                ("http_proxy", format!("http://{}", proxy.address)),
            ],
        };
        let output = launch.run(&shell);
        let error =
            std::fs::read_to_string(data.path().join("bin/install-error")).unwrap_or_default();
        assert!(error.is_empty(), "{shell:?}: {error}");
        assert_question(&shell, &output);
    }
}

#[test]
fn launcher_keeps_non_ascii_output_intact() {
    let data = tempfile::tempdir().unwrap();
    let parent = tempfile::tempdir().unwrap();
    let project = parent.path().join("projet-été");
    std::fs::create_dir_all(&project).unwrap();
    let bin = data.path().join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    std::fs::copy(env!("CARGO_BIN_EXE_wizard"), bin.join(binary_name())).unwrap();
    wait_until_executable(&bin.join(binary_name()));
    let root = plugin_copy("http://127.0.0.1:9", "");
    for shell in shells() {
        let launch = Launch {
            root: root.path(),
            data: data.path(),
            project: &project,
            env: Vec::new(),
        };
        let output = launch.run(&shell);
        assert_question(&shell, &output);
        let text = String::from_utf8(output.stdout).unwrap();
        assert!(text.contains("projet-été"), "{shell:?}: {text}");
    }
}

fn wait_until_executable(binary: &Path) {
    for _ in 0..40 {
        match Command::new(binary).arg("--version").output() {
            Err(error) if error.kind() == std::io::ErrorKind::ExecutableFileBusy => {
                thread::sleep(Duration::from_millis(50));
            }
            _ => return,
        }
    }
}
