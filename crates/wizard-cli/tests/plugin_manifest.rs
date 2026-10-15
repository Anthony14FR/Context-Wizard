//! Consistency checks between the plugin manifest, the pinned binary manifest and this crate.

#![allow(clippy::unwrap_used)]

use std::path::Path;

fn plugin_file(name: &str) -> String {
    std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../plugin")
            .join(name),
    )
    .unwrap()
}

fn json(name: &str) -> serde_json::Value {
    serde_json::from_str(&plugin_file(name)).unwrap()
}

#[test]
fn plugin_lock_and_binary_share_one_version() {
    let version = env!("CARGO_PKG_VERSION");
    assert_eq!(json(".claude-plugin/plugin.json")["version"], version);
    let lock = json("binaries.lock.json");
    assert_eq!(lock["version"], version);
    assert_eq!(
        lock["url"],
        format!("https://github.com/Anthony14FR/Context-Wizard/releases/download/v{version}")
    );
}

#[test]
fn lock_pins_every_supported_target() {
    let lock = json("binaries.lock.json");
    let targets: Vec<&String> = lock["sha256"].as_object().unwrap().keys().collect();
    assert_eq!(
        targets,
        [
            "aarch64-apple-darwin",
            "aarch64-pc-windows-msvc",
            "aarch64-unknown-linux-musl",
            "x86_64-apple-darwin",
            "x86_64-pc-windows-msvc",
            "x86_64-unknown-linux-musl",
        ]
    );
    for sha in lock["sha256"].as_object().unwrap().values() {
        let sha = sha.as_str().unwrap();
        assert!(
            sha.is_empty() || (sha.len() == 64 && sha.bytes().all(|byte| byte.is_ascii_hexdigit()))
        );
    }
}

#[test]
fn lock_keeps_one_key_per_line_for_the_posix_launcher() {
    let text = plugin_file("binaries.lock.json");
    let lock = json("binaries.lock.json");
    for key in ["version", "url"] {
        assert_eq!(
            text.lines()
                .filter(|line| line.trim_start().starts_with(&format!("\"{key}\":")))
                .count(),
            1
        );
    }
    for target in lock["sha256"].as_object().unwrap().keys() {
        assert!(
            text.lines()
                .any(|line| line.trim_start().starts_with(&format!("\"{target}\": \"")))
        );
    }
}
