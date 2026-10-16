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

use crate::compress;
use crate::context;
use crate::event::{Event, EventName};
use crate::focus;
use crate::grep;
use crate::install;
use crate::learn;
use crate::opt_in;
use crate::prediction;
use crate::update;

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
    std::panic::set_hook(Box::new(|_| {}));
    let response =
        std::panic::catch_unwind(|| Event::parse(&input).and_then(|event| respond(&event)));
    if let Ok(Some(output)) = response {
        let mut stdout = io::stdout().lock();
        let _ = stdout.write_all(output.as_bytes());
        let _ = stdout.flush();
    }
    ExitCode::SUCCESS
}

fn respond(event: &Event) -> Option<String> {
    if install::version_mismatch() {
        return None;
    }
    let project = project_dir(event)?;
    let state = decision::load(&project).ok()?;
    if matches!(&state, State::Decided(decision) if decision.enabled) {
        learn::record(&project, event);
    }
    match state {
        State::Decided(decision)
            if decision.enabled && event.hook_event_name == EventName::SessionStart =>
        {
            let text = context::refreshed_overview(&project)?;
            let mut output = json!({
                "hookSpecificOutput": {
                    "hookEventName": "SessionStart",
                    "additionalContext": text,
                }
            });
            if let Some(notice) = update::notice() {
                output["systemMessage"] = json!(notice);
            }
            Some(output.to_string())
        }
        State::Decided(decision)
            if decision.enabled
                && event.hook_event_name == EventName::PreToolUse
                && event.tool_name.as_deref() == Some("Read") =>
        {
            focus::read(&project, event)
        }
        State::Decided(decision)
            if decision.enabled
                && event.hook_event_name == EventName::PostToolUse
                && event.tool_name.as_deref() == Some("Bash") =>
        {
            compress::bash(event)
        }
        State::Decided(decision)
            if decision.enabled
                && event.hook_event_name == EventName::PreToolUse
                && event.tool_name.as_deref() == Some("Grep") =>
        {
            let pattern = event.tool_input.as_ref()?.get("pattern")?.as_str()?;
            let text = grep::known_symbol(&project, pattern)?;
            Some(additional_context("PreToolUse", &text))
        }
        State::Decided(decision)
            if decision.enabled && event.hook_event_name == EventName::UserPromptSubmit =>
        {
            let text = prediction::inject(&project, event)?;
            Some(additional_context("UserPromptSubmit", &text))
        }
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
