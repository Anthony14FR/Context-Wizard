//! Recording of hook events into the learning store of an enabled project.
//!
//! Each `Stop` also consolidates its session into learning signals.

use std::path::{Component, Path};

use serde_json::Value;
use wizard_core::secrets;
use wizard_learn::{Event as LearnEvent, EventKind, Learn, now};

use crate::event::{Event, EventName};

const MAX_TEXT_CHARS: usize = 4_000;

/// Records `event` in the learning store of `project`, ignoring events it does not describe.
pub(crate) fn record(project: &Path, event: &Event) {
    if let Some(learned) = learned_event(project, event, now())
        && let Ok(mut learn) = Learn::open_project(project)
    {
        let _ = learn.record(&learned);
        if learned.kind == EventKind::Stop {
            let _ = learn.consolidate(&learned.session);
        }
    }
}

fn learned_event(project: &Path, event: &Event, at: i64) -> Option<LearnEvent> {
    let session = event.session_id.as_deref().filter(|id| !id.is_empty())?;
    match event.hook_event_name {
        EventName::UserPromptSubmit => Some(LearnEvent {
            text: Some(clean_text(event.prompt.as_deref()?)),
            ..LearnEvent::new(session, at, EventKind::Prompt)
        }),
        EventName::Stop => Some(LearnEvent::new(session, at, EventKind::Stop)),
        EventName::PostToolUse => tool_event(
            project,
            event,
            LearnEvent::new(session, at, EventKind::Read),
        ),
        _ => None,
    }
}

fn tool_event(project: &Path, event: &Event, base: LearnEvent) -> Option<LearnEvent> {
    let input = event.tool_input.as_ref()?;
    let response = event.tool_response.as_ref();
    let file = |key: &str| {
        input
            .get(key)
            .and_then(Value::as_str)
            .and_then(|path| relative(project, path))
    };
    match event.tool_name.as_deref()? {
        "Read" => {
            let path = file("file_path")?;
            let (start_line, end_line) = read_range(input, response);
            Some(LearnEvent {
                path: Some(path),
                start_line,
                end_line,
                ..base
            })
        }
        "Grep" => Some(LearnEvent {
            kind: EventKind::Grep,
            path: file("path"),
            pattern: Some(clean_text(input.get("pattern")?.as_str()?)),
            count: response.and_then(grep_count),
            ..base
        }),
        "Glob" => Some(LearnEvent {
            kind: EventKind::Glob,
            path: file("path"),
            pattern: Some(clean_text(input.get("pattern")?.as_str()?)),
            count: response.and_then(|response| {
                number(response, "numFiles").or_else(|| list_length(response, "filenames"))
            }),
            ..base
        }),
        "Edit" | "MultiEdit" | "Write" => Some(LearnEvent {
            kind: EventKind::Edit,
            path: Some(file("file_path")?),
            ..base
        }),
        "NotebookEdit" => Some(LearnEvent {
            kind: EventKind::Edit,
            path: Some(file("notebook_path")?),
            ..base
        }),
        _ => None,
    }
}

fn read_range(input: &Value, response: Option<&Value>) -> (Option<i64>, Option<i64>) {
    let shown = response.and_then(|response| response.get("file"));
    let start = shown
        .and_then(|file| number(file, "startLine"))
        .or_else(|| number(input, "offset"))
        .unwrap_or(1)
        .max(1);
    let lines = shown
        .and_then(|file| number(file, "numLines"))
        .or_else(|| number(input, "limit"));
    (
        Some(start),
        lines
            .filter(|lines| *lines > 0)
            .map(|lines| start + lines - 1),
    )
}

fn grep_count(response: &Value) -> Option<i64> {
    if response.get("mode").and_then(Value::as_str) == Some("content") {
        number(response, "numLines")
    } else {
        number(response, "numFiles").or_else(|| list_length(response, "filenames"))
    }
}

fn number(value: &Value, key: &str) -> Option<i64> {
    value.get(key)?.as_i64()
}

fn list_length(value: &Value, key: &str) -> Option<i64> {
    i64::try_from(value.get(key)?.as_array()?.len()).ok()
}

fn clean_text(text: &str) -> String {
    let redacted = secrets::redact(text);
    redacted.chars().take(MAX_TEXT_CHARS).collect()
}

