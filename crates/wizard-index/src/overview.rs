//! The L0 overview: a short, deterministic description of the project, injected once at the
//! start of a session.
//!
//! It names the stack, the sub-projects, the entry points, the commands and the role of each
//! top-level folder. Sections are filled in that order and every line is kept only while the
//! estimate stays within [`L0_BUDGET`], counted with the most conservative ratio.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use wizard_budget::{Content, Estimator};
use wizard_scan::FileEntry;
use wizard_scan::projects::{Ecosystem, Project, detect_projects};
use wizard_store::{FileRecord, Index, StoreError};

use crate::{calibration, recipes};

/// Hard cap of the overview, in estimated tokens.
pub const L0_BUDGET: u64 = 800;

const MAX_LANGUAGES: usize = 6;
const MAX_PROJECTS: usize = 12;
const MAX_ENTRY_POINTS: usize = 10;
const MAX_COMMANDS: usize = 14;
const MAX_FOLDERS: usize = 16;
const MAX_ENTRY_DEPTH: usize = 4;

const ENTRY_POINTS: [&str; 26] = [
    "src/main.rs",
    "src/lib.rs",
    "main.go",
    "main.py",
    "__main__.py",
    "manage.py",
    "wsgi.py",
    "asgi.py",
    "app.py",
    "artisan",
    "public/index.php",
    "Program.cs",
    "index.js",
    "index.ts",
    "index.tsx",
    "main.js",
    "main.ts",
    "main.tsx",
    "server.js",
    "server.ts",
    "app.js",
    "app.ts",
    "App.tsx",
    "app/layout.tsx",
    "app/_layout.tsx",
    "pages/_app.tsx",
];

const IGNORED_SEGMENTS: [&str; 13] = [
    "test",
    "tests",
    "fixtures",
    "examples",
    "example",
    "vendor",
    "node_modules",
    "dist",
    "build",
    "target",
    "out",
    "components",
    "utils",
];
const MAX_COMMANDS_PER_SOURCE: usize = 5;
const MAKEFILES: [&str; 3] = ["GNUmakefile", "makefile", "Makefile"];

/// Renders the overview of `project` from its index.
///
/// # Errors
///
/// Returns an error when the index cannot be read.
pub fn overview(project: &Path, index: &Index) -> Result<String, StoreError> {
    let files = index.files()?;
    let entries: Vec<FileEntry> = files
        .iter()
        .map(|file| FileEntry {
            path: file.path.clone(),
            size: file.size,
            modified: None,
        })
        .collect();
    let projects = detect_projects(&entries);
    let canonical = project
        .canonicalize()
        .unwrap_or_else(|_| project.to_path_buf());
    let name = canonical.file_name().map_or_else(
        || project.display().to_string(),
        |name| name.to_string_lossy().into_owned(),
    );
    let mut sections = vec![vec![
        "# Project overview (Context Wizard)".to_owned(),
        format!("Project: {name}, {} indexed files.", files.len()),
    ]];
    sections.push(stack(&files, &projects));
    sections.push(frameworks(project, index)?);
    sections.push(sub_projects(&projects));
    sections.push(entry_points(&files));
    sections.push(commands(project, &projects));
    sections.push(folders(&files));
    sections.push(vec![
        "Context Wizard tools, cheaper than searching and reading whole files:".to_owned(),
        "- `find_symbol` and `refs`: where a name is defined and used, with lines, instead of Grep.".to_owned(),
        "- `explain`: the outline of a file without reading it; then read only the lines you need.".to_owned(),
        "- `slice`: the files most relevant to a task; `impact`: what a change to a file affects.".to_owned(),
    ]);
    Ok(fit(&sections, L0_BUDGET, &calibration::estimator(project)))
}

fn fit(sections: &[Vec<String>], budget: u64, estimator: &Estimator) -> String {
    let mut text = String::new();
    for section in sections.iter().filter(|section| !section.is_empty()) {
        for (position, line) in section.iter().enumerate() {
            let separator = if position == 0 && !text.is_empty() {
                "\n"
            } else {
                ""
            };
            let candidate = format!("{text}{separator}{line}\n");
            if estimator.estimate(&candidate, Content::Data) > budget {
                return text;
            }
            text = candidate;
        }
    }
    text
}

