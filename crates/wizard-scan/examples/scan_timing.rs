//! Prints how long a scan of the given directory takes.

use std::time::Instant;

fn main() {
    let root = std::env::args().nth(1).unwrap_or_else(|| ".".to_owned());
    let start = Instant::now();
    let report = wizard_scan::scan(root.as_ref(), &wizard_scan::ScanOptions::default());
    println!(
        "{} files ({} binary, {} too large) in {:?}",
        report.files.len(),
        report.binary,
        report.too_large,
        start.elapsed()
    );
}
