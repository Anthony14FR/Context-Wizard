//! Change detection of the index across successive scans.

#![allow(clippy::unwrap_used)]

use std::fs;
use std::path::Path;
use std::time::{Duration, SystemTime};

use wizard_scan::{FileEntry, ScanOptions, scan};
use wizard_store::{Index, SCHEMA_VERSION};

fn write(root: &Path, path: &str, content: &str) {
    let path = root.join(path);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, content).unwrap();
}

fn set_modified(root: &Path, path: &str, time: SystemTime) {
    let file = fs::File::options()
        .write(true)
        .open(root.join(path))
        .unwrap();
    file.set_modified(time).unwrap();
}

fn age(root: &Path, path: &str, seconds: u64) {
    set_modified(root, path, SystemTime::now() - Duration::from_secs(seconds));
}

fn files(root: &Path) -> Vec<FileEntry> {
    scan(root, &ScanOptions::default()).files
}

#[test]
fn reports_added_modified_removed_and_unchanged_files() {
    let project = tempfile::tempdir().unwrap();
    let root = project.path();
    for (path, content) in [
        ("src/a.rs", "fn a() {}\n"),
        ("src/b.rs", "fn b() {}\n"),
        ("src/c.rs", "fn c() {}\n"),
    ] {
        write(root, path, content);
        age(root, path, 600);
    }
    let mut index = Index::open_project(root).unwrap();
    let first = index.refresh(root, &files(root)).unwrap();
    assert_eq!(first.added, ["src/a.rs", "src/b.rs", "src/c.rs"]);
    assert_eq!(index.len().unwrap(), 3);

    write(root, "src/b.rs", "fn b() { changed() }\n");
    age(root, "src/b.rs", 300);
    age(root, "src/c.rs", 300);
    fs::remove_file(root.join("src/a.rs")).unwrap();
    write(root, "src/d.rs", "fn d() {}\n");
    age(root, "src/d.rs", 300);
    let second = index.refresh(root, &files(root)).unwrap();
    assert_eq!(second.added, ["src/d.rs"]);
    assert_eq!(second.modified, ["src/b.rs"]);
    assert_eq!(second.removed, ["src/a.rs"]);
    assert_eq!(second.unchanged, 1);

    let third = index.refresh(root, &files(root)).unwrap();
    assert!(third.is_empty());
    assert_eq!(third.unchanged, 3);
}

#[test]
fn keeps_the_index_across_reopens_and_never_commits_it() {
    let project = tempfile::tempdir().unwrap();
    let root = project.path();
    write(root, "main.py", "print('hi')\n");
    age(root, "main.py", 600);
    {
        let mut index = Index::open_project(root).unwrap();
        index.refresh(root, &files(root)).unwrap();
    }
    let mut index = Index::open_project(root).unwrap();
    assert_eq!(index.len().unwrap(), 1);
    assert_eq!(
        index.hash("main.py").unwrap().unwrap(),
        *blake3::hash(b"print('hi')\n").as_bytes()
    );
    assert!(index.refresh(root, &files(root)).unwrap().is_empty());
    assert_eq!(
        fs::read_to_string(root.join(".wizard/.gitignore")).unwrap(),
        "*\n"
    );
}

#[test]
fn rereads_files_whose_timestamp_is_too_recent_to_trust() {
    let project = tempfile::tempdir().unwrap();
    let root = project.path();
    let instant = SystemTime::now();
    write(root, "racy.txt", "one\n");
    set_modified(root, "racy.txt", instant);
    let mut index = Index::open_project(root).unwrap();
    index.refresh(root, &files(root)).unwrap();
    write(root, "racy.txt", "two\n");
    set_modified(root, "racy.txt", instant);
    let changes = index.refresh(root, &files(root)).unwrap();
    assert_eq!(changes.modified, ["racy.txt"]);
}

#[test]
fn rebuilds_on_schema_change() {
    let project = tempfile::tempdir().unwrap();
    let root = project.path();
    write(root, "lib.go", "package lib\n");
    {
        let mut index = Index::open_project(root).unwrap();
        index.refresh(root, &files(root)).unwrap();
    }
    let connection = rusqlite::Connection::open(root.join(".wizard/index.db")).unwrap();
    connection
        .pragma_update(None, "user_version", SCHEMA_VERSION + 1)
        .unwrap();
    drop(connection);
    let index = Index::open_project(root).unwrap();
    assert!(index.is_empty().unwrap());
}

#[test]
fn rebuilds_a_corrupt_database() {
    let project = tempfile::tempdir().unwrap();
    let root = project.path();
    fs::create_dir_all(root.join(".wizard")).unwrap();
    fs::write(
        root.join(".wizard/index.db"),
        "this is not a sqlite database, only some text that is long enough to read",
    )
    .unwrap();
    let index = Index::open_project(root).unwrap();
    assert!(index.is_empty().unwrap());
}

#[test]
fn concurrent_opens_share_one_schema() {
    let project = tempfile::tempdir().unwrap();
    let root = project.path().to_owned();
    let openers: Vec<_> = (0..8)
        .map(|_| {
            let root = root.clone();
            std::thread::spawn(move || {
                Index::open_project(&root)
                    .and_then(|index| index.len())
                    .unwrap()
            })
        })
        .collect();
    for opener in openers {
        assert_eq!(opener.join().unwrap(), 0);
    }
}
