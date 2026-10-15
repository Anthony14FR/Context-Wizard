//! Removal of credentials from text before it reaches Claude.
//!
//! The patterns follow the rules of common secret scanners: provider-specific token formats,
//! private key blocks, JSON Web Tokens, credentials in URLs and quoted values assigned to names
//! such as `password` or `api_key`. Environment files are reduced to their keys.

use std::borrow::Cow;
use std::sync::LazyLock;

use regex::{Captures, Regex};

/// Text that replaces every secret.
pub const REDACTED: &str = "[redacted]";

struct Rule {
    pattern: Regex,
    keep_groups: &'static [usize],
}

static RULES: LazyLock<Vec<Rule>> = LazyLock::new(|| {
    [
        (
            r"-----BEGIN [A-Z ]*PRIVATE KEY-----[\s\S]*?-----END [A-Z ]*PRIVATE KEY-----",
            &[][..],
        ),
        (r"\b(?:AKIA|ASIA|AGPA|AIDA|AROA|AIPA|ANPA|ANVA)[A-Z0-9]{16}\b", &[]),
        (r"\bgh[pousr]_[A-Za-z0-9]{36,255}\b", &[]),
        (r"\bgithub_pat_[A-Za-z0-9_]{22,255}\b", &[]),
        (r"\bglpat-[A-Za-z0-9_-]{20,}", &[]),
        (r"\bxox[abprs]-[A-Za-z0-9-]{10,}", &[]),
        (r"\b(?:sk|rk)_(?:live|test)_[A-Za-z0-9]{16,}\b", &[]),
        (r"\bsk-ant-[A-Za-z0-9_-]{20,}", &[]),
        (r"\bsk-(?:proj-)?[A-Za-z0-9_-]{32,}", &[]),
        (r"\bAIza[0-9A-Za-z_-]{35}", &[]),
        (
            r"\beyJ[A-Za-z0-9_-]{10,}\.[A-Za-z0-9_-]{10,}\.[A-Za-z0-9_-]{10,}",
            &[],
        ),
        (
            r"(?i)\b([a-z][a-z0-9+.-]*://[^:/\s@]+:)[^@/\s]+(@)",
            &[1, 2],
        ),
        (
            r#"(?i)\b([a-z0-9_.-]*(?:password|passwd|pwd|secret|token|api_?key|apikey|access_?key|private_?key|client_?secret|credentials?)[a-z0-9_.-]*["']?\s*[:=]\s*["'])[^"'\s]{8,}(["'])"#,
            &[1, 2],
        ),
    ]
    .into_iter()
    .filter_map(|(pattern, keep_groups)| {
        Regex::new(pattern).ok().map(|pattern| Rule {
            pattern,
            keep_groups,
        })
    })
    .collect()
});

/// Returns `text` with every detected secret replaced by [`REDACTED`].
#[must_use]
pub fn redact(text: &str) -> Cow<'_, str> {
    let mut current = Cow::Borrowed(text);
    for rule in RULES.iter() {
        if rule.pattern.is_match(&current) {
            let replaced = rule
                .pattern
                .replace_all(&current, |captures: &Captures<'_>| {
                    replacement(rule, captures)
                })
                .into_owned();
            current = Cow::Owned(replaced);
        }
    }
    current
}

fn replacement(rule: &Rule, captures: &Captures<'_>) -> String {
    match rule.keep_groups {
        [before, after] => format!(
            "{}{REDACTED}{}",
            captures.get(*before).map_or("", |group| group.as_str()),
            captures.get(*after).map_or("", |group| group.as_str())
        ),
        _ => REDACTED.to_owned(),
    }
}

/// Returns `true` for environment files, whose values are never shown: `.env` and `.env.*`.
#[must_use]
pub fn is_env_file(path: &str) -> bool {
    let name = path.rsplit('/').next().unwrap_or(path);
    name == ".env" || name.starts_with(".env.")
}

/// Returns the variable names defined in an environment file, in order, without values.
#[must_use]
pub fn env_keys(content: &str) -> Vec<String> {
    content
        .lines()
        .filter_map(|line| {
            let line = line.trim_start();
            let line = line.strip_prefix("export ").unwrap_or(line);
            let (key, _) = line.split_once('=')?;
            let key = key.trim();
            let valid = !key.is_empty()
                && !key.starts_with('#')
                && key
                    .chars()
                    .all(|character| character.is_ascii_alphanumeric() || character == '_');
            valid.then(|| key.to_owned())
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{REDACTED, env_keys, is_env_file, redact};

    #[test]
    fn redacts_provider_tokens() {
        for secret in [
            "AKIAIOSFODNN7EXAMPLE",
            "ghp_aBcDeFgHiJkLmNoPqRsTuVwXyZ0123456789",
            "github_pat_11ABCDEFG0123456789_abcdefghijklmnopqrstuvwxyz",
            "glpat-abcdefghijklmnopqrstu",
            "xoxb-1234567890-abcdefghij",
            "sk_live_abcdefghijklmnop1234",
            "sk-ant-api03-abcdefghijklmnopqrstuvwxyz",
            "sk-proj-abcdefghijklmnopqrstuvwxyz0123456789",
            "AIzaSyA1234567890abcdefghijklmnopqrstuv",
            "eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxMjM0NTY3ODkwIn0.dozjgNryP4J3jVmNHl0w5N_XgL0n3I9PlFUP0THsR8U",
        ] {
            let text = format!("value = {secret} // end");
            assert_eq!(
                redact(&text),
                format!("value = {REDACTED} // end"),
                "{secret}"
            );
        }
    }

    #[test]
    fn redacts_private_keys_urls_and_assignments() {
        let key =
            "-----BEGIN RSA PRIVATE KEY-----\nMIIEpAIBAAKCAQEA\n-----END RSA PRIVATE KEY-----";
        assert_eq!(redact(key), REDACTED);
        assert_eq!(
            redact("postgres://admin:hunter2secret@db.local/app"),
            "postgres://admin:[redacted]@db.local/app"
        );
        assert_eq!(
            redact(r#"DB_PASSWORD="correct-horse-battery""#),
            r#"DB_PASSWORD="[redacted]""#
        );
        assert_eq!(
            redact(r#"{"apiKey": "abcd1234efgh5678"}"#),
            r#"{"apiKey": "[redacted]"}"#
        );
    }

    #[test]
    fn leaves_ordinary_code_untouched() {
        for text in [
            "fn token_count(text: &str) -> usize { text.len() }",
            "let password = read_password()?;",
            r#"const SECRET_HEADER = "x";"#,
            "https://example.com/path?query=1",
            "skip-ant-colony",
        ] {
            assert!(
                matches!(redact(text), std::borrow::Cow::Borrowed(_)),
                "{text}"
            );
        }
    }

    #[test]
    fn reduces_env_files_to_their_keys() {
        assert!(is_env_file(".env"));
        assert!(is_env_file("config/.env.production"));
        assert!(!is_env_file("src/env.rs"));
        assert_eq!(
            env_keys("# comment\nAPI_KEY=abc\nexport DATABASE_URL=postgres://x\n\nBAD LINE\n"),
            ["API_KEY", "DATABASE_URL"]
        );
    }
}
