//! Storage of the symbols extracted from each file.

use rusqlite::{TransactionBehavior, params};

use crate::{Index, StoreError};

const NAMES_PER_QUERY: usize = 500;

/// What a stored symbol is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum SymbolKind {
    /// A definition found by a tags query.
    Definition,
    /// A reference found by a tags query.
    Reference,
    /// A Markdown heading.
    Heading,
    /// A top-level key, table or section of a data or configuration file.
    Key,
    /// A frequent identifier of a file without a grammar.
    Identifier,
    /// A fact extracted by a framework recipe, such as a route; its role names the recipe and
    /// the extraction as `recipe/extract`.
    Fact,
    /// The first sentence of the comment documenting a definition, on the definition's line;
    /// its role is the name of the definition.
    Doc,
}

impl SymbolKind {
    fn as_str(self) -> &'static str {
        match self {
            Self::Definition => "definition",
            Self::Reference => "reference",
            Self::Heading => "heading",
            Self::Key => "key",
            Self::Identifier => "identifier",
            Self::Fact => "fact",
            Self::Doc => "doc",
        }
    }

    fn parse(text: &str) -> Option<Self> {
        Some(match text {
            "definition" => Self::Definition,
            "reference" => Self::Reference,
            "heading" => Self::Heading,
            "key" => Self::Key,
            "identifier" => Self::Identifier,
            "fact" => Self::Fact,
            "doc" => Self::Doc,
            _ => return None,
        })
    }
}

/// A symbol of one file.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct SymbolRecord {
    /// First line, numbered from 1.
    pub line: u32,
    /// Kind of symbol.
    pub kind: SymbolKind,
    /// Name, heading text or key.
    pub name: String,
    /// Role given by the extractor, such as `function`; empty when not applicable.
    pub role: String,
    /// Last line, numbered from 1.
    pub end_line: u32,
    /// One-line signature of a definition; empty otherwise.
    pub signature: String,
    /// Number of occurrences, 1 except for identifiers.
    pub count: u32,
}

/// The language and symbols of one file, as produced by extraction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileSymbols {
    /// Path relative to the project root.
    pub path: String,
    /// Detected tree-sitter language, if any.
    pub language: Option<String>,
    /// Symbols of the file.
    pub symbols: Vec<SymbolRecord>,
}

/// A symbol together with the file it belongs to.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct LocatedSymbol {
    /// Path relative to the project root.
    pub path: String,
    /// The symbol.
    pub symbol: SymbolRecord,
}

/// A file of the index with its language.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileRecord {
    /// Path relative to the project root.
    pub path: String,
    /// Size in bytes.
    pub size: u64,
    /// Detected tree-sitter language, if any.
    pub language: Option<String>,
}

impl Index {
    /// Replaces the language and symbols of each file in `files`, in one transaction.
    ///
    /// Files absent from the index are skipped.
    ///
    /// # Errors
    ///
    /// Returns an error when the database cannot be written.
    pub fn store_symbols(&mut self, files: &[FileSymbols]) -> Result<(), StoreError> {
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        if !files.is_empty() {
            transaction.execute("DELETE FROM cache", [])?;
        }
        {
            let mut language =
                transaction.prepare("UPDATE files SET language = ?2 WHERE path = ?1")?;
            let mut clear = transaction.prepare("DELETE FROM symbols WHERE path = ?1")?;
            let mut insert = transaction.prepare(
                "INSERT INTO symbols (path, kind, name, role, line, end_line, signature, count)
                 SELECT ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8 WHERE EXISTS (SELECT 1 FROM files WHERE path = ?1)",
            )?;
            for file in files {
                language.execute(params![file.path, file.language])?;
                clear.execute([&file.path])?;
                for symbol in &file.symbols {
                    insert.execute(params![
                        file.path,
                        symbol.kind.as_str(),
                        symbol.name,
                        symbol.role,
                        symbol.line,
                        symbol.end_line,
                        symbol.signature,
                        symbol.count
                    ])?;
                }
            }
        }
        transaction.commit()?;
        Ok(())
    }

