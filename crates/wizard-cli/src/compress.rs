//! Compression of long command outputs before Claude reads them.
//!
//! Test runs, builds and logs often print thousands of lines of which a handful matter, and the
//! whole output stays in Claude's context for the rest of the session. A long output keeps its
//! first and last lines, every line that looks like an error, a failure or a warning with a few
//! lines around it, and a note saying how many lines were left out, so Claude can rerun the
//! command with a filter when it needs more.

use std::fmt::Write as _;

use serde_json::{Value, json};

use crate::event::Event;

/// Outputs with more lines than this are compressed.
pub(crate) const LONG_OUTPUT_LINES: usize = 200;

const HEAD: usize = 30;
const TAIL: usize = 50;
const AROUND: usize = 3;
const MAX_KEPT: usize = 400;
const SIGNALS: [&str; 14] = [
    "error",
    "fail",
    "panic",
    "exception",
    "traceback",
    "warning",
    "fatal",
    "assert",
    "denied",
    "not found",
    "undefined",
    "expected",
    "caused by",
    "✗",
];

/// The hook answer replacing a long Bash output with its compressed form, or `None`.
pub(crate) fn bash(event: &Event) -> Option<String> {
    let response = event.tool_response.as_ref()?;
    let stdout = response.get("stdout")?.as_str()?;
    let stderr = response
        .get("stderr")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let (stdout, stdout_dropped) = compress(stdout);
    let (stderr, stderr_dropped) = compress(stderr);
    if stdout_dropped + stderr_dropped == 0 {
        return None;
    }
    let mut updated = response.clone();
    updated["stdout"] = json!(stdout);
    updated["stderr"] = json!(stderr);
    Some(
        json!({
            "hookSpecificOutput": {
                "hookEventName": "PostToolUse",
                "updatedToolOutput": updated,
                "additionalContext": format!(
                    "Context Wizard shortened this output: {} of its lines were left out, keeping \
                     the first and last lines and every error, failure and warning with the lines \
                     around them. Rerun the command with a filter such as grep if you need more.",
                    stdout_dropped + stderr_dropped
                ),
            }
        })
        .to_string(),
    )
}

/// `text` shortened when it is long, with the number of lines left out.
pub(crate) fn compress(text: &str) -> (String, usize) {
    let lines: Vec<&str> = text.lines().collect();
    if lines.len() <= LONG_OUTPUT_LINES {
        return (text.to_owned(), 0);
    }
    let mut keep = vec![false; lines.len()];
    for flag in keep.iter_mut().take(HEAD) {
        *flag = true;
    }
    for flag in keep.iter_mut().rev().take(TAIL) {
        *flag = true;
    }
    for (position, line) in lines.iter().enumerate() {
        let lower = line.to_lowercase();
        if SIGNALS.iter().any(|signal| lower.contains(signal)) {
            let from = position.saturating_sub(AROUND);
            let to = (position + AROUND + 1).min(lines.len());
            for flag in &mut keep[from..to] {
                *flag = true;
            }
        }
    }
    let mut kept = 0;
    let mut output = String::new();
    let mut skipped = 0;
    for (line, keep) in lines.iter().zip(&keep) {
        if *keep && kept < MAX_KEPT {
            if skipped > 0 {
                let _ = writeln!(output, "[... {skipped} lines left out ...]");
                skipped = 0;
            }
            output.push_str(line);
            output.push('\n');
            kept += 1;
        } else {
            skipped += 1;
        }
    }
    if skipped > 0 {
        let _ = writeln!(output, "[... {skipped} lines left out ...]");
    }
    let dropped = lines.len() - kept;
    (output, dropped)
}

#[cfg(test)]
mod tests {
    use super::{LONG_OUTPUT_LINES, compress};

    #[test]
    fn keeps_short_outputs_and_the_signal_of_long_ones() {
        assert_eq!(compress("ok\n"), ("ok\n".to_owned(), 0));
        let mut log: Vec<String> = (0..1_000)
            .map(|line| format!("test case {line} ... ok"))
            .collect();
        log[500] = "test billing::rounds ... FAILED".to_owned();
        let (text, dropped) = compress(&log.join("\n"));
        assert!(dropped > 800, "{dropped}");
        assert!(text.contains("test billing::rounds ... FAILED"));
        assert!(text.contains("test case 499 ... ok"));
        assert!(text.contains("test case 0 ... ok"));
        assert!(text.contains("test case 999 ... ok"));
        assert!(text.contains("lines left out"));
        assert!(text.lines().count() < LONG_OUTPUT_LINES);
    }
}
