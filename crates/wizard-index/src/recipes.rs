//! Framework recipes applied to a project during indexing.
//!
//! Each recipe is matched against the project root and every sub-project root found by its
//! manifests. Facts become symbols of the file they come from, so they follow the same
//! incremental updates as the rest of the index. When the recipes or the frameworks detected
//! change, every file a recipe reads is extracted again.

use std::fmt::Write as _;
use std::fs;
use std::path::Path;

use wizard_recipes::{Recipe, RecipeError};
use wizard_scan::projects::detect_projects;
use wizard_scan::{FileEntry, ScanOptions, scan};
use wizard_store::{DATA_DIRECTORY, SymbolKind, SymbolRecord};

const STAMP_FILE: &str = "recipes.stamp";

/// The recipes that apply to a project, with the roots they were detected in.
#[derive(Debug, Default)]
pub struct Applied {
    recipes: Vec<Recipe>,
    detected: Vec<(String, usize)>,
    /// Problems of recipes that failed to load.
    pub errors: Vec<RecipeError>,
    stamp: String,
}

impl Applied {
    /// Detects the recipes that apply to `project`, whose files are `files`.
    #[must_use]
    pub fn detect(project: &Path, files: &[FileEntry]) -> Self {
        let (recipes, errors) = wizard_recipes::load(project);
        let mut roots: Vec<String> = detect_projects(files)
            .into_iter()
            .map(|found| found.root)
            .collect();
        roots.insert(0, String::new());
        roots.dedup();
        let mut detected = Vec::new();
        for (position, recipe) in recipes.iter().enumerate() {
            for root in &roots {
                if recipe.detects(&project.join(root)) {
                    detected.push((root.clone(), position));
                }
            }
        }
        let mut stamp = wizard_recipes::fingerprint(project);
        for (root, position) in &detected {
            let _ = write!(stamp, "\n{}@{root}", recipes[*position].name);
        }
        Self {
            recipes,
            detected,
            errors,
            stamp,
        }
    }

    /// Detects the recipes that apply to `project`, scanning it first.
    #[must_use]
    pub fn detect_project(project: &Path) -> Self {
        Self::detect(project, &scan(project, &ScanOptions::default()).files)
    }

    /// Compiles every query of the loaded recipes and adds their problems to [`Applied::errors`].
    pub fn check(&mut self) {
        let problems: Vec<RecipeError> = self.recipes.iter().flat_map(Recipe::check).collect();
        self.errors.extend(problems);
    }

    /// Names of every loaded recipe with the names of its extractions.
    #[must_use]
    pub fn loaded(&self) -> Vec<(String, Vec<String>)> {
        self.recipes
            .iter()
            .map(|recipe| {
                (
                    recipe.name.clone(),
                    recipe
                        .extracts
                        .iter()
                        .map(|extract| extract.name.clone())
                        .collect(),
                )
            })
            .collect()
    }

    /// Names of the detected recipes with their roots, `.` for the project root.
    #[must_use]
    pub fn detected(&self) -> Vec<(String, String)> {
        self.detected
            .iter()
            .map(|(root, position)| {
                let root = if root.is_empty() { "." } else { root.as_str() };
                (self.recipes[*position].name.clone(), root.to_owned())
            })
            .collect()
    }

    /// Returns `true` when a detected recipe reads `path`, relative to the project root.
    #[must_use]
    pub fn reads(&self, path: &str) -> bool {
        self.detected.iter().any(|(root, position)| {
            relative(root, path).is_some_and(|inner| self.recipes[*position].reads(inner))
        })
    }

    /// Facts of `path`, relative to the project root, as symbols to store with the file.
    #[must_use]
    pub fn symbols(&self, path: &str, content: &str) -> Vec<SymbolRecord> {
        let mut symbols = Vec::new();
        for (root, position) in &self.detected {
            let Some(inner) = relative(root, path) else {
                continue;
            };
            for fact in self.recipes[*position].facts(inner, path, content) {
                symbols.push(SymbolRecord {
                    line: fact.line,
                    kind: SymbolKind::Fact,
                    name: fact.text,
                    role: format!("{}/{}", fact.recipe, fact.extract),
                    end_line: fact.line,
                    signature: String::new(),
                    count: 1,
                });
            }
        }
        symbols
    }

    /// Returns `true` when the recipes or the detected frameworks differ from the last build.
    #[must_use]
    pub fn changed_since_last_build(&self, project: &Path) -> bool {
        fs::read_to_string(project.join(DATA_DIRECTORY).join(STAMP_FILE))
            .map_or(true, |stamp| stamp != self.stamp)
    }

    /// Records the current recipes as applied, once their facts are stored.
    pub fn mark_built(&self, project: &Path) {
        let _ = wizard_core::atomic::write(
            &project.join(DATA_DIRECTORY).join(STAMP_FILE),
            self.stamp.as_bytes(),
        );
    }
}

/// Title of each extraction of the built-in and project recipes, keyed by `recipe/extract`.
#[must_use]
pub fn titles(project: &Path) -> Vec<(String, String)> {
    wizard_recipes::load(project)
        .0
        .iter()
        .flat_map(|recipe| {
            recipe.extracts.iter().map(|extract| {
                (
                    format!("{}/{}", recipe.name, extract.name),
                    extract.title.clone(),
                )
            })
        })
        .collect()
}

fn relative<'a>(root: &str, path: &'a str) -> Option<&'a str> {
    if root.is_empty() {
        Some(path)
    } else {
        path.strip_prefix(root)?.strip_prefix('/')
    }
}
