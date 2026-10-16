//! Focused reads: a full read of a large file is narrowed to the part the task is about.
//!
//! Every tool result stays in Claude's context for the rest of the session and is read again
//! at every turn, so a whole 1,500-line file read once is paid for many times. When Claude
//! reads such a file without a range, the hook rewrites the read to the definitions whose names
//! match the words of the session's latest prompt, and gives the file's outline beside it, so
//! Claude can read any other range it needs. Without a clear match the read is left as it is.

use std::fmt::Write as _;
use std::fs;
use std::path::Path;

use serde_json::json;
use wizard_core::text::stems;
use wizard_index::vocabulary::Vocabulary;
use wizard_learn::{EventKind, Learn};
use wizard_store::{Index, LocatedSymbol, SymbolKind};

use crate::event::Event;
use crate::learn::relative;

/// Files with more lines than this are focused.
pub(crate) const LARGE_FILE_LINES: usize = 400;

const CONTEXT_LINES: u32 = 12;
const MAX_FOCUS_LINES: u32 = 300;
const MAX_OUTLINE: usize = 80;

/// The hook answer that narrows a full read of a large file, or `None` to leave it alone.
pub(crate) fn read(project: &Path, event: &Event) -> Option<String> {
    let input = event.tool_input.as_ref()?;
    if input.get("offset").is_some_and(|value| !value.is_null())
        || input.get("limit").is_some_and(|value| !value.is_null())
    {
        return None;
    }
    let raw = input.get("file_path")?.as_str()?;
    let path = relative(project, raw)?;
    let source = fs::read_to_string(project.join(&path)).ok()?;
    let total = source.lines().count();
    if total <= LARGE_FILE_LINES {
        return None;
    }
    let session = event.session_id.as_deref()?;
    let task = latest_prompt(project, session)?;
    let index = Index::open_project(project).ok()?;
    let symbols = index.symbols_of(&path).ok()?;
    let outline: Vec<&LocatedSymbol> = symbols
        .iter()
        .filter(|located| {
            matches!(
                located.symbol.kind,
                SymbolKind::Definition | SymbolKind::Heading
            )
        })
        .collect();
    let (start, end, names) = focus(&outline, &task, total)?;
    let mut narrowed = input.clone();
    narrowed["offset"] = json!(start);
    narrowed["limit"] = json!(end - start + 1);
    let mut context = format!(
        "Context Wizard narrowed this read of {path} ({total} lines) to lines {start}-{end}, \
         where {} defined, as it matches the task. Read other ranges with offset and limit \
         if you need them. Outline of the file:\n",
        if names.len() == 1 {
            format!("`{}` is", names[0])
        } else {
            format!(
                "{} are",
                names
                    .iter()
                    .map(|name| format!("`{name}`"))
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        }
    );
    for located in outline.iter().take(MAX_OUTLINE) {
        let _ = writeln!(
            context,
            "{:>6}-{} {}",
            located.symbol.line,
            located.symbol.end_line,
            describe(located)
        );
    }
    if outline.len() > MAX_OUTLINE {
        let _ = writeln!(
            context,
            "- and {} more definitions",
            outline.len() - MAX_OUTLINE
        );
    }
    Some(
        json!({
            "hookSpecificOutput": {
                "hookEventName": "PreToolUse",
                "updatedInput": narrowed,
                "additionalContext": context,
            }
        })
        .to_string(),
    )
}

/// The range to read: the definitions that share the most words with `task`, with some context,
/// merged while they fit, or `None` when no definition matches.
fn focus(outline: &[&LocatedSymbol], task: &str, total: usize) -> Option<(u32, u32, Vec<String>)> {
    let words = stems(task);
    let mut scored: Vec<(usize, &LocatedSymbol)> = outline
        .iter()
        .filter(|located| located.symbol.kind == SymbolKind::Definition)
        .map(|located| {
            let shared = stems(&located.symbol.name)
                .iter()
                .filter(|word| words.contains(word))
                .count();
            (shared, *located)
        })
        .filter(|(shared, located)| {
            *shared > 0 && Vocabulary::relates(&located.symbol.name, &words)
        })
        .collect();
    scored.sort_by(|left, right| {
        right.0.cmp(&left.0).then_with(|| {
            let span = |located: &LocatedSymbol| located.symbol.end_line - located.symbol.line;
            span(left.1).cmp(&span(right.1))
        })
    });
    let last = u32::try_from(total).unwrap_or(u32::MAX);
    let (first, _) = scored.first()?;
    if *first == 0 {
        return None;
    }
    let mut start = u32::MAX;
    let mut end = 0;
    let mut names = Vec::new();
    for (_, located) in scored.iter().take(3) {
        let from = located.symbol.line.saturating_sub(CONTEXT_LINES).max(1);
        let to = (located.symbol.end_line + CONTEXT_LINES).min(last);
        let (new_start, new_end) = (start.min(from), end.max(to));
        if names.is_empty() || new_end - new_start < MAX_FOCUS_LINES {
            start = new_start;
            end = new_end;
            names.push(located.symbol.name.clone());
        }
    }
    if end - start >= MAX_FOCUS_LINES {
        end = start + MAX_FOCUS_LINES - 1;
    }
    Some((start, end, names))
}

fn latest_prompt(project: &Path, session: &str) -> Option<String> {
    let learn = Learn::open_project(project).ok()?;
    learn
        .events(session)
        .ok()?
        .into_iter()
        .rev()
        .find(|event| event.kind == EventKind::Prompt)
        .and_then(|event| event.text)
}

fn describe(located: &LocatedSymbol) -> String {
    if located.symbol.signature.is_empty() {
        located.symbol.name.clone()
    } else {
        located.symbol.signature.clone()
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use wizard_store::{LocatedSymbol, SymbolKind, SymbolRecord};

    use super::focus;

    fn definition(name: &str, line: u32, end_line: u32) -> LocatedSymbol {
        LocatedSymbol {
            path: "app/Services/SongService.php".to_owned(),
            symbol: SymbolRecord {
                line,
                kind: SymbolKind::Definition,
                name: name.to_owned(),
                role: "method".to_owned(),
                end_line,
                signature: format!("function {name}()"),
                count: 1,
            },
        }
    }

    #[test]
    fn narrows_to_the_matching_definitions() {
        let symbols = [
            definition("updateSongs", 40, 90),
            definition("deleteSongs", 500, 540),
            definition("deleteSongFiles", 560, 600),
            definition("scanLibrary", 900, 1000),
        ];
        let outline: Vec<&LocatedSymbol> = symbols.iter().collect();
        let (start, end, names) = focus(
            &outline,
            "an admin deletes several songs and their files",
            1200,
        )
        .unwrap();
        assert_eq!((start, end), (488, 612));
        assert_eq!(names, ["deleteSongFiles", "deleteSongs"]);
        assert_eq!(focus(&outline, "change the colour theme", 1200), None);
    }
}
