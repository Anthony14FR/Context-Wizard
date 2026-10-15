//! Lexical extraction for files without a grammar or without a tags query.
//!
//! It keeps what helps navigation without a parser: Markdown headings, top-level keys of data
//! files, section names of INI files, variable names of environment files, never their values,
//! and the most frequent identifiers of anything else.

use std::collections::HashMap;

/// What a lexical entry is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum EntryKind {
    /// A Markdown heading.
    Heading,
    /// A top-level key, table or section of a data or configuration file.
    Key,
    /// A frequent identifier, with its number of occurrences.
    Identifier,
}

/// One lexical entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// Kind of entry.
    pub kind: EntryKind,
    /// Text of the heading, key or identifier.
    pub text: String,
    /// Line of the entry, numbered from 1; for identifiers, the line of the first occurrence.
    pub line: u32,
    /// Number of occurrences, 1 for headings and keys.
    pub count: u32,
}

const MAX_IDENTIFIERS: usize = 20;
const MIN_IDENTIFIER_LENGTH: usize = 3;
const COMMON_WORDS: [&str; 24] = [
    "and", "are", "but", "for", "from", "has", "have", "not", "the", "this", "that", "was", "were",
    "will", "with", "you", "your", "can", "all", "any", "its", "use", "into", "true",
];

/// Extracts lexical entries from `content`, choosing the strategy from `path`.
#[must_use]
pub fn extract(path: &str, content: &str) -> Vec<Entry> {
    let name = path.rsplit('/').next().unwrap_or(path).to_ascii_lowercase();
    let extension = name.rsplit_once('.').map_or("", |(_, extension)| extension);
    if name == ".env" || name.starts_with(".env.") {
        return env_keys(content);
    }
    match extension {
        "md" | "markdown" | "mdx" => headings(content),
        "yaml" | "yml" => yaml_keys(content),
        "toml" => toml_keys(content),
        "json" => json_keys(content),
        "ini" | "cfg" | "conf" => ini_sections(content),
        _ => identifiers(content),
    }
}

fn entry(kind: EntryKind, text: &str, index: usize) -> Entry {
    Entry {
        kind,
        text: text.to_owned(),
        line: u32::try_from(index.saturating_add(1)).unwrap_or(u32::MAX),
        count: 1,
    }
}

fn headings(content: &str) -> Vec<Entry> {
    let mut fenced = false;
    let mut entries = Vec::new();
    for (index, line) in content.lines().enumerate() {
        if line.trim_start().starts_with("```") {
            fenced = !fenced;
            continue;
        }
        let level = line.bytes().take_while(|byte| *byte == b'#').count();
        if !fenced && (1..=6).contains(&level) && line[level..].starts_with(' ') {
            let text = line[level..].trim().trim_end_matches('#').trim_end();
            if !text.is_empty() {
                entries.push(entry(
                    EntryKind::Heading,
                    &format!("{} {text}", "#".repeat(level)),
                    index,
                ));
            }
        }
    }
    entries
}

fn yaml_keys(content: &str) -> Vec<Entry> {
    content
        .lines()
        .enumerate()
        .filter_map(|(index, line)| {
            let first = line.chars().next()?;
            if first.is_whitespace() || matches!(first, '#' | '-' | '{' | '[') {
                return None;
            }
            let (key, _) = line.split_once(':')?;
            let key = key.trim().trim_matches(['"', '\'']);
            (!key.is_empty() && !key.contains(' ')).then(|| entry(EntryKind::Key, key, index))
        })
        .collect()
}

fn toml_keys(content: &str) -> Vec<Entry> {
    let mut in_table = false;
    let mut entries = Vec::new();
    for (index, line) in content.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            in_table = true;
            let table = trimmed.trim_start_matches('[').trim_end_matches(']').trim();
            entries.push(entry(EntryKind::Key, &format!("[{table}]"), index));
        } else if !in_table
            && !trimmed.starts_with('#')
            && let Some((key, _)) = trimmed.split_once('=')
        {
            entries.push(entry(EntryKind::Key, key.trim(), index));
        }
    }
    entries
}

fn json_keys(content: &str) -> Vec<Entry> {
    let Ok(serde_json::Value::Object(object)) = serde_json::from_str::<serde_json::Value>(content)
    else {
        return Vec::new();
    };
    object
        .keys()
        .map(|key| {
            let needle = format!("\"{key}\"");
            let index = content
                .lines()
                .position(|line| line.contains(&needle))
                .unwrap_or(0);
            entry(EntryKind::Key, key, index)
        })
        .collect()
}

