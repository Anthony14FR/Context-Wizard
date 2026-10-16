//! Token estimation for Context Wizard, without a tokenizer.
//!
//! Claude's tokenizer is not public. The estimator counts ASCII bytes and other characters
//! separately: ASCII text is divided by a bytes-per-token ratio that depends on the kind of
//! content and on the model family, and every other character counts as one token, which errs
//! on the high side for accented and CJK text. Estimates round up, so budgets built on them are
//! conservative. Ratios can be replaced by calibrated ones measured with Anthropic's
//! `count_tokens` endpoint.

/// Kind of content, which drives how many bytes a token covers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Content {
    /// Source code.
    Code,
    /// Natural language, such as Markdown or comments.
    Prose,
    /// Structured data, such as JSON, YAML or TOML.
    Data,
}

/// Tokenizer generation of the target model.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum ModelFamily {
    /// Models before the current tokenizer generation.
    Previous,
    /// Current models, which produce about 30% more tokens for the same text.
    #[default]
    Current,
}

/// Bytes of ASCII text per token, by kind of content.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Ratios {
    /// Ratio for source code.
    pub code: f64,
    /// Ratio for natural language.
    pub prose: f64,
    /// Ratio for structured data.
    pub data: f64,
}

impl Ratios {
    /// Built-in ratios for `family`, chosen to slightly overestimate.
    #[must_use]
    pub fn builtin(family: ModelFamily) -> Self {
        match family {
            ModelFamily::Previous => Self {
                code: 3.6,
                prose: 4.2,
                data: 3.1,
            },
            ModelFamily::Current => Self {
                code: 2.8,
                prose: 3.2,
                data: 2.4,
            },
        }
    }

    /// Writes the ratios as `code=`, `prose=` and `data=` lines, which [`Ratios::parse`] reads.
    #[must_use]
    pub fn to_text(&self) -> String {
        format!(
            "code={}\nprose={}\ndata={}\n",
            self.code, self.prose, self.data
        )
    }

    /// Reads ratios written by [`Ratios::to_text`], or `None` when one is missing or invalid.
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        let value = |key: &str| {
            text.lines()
                .filter_map(|line| line.split_once('='))
                .find(|(name, _)| name.trim() == key)
                .and_then(|(_, value)| value.trim().parse::<f64>().ok())
                .filter(|ratio| ratio.is_finite() && *ratio > 0.0)
        };
        Some(Self {
            code: value("code")?,
            prose: value("prose")?,
            data: value("data")?,
        })
    }

    /// Ratio for `content`.
    #[must_use]
    pub fn for_content(&self, content: Content) -> f64 {
        match content {
            Content::Code => self.code,
            Content::Prose => self.prose,
            Content::Data => self.data,
        }
    }
}

/// Estimates token counts with a fixed set of ratios.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Estimator {
    ratios: Ratios,
}

impl Default for Estimator {
    fn default() -> Self {
        Self::for_family(ModelFamily::default())
    }
}

impl Estimator {
    /// Creates an estimator with the built-in ratios of `family`.
    #[must_use]
    pub fn for_family(family: ModelFamily) -> Self {
        Self::with_ratios(Ratios::builtin(family))
    }

    /// Creates an estimator with calibrated ratios. Ratios below one byte per token are raised
    /// to one, so an estimate never exceeds one token per byte.
    #[must_use]
    pub fn with_ratios(ratios: Ratios) -> Self {
        let floor = |ratio: f64| {
            if ratio.is_finite() {
                ratio.max(1.0)
            } else {
                1.0
            }
        };
        Self {
            ratios: Ratios {
                code: floor(ratios.code),
                prose: floor(ratios.prose),
                data: floor(ratios.data),
            },
        }
    }

    /// Estimated number of tokens in `text`, rounded up.
    #[must_use]
    pub fn estimate(&self, text: &str, content: Content) -> u64 {
        let ascii = text.bytes().filter(u8::is_ascii).count();
        let other = text
            .chars()
            .filter(|character| !character.is_ascii())
            .count();
        let ascii_tokens = to_u64((count_to_f64(ascii) / self.ratios.for_content(content)).ceil());
        ascii_tokens.saturating_add(u64::try_from(other).unwrap_or(u64::MAX))
    }
}