fn stack(files: &[FileRecord], projects: &[Project]) -> Vec<String> {
    let mut languages: BTreeMap<&str, usize> = BTreeMap::new();
    for file in files {
        if let Some(language) = file.language.as_deref() {
            *languages.entry(language).or_default() += 1;
        }
    }
    let mut ranked: Vec<(&str, usize)> = languages.into_iter().collect();
    ranked.sort_unstable_by(|left, right| right.1.cmp(&left.1).then(left.0.cmp(right.0)));
    let mut ecosystems: Vec<Ecosystem> = projects
        .iter()
        .flat_map(|project| project.ecosystems.iter().copied())
        .collect();
    ecosystems.sort_unstable();
    ecosystems.dedup();
    let mut lines = vec!["## Stack".to_owned()];
    if !ecosystems.is_empty() {
        let names: Vec<String> = ecosystems
            .iter()
            .map(|ecosystem| format!("{ecosystem:?}"))
            .collect();
        lines.push(format!("Ecosystems: {}.", names.join(", ")));
    }
    if !ranked.is_empty() {
        let names: Vec<String> = ranked
            .iter()
            .take(MAX_LANGUAGES)
            .map(|(language, count)| format!("{language} ({count})"))
            .collect();
        lines.push(format!("Languages by files: {}.", names.join(", ")));
    }
    if lines.len() == 1 { Vec::new() } else { lines }
}

fn frameworks(project: &Path, index: &Index) -> Result<Vec<String>, StoreError> {
    let facts = index.facts()?;
    if facts.is_empty() {
        return Ok(Vec::new());
    }
    let titles: BTreeMap<String, String> = recipes::titles(project).into_iter().collect();
    let mut counts: BTreeMap<&str, BTreeMap<String, usize>> = BTreeMap::new();
    for fact in &facts {
        let (recipe, _) = fact
            .symbol
            .role
            .split_once('/')
            .unwrap_or((&fact.symbol.role, ""));
        let title = titles
            .get(&fact.symbol.role)
            .cloned()
            .unwrap_or_else(|| fact.symbol.role.clone());
        *counts.entry(recipe).or_default().entry(title).or_default() += 1;
    }
    let mut lines = vec!["Framework facts (the `facts` tool lists them):".to_owned()];
    for (recipe, kinds) in counts {
        let kinds: Vec<String> = kinds
            .into_iter()
            .map(|(title, count)| format!("{count} {}", title.to_lowercase()))
            .collect();
        lines.push(format!("- {recipe}: {}", kinds.join(", ")));
    }
    Ok(lines)
}

fn sub_projects(projects: &[Project]) -> Vec<String> {
    if projects.len() < 2 {
        return Vec::new();
    }
    let mut lines = vec!["## Sub-projects".to_owned()];
    for project in projects.iter().take(MAX_PROJECTS) {
        let root = if project.root.is_empty() {
            "."
        } else {
            &project.root
        };
        let ecosystems: Vec<String> = project
            .ecosystems
            .iter()
            .map(|ecosystem| format!("{ecosystem:?}"))
            .collect();
        lines.push(format!("- {root}: {}", ecosystems.join(", ")));
    }
    push_remainder(&mut lines, projects.len(), MAX_PROJECTS);
    lines
}

fn entry_points(files: &[FileRecord]) -> Vec<String> {
    let mut found: Vec<&str> = files
        .iter()
        .map(|file| file.path.as_str())
        .filter(|path| is_entry_point(path))
        .collect();
    found.sort_by_key(|path| (path.matches('/').count(), *path));
    if found.is_empty() {
        return Vec::new();
    }
    let mut lines = vec!["## Entry points".to_owned()];
    lines.extend(
        found
            .iter()
            .take(MAX_ENTRY_POINTS)
            .map(|path| format!("- {path}")),
    );
    push_remainder(&mut lines, found.len(), MAX_ENTRY_POINTS);
    lines
}

