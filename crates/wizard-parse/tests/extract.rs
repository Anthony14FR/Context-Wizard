//! Extraction of definitions and references on real grammars, downloaded on first use.

#![allow(clippy::unwrap_used)]

use wizard_parse::extract::{Tag, TagKind, extract};

fn definitions(tags: &[Tag]) -> Vec<(u32, &str, &str)> {
    tags.iter()
        .filter(|tag| tag.kind == TagKind::Definition)
        .map(|tag| (tag.line, tag.role.as_str(), tag.name.as_str()))
        .collect()
}

#[test]
fn extracts_rust_definitions_with_signatures_and_references() {
    let source = "pub struct Point {\n    x: i32,\n}\n\nimpl Point {\n    pub fn norm(&self) -> i32 {\n        helper(self.x)\n    }\n}\n\nfn helper(value: i32) -> i32 {\n    value\n}\n";
    let tags = extract("rust", source).unwrap();
    let defined = definitions(&tags);
    assert!(defined.contains(&(1, "class", "Point")), "{defined:?}");
    assert!(defined.contains(&(6, "method", "norm")), "{defined:?}");
    assert!(defined.contains(&(11, "function", "helper")), "{defined:?}");
    let helper = tags
        .iter()
        .find(|tag| tag.kind == TagKind::Definition && tag.name == "helper")
        .unwrap();
    assert_eq!(helper.signature, "fn helper(value: i32) -> i32 {");
    assert_eq!(helper.end_line, 13);
    assert!(
        tags.iter()
            .any(|tag| tag.kind == TagKind::Reference && tag.name == "helper" && tag.line == 7),
        "{tags:?}"
    );
}

#[test]
fn extracts_python_classes_and_functions() {
    let source = "class Repository:\n    def save(self, item):\n        validate(item)\n\ndef validate(item):\n    return item\n";
    let defined_tags = extract("python", source).unwrap();
    let defined = definitions(&defined_tags);
    assert!(defined.contains(&(1, "class", "Repository")), "{defined:?}");
    assert!(
        defined.contains(&(5, "function", "validate")),
        "{defined:?}"
    );
}

#[test]
fn output_is_deterministic() {
    let source = "function a() { b(); }\nfunction b() { a(); }\n";
    assert_eq!(
        extract("javascript", source).unwrap(),
        extract("javascript", source).unwrap()
    );
}

#[test]
fn unknown_languages_are_unavailable() {
    assert!(extract("not-a-language", "x").is_err());
}

#[test]
fn typescript_files_get_their_functions_and_classes() {
    let source = "export function loadFeed(id: string): Feed {\n  return fetchFeed(id);\n}\n\nexport class FeedTuner {\n  tune(feed: Feed) {}\n}\n\ninterface Feed { id: string }\n";
    for language in ["typescript", "tsx"] {
        let tags = extract(language, source).unwrap();
        let defined = definitions(&tags);
        assert!(
            defined
                .iter()
                .any(|(line, _, name)| *line == 1 && *name == "loadFeed"),
            "{language}: {defined:?}"
        );
        assert!(
            defined
                .iter()
                .any(|(line, _, name)| *line == 5 && *name == "FeedTuner"),
            "{language}: {defined:?}"
        );
        assert!(
            defined.iter().any(|(_, _, name)| *name == "Feed"),
            "{language}: {defined:?}"
        );
        assert!(
            tags.iter()
                .any(|tag| tag.kind == TagKind::Reference && tag.name == "fetchFeed"),
            "{language}"
        );
    }
}

proptest::proptest! {
    #[test]
    fn lexical_extraction_never_panics(path in proptest::prelude::any::<String>(), content in proptest::prelude::any::<String>()) {
        let _ = wizard_parse::lexical::extract(&path, &content);
        let _ = wizard_parse::detect_language(&path, &content);
    }
}

#[test]
fn definitions_carry_the_first_sentence_of_their_documentation() {
    let doc = |language: &str, source: &str, name: &str| {
        extract(language, source)
            .unwrap()
            .into_iter()
            .find(|tag| tag.kind == TagKind::Definition && tag.name == name)
            .unwrap()
            .doc
    };
    assert_eq!(
        doc(
            "python",
            "def total(items):\n    \"\"\"Sum the prices of a cart. Taxes excluded.\"\"\"\n    return 0\n",
            "total"
        ),
        "Sum the prices of a cart."
    );
    assert_eq!(
        doc(
            "php",
            "<?php\nclass Song {\n    /**\n     * Remove the track from the library.\n     * @param int $id\n     */\n    public function delete($id) {}\n}\n",
            "delete"
        ),
        "Remove the track from the library."
    );
    assert_eq!(
        doc(
            "javascript",
            "/** Plays the next track in the queue. */\nexport function playNext() {}\n",
            "playNext"
        ),
        "Plays the next track in the queue."
    );
    assert_eq!(
        doc(
            "rust",
            "/// Rounds an invoice total to cents.\nfn round(total: f64) -> f64 { total }\n",
            "round"
        ),
        "Rounds an invoice total to cents."
    );
    assert_eq!(doc("rust", "fn bare() {}\n", "bare"), "");
}
