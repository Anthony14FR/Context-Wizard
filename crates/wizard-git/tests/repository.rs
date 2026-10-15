//! History signals read from a real repository created for the test.

#![allow(clippy::unwrap_used)]

use std::fs;
use std::path::Path;
use std::process::Command;

use wizard_git::{HistoryOptions, history};

fn git(root: &Path, arguments: &[&str]) {
    let status = Command::new("git")
        .arg("-C")
        .arg(root)
        .args([
            "-c",
            "user.name=Test",
            "-c",
            "user.email=test@example.com",
            "-c",
            "commit.gpgsign=false",
        ])
        .args(arguments)
        .output()
        .unwrap()
        .status;
    assert!(status.success(), "git {arguments:?}");
}

#[test]
fn reads_churn_co_change_and_renames() {
    let repository = tempfile::tempdir().unwrap();
    let root = repository.path();
    git(root, &["init", "--quiet"]);
    fs::write(
        root.join("old.rs"),
        "pub fn one() {}\npub fn two() {}\npub fn three() {}\n",
    )
    .unwrap();
    fs::write(root.join("lib.rs"), "mod old;\n").unwrap();
    git(root, &["add", "."]);
    git(root, &["commit", "--quiet", "-m", "first"]);
    git(root, &["mv", "old.rs", "new.rs"]);
    fs::write(root.join("lib.rs"), "mod new;\n").unwrap();
    git(root, &["add", "."]);
    git(root, &["commit", "--quiet", "-m", "rename"]);

    let history = history(root, &HistoryOptions::default()).unwrap();
    assert_eq!(history.commits, 2);
    assert_eq!(history.churn["new.rs"].commits, 2);
    assert_eq!(history.churn["lib.rs"].commits, 2);
    assert!(!history.churn.contains_key("old.rs"));
    assert_eq!(
        history.co_change[&("lib.rs".to_owned(), "new.rs".to_owned())],
        2
    );
}

#[test]
fn outside_a_repository_there_is_no_history() {
    let directory = tempfile::tempdir().unwrap();
    assert!(history(directory.path(), &HistoryOptions::default()).is_none());
}