fn is_entry_point(path: &str) -> bool {
    let segments: Vec<&str> = path.split('/').collect();
    if segments.len() > MAX_ENTRY_DEPTH
        || segments[..segments.len() - 1]
            .iter()
            .any(|segment| IGNORED_SEGMENTS.contains(&segment.to_ascii_lowercase().as_str()))
    {
        return false;
    }
    let barrel = segments
        .last()
        .is_some_and(|name| name.starts_with("index."));
    if barrel && segments.len() > 2 {
        return false;
    }
    ENTRY_POINTS
        .iter()
        .any(|entry| path == *entry || path.ends_with(&format!("/{entry}")))
}

fn commands(project: &Path, projects: &[Project]) -> Vec<String> {
    let mut found = Vec::new();
    for (position, sub_project) in projects.iter().enumerate() {
        let directory = project.join(&sub_project.root);
        let prefix = if sub_project.root.is_empty() {
            String::new()
        } else {
            format!("({}) ", sub_project.root)
        };
        for ecosystem in &sub_project.ecosystems {
            let nested = projects[..position].iter().any(|outer| {
                outer.ecosystems.contains(ecosystem) && is_inside(&sub_project.root, &outer.root)
            });
            let commands = ecosystem_commands(&directory, *ecosystem, nested);
            for command in commands.iter().take(MAX_COMMANDS_PER_SOURCE) {
                found.push(format!("{prefix}{command}"));
            }
        }
        for target in make_targets(&directory)
            .iter()
            .take(MAX_COMMANDS_PER_SOURCE)
        {
            found.push(format!("{prefix}make {target}"));
        }
    }
    if found.is_empty() {
        return Vec::new();
    }
    let mut lines = vec!["## Commands".to_owned()];
    lines.extend(
        found
            .iter()
            .take(MAX_COMMANDS)
            .map(|command| format!("- {command}")),
    );
    push_remainder(&mut lines, found.len(), MAX_COMMANDS);
    lines
}

fn is_inside(root: &str, outer: &str) -> bool {
    outer.is_empty() || root.starts_with(&format!("{outer}/"))
}

fn ecosystem_commands(directory: &Path, ecosystem: Ecosystem, nested: bool) -> Vec<String> {
    match ecosystem {
        Ecosystem::Node => {
            let runner = node_runner(directory);
            json_keys(&directory.join("package.json"), "scripts")
                .into_iter()
                .map(|script| format!("{runner} run {script}"))
                .collect()
        }
        Ecosystem::Php => {
            let mut commands: Vec<String> = json_keys(&directory.join("composer.json"), "scripts")
                .into_iter()
                .filter(|script| !script.ends_with("-cmd"))
                .map(|script| format!("composer {script}"))
                .collect();
            if directory.join("artisan").is_file() {
                commands.push("php artisan".to_owned());
            }
            commands
        }
        _ if nested => Vec::new(),
        Ecosystem::Rust => vec!["cargo build".to_owned(), "cargo test".to_owned()],
        Ecosystem::Go => vec!["go build ./...".to_owned(), "go test ./...".to_owned()],
        Ecosystem::Dart => vec!["flutter test".to_owned()],
        Ecosystem::DotNet => vec!["dotnet build".to_owned(), "dotnet test".to_owned()],
        Ecosystem::Elixir => vec!["mix test".to_owned()],
        Ecosystem::Maven => vec!["mvn verify".to_owned()],
        Ecosystem::Gradle => vec!["./gradlew build".to_owned()],
        Ecosystem::Ruby | Ecosystem::Python | Ecosystem::PowerShell => Vec::new(),
    }
}

fn node_runner(directory: &Path) -> &'static str {
    if directory.join("pnpm-lock.yaml").is_file() {
        "pnpm"
    } else if directory.join("yarn.lock").is_file() {
        "yarn"
    } else if directory.join("bun.lockb").is_file() || directory.join("bun.lock").is_file() {
        "bun"
    } else {
        "npm"
    }
}