fn relative(project: &Path, raw: &str) -> Option<String> {
    let path = Path::new(raw);
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        project.join(path)
    };
    let inside = absolute
        .strip_prefix(project)
        .map(Path::to_path_buf)
        .ok()
        .or_else(|| {
            let project = project.canonicalize().ok()?;
            let absolute = absolute.canonicalize().ok()?;
            absolute.strip_prefix(project).map(Path::to_path_buf).ok()
        })?;
    let mut parts = Vec::new();
    for component in inside.components() {
        match component {
            Component::Normal(part) => parts.push(part.to_str()?.to_owned()),
            Component::CurDir => {}
            _ => return None,
        }
    }
    (!parts.is_empty()).then(|| parts.join("/"))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use std::path::Path;

    use wizard_learn::EventKind;

    use super::{learned_event, relative};
    use crate::event::Event;

    fn learned(project: &Path, json: &str) -> Option<wizard_learn::Event> {
        learned_event(project, &Event::parse(json.as_bytes()).unwrap(), 7)
    }

    fn project_and_file(name: &str) -> (tempfile::TempDir, String) {
        let project = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(project.path().join("src")).unwrap();
        let file = project.path().join("src").join(name);
        std::fs::write(&file, "x\n").unwrap();
        let file = serde_json::to_string(file.to_str().unwrap()).unwrap();
        (project, file)
    }

    #[test]
    fn records_reads_with_their_line_range() {
        let (project, file) = project_and_file("auth.rs");
        let event = learned(
            project.path(),
            &format!(
                r#"{{"hook_event_name":"PostToolUse","session_id":"s","tool_name":"Read",
                "tool_input":{{"file_path":{file},"offset":10,"limit":50}},
                "tool_response":{{"type":"text","file":{{"startLine":10,"numLines":20}}}}}}"#
            ),
        )
        .unwrap();
        assert_eq!(event.kind, EventKind::Read);
        assert_eq!(event.path.as_deref(), Some("src/auth.rs"));
        assert_eq!((event.start_line, event.end_line), (Some(10), Some(29)));
        assert_eq!(event.session, "s");
        assert_eq!(event.at, 7);
    }

    #[test]
    fn records_searches_with_their_result_count() {
        let project = tempfile::tempdir().unwrap();
        let event = learned(
            project.path(),
            r#"{"hook_event_name":"PostToolUse","session_id":"s","tool_name":"Grep",
            "tool_input":{"pattern":"invoice"},
            "tool_response":{"mode":"files_with_matches","numFiles":0,"filenames":[]}}"#,
        )
        .unwrap();
        assert_eq!(event.kind, EventKind::Grep);
        assert_eq!(event.pattern.as_deref(), Some("invoice"));
        assert_eq!(event.count, Some(0));
        let glob = learned(
            project.path(),
            r#"{"hook_event_name":"PostToolUse","session_id":"s","tool_name":"Glob",
            "tool_input":{"pattern":"**/*.rs"},"tool_response":{"filenames":["a.rs","b.rs"]}}"#,
        )
        .unwrap();
        assert_eq!((glob.kind, glob.count), (EventKind::Glob, Some(2)));
    }

    #[test]
    fn records_edits_prompts_and_stops() {
        let (project, file) = project_and_file("billing.rs");
        let edit = learned(
            project.path(),
            &format!(
                r#"{{"hook_event_name":"PostToolUse","session_id":"s","tool_name":"Edit",
                "tool_input":{{"file_path":{file},"old_string":"a","new_string":"b"}}}}"#
            ),
        )
        .unwrap();
        assert_eq!(
            (edit.kind, edit.path.as_deref()),
            (EventKind::Edit, Some("src/billing.rs"))
        );
        let prompt = learned(
            project.path(),
            r#"{"hook_event_name":"UserPromptSubmit","session_id":"s","prompt":"use key sk-ant-api03-abcdefghijklmnopqrstuvwxyz0123456789"}"#,
        )
        .unwrap();
        assert_eq!(prompt.kind, EventKind::Prompt);
        assert!(!prompt.text.unwrap().contains("abcdefghijklmnop"));
        let stop = learned(
            project.path(),
            r#"{"hook_event_name":"Stop","session_id":"s"}"#,
        )
        .unwrap();
        assert_eq!(stop.kind, EventKind::Stop);
    }

    #[test]
    fn ignores_files_outside_the_project_and_events_without_session() {
        let project = tempfile::tempdir().unwrap();
        let elsewhere = tempfile::tempdir().unwrap();
        let file = serde_json::to_string(elsewhere.path().join("x.rs").to_str().unwrap()).unwrap();
        assert!(
            learned(
                project.path(),
                &format!(
                    r#"{{"hook_event_name":"PostToolUse","session_id":"s","tool_name":"Read",
                    "tool_input":{{"file_path":{file}}}}}"#
                ),
            )
            .is_none()
        );
        assert!(learned(project.path(), r#"{"hook_event_name":"Stop"}"#).is_none());
        assert_eq!(
            relative(project.path(), "src/../../x.rs"),
            None,
            "parent components never escape the project"
        );
    }
}
