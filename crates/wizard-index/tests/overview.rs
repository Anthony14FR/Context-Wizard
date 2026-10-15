//! The L0 overview on representative projects.

#![allow(clippy::unwrap_used)]

use std::fs;
use std::path::Path;
use std::time::{Duration, SystemTime};

use wizard_budget::{Content, Estimator};
use wizard_index::build::{BuildOptions, build};
use wizard_index::overview::{L0_BUDGET, overview};
use wizard_store::Index;

fn write(root: &Path, path: &str, content: &str) {
    let path = root.join(path);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&path, content).unwrap();
    fs::File::options()
        .write(true)
        .open(&path)
        .unwrap()
        .set_modified(SystemTime::now() - Duration::from_secs(600))
        .unwrap();
}

fn render(root: &Path) -> String {
    build(root, &BuildOptions::default()).unwrap();
    overview(root, &Index::open_project(root).unwrap()).unwrap()
}

#[test]
fn describes_a_monorepo() {
    let project = tempfile::tempdir().unwrap();
    let root = project.path();
    write(
        root,
        "Cargo.toml",
        "[workspace]\nmembers = [\"crates/*\"]\n",
    );
    write(root, "crates/cli/Cargo.toml", "[package]\nname = \"cli\"\n");
    write(root, "crates/cli/src/main.rs", "fn main() {}\n");
    write(
        root,
        "web/package.json",
        "{\"scripts\": {\"dev\": \"vite\", \"build\": \"vite build\"}}",
    );
    write(root, "web/pnpm-lock.yaml", "lockfileVersion: 9\n");
    write(root, "web/src/main.ts", "export const app = 1;\n");
    write(root, "docs/guide.md", "# Guide\n");
    write(
        root,
        "Makefile",
        "build:\n\tcargo build\n.PHONY: build\nVERSION := 1\n",
    );
    let text = render(root);
    assert!(
        text.starts_with("# Project overview (Context Wizard)\n"),
        "{text}"
    );
    assert!(text.contains("Ecosystems: Node, Rust."), "{text}");
    assert!(text.contains("- crates/cli: Rust"), "{text}");
    assert!(text.contains("- crates/cli/src/main.rs"), "{text}");
    assert!(text.contains("- web/src/main.ts"), "{text}");
    assert!(text.contains("- (web) pnpm run dev"), "{text}");
    assert!(text.contains("- make build"), "{text}");
    assert!(!text.contains("make VERSION"), "{text}");
    assert!(
        text.contains("- docs/: 1 files, mostly markdown, documentation"),
        "{text}"
    );
    assert_eq!(render(root), text);
}

#[test]
fn never_exceeds_the_budget() {
    let project = tempfile::tempdir().unwrap();
    let root = project.path();
    let scripts: Vec<String> = (0..200)
        .map(|index| format!("\"script{index}\": \"x\""))
        .collect();
    write(
        root,
        "package.json",
        &format!("{{\"scripts\": {{{}}}}}", scripts.join(",")),
    );
    for index in 0..60 {
        write(root, &format!("folder{index:02}/index.ts"), "export {};\n");
        write(root, &format!("folder{index:02}/package.json"), "{}\n");
    }
    let text = render(root);
    assert!(Estimator::default().estimate(&text, Content::Data) <= L0_BUDGET);
    assert!(text.contains("## Sub-projects"));
}
