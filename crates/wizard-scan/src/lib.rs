//! File walking for Context Wizard.
//!
//! The walk honours `.gitignore`, `.ignore`, `.git/info/exclude` and `.wizardignore` files inside
//! the project only: neither the user's global git ignore file nor ignore files in directories
//! above the root apply. It skips version control, Claude Code settings and Context Wizard
//! entries whatever their
//! case, symbolic links, binary files (a NUL byte in the first 8 KiB, which also covers UTF-16
//! text) and oversized files, and returns files sorted by path so the same tree always gives the
//! same result. Empty files are kept.

use std::fs::{self, File};
use std::io::Read;
use std::path::{Component, Path};
use std::sync::mpsc;
use std::time::SystemTime;

use ignore::{WalkBuilder, WalkState};

pub mod projects;

/// Name of the project-level ignore file read in addition to `.gitignore`.
pub const IGNORE_FILE: &str = ".wizardignore";

const SKIPPED_DIRECTORIES: [&str; 5] = [".claude", ".git", ".hg", ".svn", ".wizard"];
const SNIFF_BYTES: usize = 8 * 1024;

/// Limits applied while scanning.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScanOptions {
    /// Files larger than this many bytes are skipped.
    pub max_file_size: u64,
}

impl Default for ScanOptions {
    fn default() -> Self {
        Self {
            max_file_size: 1024 * 1024,
        }
    }
}

/// A text file found by the scan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileEntry {
    /// Path relative to the scanned root, with `/` separators.
    pub path: String,
    /// Size in bytes.
    pub size: u64,
    /// Last modification time, when the platform reports it.
    pub modified: Option<SystemTime>,
}

/// Result of a scan.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ScanReport {
    /// Text files, sorted by path.
    pub files: Vec<FileEntry>,
    /// Number of files skipped because they look binary.
    pub binary: usize,
    /// Number of files skipped because they exceed the size limit.
    pub too_large: usize,
    /// Number of entries that could not be read or have a path that is not valid UTF-8.
    pub unreadable: usize,
}

enum Outcome {
    Text(FileEntry),
    Binary,
    TooLarge,
    Unreadable,
    NotAFile,
}

/// Scans `root` in parallel and returns its text files.
#[must_use]
pub fn scan(root: &Path, options: &ScanOptions) -> ScanReport {
    let (sender, receiver) = mpsc::channel();
    WalkBuilder::new(root)
        .hidden(false)
        .parents(false)
        .git_global(false)
        .require_git(false)
        .follow_links(false)
        .add_custom_ignore_filename(IGNORE_FILE)
        .filter_entry(|entry| entry.depth() == 0 || !is_skipped_name(entry.file_name()))
        .build_parallel()
        .run(|| {
            let sender = sender.clone();
            Box::new(move |entry| {
                let outcome = match entry {
                    Ok(entry) => inspect(root, entry.path(), options),
                    Err(_) => Outcome::Unreadable,
                };
                let _ = sender.send(outcome);
                WalkState::Continue
            })
        });
    drop(sender);
    let mut report = ScanReport::default();
    for outcome in receiver {
        record(&mut report, outcome);
    }
    report
        .files
        .sort_unstable_by(|left, right| left.path.cmp(&right.path));
    report
}

fn is_skipped_name(name: &std::ffi::OsStr) -> bool {
    name.to_str().is_some_and(|name| {
        SKIPPED_DIRECTORIES
            .iter()
            .any(|skipped| skipped.eq_ignore_ascii_case(name))
    })
}

fn record(report: &mut ScanReport, outcome: Outcome) {
    match outcome {
        Outcome::Text(file) => report.files.push(file),
        Outcome::Binary => report.binary += 1,
        Outcome::TooLarge => report.too_large += 1,
        Outcome::Unreadable => report.unreadable += 1,
        Outcome::NotAFile => {}
    }
}

fn inspect(root: &Path, path: &Path, options: &ScanOptions) -> Outcome {
    let Ok(metadata) = fs::symlink_metadata(path) else {
        return Outcome::Unreadable;
    };
    if !metadata.is_file() {
        return Outcome::NotAFile;
    }
    let Some(relative) = relative_path(root, path) else {
        return Outcome::Unreadable;
    };
    if metadata.len() > options.max_file_size {
        return Outcome::TooLarge;
    }
    match looks_binary(path) {
        Some(true) => Outcome::Binary,
        Some(false) => Outcome::Text(FileEntry {
            path: relative,
            size: metadata.len(),
            modified: metadata.modified().ok(),
        }),
        None => Outcome::Unreadable,
    }
}

fn relative_path(root: &Path, path: &Path) -> Option<String> {
    let relative = path.strip_prefix(root).ok()?;
    let mut parts = Vec::new();
    for component in relative.components() {
        match component {
            Component::Normal(part) => parts.push(part.to_str()?),
            _ => return None,
        }
    }
    (!parts.is_empty()).then(|| parts.join("/"))
}

fn looks_binary(path: &Path) -> Option<bool> {
    let mut buffer = [0_u8; SNIFF_BYTES];
    let mut file = File::open(path).ok()?;
    let mut filled = 0;
    while filled < buffer.len() {
        match file.read(&mut buffer[filled..]) {
            Ok(0) => break,
            Ok(read) => filled += read,
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
            Err(_) => return None,
        }
    }
    Some(buffer[..filled].contains(&0))
}
