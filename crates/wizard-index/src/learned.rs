//! What past sessions taught about the project, weighted by age.
//!
//! Learned evidence fades with a 30-day half-life, so the habits of an old phase of the project
//! stop steering the ranking.

use std::path::Path;

use wizard_learn::{LEARN_FILE, Learn};
use wizard_store::DATA_DIRECTORY;

/// Time after which learned evidence counts half, in milliseconds.
pub const HALF_LIFE_MS: f64 = 30.0 * 24.0 * 60.0 * 60.0 * 1000.0;

/// Two files touched in the same sessions.
#[derive(Debug, Clone, PartialEq)]
pub struct LearnedPair {
    /// First file, the smaller path.
    pub first: String,
    /// Second file.
    pub second: String,
    /// Number of shared sessions, faded by the age of the latest one.
    pub weight: f64,
}

/// Evidence learned from past sessions of a project.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Learned {
    /// Files touched together, strongest first.
    pub co_access: Vec<LearnedPair>,
}

impl Learned {
    /// Reads the evidence of `project` as of `now`, in milliseconds since the Unix epoch.
    ///
    /// A project without a learning store, or whose store cannot be read, has no evidence.
    #[must_use]
    pub fn load(project: &Path, now: i64) -> Self {
        if !project.join(DATA_DIRECTORY).join(LEARN_FILE).is_file() {
            return Self::default();
        }
        Learn::open_project(project)
            .ok()
            .and_then(|learn| Self::from_store(&learn, now))
            .unwrap_or_default()
    }

    /// Reads the evidence held by `learn` as of `now`.
    #[must_use]
    pub fn from_store(learn: &Learn, now: i64) -> Option<Self> {
        let mut co_access: Vec<LearnedPair> = learn
            .co_access()
            .ok()?
            .into_iter()
            .map(|pair| LearnedPair {
                weight: f64::from(pair.sessions) * decay(now - pair.last_at),
                first: pair.first,
                second: pair.second,
            })
            .collect();
        co_access.sort_by(|left, right| right.weight.total_cmp(&left.weight));
        Some(Self { co_access })
    }

    /// Files most often touched with `path`, strongest first.
    pub fn partners<'a>(&'a self, path: &'a str) -> impl Iterator<Item = (&'a str, f64)> + 'a {
        self.co_access.iter().filter_map(move |pair| {
            if pair.first == path {
                Some((pair.second.as_str(), pair.weight))
            } else if pair.second == path {
                Some((pair.first.as_str(), pair.weight))
            } else {
                None
            }
        })
    }
}

/// Weight left to evidence `age` milliseconds old.
#[must_use]
#[allow(
    clippy::cast_precision_loss,
    reason = "ages stay far below 2^52 milliseconds"
)]
pub fn decay(age: i64) -> f64 {
    0.5_f64.powf(age.max(0) as f64 / HALF_LIFE_MS)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use wizard_learn::{Event, EventKind, Learn};

    use super::{HALF_LIFE_MS, Learned, decay};

    #[test]
    fn evidence_halves_every_thirty_days() {
        assert!((decay(0) - 1.0).abs() < 1e-12);
        #[allow(
            clippy::cast_possible_truncation,
            reason = "the half-life is an integer"
        )]
        let half_life = HALF_LIFE_MS as i64;
        assert!((decay(half_life) - 0.5).abs() < 1e-12);
        assert!((decay(2 * half_life) - 0.25).abs() < 1e-12);
        assert!((decay(-5) - 1.0).abs() < 1e-12);
    }

    #[test]
    fn loads_co_access_pairs_from_the_store() {
        let project = tempfile::tempdir().unwrap();
        assert_eq!(Learned::load(project.path(), 0), Learned::default());
        assert!(!project.path().join(".wizard").exists());
        let mut learn = Learn::open_project(project.path()).unwrap();
        for (path, at) in [("a.rs", 1), ("b.rs", 2)] {
            learn
                .record(&Event {
                    path: Some(path.to_owned()),
                    ..Event::new("s", at, EventKind::Read)
                })
                .unwrap();
        }
        learn.consolidate("s").unwrap();
        drop(learn);
        let learned = Learned::load(project.path(), 2);
        assert_eq!(learned.co_access.len(), 1);
        assert!((learned.co_access[0].weight - 1.0).abs() < 1e-12);
        assert_eq!(
            learned.partners("b.rs").collect::<Vec<_>>(),
            [("a.rs", 1.0)]
        );
    }
}
