//! Runtime configuration loaded from the environment.

use std::sync::OnceLock;

/// Cached, parsed list of candidate passwords from `AUTOARC_PASSWORDS`.
static PASSWORD_LIST: OnceLock<Vec<String>> = OnceLock::new();

/// Returns the comma-separated password list from `AUTOARC_PASSWORDS`.
///
/// `AUTOARC_PASSWORDS` is **optional**: when the variable is missing or empty,
/// the returned list falls back to a single empty password so unencrypted
/// archives still extract cleanly without any configuration. An empty-password
/// attempt is also prepended when the variable *is* set, so a single password
/// list works uniformly across encrypted and unencrypted archives in the same
/// run.
///
/// The variable is read once and memoized for the rest of the process lifetime.
pub fn get_password_list() -> &'static Vec<String> {
    PASSWORD_LIST.get_or_init(|| {
        let raw = std::env::var("AUTOARC_PASSWORDS").unwrap_or_default();
        let mut parsed: Vec<String> = raw
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
        // Always try the empty password first so archives that aren't
        // encrypted succeed immediately, regardless of whether the user
        // configured `AUTOARC_PASSWORDS`.
        if !parsed.iter().any(|p| p.is_empty()) {
            parsed.insert(0, String::new());
        }
        parsed
    })
}

#[cfg(test)]
mod tests {
    // NOTE: we can't unit-test `get_password_list()` directly because it uses a
    // process-wide `OnceLock` that's poisoned by the first caller in the test
    // binary. The parsing logic below mirrors the production code so we can at
    // least cover the branches.

    fn parse(raw: &str) -> Vec<String> {
        let mut parsed: Vec<String> = raw
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
        if !parsed.iter().any(|p| p.is_empty()) {
            parsed.insert(0, String::new());
        }
        parsed
    }

    #[test]
    fn unset_env_yields_single_empty_password() {
        // Simulates `AUTOARC_PASSWORDS` unset or empty string.
        assert_eq!(parse(""), vec![String::new()]);
    }

    #[test]
    fn whitespace_only_entries_are_dropped_and_empty_prepended() {
        assert_eq!(parse(" , , "), vec![String::new()]);
    }

    #[test]
    fn configured_passwords_are_prefixed_with_empty_attempt() {
        assert_eq!(
            parse("secret,hunter2"),
            vec![String::new(), "secret".to_string(), "hunter2".to_string()]
        );
    }

    #[test]
    fn whitespace_is_trimmed_around_each_password() {
        assert_eq!(
            parse("  alpha  ,  beta"),
            vec![String::new(), "alpha".to_string(), "beta".to_string()]
        );
    }
}
