//! Files written by users or other tools must never make Context Wizard panic or corrupt them.

#![allow(clippy::unwrap_used)]

use std::fs;

use proptest::prelude::{any, proptest};
use wizard_core::{decision, secrets, settings};

proptest! {
    #[test]
    fn any_decision_file_loads_or_fails_cleanly(bytes in any::<Vec<u8>>()) {
        let project = tempfile::tempdir().unwrap();
        fs::create_dir_all(project.path().join(".claude")).unwrap();
        fs::write(project.path().join(decision::RELATIVE_PATH), &bytes).unwrap();
        let _ = decision::load(project.path());
    }

    #[test]
    fn settings_stay_valid_json_or_are_left_untouched(bytes in any::<Vec<u8>>(), enabled in any::<bool>()) {
        let project = tempfile::tempdir().unwrap();
        fs::create_dir_all(project.path().join(".claude")).unwrap();
        let path = project.path().join(settings::RELATIVE_PATH);
        fs::write(&path, &bytes).unwrap();
        match settings::set_plugin_enabled(project.path(), "context-wizard@context-wizard", enabled) {
            Ok(()) => {
                let written = fs::read(&path).unwrap();
                if written != bytes {
                    let value: serde_json::Value = serde_json::from_slice(&written).unwrap();
                    assert!(value.is_object());
                }
            }
            Err(_) => assert_eq!(fs::read(&path).unwrap(), bytes),
        }
    }

    #[test]
    fn redaction_never_panics_and_is_stable(text in any::<String>()) {
        let once = secrets::redact(&text).into_owned();
        assert_eq!(secrets::redact(&once), once.as_str());
        let _ = secrets::env_keys(&text);
    }
}