    /// Returns every file of the index with its language, sorted by path.
    ///
    /// # Errors
    ///
    /// Returns an error when the database cannot be queried.
    pub fn files(&self) -> Result<Vec<FileRecord>, StoreError> {
        let mut statement = self
            .connection
            .prepare("SELECT path, size, language FROM files ORDER BY path")?;
        let rows = statement.query_map([], |row| {
            Ok(FileRecord {
                path: row.get(0)?,
                size: u64::try_from(row.get::<_, i64>(1)?).unwrap_or(0),
                language: row.get(2)?,
            })
        })?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    /// Returns every symbol of the index, sorted by path, then line, kind and name.
    ///
    /// # Errors
    ///
    /// Returns an error when the database cannot be queried.
    pub fn symbols(&self) -> Result<Vec<LocatedSymbol>, StoreError> {
        self.query_symbols(
            "SELECT path, kind, name, role, line, end_line, signature, count FROM symbols",
            [],
        )
    }

    /// Returns the symbols of `path`, sorted by line, kind and name.
    ///
    /// # Errors
    ///
    /// Returns an error when the database cannot be queried.
    pub fn symbols_of(&self, path: &str) -> Result<Vec<LocatedSymbol>, StoreError> {
        self.query_symbols(
            "SELECT path, kind, name, role, line, end_line, signature, count FROM symbols
             WHERE path = ?1",
            [path],
        )
    }

    /// Returns the symbols named `name` in every file, sorted by path and line.
    ///
    /// # Errors
    ///
    /// Returns an error when the database cannot be queried.
    pub fn symbols_named(&self, name: &str) -> Result<Vec<LocatedSymbol>, StoreError> {
        self.query_symbols(
            "SELECT path, kind, name, role, line, end_line, signature, count FROM symbols
             WHERE name = ?1",
            [name],
        )
    }

    /// Returns the symbols other than references named one of `names`, sorted by path and line.
    ///
    /// # Errors
    ///
    /// Returns an error when the database cannot be queried.
    pub fn definitions_named(&self, names: &[&str]) -> Result<Vec<LocatedSymbol>, StoreError> {
        let mut symbols = Vec::new();
        for chunk in names.chunks(NAMES_PER_QUERY) {
            let placeholders = vec!["?"; chunk.len()].join(", ");
            let sql = format!(
                "SELECT path, kind, name, role, line, end_line, signature, count FROM symbols
                 WHERE kind != 'reference' AND name IN ({placeholders})"
            );
            symbols.extend(self.query_symbols(&sql, rusqlite::params_from_iter(chunk))?);
        }
        symbols.sort_unstable();
        Ok(symbols)
    }

    /// Returns every fact extracted by a recipe, sorted by path and line.
    ///
    /// # Errors
    ///
    /// Returns an error when the database cannot be queried.
    pub fn facts(&self) -> Result<Vec<LocatedSymbol>, StoreError> {
        self.query_symbols(
            "SELECT path, kind, name, role, line, end_line, signature, count FROM symbols
             WHERE kind = 'fact'",
            [],
        )
    }

    fn query_symbols<P: rusqlite::Params>(
        &self,
        sql: &str,
        parameters: P,
    ) -> Result<Vec<LocatedSymbol>, StoreError> {
        let mut statement = self.connection.prepare(sql)?;
        let rows = statement.query_map(parameters, |row| {
            let kind: String = row.get(1)?;
            Ok((
                row.get::<_, String>(0)?,
                kind,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, i64>(4)?,
                row.get::<_, i64>(5)?,
                row.get::<_, String>(6)?,
                row.get::<_, i64>(7)?,
            ))
        })?;
        let mut symbols = Vec::new();
        for row in rows {
            let (path, kind, name, role, line, end_line, signature, count) = row?;
            let Some(kind) = SymbolKind::parse(&kind) else {
                continue;
            };
            symbols.push(LocatedSymbol {
                path,
                symbol: SymbolRecord {
                    line: to_u32(line),
                    kind,
                    name,
                    role,
                    end_line: to_u32(end_line),
                    signature,
                    count: to_u32(count),
                },
            });
        }
        symbols.sort_unstable();
        Ok(symbols)
    }
}

fn to_u32(value: i64) -> u32 {
    u32::try_from(value).unwrap_or(0)
}
