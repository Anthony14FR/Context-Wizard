//! The words of each file, ranked against a task with BM25.
//!
//! A file's words are the stemmed parts of its definition names, headings and recipe facts, of
//! its file name, which counts twice, and of its folder. Matching whole stemmed words weighted by rarity finds the files of a task far
//! better than substrings: `select` no longer pulls in every component whose name contains it,
//! and a rare word such as `podcast` outweighs a common one such as `list`.

use std::collections::{BTreeMap, HashMap};

use wizard_core::text::stems;
use wizard_store::{Index, LocatedSymbol, StoreError, SymbolKind};

/// Name of the cached vocabulary in the index.
pub const VOCABULARY_CACHE: &str = "vocabulary-v2";

const K1: f64 = 1.2;
const B: f64 = 0.75;
const PATH_WEIGHT: u32 = 2;

/// The stemmed words of every file, as an inverted index over the files of a structure.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Vocabulary {
    terms: Vec<String>,
    postings: Vec<Vec<(u32, u32)>>,
    lengths: Vec<u32>,
}

impl Vocabulary {
    /// Builds the vocabulary of `paths`, the sorted nodes of a structure, from `symbols`.
    #[must_use]
    pub fn build(symbols: &[LocatedSymbol], paths: &[String]) -> Self {
        let position = |path: &str| {
            paths
                .binary_search_by(|known| known.as_str().cmp(path))
                .ok()
        };
        let mut counts: BTreeMap<String, BTreeMap<u32, u32>> = BTreeMap::new();
        let mut lengths = vec![0_u32; paths.len()];
        let mut add = |term: String, node: usize, weight: u32| {
            let Ok(file) = u32::try_from(node) else {
                return;
            };
            *counts.entry(term).or_default().entry(file).or_default() += weight;
            lengths[node] += weight;
        };
        for located in symbols {
            if !matches!(
                located.symbol.kind,
                SymbolKind::Definition | SymbolKind::Heading | SymbolKind::Fact
            ) {
                continue;
            }
            if let Some(node) = position(&located.path) {
                for term in stems(&located.symbol.name) {
                    add(term, node, 1);
                }
            }
        }
        for (node, path) in paths.iter().enumerate() {
            let mut parts = path.rsplit('/');
            let name = parts.next().unwrap_or(path);
            let stem = name.split('.').next().unwrap_or(name);
            for term in stems(stem) {
                add(term, node, PATH_WEIGHT);
            }
            if let Some(folder) = parts.next() {
                for term in stems(folder) {
                    add(term, node, 1);
                }
            }
        }
        let (terms, postings) = counts
            .into_iter()
            .map(|(term, files)| (term, files.into_iter().collect()))
            .unzip();
        Self {
            terms,
            postings,
            lengths,
        }
    }

    /// Reads the cached vocabulary of `index`, building and caching it when missing.
    ///
    /// # Errors
    ///
    /// Returns an error when the index cannot be read.
    pub fn load(index: &mut Index, paths: &[String]) -> Result<Self, StoreError> {
        if let Some(vocabulary) = index
            .cached(VOCABULARY_CACHE)?
            .as_deref()
            .and_then(Self::from_bytes)
            .filter(|vocabulary| vocabulary.lengths.len() == paths.len())
        {
            return Ok(vocabulary);
        }
        let vocabulary = Self::build(&index.symbols()?, paths);
        let _ = index.set_cached(VOCABULARY_CACHE, &vocabulary.to_bytes());
        Ok(vocabulary)
    }