fn json_keys(path: &Path, field: &str) -> Vec<String> {
    let Ok(text) = fs::read_to_string(path) else {
        return Vec::new();
    };
    let Ok(serde_json::Value::Object(root)) = serde_json::from_str(&text) else {
        return Vec::new();
    };
    root.get(field)
        .and_then(serde_json::Value::as_object)
        .map(|scripts| scripts.keys().cloned().collect())
        .unwrap_or_default()
}

fn make_targets(directory: &Path) -> Vec<String> {
    let Some(text) = MAKEFILES
        .iter()
        .find_map(|name| fs::read_to_string(directory.join(name)).ok())
    else {
        return Vec::new();
    };
    let mut targets: Vec<String> = Vec::new();
    for line in text.lines() {
        let Some((target, rest)) = line.split_once(':') else {
            continue;
        };
        let simple = !target.is_empty()
            && !target.starts_with('.')
            && !rest.starts_with([':', '='])
            && target
                .chars()
                .all(|character| character.is_ascii_alphanumeric() || "-_".contains(character));
        if simple && !targets.iter().any(|known| known == target) {
            targets.push(target.to_owned());
        }
    }
    targets
}

type FolderStats<'a> = (usize, BTreeMap<&'a str, usize>);

fn folders(files: &[FileRecord]) -> Vec<String> {
    let mut folders: BTreeMap<&str, FolderStats<'_>> = BTreeMap::new();
    for file in files {
        let Some((folder, _)) = file.path.split_once('/') else {
            continue;
        };
        let entry = folders.entry(folder).or_default();
        entry.0 += 1;
        if let Some(language) = file.language.as_deref() {
            *entry.1.entry(language).or_default() += 1;
        }
    }
    if folders.is_empty() {
        return Vec::new();
    }
    let mut largest: Vec<(&str, &FolderStats<'_>)> = folders
        .iter()
        .map(|(folder, stats)| (*folder, stats))
        .collect();
    largest.sort_by(|left, right| right.1.0.cmp(&left.1.0).then(left.0.cmp(right.0)));
    largest.truncate(MAX_FOLDERS);
    largest.sort_by_key(|(folder, _)| *folder);
    let mut lines = vec!["## Folders".to_owned()];
    for (folder, (count, languages)) in largest {
        let dominant = languages
            .iter()
            .max_by(|left, right| left.1.cmp(right.1).then(right.0.cmp(left.0)))
            .map(|(language, _)| *language);
        let language = dominant.map(|language| format!(", mostly {language}"));
        let role = folder_role(folder).map(|role| format!(", {role}"));
        lines.push(format!(
            "- {folder}/: {count} files{}{}",
            language.unwrap_or_default(),
            role.unwrap_or_default()
        ));
    }
    push_remainder(&mut lines, folders.len(), MAX_FOLDERS);
    lines
}

fn folder_role(folder: &str) -> Option<&'static str> {
    let role = match folder.to_ascii_lowercase().as_str() {
        "src" | "lib" | "app" | "crates" | "packages" | "pkg" | "internal" | "cmd" => "source",
        "test" | "tests" | "spec" | "specs" | "__tests__" | "e2e" => "tests",
        "doc" | "docs" | "documentation" => "documentation",
        "script" | "scripts" | "bin" | "tools" => "scripts",
        ".github" | ".gitlab" | "ci" | "config" | "configs" | ".circleci" => "configuration",
        "migrations" | "database" | "db" | "prisma" => "database",
        "public" | "static" | "assets" | "resources" => "assets",
        "examples" | "example" | "samples" => "examples",
        "bench" | "benches" | "benchmarks" => "benchmarks",
        "vendor" | "third_party" | "external" => "third-party code",
        "routes" => "routes",
        "components" => "UI components",
        _ => return None,
    };
    Some(role)
}

fn push_remainder(lines: &mut Vec<String>, total: usize, shown: usize) {
    if total > shown {
        lines.push(format!("- and {} more", total - shown));
    }
}
