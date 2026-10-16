//! Framework recipes: declarative extraction of routes, models, components and the like.
//!
//! A recipe is a directory holding `recipe.toml` and the tree-sitter query files it names. The
//! `[detect]` table says which projects use the framework, and each `[[extract]]` table selects
//! files with glob patterns and turns them into one-line facts, either from the captures of a
//! query or from named groups of a regular expression on the file path, through a template
//! such as `{verb} {uri} -> {action}`.
//!
//! Recipes ship inside the binary and projects can add their own in
//! `.claude/context-wizard/recipes/<name>/`. They are read again at every index refresh, so an
//! edited recipe applies without restarting anything. Every recipe is validated as a whole:
//! unknown fields, invalid globs, queries that do not compile and template placeholders that
//! nothing provides are reported with the recipe, the extract and the field at fault. Queries
//! compile the first time a file needs them, and [`Recipe::check`] compiles them all.

use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::fmt;
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::sync::OnceLock;

use globset::{Glob, GlobSet, GlobSetBuilder};
use regex::Regex;
use serde::Deserialize;
use streaming_iterator::StreamingIterator;
use tree_sitter::{Parser, Query, QueryCursor};

mod builtin;

/// Directory of project recipes, relative to the project root.
pub const PROJECT_RECIPES: &str = ".claude/context-wizard/recipes";

const MAX_FACT_CHARS: usize = 200;

/// A recipe that failed to load.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecipeError {
    /// Name or directory of the recipe.
    pub recipe: String,
    /// Where in the recipe the problem is, such as `extract "routes", query`.
    pub location: String,
    /// What is wrong.
    pub message: String,
}

impl fmt::Display for RecipeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.location.is_empty() {
            write!(formatter, "recipe `{}`: {}", self.recipe, self.message)
        } else {
            write!(
                formatter,
                "recipe `{}`, {}: {}",
                self.recipe, self.location, self.message
            )
        }
    }
}

