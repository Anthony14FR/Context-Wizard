//! Hook entry point: reads the event Claude Code sends on standard input and answers on
//! standard output.
//!
//! A hook never fails loudly: any problem leaves Claude Code working as without the plugin.

use std::env;
use std::io::{self, Read, Write};
use std::path::PathBuf;
use std::process::ExitCode;

use serde_json::json;
use wizard_core::decision::{self, State};

use crate::event::{Event, EventName};
use crate::opt_in;

const MAX_INPUT_BYTES: u64 = 8 * 1024 * 1024;

/// Handles one hook event and always exits successfully.
pub(crate) fn run() -> ExitCode {
    let mut input = Vec::new();
    if io::stdin()
        .take(MAX_INPUT_BYTES)
        .read_to_end(&mut input)
        .is_err()
    {
        return ExitCode::SUCCESS;
    }
    if let Some(output) = Event::parse(&input).and_then(|event| respond(&event)) {
        let mut stdout = io::stdout().lock();
        let _ = stdout.write_all(output.as_bytes());
        let _ = stdout.flush();
    }
    ExitCode::SUCCESS
}

fn respond(event: &Event) -> Option<String> {
    let project = project_dir(event)?;
    match decision::load(&project).ok()? {
        State::Undecided if event.hook_event_name == EventName::UserPromptSubmit => {
            let binary = env::current_exe().ok()?;
            let question = opt_in::question(&binary, &project)?;
            Some(additional_context("UserPromptSubmit", &question))
        }
        _ => None,
    }
}

fn project_dir(event: &Event) -> Option<PathBuf> {
    env::var_os("CLAUDE_PROJECT_DIR")
        .filter(|dir| !dir.is_empty())
        .map(PathBuf::from)
        .or_else(|| event.cwd.clone())
}

fn additional_context(event: &str, context: &str) -> String {
    json!({
        "hookSpecificOutput": {
            "hookEventName": event,
            "additionalContext": context,
        }
    })
    .to_string()
}
