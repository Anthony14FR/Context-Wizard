//! The answers behind the MCP tools, on a small indexed project.

#![allow(clippy::unwrap_used)]

use std::fs;
use std::path::Path;
use std::time::{Duration, SystemTime};

use wizard_git::History;
use wizard_index::build::{BuildOptions, build};
use wizard_index::learned::{Learned, LearnedPair};
use wizard_index::query::{explain, find_symbol, impact, refs, slice};
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

fn project() -> (tempfile::TempDir, Index) {
    let project = tempfile::tempdir().unwrap();
    let root = project.path();
    write(
        root,
        "src/session.py",
        "class SessionStore:\n    def refresh(self):\n        return load_token()\n\ndef load_token():\n    return None\n",
    );
    write(
        root,
        "src/login.py",
        "from session import SessionStore\n\ndef login():\n    store = SessionStore()\n    store.refresh()\n",
    );
    write(
        root,
        "src/app.py",
        "from login import login\n\ndef main():\n    login()\n",
    );
    write(root, "docs/guide.md", "# Guide\n\n## Sessions\n");
    build(root, &BuildOptions::default()).unwrap();
    let index = Index::open_project(root).unwrap();
    (project, index)
}

#[test]
fn finds_symbols_exactly_then_by_substring() {
    let (_project, index) = project();
    let exact = find_symbol(&index, "SessionStore").unwrap();
    assert!(
        exact.contains("- src/session.py:1 class SessionStore:"),
        "{exact}"
    );
    let similar = find_symbol(&index, "token").unwrap();
    assert!(similar.contains("Similar names"), "{similar}");
    assert!(similar.contains("src/session.py:5"), "{similar}");
    assert!(
        find_symbol(&index, "nothing_like_this")
            .unwrap()
            .starts_with("No definition")
    );
}

#[test]
fn lists_references_by_file() {
    let (_project, index) = project();
    let text = refs(&index, "login").unwrap();
    assert!(text.contains("Defined in:\n- src/login.py:3"), "{text}");
    assert!(text.contains("- src/app.py: lines 4"), "{text}");
}

#[test]
fn explains_a_file_with_its_neighbours() {
    let (_project, index) = project();
    let text = explain(&index, "src/login.py").unwrap();
    assert!(text.starts_with("# src/login.py (python"), "{text}");
    assert!(text.contains("def login():"), "{text}");
    assert!(text.contains("Uses (1):\n- src/session.py"), "{text}");
    assert!(text.contains("Used by (1):\n- src/app.py"), "{text}");
    assert!(
        explain(&index, "missing.py")
            .unwrap()
            .contains("is not indexed")
    );
}

#[test]
fn impact_follows_dependents_and_co_changes() {
    let (_project, index) = project();
    let mut history = History::default();
    history
        .co_change
        .insert(("docs/guide.md".to_owned(), "src/session.py".to_owned()), 4);
    let learned = Learned {
        co_access: vec![LearnedPair {
            first: "src/config.py".to_owned(),
            second: "src/session.py".to_owned(),
            weight: 2.0,
        }],
    };
    let text = impact(&index, "src/session.py", Some(&history), &learned).unwrap();
    assert!(
        text.contains(
            "Usually opened with, in past sessions:
- src/config.py"
        ),
        "{text}"
    );
    assert!(
        text.contains("Direct dependents (1):\n- src/login.py"),
        "{text}"
    );
    assert!(
        text.contains("Indirect dependents (1):\n- src/app.py"),
        "{text}"
    );
    assert!(text.contains("- docs/guide.md (4 commits)"), "{text}");
}

#[test]
fn slices_the_files_of_a_task() {
    let (_project, index) = project();
    let text = slice(
        &index,
        "the session refresh fails after login",
        None,
        &Learned::default(),
    )
    .unwrap();
    let first = text.lines().nth(2).unwrap();
    assert!(
        first == "src/session.py" || first == "src/login.py",
        "{text}"
    );
    assert!(text.contains("class SessionStore:"), "{text}");
    assert!(
        slice(&index, "the and for", None, &Learned::default())
            .unwrap()
            .starts_with("Describe the task")
    );
}
