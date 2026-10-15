//! Storage and queries of extracted symbols.

#![allow(clippy::unwrap_used)]

use std::fs;
use std::path::Path;
use std::time::{Duration, SystemTime};

use wizard_scan::{ScanOptions, scan};
use wizard_store::{FileSymbols, Index, SymbolKind, SymbolRecord};

fn project(files: &[(&str, &str)]) -> tempfile::TempDir {
    let project = tempfile::tempdir().unwrap();
    for (path, content) in files {
        let path = project.path().join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, content).unwrap();
        fs::File::options()
            .write(true)
            .open(&path)
            .unwrap()
            .set_modified(SystemTime::now() - Duration::from_secs(600))
            .unwrap();
    }
    project
}

fn refreshed(root: &Path) -> Index {
    let mut index = Index::open_project(root).unwrap();
    index
        .refresh(root, &scan(root, &ScanOptions::default()).files)
        .unwrap();
    index
}

fn definition(name: &str, line: u32) -> SymbolRecord {
    SymbolRecord {
        line,
        kind: SymbolKind::Definition,
        name: name.to_owned(),
        role: "function".to_owned(),
        end_line: line + 2,
        signature: format!("fn {name}()"),
        count: 1,
    }
}

#[test]
fn stores_replaces_and_queries_symbols() {
    let project = project(&[
        ("src/a.rs", "fn alpha() {}\n"),
        ("src/b.rs", "fn beta() {}\n"),
    ]);
    let root = project.path();
    let mut index = refreshed(root);
    index
        .store_symbols(&[
            FileSymbols {
                path: "src/a.rs".to_owned(),
                language: Some("rust".to_owned()),
                symbols: vec![definition("alpha", 1), definition("shared", 5)],
            },
            FileSymbols {
                path: "src/b.rs".to_owned(),
                language: Some("rust".to_owned()),
                symbols: vec![definition("shared", 3)],
            },
        ])
        .unwrap();
    let shared: Vec<(String, u32)> = index
        .symbols_named("shared")
        .unwrap()
        .into_iter()
        .map(|located| (located.path, located.symbol.line))
        .collect();
    assert_eq!(
        shared,
        [("src/a.rs".to_owned(), 5), ("src/b.rs".to_owned(), 3)]
    );
    assert_eq!(index.files().unwrap()[0].language.as_deref(), Some("rust"));

    index
        .store_symbols(&[FileSymbols {
            path: "src/a.rs".to_owned(),
            language: Some("rust".to_owned()),
            symbols: vec![definition("alpha", 2)],
        }])
        .unwrap();
    let a = index.symbols_of("src/a.rs").unwrap();
    assert_eq!(a.len(), 1);
    assert_eq!(a[0].symbol.line, 2);
    assert_eq!(index.symbols().unwrap().len(), 2);
}

#[test]
fn removing_a_file_removes_its_symbols() {
    let project = project(&[("main.py", "def main(): pass\n")]);
    let root = project.path();
    let mut index = refreshed(root);
    index
        .store_symbols(&[FileSymbols {
            path: "main.py".to_owned(),
            language: Some("python".to_owned()),
            symbols: vec![definition("main", 1)],
        }])
        .unwrap();
    fs::remove_file(root.join("main.py")).unwrap();
    let changes = index
        .refresh(root, &scan(root, &ScanOptions::default()).files)
        .unwrap();
    assert_eq!(changes.removed, ["main.py"]);
    assert_eq!(index.symbols().unwrap(), []);
}

#[test]
fn ignores_symbols_of_files_outside_the_index() {
    let project = project(&[("kept.md", "# Kept\n")]);
    let mut index = refreshed(project.path());
    index
        .store_symbols(&[FileSymbols {
            path: "missing.md".to_owned(),
            language: None,
            symbols: vec![definition("ghost", 1)],
        }])
        .unwrap();
    assert_eq!(index.symbols().unwrap(), []);
}
