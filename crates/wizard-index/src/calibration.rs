//! Token ratios calibrated for a project, used for every token budget of that project.

use std::path::Path;

use wizard_budget::{Estimator, Ratios};
use wizard_store::DATA_DIRECTORY;

/// File of the calibrated ratios, inside the `.wizard` directory.
pub const CALIBRATION_FILE: &str = "calibration.txt";

/// The estimator of `project`: its calibrated ratios when `wizard calibrate` measured them,
/// the built-in ones otherwise.
#[must_use]
pub fn estimator(project: &Path) -> Estimator {
    std::fs::read_to_string(project.join(DATA_DIRECTORY).join(CALIBRATION_FILE))
        .ok()
        .and_then(|text| Ratios::parse(&text))
        .map_or_else(Estimator::default, Estimator::with_ratios)
}

/// Stores `ratios` as the calibration of `project`.
///
/// # Errors
///
/// Returns an error when the file cannot be written.
pub fn save(project: &Path, ratios: &Ratios) -> std::io::Result<()> {
    wizard_core::atomic::write(
        &project.join(DATA_DIRECTORY).join(CALIBRATION_FILE),
        ratios.to_text().as_bytes(),
    )
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use wizard_budget::{Content, Estimator, Ratios};

    use super::{estimator, save};

    #[test]
    fn uses_the_calibration_when_present() {
        let project = tempfile::tempdir().unwrap();
        let text = "x".repeat(100);
        assert_eq!(
            estimator(project.path()).estimate(&text, Content::Code),
            Estimator::default().estimate(&text, Content::Code)
        );
        std::fs::create_dir_all(project.path().join(".wizard")).unwrap();
        save(
            project.path(),
            &Ratios {
                code: 10.0,
                prose: 10.0,
                data: 10.0,
            },
        )
        .unwrap();
        assert_eq!(estimator(project.path()).estimate(&text, Content::Code), 10);
    }
}
