//! Words of a task description, as used to match it against code and past tasks.
//!
//! Text is split on anything that is not a letter or a digit, identifiers written in camelCase
//! also yield their parts, words are lowercased, and short or common words are dropped.

use std::collections::BTreeSet;

const MIN_TERM_LENGTH: usize = 3;
const STOP_WORDS: [&str; 40] = [
    "the", "and", "for", "with", "that", "this", "from", "into", "when", "where", "what", "add",
    "fix", "make", "use", "new", "all", "how", "should", "does", "les", "des", "une", "pour",
    "dans", "avec", "est", "que", "qui", "sur", "pas", "par", "mais", "fait", "faire", "quand",
    "comme", "elle", "ils", "can",
];

/// Every meaningful word of `text`, in order and with repetitions.
#[must_use]
pub fn terms(text: &str) -> Vec<String> {
    let mut terms = Vec::new();
    for word in text.split(|character: char| !character.is_alphanumeric()) {
        for part in split_identifier(word) {
            let lower = part.to_lowercase();
            if lower.chars().count() >= MIN_TERM_LENGTH && !STOP_WORDS.contains(&lower.as_str()) {
                terms.push(lower);
            }
        }
    }
    terms
}

/// The meaningful words of `text` reduced to a common stem, in order and with repetitions, so
/// that `tracks`, `tracked` and `track` match.
#[must_use]
pub fn stems(text: &str) -> Vec<String> {
    terms(text).iter().map(|term| stem(term)).collect()
}

/// A light stem of a lowercase word: plural and verb endings, then a final `e`, are removed.
#[must_use]
pub fn stem(word: &str) -> String {
    let length = word.chars().count();
    let cut = |suffix: &str, replacement: &str| -> Option<String> {
        word.strip_suffix(suffix)
            .map(|base| format!("{base}{replacement}"))
    };
    let stemmed = if length > 4 && word.ends_with("ies") {
        cut("ies", "y")
    } else if length > 4
        && ["sses", "ches", "shes", "xes"]
            .iter()
            .any(|end| word.ends_with(end))
    {
        cut("es", "")
    } else if length > 3 && word.ends_with('s') && !word.ends_with("ss") && !word.ends_with("us") {
        cut("s", "")
    } else if length > 5 && word.ends_with("ing") {
        cut("ing", "")
    } else if length > 4 && word.ends_with("ed") {
        cut("ed", "")
    } else {
        None
    };
    let stemmed = stemmed.unwrap_or_else(|| word.to_owned());
    match stemmed.strip_suffix('e') {
        Some(base) if base.chars().count() > 3 => base.to_owned(),
        _ => stemmed,
    }
}

/// The distinct meaningful words of `text`, sorted.
#[must_use]
pub fn keywords(text: &str) -> Vec<String> {
    terms(text)
        .into_iter()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

fn split_identifier(word: &str) -> Vec<String> {
    let mut parts = vec![word.to_owned()];
    let mut current = String::new();
    let mut previous_lower = false;
    for character in word.chars() {
        if character.is_uppercase() && previous_lower && !current.is_empty() {
            parts.push(std::mem::take(&mut current));
        }
        previous_lower = character.is_lowercase();
        current.push(character);
    }
    if !current.is_empty() && current != word {
        parts.push(current);
    }
    parts
}

#[cfg(test)]
mod tests {
    use super::{keywords, stem, stems, terms};

    #[test]
    fn stems_join_forms_of_a_word() {
        assert_eq!(stem("tracks"), "track");
        assert_eq!(stem("playlists"), "playlist");
        assert_eq!(stem("libraries"), "library");
        assert_eq!(stem("matches"), "match");
        assert_eq!(stem("deleted"), "delet");
        assert_eq!(stem("deleting"), "delet");
        assert_eq!(stem("class"), "class");
        assert_eq!(stem("status"), "status");
        assert_eq!(
            stems("DeleteSongFiles"),
            ["deletesongfil", "delet", "song", "file"]
        );
    }

    #[test]
    fn keywords_split_identifiers_and_drop_noise() {
        assert_eq!(
            keywords("Fix the sessionStore refresh when login fails"),
            [
                "fails",
                "login",
                "refresh",
                "session",
                "sessionstore",
                "store"
            ]
        );
    }

    #[test]
    fn terms_keep_order_and_repetitions() {
        assert_eq!(
            terms("corrige le login, puis le login admin"),
            ["corrige", "login", "puis", "login", "admin"]
        );
    }
}
