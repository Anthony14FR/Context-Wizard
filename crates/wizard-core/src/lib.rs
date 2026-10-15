//! Domain types, configuration and errors shared by every Context Wizard crate.

pub mod atomic;
pub mod date;
pub mod decision;

/// Version of the Context Wizard binary, identical to the plugin version.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

#[cfg(test)]
mod tests {
    use super::VERSION;

    #[test]
    fn version_is_semver() {
        let core = VERSION.split(['-', '+']).next().unwrap_or_default();
        let parts: Vec<&str> = core.split('.').collect();
        assert_eq!(parts.len(), 3);
        assert!(parts.iter().all(|part| part.parse::<u64>().is_ok()));
    }
}
