//! The `calibrate` command, which measures the token ratios of a project with Anthropic's
//! `count_tokens` endpoint.
//!
//! It sends a sample of the project's files to the API with the user's own key, never files
//! holding secrets, and stores the measured bytes per token so every budget of the project
//! uses them. Nothing else in Context Wizard calls the API.

use std::env;
use std::fmt::Write as _;
use std::fs;
use std::path::Path;
use std::process::ExitCode;
use std::time::Duration;

use serde_json::{Value, json};
use wizard_budget::{Content, Ratios, content_of_language};
use wizard_core::decision::{self, State};
use wizard_core::secrets;
use wizard_index::calibration;
use wizard_store::Index;

const ENDPOINT: &str = "https://api.anthropic.com/v1/messages/count_tokens";
const API_VERSION: &str = "2023-06-01";
const FILES_PER_KIND: usize = 6;
const MIN_BYTES: u64 = 2_000;
const MAX_BYTES: usize = 32_000;
const TIMEOUT: Duration = Duration::from_secs(30);

/// Measures the ratios of `project` for `model` and stores them.
#[allow(
    clippy::print_stderr,
    reason = "the calibrate command reports failures to the user"
)]
pub(crate) fn run(project: &Path, model: &str) -> ExitCode {
    match calibrate(project, model) {
        Ok(text) => {
            print!("{text}");
            ExitCode::SUCCESS
        }
        Err(message) => {
            eprintln!("Calibration failed: {message}");
            ExitCode::FAILURE
        }
    }
}

fn calibrate(project: &Path, model: &str) -> Result<String, String> {
    if !decision::load(project)
        .is_ok_and(|state| matches!(state, State::Decided(decision) if decision.enabled))
    {
        return Err("Context Wizard is not enabled for this project.".to_owned());
    }
    let key = env::var("ANTHROPIC_API_KEY")
        .ok()
        .filter(|key| !key.is_empty())
        .ok_or("set ANTHROPIC_API_KEY to an Anthropic API key; counting tokens is free")?;
    let index = Index::open_project(project).map_err(|error| error.to_string())?;
    let files = index.files().map_err(|error| error.to_string())?;
    let agent = ureq::Agent::config_builder()
        .timeout_global(Some(TIMEOUT))
        .proxy(ureq::Proxy::try_from_env())
        .http_status_as_error(false)
        .build()
        .new_agent();
    let count = |text: &str| count_tokens(&agent, &key, model, text);
    let overhead = count("a")?.saturating_sub(1);
    let builtin = Ratios::builtin(wizard_budget::ModelFamily::Current);
    let mut ratios = builtin;
    let mut text = format!("Calibration for {model}, bytes per token:\n");
    for content in [Content::Code, Content::Prose, Content::Data] {
        let mut bytes = 0_u64;
        let mut tokens = 0_u64;
        let mut sampled = 0;
        for file in files.iter().filter(|file| {
            file.size >= MIN_BYTES
                && content_of_language(file.language.as_deref()) == content
                && !secrets::is_env_file(&file.path)
        }) {
            if sampled == FILES_PER_KIND {
                break;
            }
            let Ok(source) = fs::read_to_string(project.join(&file.path)) else {
                continue;
            };
            let sample: String = source.chars().take(MAX_BYTES).collect();
            if secrets::redact(&sample) != sample || !sample.is_ascii() {
                continue;
            }
            bytes += u64::try_from(sample.len()).unwrap_or(0);
            tokens += count(&sample)?.saturating_sub(overhead);
            sampled += 1;
        }
        let name = match content {
            Content::Code => "code",
            Content::Prose => "prose",
            Content::Data => "data",
        };
        if tokens == 0 {
            let _ = writeln!(
                text,
                "- {name}: no suitable file, kept {:.2}",
                builtin.for_content(content)
            );
            continue;
        }
        let measured = ratio(bytes, tokens);
        match content {
            Content::Code => ratios.code = measured,
            Content::Prose => ratios.prose = measured,
            Content::Data => ratios.data = measured,
        }
        let _ = writeln!(
            text,
            "- {name}: {measured:.2} over {sampled} files (built-in {:.2})",
            builtin.for_content(content)
        );
    }
    calibration::save(project, &ratios).map_err(|error| error.to_string())?;
    let _ = writeln!(
        text,
        "Stored in .wizard/{}; budgets of this project now use these ratios.",
        calibration::CALIBRATION_FILE
    );
    Ok(text)
}

fn count_tokens(agent: &ureq::Agent, key: &str, model: &str, text: &str) -> Result<u64, String> {
    let body = json!({ "model": model, "messages": [{ "role": "user", "content": text }] });
    let mut response = agent
        .post(ENDPOINT)
        .header("x-api-key", key)
        .header("anthropic-version", API_VERSION)
        .header("content-type", "application/json")
        .send(body.to_string())
        .map_err(|error| error.to_string())?;
    let status = response.status();
    let text = response
        .body_mut()
        .read_to_string()
        .map_err(|error| error.to_string())?;
    let answer: Value = serde_json::from_str(&text).map_err(|error| error.to_string())?;
    if !status.is_success() {
        let message = answer
            .pointer("/error/message")
            .and_then(Value::as_str)
            .unwrap_or("unknown error");
        return Err(format!("the API answered {status}: {message}"));
    }
    answer
        .get("input_tokens")
        .and_then(Value::as_u64)
        .ok_or_else(|| "the API answer has no input_tokens".to_owned())
}

#[allow(
    clippy::cast_precision_loss,
    reason = "sampled sizes stay far below 2^52"
)]
fn ratio(bytes: u64, tokens: u64) -> f64 {
    bytes as f64 / tokens as f64
}
