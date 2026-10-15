//! Prints how long a full and an incremental index refresh of the given directory take.

use std::time::Instant;

fn main() {
    let root = std::path::PathBuf::from(std::env::args().nth(1).unwrap_or_else(|| ".".to_owned()));
    for pass in ["first", "second"] {
        let start = Instant::now();
        let files = wizard_scan::scan(&root, &wizard_scan::ScanOptions::default()).files;
        let outcome = wizard_store::Index::open_project(&root)
            .and_then(|mut index| index.refresh(&root, &files));
        match outcome {
            Ok(changes) => println!(
                "{pass} refresh: {} files, {} added, {} unchanged in {:?}",
                files.len(),
                changes.added.len(),
                changes.unchanged,
                start.elapsed()
            ),
            Err(error) => println!("{pass} refresh failed: {error}"),
        }
    }
}