/// Kind of content of a file in `language`, a tree-sitter language name.
#[must_use]
pub fn content_of_language(language: Option<&str>) -> Content {
    match language {
        Some("markdown" | "rst" | "org" | "latex" | "asciidoc") | None => Content::Prose,
        Some("json" | "jsonc" | "yaml" | "toml" | "xml" | "csv" | "ini" | "properties") => {
            Content::Data
        }
        Some(_) => Content::Code,
    }
}

#[allow(
    clippy::cast_precision_loss,
    reason = "character counts stay far below 2^52"
)]
fn count_to_f64(count: usize) -> f64 {
    count as f64
}

#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "the value is a non-negative whole number of tokens"
)]
fn to_u64(value: f64) -> u64 {
    if value.is_finite() && value > 0.0 {
        value as u64
    } else {
        0
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn ratios_round_trip_through_text() {
        let ratios = Ratios {
            code: 3.25,
            prose: 4.0,
            data: 2.5,
        };
        assert_eq!(Ratios::parse(&ratios.to_text()), Some(ratios));
        assert_eq!(Ratios::parse("code=3\nprose=x\ndata=2\n"), None);
        assert_eq!(Ratios::parse("code=3\nprose=-1\ndata=2\n"), None);
    }

    use super::{Content, Estimator, ModelFamily, Ratios, content_of_language};

    #[test]
    fn empty_text_has_no_tokens() {
        assert_eq!(Estimator::default().estimate("", Content::Code), 0);
    }

    #[test]
    fn rounds_up_and_is_deterministic() {
        let estimator = Estimator::with_ratios(Ratios {
            code: 4.0,
            prose: 4.0,
            data: 4.0,
        });
        assert_eq!(estimator.estimate("abcde", Content::Code), 2);
        assert_eq!(estimator.estimate("abcd", Content::Code), 1);
    }

    #[test]
    fn current_models_count_about_thirty_percent_more() {
        let text = "fn main() { println!(\"hello\"); }\n".repeat(100);
        let previous = Estimator::for_family(ModelFamily::Previous).estimate(&text, Content::Code);
        let current = Estimator::for_family(ModelFamily::Current).estimate(&text, Content::Code);
        let increase = (current * 100) / previous;
        assert!((120..=140).contains(&increase), "{increase}");
    }

    #[test]
    fn non_ascii_characters_count_one_token_each() {
        let estimator = Estimator::default();
        assert_eq!(estimator.estimate("été", Content::Prose), 3);
        assert_eq!(estimator.estimate("日本語", Content::Prose), 3);
    }

    #[test]
    fn denser_content_costs_more_tokens() {
        let estimator = Estimator::default();
        let text = "x".repeat(1_000);
        assert!(
            estimator.estimate(&text, Content::Data) > estimator.estimate(&text, Content::Code)
        );
        assert!(
            estimator.estimate(&text, Content::Code) > estimator.estimate(&text, Content::Prose)
        );
    }

    #[test]
    fn invalid_ratios_never_exceed_one_token_per_byte() {
        let estimator = Estimator::with_ratios(Ratios {
            code: 0.0,
            prose: f64::NAN,
            data: -3.0,
        });
        assert_eq!(estimator.estimate("abcd", Content::Code), 4);
        assert_eq!(estimator.estimate("abcd", Content::Prose), 4);
        assert_eq!(estimator.estimate("abcd", Content::Data), 4);
    }

    #[test]
    fn maps_languages_to_content() {
        assert_eq!(content_of_language(Some("rust")), Content::Code);
        assert_eq!(content_of_language(Some("yaml")), Content::Data);
        assert_eq!(content_of_language(Some("markdown")), Content::Prose);
        assert_eq!(content_of_language(None), Content::Prose);
    }
}
