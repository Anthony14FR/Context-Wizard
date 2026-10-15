//! Atomic file replacement.

use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;
use std::time::Duration;

const RENAME_ATTEMPTS: u32 = 20;
const RENAME_BACKOFF: Duration = Duration::from_millis(10);

static SEQUENCE: AtomicU64 = AtomicU64::new(0);

/// Replaces the content of `path` with `bytes`, so readers see either the old or the new file.
///
/// The parent directory is created when missing. Concurrent writers never share a temporary
/// file, and the last rename wins.
///
/// # Errors
///
/// Returns an error when the directory, the temporary file or the rename fails.
pub fn write(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let directory = path.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(directory)?;
    let temporary = temporary_path(directory, path);
    let result = write_new(&temporary, bytes).and_then(|()| rename(&temporary, path));
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

fn temporary_path(directory: &Path, path: &Path) -> PathBuf {
    let name = path
        .file_name()
        .map_or_else(|| "file".into(), |name| name.to_string_lossy());
    let sequence = SEQUENCE.fetch_add(1, Ordering::Relaxed);
    directory.join(format!(".{name}.{}.{sequence}.tmp", std::process::id()))
}

fn write_new(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let mut file = OpenOptions::new().write(true).create_new(true).open(path)?;
    file.write_all(bytes)?;
    file.sync_all()
}

fn rename(from: &Path, to: &Path) -> io::Result<()> {
    let mut attempt = 1;
    loop {
        match fs::rename(from, to) {
            Err(error)
                if error.kind() == io::ErrorKind::PermissionDenied && attempt < RENAME_ATTEMPTS =>
            {
                attempt += 1;
                thread::sleep(RENAME_BACKOFF);
            }
            result => return result,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::sync::Arc;
    use std::thread;

    use super::write;

    #[test]
    fn creates_and_replaces() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("nested").join("file.json");
        write(&path, b"one").unwrap();
        write(&path, b"two").unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"two");
        assert_eq!(fs::read_dir(path.parent().unwrap()).unwrap().count(), 1);
    }

    #[test]
    fn concurrent_writers_leave_one_complete_file() {
        let directory = tempfile::tempdir().unwrap();
        let path = Arc::new(directory.path().join("file.json"));
        let writers: Vec<_> = (0..8_u8)
            .map(|index| {
                let path = Arc::clone(&path);
                thread::spawn(move || write(&path, &[index; 4096]))
            })
            .collect();
        for writer in writers {
            writer.join().unwrap().unwrap();
        }
        let content = fs::read(path.as_ref()).unwrap();
        assert_eq!(content.len(), 4096);
        assert!(content.iter().all(|byte| *byte == content[0]));
        assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 1);
    }
}