    /// BM25 score of every file sharing a word with `query`, by node index.
    #[must_use]
    #[allow(
        clippy::cast_precision_loss,
        reason = "file and word counts stay far below 2^52"
    )]
    pub fn scores(&self, query: &str) -> HashMap<usize, f64> {
        let files = self.lengths.len() as f64;
        let mut scores: HashMap<usize, f64> = HashMap::new();
        if self.lengths.is_empty() {
            return scores;
        }
        let average = self
            .lengths
            .iter()
            .map(|length| f64::from(*length))
            .sum::<f64>()
            / files;
        let mut words = stems(query);
        words.sort_unstable();
        words.dedup();
        for word in words {
            let Ok(found) = self.terms.binary_search(&word) else {
                continue;
            };
            let postings = &self.postings[found];
            let frequency = postings.len() as f64;
            let idf = ((files - frequency + 0.5) / (frequency + 0.5) + 1.0).ln();
            for (file, count) in postings {
                let Ok(node) = usize::try_from(*file) else {
                    continue;
                };
                let count = f64::from(*count);
                let length = f64::from(self.lengths[node]);
                let norm = K1 * (1.0 - B + B * length / average.max(1.0));
                *scores.entry(node).or_default() += idf * count * (K1 + 1.0) / (count + norm);
            }
        }
        scores
    }

    /// Share of the rarity-weighted words of `query` known to the project that `files` hold, or
    /// `None` when the project knows none of them.
    ///
    /// A prediction whose files hold the rare words of the task is likely right; one that only
    /// matches common words, or matches nothing the task is about, is likely wrong.
    #[must_use]
    #[allow(
        clippy::cast_precision_loss,
        reason = "file and word counts stay far below 2^52"
    )]
    pub fn coverage(&self, query: &str, files: &[usize]) -> Option<f64> {
        let total = self.lengths.len() as f64;
        let mut words = stems(query);
        words.sort_unstable();
        words.dedup();
        let (mut known, mut held) = (0.0, 0.0);
        for word in words {
            let Ok(found) = self.terms.binary_search(&word) else {
                continue;
            };
            let postings = &self.postings[found];
            let frequency = postings.len() as f64;
            let idf = ((total - frequency + 0.5) / (frequency + 0.5) + 1.0).ln();
            known += idf;
            if postings
                .iter()
                .any(|(file, _)| usize::try_from(*file).is_ok_and(|file| files.contains(&file)))
            {
                held += idf;
            }
        }
        (known > 0.0).then(|| held / known)
    }

    /// Whether `name` shares a stemmed word with `query`.
    #[must_use]
    pub fn relates(name: &str, query: &[String]) -> bool {
        stems(name).iter().any(|word| query.contains(word))
    }

    /// Encodes the vocabulary for the index cache.
    #[must_use]
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::new();
        push(&mut bytes, self.lengths.len());
        for length in &self.lengths {
            bytes.extend_from_slice(&length.to_le_bytes());
        }
        push(&mut bytes, self.terms.len());
        for (term, postings) in self.terms.iter().zip(&self.postings) {
            push(&mut bytes, term.len());
            bytes.extend_from_slice(term.as_bytes());
            push(&mut bytes, postings.len());
            for (file, count) in postings {
                bytes.extend_from_slice(&file.to_le_bytes());
                bytes.extend_from_slice(&count.to_le_bytes());
            }
        }
        bytes
    }

    /// Decodes a vocabulary written by [`Vocabulary::to_bytes`].
    #[must_use]
    pub fn from_bytes(bytes: &[u8]) -> Option<Self> {
        let mut reader = Reader { bytes };
        let files = reader.count()?;
        let lengths = (0..files)
            .map(|_| reader.u32())
            .collect::<Option<Vec<u32>>>()?;
        let count = reader.count()?;
        let mut terms = Vec::with_capacity(count.min(1 << 20));
        let mut postings = Vec::with_capacity(count.min(1 << 20));
        for _ in 0..count {
            let length = reader.count()?;
            terms.push(String::from_utf8(reader.take(length)?.to_vec()).ok()?);
            let entries = reader.count()?;
            let list = (0..entries)
                .map(|_| Some((reader.u32()?, reader.u32()?)))
                .collect::<Option<Vec<(u32, u32)>>>()?;
            if list
                .iter()
                .any(|(file, _)| usize::try_from(*file).map_or(true, |file| file >= files))
            {
                return None;
            }
            postings.push(list);
        }
        (reader.bytes.is_empty() && terms.is_sorted()).then_some(Self {
            terms,
            postings,
            lengths,
        })
    }
}

fn push(bytes: &mut Vec<u8>, count: usize) {
    bytes.extend_from_slice(&(count as u64).to_le_bytes());
}

struct Reader<'a> {
    bytes: &'a [u8],
}

impl<'a> Reader<'a> {
    fn take(&mut self, length: usize) -> Option<&'a [u8]> {
        if self.bytes.len() < length {
            return None;
        }
        let (head, rest) = self.bytes.split_at(length);
        self.bytes = rest;
        Some(head)
    }

    fn count(&mut self) -> Option<usize> {
        let (head, rest) = self.bytes.split_first_chunk::<8>()?;
        self.bytes = rest;
        usize::try_from(u64::from_le_bytes(*head)).ok()
    }

    fn u32(&mut self) -> Option<u32> {
        let (head, rest) = self.bytes.split_first_chunk::<4>()?;
        self.bytes = rest;
        Some(u32::from_le_bytes(*head))
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use wizard_store::{LocatedSymbol, SymbolKind, SymbolRecord};

    use super::Vocabulary;

    fn symbol(path: &str, name: &str, kind: SymbolKind) -> LocatedSymbol {
        LocatedSymbol {
            path: path.to_owned(),
            symbol: SymbolRecord {
                line: 1,
                kind,
                name: name.to_owned(),
                role: String::new(),
                end_line: 1,
                signature: String::new(),
                count: 1,
            },
        }
    }

    #[test]
    fn ranks_files_by_rare_whole_words() {
        let paths: Vec<String> = [
            "app/Jobs/DeleteSongFilesJob.php",
            "app/Services/SongService.php",
            "ui/ThemeSelectBox.vue",
        ]
        .map(str::to_owned)
        .to_vec();
        let symbols = vec![
            symbol(
                "app/Services/SongService.php",
                "deleteSongs",
                SymbolKind::Definition,
            ),
            symbol("ui/ThemeSelectBox.vue", "selected", SymbolKind::Identifier),
        ];
        let vocabulary = Vocabulary::build(&symbols, &paths);
        let scores = vocabulary.scores("an admin removes tracks and deletes songs");
        assert!(scores[&1] > 0.0 && scores[&0] > 0.0, "{scores:?}");
        assert!(!scores.contains_key(&2), "{scores:?}");
        let bytes = vocabulary.to_bytes();
        assert_eq!(Vocabulary::from_bytes(&bytes), Some(vocabulary));
        assert_eq!(Vocabulary::from_bytes(&bytes[..bytes.len() - 1]), None);
        assert!(Vocabulary::relates("deleteSongs", &["song".to_owned()]));
    }
}