impl Error for RecipeError {}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RecipeFile {
    name: String,
    #[serde(default)]
    description: String,
    detect: DetectFile,
    #[serde(default)]
    extract: Vec<ExtractFile>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct DetectFile {
    #[serde(default)]
    files: Vec<String>,
    #[serde(default)]
    contains: Vec<ContainsFile>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ContainsFile {
    file: String,
    text: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ExtractFile {
    name: String,
    title: Option<String>,
    glob: Vec<String>,
    #[serde(default)]
    exclude: Vec<String>,
    language: Option<String>,
    query: Option<String>,
    path_pattern: Option<String>,
    template: String,
}

/// A loaded and validated recipe.
#[derive(Debug)]
pub struct Recipe {
    /// Name of the recipe, such as `laravel`.
    pub name: String,
    /// What the recipe extracts.
    pub description: String,
    detect: DetectFile,
    /// The extractions of the recipe.
    pub extracts: Vec<Extract>,
}

/// One extraction of a recipe.
#[derive(Debug)]
pub struct Extract {
    /// Identifier of the extraction, such as `routes`.
    pub name: String,
    /// Heading of its facts, such as `Routes`.
    pub title: String,
    include: GlobSet,
    exclude: GlobSet,
    query: Option<LazyQuery>,
    path_pattern: Option<Regex>,
    template: Vec<Piece>,
}

/// A query file, compiled the first time a file needs it.
#[derive(Debug)]
struct LazyQuery {
    file: String,
    language: String,
    source: String,
    compiled: OnceLock<Result<Query, String>>,
}

impl LazyQuery {
    fn compiled(&self) -> Result<&Query, &String> {
        self.compiled
            .get_or_init(|| {
                let grammar = wizard_parse::grammar::language(&self.language)
                    .map_err(|problem| format!("`{}` is unavailable: {problem}", self.language))?;
                Query::new(&grammar, &self.source)
                    .map_err(|problem| format!("`{}` does not compile: {problem}", self.file))
            })
            .as_ref()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Piece {
    Text(String),
    Value { name: String, optional: bool },
}

/// A fact extracted by a recipe from one file.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Fact {
    /// File, relative to the project root.
    pub path: String,
    /// Line of the fact, numbered from 1.
    pub line: u32,
    /// Name of the recipe.
    pub recipe: String,
    /// Name of the extraction.
    pub extract: String,
    /// The rendered fact.
    pub text: String,
}

impl Recipe {
    /// Parses and validates a recipe from its `recipe.toml` text, reading query files with
    /// `read_query`.
    ///
    /// # Errors
    ///
    /// Returns every problem found, so an author can fix them all at once.
    pub fn parse(
        label: &str,
        text: &str,
        read_query: &dyn Fn(&str) -> Option<String>,
    ) -> Result<Self, Vec<RecipeError>> {
        let error = |location: &str, message: String| RecipeError {
            recipe: label.to_owned(),
            location: location.to_owned(),
            message,
        };
        let file: RecipeFile =
            toml::from_str(text).map_err(|parse| vec![error("", parse.to_string())])?;
        let mut errors = Vec::new();
        if file.detect.files.is_empty() && file.detect.contains.is_empty() {
            errors.push(error(
                "detect",
                "needs at least one entry in `files` or `contains`".to_owned(),
            ));
        }
        let mut extracts = Vec::new();
        let mut names = BTreeSet::new();
        for extract in file.extract {
            let location = format!("extract \"{}\"", extract.name);
            if !names.insert(extract.name.clone()) {
                errors.push(error(&location, "is defined twice".to_owned()));
            }
            match build_extract(extract, read_query) {
                Ok(built) => extracts.push(built),
                Err(problems) => errors.extend(
                    problems
                        .into_iter()
                        .map(|(field, message)| error(&format!("{location}, {field}"), message)),
                ),
            }
        }
        if extracts.is_empty() && errors.is_empty() {
            errors.push(error("", "has no `[[extract]]` table".to_owned()));
        }
        if errors.is_empty() {
            Ok(Self {
                name: file.name,
                description: file.description,
                detect: file.detect,
                extracts,
            })
        } else {
            Err(errors)
        }
    }

    /// Loads the recipe in `directory`.
    ///
    /// # Errors
    ///
    /// Returns every problem found in the recipe.
    pub fn load(directory: &Path) -> Result<Self, Vec<RecipeError>> {
        let label = directory.file_name().map_or_else(
            || directory.display().to_string(),
            |name| name.to_string_lossy().into_owned(),
        );
        let text = fs::read_to_string(directory.join("recipe.toml")).map_err(|source| {
            vec![RecipeError {
                recipe: label.clone(),
                location: "recipe.toml".to_owned(),
                message: source.to_string(),
            }]
        })?;
        let read_query = |name: &str| {
            let inside = Path::new(name)
                .components()
                .all(|component| matches!(component, Component::Normal(_)));
            inside
                .then(|| fs::read_to_string(directory.join(name)).ok())
                .flatten()
        };
        Self::parse(&label, &text, &read_query)
    }

    /// Compiles every query of the recipe and reports those that fail, which loading alone does
    /// not, so that loading stays cheap enough for every index refresh.
    #[must_use]
    pub fn check(&self) -> Vec<RecipeError> {
        self.extracts
            .iter()
            .filter_map(|extract| {
                let lazy = extract.query.as_ref()?;
                let problem = lazy.compiled().err()?;
                Some(RecipeError {
                    recipe: self.name.clone(),
                    location: format!("extract \"{}\", query", extract.name),
                    message: problem.clone(),
                })
            })
            .collect()
    }

    /// Returns `true` when the project rooted at `root` uses the recipe's framework.
    #[must_use]
    pub fn detects(&self, root: &Path) -> bool {
        self.detect
            .files
            .iter()
            .all(|file| root.join(file).exists())
            && self.detect.contains.iter().all(|contains| {
                fs::read_to_string(root.join(&contains.file))
                    .is_ok_and(|content| content.contains(&contains.text))
            })
    }

    /// Returns `true` when an extraction of the recipe reads `path`, relative to the root the
    /// recipe was detected in.
    #[must_use]
    pub fn reads(&self, path: &str) -> bool {
        self.extracts.iter().any(|extract| extract.selects(path))
    }

    /// Extracts the facts of one file, `path` being relative to the detected root and
    /// `project_path` relative to the project root.
    #[must_use]
    pub fn facts(&self, path: &str, project_path: &str, source: &str) -> Vec<Fact> {
        let mut facts = Vec::new();
        for extract in self.extracts.iter().filter(|extract| extract.selects(path)) {
            for (line, values) in extract.values(path, source) {
                if let Some(text) = render(&extract.template, &values) {
                    facts.push(Fact {
                        path: project_path.to_owned(),
                        line,
                        recipe: self.name.clone(),
                        extract: extract.name.clone(),
                        text,
                    });
                }
            }
        }
        facts.sort();
        facts.dedup();
        facts
    }
}

impl Extract {
    fn selects(&self, path: &str) -> bool {
        self.include.is_match(path) && !self.exclude.is_match(path)
    }

    fn values(&self, path: &str, source: &str) -> Vec<(u32, BTreeMap<String, String>)> {
        let mut base = BTreeMap::from([("file".to_owned(), path.to_owned())]);
        if let Some(pattern) = &self.path_pattern {
            let Some(found) = pattern.captures(path) else {
                return Vec::new();
            };
            for name in pattern.capture_names().flatten() {
                if let Some(group) = found.name(name) {
                    base.insert(name.to_owned(), group.as_str().to_owned());
                }
            }
        }
        let Some(lazy) = &self.query else {
            return vec![(1, base)];
        };
        let Ok(query) = lazy.compiled() else {
            return Vec::new();
        };
        let Ok(grammar) = wizard_parse::grammar::language(&lazy.language) else {
            return Vec::new();
        };
        let mut parser = Parser::new();
        if parser.set_language(&grammar).is_err() {
            return Vec::new();
        }
        let Some(tree) = parser.parse(source, None) else {
            return Vec::new();
        };
        let names = query.capture_names();
        let mut cursor = QueryCursor::new();
        let mut matches = cursor.matches(query, tree.root_node(), source.as_bytes());
        let mut values = Vec::new();
        while let Some(found) = matches.next() {
            let mut captured: BTreeMap<String, Vec<String>> = BTreeMap::new();
            let mut line = u32::MAX;
            for capture in found.captures() {
                let Some(name) = names.get(usize::try_from(capture.index).unwrap_or(usize::MAX))
                else {
                    continue;
                };
                if name.starts_with('_') {
                    continue;
                }
                let start = capture.node.start_position().row;
                line = line.min(u32::try_from(start + 1).unwrap_or(u32::MAX));
                if let Ok(text) = capture.node.utf8_text(source.as_bytes()) {
                    captured
                        .entry((*name).to_owned())
                        .or_default()
                        .push(clean(text));
                }
            }
            if captured.is_empty() {
                continue;
            }
            let mut entry = base.clone();
            for (name, texts) in captured {
                entry.insert(name, texts.join(", "));
            }
            values.push((line, entry));
        }
        values
    }
}

/// The recipes that ship with Context Wizard, with the problems of any that fail to load.
#[must_use]
pub fn builtin() -> (Vec<Recipe>, Vec<RecipeError>) {
    let mut recipes = Vec::new();
    let mut errors = Vec::new();
    for (name, recipe, files) in builtin::RECIPES {
        let read_query = |wanted: &str| {
            files
                .iter()
                .find(|(file, _)| *file == wanted)
                .map(|(_, content)| (*content).to_owned())
        };
        match Recipe::parse(name, recipe, &read_query) {
            Ok(recipe) => recipes.push(recipe),
            Err(problems) => errors.extend(problems),
        }
    }
    (recipes, errors)
}

/// The recipes of `project`: the built-in ones, then the project's own, which replace a
/// built-in recipe of the same name.
#[must_use]
pub fn load(project: &Path) -> (Vec<Recipe>, Vec<RecipeError>) {
    let (mut recipes, mut errors) = builtin();
    let directory = project.join(PROJECT_RECIPES);
    let Ok(entries) = fs::read_dir(&directory) else {
        return (recipes, errors);
    };
    let mut directories: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.join("recipe.toml").is_file())
        .collect();
    directories.sort();
    for directory in directories {
        match Recipe::load(&directory) {
            Ok(recipe) => {
                recipes.retain(|known| known.name != recipe.name);
                recipes.push(recipe);
            }
            Err(problems) => errors.extend(problems),
        }
    }
    (recipes, errors)
}

/// A fingerprint of the recipe sources of `project`, which changes whenever a recipe does.
#[must_use]
pub fn fingerprint(project: &Path) -> String {
    let mut hasher = blake3::Hasher::new();
    hasher.update(wizard_core::VERSION.as_bytes());
    let directory = project.join(PROJECT_RECIPES);
    let mut files: Vec<PathBuf> = Vec::new();
    if let Ok(entries) = fs::read_dir(&directory) {
        for entry in entries.filter_map(Result::ok) {
            if let Ok(inner) = fs::read_dir(entry.path()) {
                files.extend(inner.filter_map(Result::ok).map(|file| file.path()));
            }
        }
    }
    files.sort();
    for file in files {
        hasher.update(file.to_string_lossy().as_bytes());
        if let Ok(content) = fs::read(&file) {
            hasher.update(&content);
        }
    }
    hasher.finalize().to_hex().to_string()
}

fn build_extract(
    extract: ExtractFile,
    read_query: &dyn Fn(&str) -> Option<String>,
) -> Result<Extract, Vec<(&'static str, String)>> {
    let mut errors = Vec::new();
    let include = globs(&extract.glob, "glob", &mut errors);
    let exclude = globs(&extract.exclude, "exclude", &mut errors);
    if extract.glob.is_empty() {
        errors.push(("glob", "needs at least one pattern".to_owned()));
    }
    let mut provided: BTreeSet<String> = BTreeSet::from(["file".to_owned()]);
    let query = read_query_file(
        extract.query.as_ref(),
        extract.language.as_ref(),
        read_query,
        &mut provided,
        &mut errors,
    );
    let path_pattern =
        extract
            .path_pattern
            .as_deref()
            .and_then(|pattern| match Regex::new(pattern) {
                Ok(regex) => {
                    provided.extend(regex.capture_names().flatten().map(str::to_owned));
                    Some(regex)
                }
                Err(problem) => {
                    errors.push(("path_pattern", problem.to_string()));
                    None
                }
            });
    if extract.query.is_none() && extract.path_pattern.is_none() {
        errors.push((
            "query",
            "or `path_pattern` is needed to produce facts".to_owned(),
        ));
    }
    let template = match parse_template(&extract.template) {
        Ok(template) => template,
        Err(message) => {
            errors.push(("template", message));
            Vec::new()
        }
    };
    let queries_failed = extract.query.is_some() && query.is_none();
    let pattern_failed = extract.path_pattern.is_some() && path_pattern.is_none();
    if !queries_failed && !pattern_failed {
        for piece in &template {
            if let Piece::Value { name, .. } = piece
                && !provided.contains(name)
            {
                errors.push((
                    "template",
                    format!("`{{{name}}}` is neither a query capture nor a path group"),
                ));
            }
        }
    }
    if !errors.is_empty() {
        return Err(errors);
    }
    Ok(Extract {
        title: extract.title.unwrap_or_else(|| extract.name.clone()),
        name: extract.name,
        include,
        exclude,
        query,
        path_pattern,
        template,
    })
}

fn read_query_file(
    query: Option<&String>,
    language: Option<&String>,
    read_query: &dyn Fn(&str) -> Option<String>,
    provided: &mut BTreeSet<String>,
    errors: &mut Vec<(&'static str, String)>,
) -> Option<LazyQuery> {
    match (query, language) {
        (Some(file), Some(language)) => {
            let Some(source) = read_query(file) else {
                errors.push(("query", format!("file `{file}` cannot be read")));
                return None;
            };
            provided.extend(capture_names(&source));
            Some(LazyQuery {
                file: file.clone(),
                language: language.clone(),
                source,
                compiled: OnceLock::new(),
            })
        }
        (Some(_), None) => {
            errors.push(("language", "is required with `query`".to_owned()));
            None
        }
        (None, Some(_)) => {
            errors.push(("query", "is required with `language`".to_owned()));
            None
        }
        (None, None) => None,
    }
}

fn capture_names(source: &str) -> Vec<String> {
    let mut names = Vec::new();
    let mut rest = source;
    while let Some(at) = rest.find('@') {
        rest = &rest[at + 1..];
        let name: String = rest
            .chars()
            .take_while(|character| character.is_alphanumeric() || "_.-".contains(*character))
            .collect();
        if !name.is_empty() && !name.starts_with('_') {
            names.push(name);
        }
    }
    names
}

fn globs(
    patterns: &[String],
    field: &'static str,
    errors: &mut Vec<(&'static str, String)>,
) -> GlobSet {
    let mut builder = GlobSetBuilder::new();
    for pattern in patterns {
        match Glob::new(pattern) {
            Ok(glob) => {
                builder.add(glob);
            }
            Err(problem) => errors.push((field, format!("`{pattern}`: {problem}"))),
        }
    }
    builder.build().unwrap_or_else(|_| GlobSet::empty())
}

fn parse_template(template: &str) -> Result<Vec<Piece>, String> {
    let mut pieces = Vec::new();
    let mut rest = template;
    while let Some(open) = rest.find('{') {
        if open > 0 {
            pieces.push(Piece::Text(rest[..open].to_owned()));
        }
        let after = &rest[open + 1..];
        let close = after
            .find('}')
            .ok_or_else(|| format!("unclosed `{{` in `{template}`"))?;
        let name = &after[..close];
        let (name, optional) = name
            .strip_suffix('?')
            .map_or((name, false), |name| (name, true));
        if name.is_empty()
            || !name
                .chars()
                .all(|character| character.is_ascii_alphanumeric() || character == '_')
        {
            return Err(format!("`{{{name}}}` is not a valid placeholder"));
        }
        pieces.push(Piece::Value {
            name: name.to_owned(),
            optional,
        });
        rest = &after[close + 1..];
    }
    if !rest.is_empty() {
        pieces.push(Piece::Text(rest.to_owned()));
    }
    if pieces.is_empty() {
        return Err("is empty".to_owned());
    }
    Ok(pieces)
}

fn render(template: &[Piece], values: &BTreeMap<String, String>) -> Option<String> {
    let mut text = String::new();
    for piece in template {
        match piece {
            Piece::Text(part) => text.push_str(part),
            Piece::Value { name, optional } => match values.get(name) {
                Some(value) => text.push_str(value),
                None if *optional => {}
                None => return None,
            },
        }
    }
    let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
    let text: String = wizard_core::secrets::redact(&text)
        .chars()
        .take(MAX_FACT_CHARS)
        .collect();
    (!text.is_empty()).then_some(text)
}

fn clean(text: &str) -> String {
    let trimmed = text.trim();
    let unquoted = ['\'', '"', '`']
        .iter()
        .find_map(|quote| {
            trimmed
                .strip_prefix(*quote)
                .and_then(|inner| inner.strip_suffix(*quote))
        })
        .unwrap_or(trimmed);
    unquoted.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests;
