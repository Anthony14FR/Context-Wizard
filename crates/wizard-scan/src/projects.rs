//! Detection of the sub-projects of a repository from their manifest files.
//!
//! Every directory that holds a known manifest is a sub-project, so a monorepo yields one entry
//! per package and each gets its own graph later.

use std::collections::BTreeMap;

use crate::FileEntry;

/// The ecosystem a manifest belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Ecosystem {
    /// .NET, from a `*.csproj`, `*.fsproj` or `*.vbproj` file.
    DotNet,
    /// Dart and Flutter, from `pubspec.yaml`.
    Dart,
    /// Elixir, from `mix.exs`.
    Elixir,
    /// Go, from `go.mod`.
    Go,
    /// Gradle builds, from `build.gradle` or `build.gradle.kts`.
    Gradle,
    /// Maven builds, from `pom.xml`.
    Maven,
    /// JavaScript and TypeScript, from `package.json`.
    Node,
    /// PHP, from `composer.json`.
    Php,
    /// PowerShell modules, from a `*.psd1` manifest.
    PowerShell,
    /// Python, from `pyproject.toml` or `setup.py`.
    Python,
    /// Ruby, from `Gemfile`.
    Ruby,
    /// Rust, from `Cargo.toml`.
    Rust,
}

impl Ecosystem {
    /// Returns the ecosystem whose manifest is named `file_name`.
    #[must_use]
    pub fn from_manifest(file_name: &str) -> Option<Self> {
        let lower = file_name.to_ascii_lowercase();
        let extension = std::path::Path::new(file_name)
            .extension()
            .and_then(std::ffi::OsStr::to_str)
            .unwrap_or_default();
        let ecosystem = match lower.as_str() {
            "package.json" => Self::Node,
            "composer.json" => Self::Php,
            "cargo.toml" => Self::Rust,
            "go.mod" => Self::Go,
            "pyproject.toml" | "setup.py" => Self::Python,
            "pom.xml" => Self::Maven,
            "build.gradle" | "build.gradle.kts" => Self::Gradle,
            "gemfile" => Self::Ruby,
            "pubspec.yaml" => Self::Dart,
            "mix.exs" => Self::Elixir,
            _ if ["csproj", "fsproj", "vbproj"]
                .iter()
                .any(|known| extension.eq_ignore_ascii_case(known)) =>
            {
                Self::DotNet
            }
            _ if extension.eq_ignore_ascii_case("psd1") => Self::PowerShell,
            _ => return None,
        };
        Some(ecosystem)
    }
}

/// A directory that holds at least one manifest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Project {
    /// Directory relative to the scanned root, with `/` separators. Empty for the root itself.
    pub root: String,
    /// Ecosystems of the manifests found there, sorted and without duplicates.
    pub ecosystems: Vec<Ecosystem>,
    /// Paths of those manifests, sorted.
    pub manifests: Vec<String>,
}

/// Groups the manifests among `files` by directory, sorted by directory.
#[must_use]
pub fn detect_projects(files: &[FileEntry]) -> Vec<Project> {
    let mut projects: BTreeMap<&str, Project> = BTreeMap::new();
    for file in files {
        let (directory, name) = file.path.rsplit_once('/').unwrap_or(("", &file.path));
        let Some(ecosystem) = Ecosystem::from_manifest(name) else {
            continue;
        };
        let project = projects.entry(directory).or_insert_with(|| Project {
            root: directory.to_owned(),
            ecosystems: Vec::new(),
            manifests: Vec::new(),
        });
        project.ecosystems.push(ecosystem);
        project.manifests.push(file.path.clone());
    }
    projects
        .into_values()
        .map(|mut project| {
            project.ecosystems.sort_unstable();
            project.ecosystems.dedup();
            project.manifests.sort_unstable();
            project
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{Ecosystem, detect_projects};
    use crate::FileEntry;

    fn entry(path: &str) -> FileEntry {
        FileEntry {
            path: path.to_owned(),
            size: 1,
            modified: None,
        }
    }

    #[test]
    fn recognises_every_supported_manifest() {
        for (name, ecosystem) in [
            ("package.json", Ecosystem::Node),
            ("composer.json", Ecosystem::Php),
            ("Cargo.toml", Ecosystem::Rust),
            ("go.mod", Ecosystem::Go),
            ("pyproject.toml", Ecosystem::Python),
            ("setup.py", Ecosystem::Python),
            ("pom.xml", Ecosystem::Maven),
            ("build.gradle.kts", Ecosystem::Gradle),
            ("Gemfile", Ecosystem::Ruby),
            ("pubspec.yaml", Ecosystem::Dart),
            ("mix.exs", Ecosystem::Elixir),
            ("Api.csproj", Ecosystem::DotNet),
            ("Tools.psd1", Ecosystem::PowerShell),
        ] {
            assert_eq!(Ecosystem::from_manifest(name), Some(ecosystem), "{name}");
        }
        assert_eq!(Ecosystem::from_manifest("README.md"), None);
    }

    #[test]
    fn splits_a_monorepo_by_manifest_directory() {
        let files: Vec<FileEntry> = [
            "Cargo.toml",
            "crates/core/Cargo.toml",
            "crates/core/src/lib.rs",
            "web/package.json",
            "web/composer.json",
            "web/src/app.ts",
        ]
        .map(entry)
        .to_vec();
        let projects = detect_projects(&files);
        let roots: Vec<&str> = projects
            .iter()
            .map(|project| project.root.as_str())
            .collect();
        assert_eq!(roots, ["", "crates/core", "web"]);
        assert_eq!(projects[2].ecosystems, [Ecosystem::Node, Ecosystem::Php]);
        assert_eq!(
            projects[2].manifests,
            ["web/composer.json", "web/package.json"]
        );
    }
}
