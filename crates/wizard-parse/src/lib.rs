//! Language detection and syntax extraction for Context Wizard, on top of tree-sitter.

pub mod extract;
pub mod grammar;

/// Returns the tree-sitter language of a file from its path, or from its first line when the
/// path is not enough, such as a script with a shebang and no extension.
#[must_use]
pub fn detect_language(path: &str, head: &str) -> Option<&'static str> {
    tree_sitter_language_pack::detect_language_from_path(path).or_else(|| {
        let first_line = head.lines().next().unwrap_or_default();
        first_line
            .starts_with("#!")
            .then(|| tree_sitter_language_pack::detect_language_from_content(first_line))
            .flatten()
    })
}

#[cfg(test)]
mod tests {
    use super::detect_language;

    #[test]
    fn detects_languages_from_paths() {
        for (path, language) in [
            ("src/main.rs", "rust"),
            ("web/app.ts", "typescript"),
            ("web/App.tsx", "tsx"),
            ("api/views.py", "python"),
            ("app/Http/Kernel.php", "php"),
            ("cmd/main.go", "go"),
            ("scripts/deploy.ps1", "powershell"),
            ("config/app.yaml", "yaml"),
        ] {
            assert_eq!(detect_language(path, ""), Some(language), "{path}");
        }
    }

    #[test]
    fn falls_back_to_the_shebang() {
        assert_eq!(
            detect_language("bin/tool", "#!/usr/bin/env python3\nprint(1)\n"),
            Some("python")
        );
        assert_eq!(detect_language("LICENSE", "MIT License\n"), None);
    }
}
