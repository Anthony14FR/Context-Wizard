//! Hook entry point: reads the event Claude Code sends on standard input.
//!
//! A hook never fails loudly: any problem leaves Claude Code working as without the plugin.

use std::io::{self, Read};
use std::process::ExitCode;

const MAX_INPUT_BYTES: u64 = 8 * 1024 * 1024;

/// Reads the hook event and exits successfully without output.
pub(crate) fn run() -> ExitCode {
    let mut input = Vec::new();
    let _ = io::stdin().take(MAX_INPUT_BYTES).read_to_end(&mut input);
    ExitCode::SUCCESS
}
