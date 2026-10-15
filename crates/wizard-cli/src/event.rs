//! The JSON event Claude Code sends to a hook on standard input.

use std::path::PathBuf;

use serde::Deserialize;

/// Hook events handled by Context Wizard.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
pub(crate) enum EventName {
    /// A session starts, resumes, or is cleared or compacted.
    SessionStart,
    /// The user submitted a prompt.
    UserPromptSubmit,
    /// A tool is about to run.
    PreToolUse,
    /// A tool has run.
    PostToolUse,
    /// Claude finished answering.
    Stop,
    /// Any event this build does not handle.
    #[serde(other)]
    Other,
}

/// The fields of a hook event that Context Wizard reads.
#[derive(Debug, Deserialize)]
pub(crate) struct Event {
    /// Name of the event.
    pub(crate) hook_event_name: EventName,
    /// Working directory of the session.
    #[serde(default)]
    pub(crate) cwd: Option<PathBuf>,
}

impl Event {
    /// Parses an event, or returns `None` when the input is not a valid event.
    pub(crate) fn parse(input: &[u8]) -> Option<Self> {
        serde_json::from_slice(input).ok()
    }
}

#[cfg(test)]
mod tests {
    use super::{Event, EventName};

    #[test]
    fn parses_known_and_unknown_events() {
        let event =
            Event::parse(br#"{"hook_event_name":"UserPromptSubmit","cwd":"/p","prompt":"x"}"#)
                .unwrap();
        assert_eq!(event.hook_event_name, EventName::UserPromptSubmit);
        assert_eq!(event.cwd.unwrap().to_str(), Some("/p"));
        let other = Event::parse(br#"{"hook_event_name":"Notification"}"#).unwrap();
        assert_eq!(other.hook_event_name, EventName::Other);
        assert!(Event::parse(b"not json").is_none());
    }
}
