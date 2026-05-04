//! Runtime configuration loaded from the environment.

use std::sync::OnceLock;

use crate::error::AutoarcError;

/// Cached, parsed list of candidate passwords from `AUTOARC_PASSWORDS`.
static PASSWORD_LIST: OnceLock<Vec<String>> = OnceLock::new();

/// Returns the comma-separated password list from `AUTOARC_PASSWORDS`.
///
/// The variable is read once and memoized for the rest of the process lifetime.
/// Returns [`AutoarcError::MissingPasswords`] when the variable is unset or empty.
pub fn get_password_list() -> Result<&'static Vec<String>, AutoarcError> {
    // Try to populate the cache the first time we're called.
    if PASSWORD_LIST.get().is_none() {
        let raw = std::env::var("AUTOARC_PASSWORDS").map_err(|_| AutoarcError::MissingPasswords)?;
        let parsed: Vec<String> = raw
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
        if parsed.is_empty() {
            return Err(AutoarcError::MissingPasswords);
        }
        // Ignore the result: a concurrent caller may have set it first, which is fine.
        let _ = PASSWORD_LIST.set(parsed);
    }
    Ok(PASSWORD_LIST.get().expect("password list just populated"))
}
