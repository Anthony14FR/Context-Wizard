//! Behaviour of the scanner on small fixture trees.

#![allow(clippy::unwrap_used)]

use std::fs;
use std::path::Path;

use wizard_scan::{ScanOptions, scan};

fn write(root: &Path, path: &str, content: &[u8]) {
    let path = root.join(path);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, content).unwrap();
}

fn paths(root: &Path) -> Vec<String> {
    scan(root, &ScanOptions::default())
        .files
        .into_iter()
        .map(|file| file.path)
        .collect()
}

#[test]
fn honours_gitignore_and_wizardignore_without_a_repository() {
    let root = tempfile::tempdir().unwrap();
    write(root.path(), ".gitignore", b"target/\n*.log\n");
    write(root.path(), ".wizardignore", b"fixtures/\n");
    write(root.path(), "src/main.rs", b"fn main() {}\n");
    write(root.path(), "target/out.rs", b"generated\n");
    write(root.path(), "debug.log", b"noise\n");
    write(root.path(), "fixtures/big.json", b"{}\n");
    write(root.path(), "app/.gitignore", b"local.txt\n");
    write(root.path(), "app/local.txt", b"ignored\n");
    write(root.path(), "app/kept.txt", b"kept\n");
    assert_eq!(
        paths(root.path()),
        [
            ".gitignore",
            ".wizardignore",
            "app/.gitignore",
            "app/kept.txt",
            "src/main.rs"
        ]
    );
}

#[test]
fn skips_vcs_and_wizard_directories_but_keeps_hidden_files() {
    let root = tempfile::tempdir().unwrap();
    write(root.path(), ".git/config", b"[core]\n");
    write(root.path(), ".wizard/index.db", b"cache\n");
    write(root.path(), ".github/workflows/ci.yml", b"name: CI\n");
    write(root.path(), ".env.example", b"KEY=\n");
    assert_eq!(
        paths(root.path()),
        [".env.example", ".github/workflows/ci.yml"]
    );
}

#[test]
fn ignores_ignore_files_above_the_root_and_skips_reserved_names_in_any_case() {
    let parent = tempfile::tempdir().unwrap();
    write(
        parent.path(),
        ".gitignore",
        b"*.md
",
    );
    write(
        parent.path(),
        ".ignore",
        b"src/
",
    );
    let root = parent.path().join("project");
    write(
        &root,
        "README.md",
        b"# Project
",
    );
    write(
        &root,
        "src/lib.rs",
        b"pub fn f() {}
",
    );
    write(
        &root,
        ".GIT/config",
        b"[core]
",
    );
    write(
        &root,
        "vendor/.git",
        b"gitdir: ../.git/modules/vendor
",
    );
    assert_eq!(paths(&root), ["README.md", "src/lib.rs"]);
}

#[test]
fn skips_binary_and_oversized_files() {
    let root = tempfile::tempdir().unwrap();
    write(root.path(), "logo.png", b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR");
    write(root.path(), "large.txt", &vec![b'a'; 2048]);
    write(root.path(), "small.txt", b"hello\n");
    let report = scan(
        root.path(),
        &ScanOptions {
            max_file_size: 1024,
        },
    );
    let files: Vec<&str> = report.files.iter().map(|file| file.path.as_str()).collect();
    assert_eq!(files, ["small.txt"]);
    assert_eq!(report.binary, 1);
    assert_eq!(report.too_large, 1);
    assert_eq!(report.files[0].size, 6);
}

#[test]
fn output_is_sorted_and_stable() {
    let root = tempfile::tempdir().unwrap();
    for index in (0..200).rev() {
        write(
            root.path(),
            &format!("dir{}/file{index:03}.txt", index % 7),
            b"x\n",
        );
    }
    let first = paths(root.path());
    let mut sorted = first.clone();
    sorted.sort();
    assert_eq!(first, sorted);
    assert_eq!(first.len(), 200);
    assert_eq!(paths(root.path()), first);
}
