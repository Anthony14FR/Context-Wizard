//! Access to tree-sitter grammars, downloaded on first use and cached on disk.
//!
//! Grammars come from `tree-sitter-language-pack`, which verifies each downloaded bundle against
//! the SHA-256 published in its manifest. When a grammar cannot be obtained, for example
//! offline, callers fall back to lexical extraction.

use std::error::Error;
use std::fmt;
use std::path::{Path, PathBuf};

use tree_sitter_language_pack::PackConfig;

/// Failure to obtain or use a grammar.
#[derive(Debug)]
#[non_exhaustive]
pub enum GrammarError {
    /// The grammar is unknown, could not be downloaded, or could not be loaded.
    Unavailable {
        /// Language that was requested.
        language: String,
        /// Reason reported by the language pack.
        reason: String,
    },
    /// The language has no tags query, so definitions and references cannot be extracted.
    NoTagsQuery {
        /// Language that was requested.
        language: String,
    },
    /// The parser could not be set up or did not produce a tree.
    Parse {
        /// Language that was requested.
        language: String,
    },
}

impl fmt::Display for GrammarError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unavailable { language, reason } => {
                write!(formatter, "grammar for {language} is unavailable: {reason}")
            }
            Self::NoTagsQuery { language } => {
                write!(formatter, "no tags query is available for {language}")
            }
            Self::Parse { language } => write!(formatter, "could not parse {language} source"),
        }
    }
}

impl Error for GrammarError {}

/// Stores downloaded grammars under `cache_base`, typically the plugin data directory.
///
/// Call this once, before the first extraction.
///
/// # Errors
///
/// Returns an error when the language pack rejects the configuration.
pub fn configure_cache(cache_base: &Path) -> Result<(), GrammarError> {
    let config = PackConfig {
        cache_dir: Some(PathBuf::from(cache_base)),
        languages: None,
        groups: None,
    };
    tree_sitter_language_pack::configure(&config).map_err(|error| GrammarError::Unavailable {
        language: String::new(),
        reason: error.to_string(),
    })
}

/// Returns the grammar of `language`, downloading it when it is not cached yet.
///
/// # Errors
///
/// Returns [`GrammarError::Unavailable`] when the grammar cannot be found, downloaded or loaded.
pub fn language(language: &str) -> Result<tree_sitter::Language, GrammarError> {
    tree_sitter_language_pack::get_language(language).map_err(|error| GrammarError::Unavailable {
        language: language.to_owned(),
        reason: error.to_string(),
    })
}
