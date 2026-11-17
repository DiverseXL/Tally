//! Redaction of known secrets from any text that leaves the process.

use std::fmt;

use secrecy::{ExposeSecret, SecretString};

/// Replacement text printed in place of a secret.
pub const REDACTED: &str = "[REDACTED]";

/// Secrets shorter than this are ignored, so that short fragments do not
/// destroy every message that happens to contain them.
pub const MIN_SECRET_LEN: usize = 8;

/// Holds the secrets known to the current run and replaces them in text.
///
/// Every string printed by a tool that handles wallet connection strings must be
/// passed through [`Scrubber::scrub`] first.
pub struct Scrubber {
    secrets: Vec<SecretString>,
}

impl Scrubber {
    /// Creates an empty scrubber.
    #[must_use]
    pub fn new() -> Self {
        Self {
            secrets: Vec::new(),
        }
    }

    /// Adds a secret to be redacted.
    ///
    /// Secrets shorter than [`MIN_SECRET_LEN`] characters are ignored.
    pub fn add(&mut self, secret: SecretString) {
        if secret.expose_secret().chars().count() >= MIN_SECRET_LEN {
            self.secrets.push(secret);
        }
    }

    /// Returns true if any known secret occurs in `text`.
    #[must_use]
    pub fn contains_secret(&self, text: &str) -> bool {
        self.secrets
            .iter()
            .any(|secret| text.contains(secret.expose_secret()))
    }

    /// Returns `text` with every occurrence of every known secret replaced.
    #[must_use]
    pub fn scrub(&self, text: &str) -> String {
        let mut scrubbed = text.to_owned();
        for secret in &self.secrets {
            scrubbed = scrubbed.replace(secret.expose_secret(), REDACTED);
        }
        scrubbed
    }

    /// Returns how many secrets are held.
    #[must_use]
    pub fn len(&self) -> usize {
        self.secrets.len()
    }

    /// Returns true when no secret is held.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.secrets.is_empty()
    }
}

impl Default for Scrubber {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Debug for Scrubber {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Scrubber")
            .field("secret_count", &self.secrets.len())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::{MIN_SECRET_LEN, REDACTED, Scrubber};
    use secrecy::SecretString;

    const SENTINEL: &str = "sentinel-secret-value-0123456789";

    #[test]
    fn scrub_removes_sentinel_and_detects_it() {
        let mut scrubber = Scrubber::new();
        scrubber.add(SecretString::from(SENTINEL));

        assert!(scrubber.contains_secret(&format!("before {SENTINEL} after")));

        let scrubbed = scrubber.scrub(&format!("before {SENTINEL} after"));
        assert_eq!(scrubbed, format!("before {REDACTED} after"));
        assert!(!scrubber.contains_secret(&scrubbed));
    }

    #[test]
    fn scrub_redacts_every_occurrence() {
        let mut scrubber = Scrubber::new();
        scrubber.add(SecretString::from(SENTINEL));

        let scrubbed = scrubber.scrub(&format!("{SENTINEL}/{SENTINEL}"));
        assert_eq!(scrubbed, format!("{REDACTED}/{REDACTED}"));
    }

    #[test]
    fn short_secrets_are_ignored() {
        let short = "abc1234";
        assert!(short.len() < MIN_SECRET_LEN);

        let mut scrubber = Scrubber::new();
        scrubber.add(SecretString::from(short));

        assert!(scrubber.is_empty());
        assert!(!scrubber.contains_secret(short));
        assert_eq!(scrubber.scrub(short), short);
    }

    #[test]
    fn debug_does_not_reveal_secrets() {
        let mut scrubber = Scrubber::new();
        scrubber.add(SecretString::from(SENTINEL));

        let debug = format!("{scrubber:?}");
        assert!(!debug.contains(SENTINEL));
        assert!(debug.contains("secret_count"));
    }
}
