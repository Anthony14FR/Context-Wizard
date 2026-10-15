//! Git history signals for Context Wizard: churn, co-change and renames.
//!
//! History is read through the `git` command line, whose rename detection is the reference
//! one, and only the most recent commits are considered. Paths are followed through renames, so
//! the history of a file counts under its current name. Commits that touch many files, such as
//! mass reformatting, are left out of co-change: they do not reveal coupling. Without `git`, or
//! outside a repository, there are simply no signals.

use std::collections::{BTreeMap, HashMap};
use std::path::Path;
use std::process::{Command, Stdio};

/// Limits of the history analysis.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HistoryOptions {
    /// Number of most recent non-merge commits to read.
    pub max_commits: usize,
    /// Commits touching more files than this are ignored for co-change.
    pub max_files_per_commit: usize,
}

impl Default for HistoryOptions {
    fn default() -> Self {
        Self {
            max_commits: 500,
            max_files_per_commit: 30,
        }
    }
}

/// How often a file changed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Churn {
    /// Number of commits that touched the file.
    pub commits: u32,
    /// Commit time of the latest change, in seconds since the Unix epoch.
    pub last_changed: i64,
}

/// Signals extracted from the history of a repository.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct History {
    /// Churn per current path.
    pub churn: BTreeMap<String, Churn>,
    /// Number of commits that changed both files of each pair, with the pair sorted.
    pub co_change: BTreeMap<(String, String), u32>,
    /// Number of commits read.
    pub commits: usize,
}

struct Commit {
    time: i64,
    paths: Vec<String>,
}

/// Reads the history of the repository containing `root`, or `None` when `git` is unavailable
/// or `root` is not inside a repository.
#[must_use]
pub fn history(root: &Path, options: &HistoryOptions) -> Option<History> {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args([
            "-c",
            "core.quotepath=false",
            "log",
            "--no-merges",
            "--relative",
            "-M",
            "--name-status",
            "--format=%x00%ct",
        ])
        .arg(format!("-n{}", options.max_commits))
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    Some(analyse(&String::from_utf8_lossy(&output.stdout), options))
}

/// Builds the signals from the output of `git log --name-status --format=%x00%ct`, newest first.
#[must_use]
pub fn analyse(log: &str, options: &HistoryOptions) -> History {
    let (commits, renames) = parse(log);
    let resolve = |path: &str| current_name(path, &renames);
    let mut history = History {
        commits: commits.len(),
        ..History::default()
    };
    for commit in &commits {
        let mut paths: Vec<String> = commit.paths.iter().map(|path| resolve(path)).collect();
        paths.sort_unstable();
        paths.dedup();
        for path in &paths {
            let churn = history.churn.entry(path.clone()).or_insert(Churn {
                commits: 0,
                last_changed: commit.time,
            });
            churn.commits += 1;
            churn.last_changed = churn.last_changed.max(commit.time);
        }
        if paths.len() <= options.max_files_per_commit {
            for (position, left) in paths.iter().enumerate() {
                for right in &paths[position + 1..] {
                    *history
                        .co_change
                        .entry((left.clone(), right.clone()))
                        .or_default() += 1;
                }
            }
        }
    }
    history
}

fn parse(log: &str) -> (Vec<Commit>, HashMap<String, String>) {
    let mut commits = Vec::new();
    let mut renames = HashMap::new();
    for record in log.split('\0').filter(|record| !record.trim().is_empty()) {
        let mut lines = record.lines();
        let Some(time) = lines
            .next()
            .and_then(|line| line.trim().parse::<i64>().ok())
        else {
            continue;
        };
        let mut paths = Vec::new();
        for line in lines {
            let fields: Vec<&str> = line.split('\t').collect();
            match fields.as_slice() {
                [status, old, new] if status.starts_with('R') => {
                    renames
                        .entry((*old).to_owned())
                        .or_insert_with(|| (*new).to_owned());
                    paths.push((*new).to_owned());
                }
                [status, _, new] if status.starts_with('C') => paths.push((*new).to_owned()),
                [status, path] if !status.starts_with('D') => paths.push((*path).to_owned()),
                _ => {}
            }
        }
        commits.push(Commit { time, paths });
    }
    (commits, renames)
}

fn current_name(path: &str, renames: &HashMap<String, String>) -> String {
    let mut current = path;
    for _ in 0..renames.len() {
        match renames.get(current) {
            Some(next) if next != current => current = next,
            _ => break,
        }
    }
    current.to_owned()
}

#[cfg(test)]
mod tests {
    use super::{HistoryOptions, analyse};

    const LOG: &str = "\u{0}300\n\nM\tsrc/b.rs\nM\tsrc/c.rs\n\u{0}200\n\nR100\tsrc/a.rs\tsrc/b.rs\nM\tsrc/c.rs\n\u{0}100\n\nA\tsrc/a.rs\nA\tsrc/c.rs\nD\tsrc/gone.rs\n";

    #[test]
    fn follows_renames_and_counts_co_changes() {
        let history = analyse(LOG, &HistoryOptions::default());
        assert_eq!(history.commits, 3);
        let b = &history.churn["src/b.rs"];
        assert_eq!((b.commits, b.last_changed), (3, 300));
        assert!(!history.churn.contains_key("src/a.rs"));
        assert!(!history.churn.contains_key("src/gone.rs"));
        assert_eq!(
            history.co_change[&("src/b.rs".to_owned(), "src/c.rs".to_owned())],
            3
        );
    }

    #[test]
    fn large_commits_do_not_count_as_coupling() {
        let options = HistoryOptions {
            max_files_per_commit: 1,
            ..HistoryOptions::default()
        };
        let history = analyse(LOG, &options);
        assert!(history.co_change.is_empty());
        assert_eq!(history.churn["src/c.rs"].commits, 3);
    }
}