fn ini_sections(content: &str) -> Vec<Entry> {
    content
        .lines()
        .enumerate()
        .filter_map(|(index, line)| {
            let section = line.trim().strip_prefix('[')?.strip_suffix(']')?.trim();
            (!section.is_empty()).then(|| entry(EntryKind::Key, &format!("[{section}]"), index))
        })
        .collect()
}

fn env_keys(content: &str) -> Vec<Entry> {
    content
        .lines()
        .enumerate()
        .filter_map(|(index, line)| {
            let line = line.trim_start();
            let line = line.strip_prefix("export ").unwrap_or(line);
            let (key, _) = line.split_once('=')?;
            let key = key.trim();
            let valid = !key.is_empty()
                && key
                    .chars()
                    .all(|character| character.is_ascii_alphanumeric() || character == '_');
            valid.then(|| entry(EntryKind::Key, key, index))
        })
        .collect()
}

fn identifiers(content: &str) -> Vec<Entry> {
    let mut seen: HashMap<&str, (u32, usize)> = HashMap::new();
    for (index, line) in content.lines().enumerate() {
        for word in line.split(|character: char| !(character.is_alphanumeric() || character == '_'))
        {
            let starts_like_identifier = word
                .chars()
                .next()
                .is_some_and(|character| character.is_alphabetic() || character == '_');
            if starts_like_identifier
                && word.chars().count() >= MIN_IDENTIFIER_LENGTH
                && !COMMON_WORDS.contains(&word.to_ascii_lowercase().as_str())
            {
                let slot = seen.entry(word).or_insert((0, index));
                slot.0 = slot.0.saturating_add(1);
            }
        }
    }
    let mut ranked: Vec<(&str, (u32, usize))> = seen.into_iter().collect();
    ranked.sort_unstable_by(|left, right| right.1.0.cmp(&left.1.0).then(left.0.cmp(right.0)));
    ranked
        .into_iter()
        .take(MAX_IDENTIFIERS)
        .map(|(word, (count, index))| Entry {
            count,
            ..entry(EntryKind::Identifier, word, index)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{Entry, EntryKind, extract};

    fn texts(entries: &[Entry]) -> Vec<(u32, &str)> {
        entries
            .iter()
            .map(|entry| (entry.line, entry.text.as_str()))
            .collect()
    }

    #[test]
    fn markdown_headings_skip_code_fences() {
        let content = "# Title\n\nText\n\n```sh\n# not a heading\n```\n\n## Install ##\n";
        assert_eq!(
            texts(&extract("docs/README.md", content)),
            [(1, "# Title"), (9, "## Install")]
        );
    }

    #[test]
    fn data_files_keep_top_level_keys() {
        let yaml = "name: CI\non:\n  push:\n    branches: [main]\njobs:\n  test: {}\n";
        assert_eq!(
            texts(&extract(".github/ci.yml", yaml)),
            [(1, "name"), (2, "on"), (5, "jobs")]
        );
        let toml = "edition = \"2024\"\n\n[dependencies]\nserde = \"1\"\n";
        assert_eq!(
            texts(&extract("Cargo.toml", toml)),
            [(1, "edition"), (3, "[dependencies]")]
        );
        let json = "{\n  \"name\": \"app\",\n  \"scripts\": {\n    \"test\": \"x\"\n  }\n}\n";
        assert_eq!(
            texts(&extract("package.json", json)),
            [(2, "name"), (3, "scripts")]
        );
        assert_eq!(
            texts(&extract("setup.cfg", "[metadata]\nname = x\n[options]\n")),
            [(1, "[metadata]"), (3, "[options]")]
        );
    }

    #[test]
    fn env_files_never_expose_values() {
        let entries = extract("config/.env.local", "API_KEY=sk-secret\nexport DEBUG=1\n");
        assert_eq!(texts(&entries), [(1, "API_KEY"), (2, "DEBUG")]);
        assert!(entries.iter().all(|entry| !entry.text.contains("secret")));
    }

    #[test]
    fn other_files_keep_frequent_identifiers() {
        let content = "Get-Service | Where-Object Status\nRestart-Service ServiceName\nServiceName ServiceName\n";
        let entries = extract("scripts/run.ps1x", content);
        assert_eq!(entries[0].kind, EntryKind::Identifier);
        assert_eq!(entries[0].text, "ServiceName");
        assert_eq!(entries[0].count, 3);
        assert_eq!(entries[0].line, 2);
        assert!(entries.iter().all(|entry| entry.text != "the"));
    }
}
