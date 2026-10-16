//! End-to-end index builds on small projects.

#![allow(clippy::unwrap_used)]

use std::fs;
use std::path::Path;
use std::time::{Duration, SystemTime};

use wizard_index::build::{BuildOptions, build, extract_file};
use wizard_store::{Index, SymbolKind};

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

#[test]
fn builds_then_updates_only_changed_files() {
    let project = tempfile::tempdir().unwrap();
    let root = project.path();
    write(
        root,
        "src/lib.rs",
        "pub fn parse() {}\n\npub fn run() {\n    parse();\n}\n",
    );
    write(root, "README.md", "# Demo\n\n## Usage\n");
    write(
        root,
        ".env",
        "API_KEY=sk-ant-api03-abcdefghijklmnopqrstuvwxyz\n",
    );
    let first = build(root, &BuildOptions::default()).unwrap();
    assert_eq!(first.files, 3);
    assert_eq!(first.changes.added.len(), 3);
    assert!(first.tagged >= 1);

    let index = Index::open_project(root).unwrap();
    let parse = index.symbols_named("parse").unwrap();
    assert!(
        parse
            .iter()
            .any(|located| located.symbol.kind == SymbolKind::Definition)
    );
    assert!(
        parse
            .iter()
            .any(|located| located.symbol.kind == SymbolKind::Reference)
    );
    let readme: Vec<String> = index
        .symbols_of("README.md")
        .unwrap()
        .into_iter()
        .map(|located| located.symbol.name)
        .collect();
    assert_eq!(readme, ["# Demo", "## Usage"]);
    let env: Vec<String> = index
        .symbols_of(".env")
        .unwrap()
        .into_iter()
        .map(|located| located.symbol.name)
        .collect();
    assert_eq!(env, ["API_KEY"]);

    let second = build(root, &BuildOptions::default()).unwrap();
    assert!(second.changes.is_empty());
    assert_eq!(second.tagged + second.lexical, 0);
}

#[test]
fn signatures_never_store_secrets() {
    let (file, tagged) = extract_file(
        "config.py",
        "def connect(token=\"ghp_aBcDeFgHiJkLmNoPqRsTuVwXyZ0123456789\"):\n    pass\n",
    );
    assert!(tagged);
    let signature = &file
        .symbols
        .iter()
        .find(|symbol| symbol.name == "connect")
        .unwrap()
        .signature;
    assert!(signature.contains("[redacted]"), "{signature}");
    assert!(!signature.contains("ghp_"));
}

#[test]
fn applies_recipes_and_reapplies_them_when_a_recipe_changes() {
    let project = tempfile::tempdir().unwrap();
    let root = project.path();
    write(root, "artisan", "#!/usr/bin/env php\n");
    write(
        root,
        "composer.json",
        r#"{"require":{"laravel/framework":"^12.0"}}"#,
    );
    write(
        root,
        "routes/web.php",
        "<?php\nRoute::get('/songs', [SongController::class, 'index']);\n",
    );
    write(root, "app/Console/Sync.php", "<?php\nclass Sync {}\n");
    build(root, &BuildOptions::default()).unwrap();
    let facts = |root: &Path| -> Vec<(String, String)> {
        Index::open_project(root)
            .unwrap()
            .facts()
            .unwrap()
            .into_iter()
            .map(|fact| (fact.path, fact.symbol.name))
            .collect()
    };
    assert_eq!(
        facts(root),
        [(
            "routes/web.php".to_owned(),
            "get /songs -> [SongController::class, 'index']".to_owned()
        )]
    );

    let recipe = root.join(".claude/context-wizard/recipes/console");
    fs::create_dir_all(&recipe).unwrap();
    fs::write(
        recipe.join("recipe.toml"),
        "name = \"console\"\n[detect]\nfiles = [\"artisan\"]\n[[extract]]\nname = \"commands\"\nglob = [\"app/Console/*.php\"]\npath_pattern = '(?P<name>[A-Za-z]+)\\.php$'\ntemplate = \"command {name}\"\n",
    )
    .unwrap();
    let rebuilt = build(root, &BuildOptions::default()).unwrap();
    assert!(rebuilt.changes.is_empty());
    assert!(facts(root).contains(&("app/Console/Sync.php".to_owned(), "command Sync".to_owned())));

    fs::remove_dir_all(&recipe).unwrap();
    build(root, &BuildOptions::default()).unwrap();
    assert_eq!(facts(root).len(), 1);
    let index = Index::open_project(root).unwrap();
    assert!(
        index
            .symbols_of("app/Console/Sync.php")
            .unwrap()
            .iter()
            .all(|symbol| symbol.symbol.kind != SymbolKind::Fact)
    );
}
