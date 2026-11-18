//! The on-disk run guard.
//!
//! Before a payment, the runner creates `<guard dir>/<payment hash>.state`
//! atomically and writes `PAYING`. It then updates the file to `PAID`, `FAILED`
//! or `AMBIGUOUS`. A later run refuses to pay while any state file is `PAYING`
//! or `AMBIGUOUS`, because that payment has an unknown outcome. The files hold
//! a state name and a timestamp only: no invoice, preimage or connection
//! string.

use std::fmt;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use tally_core::time::now_rfc3339;

/// Suffix of a run-guard state file.
pub const STATE_SUFFIX: &str = ".state";

/// The state of one payment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GuardState {
    /// A payment was started and has not reported back.
    Paying,
    /// The wallet reported the payment as paid.
    Paid,
    /// The wallet reported a failure.
    Failed,
    /// The wallet did not say whether the payment happened.
    Ambiguous,
}

impl GuardState {
    /// The value stored in the file.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Paying => "PAYING",
            Self::Paid => "PAID",
            Self::Failed => "FAILED",
            Self::Ambiguous => "AMBIGUOUS",
        }
    }

    /// Parses a stored value.
    #[must_use]
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "PAYING" => Some(Self::Paying),
            "PAID" => Some(Self::Paid),
            "FAILED" => Some(Self::Failed),
            "AMBIGUOUS" => Some(Self::Ambiguous),
            _ => None,
        }
    }

    /// Whether this state blocks a new payment.
    #[must_use]
    pub const fn blocks_payment(self) -> bool {
        matches!(self, Self::Paying | Self::Ambiguous)
    }
}

/// Why the guard refused an operation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GuardError {
    /// A state file for this payment hash already exists.
    AlreadyExists,
    /// A state file or the directory could not be read or written.
    Io,
    /// The payment hash is not a safe file name.
    BadPaymentHash,
}

impl fmt::Display for GuardError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::AlreadyExists => f.write_str("guard file already exists"),
            Self::Io => f.write_str("guard file io failure"),
            Self::BadPaymentHash => f.write_str("payment hash is not usable as a file name"),
        }
    }
}

impl std::error::Error for GuardError {}

/// One state file that blocks payments.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlockingFile {
    /// Path of the file.
    pub path: PathBuf,
    /// The state it holds.
    pub state: GuardState,
}

/// Lists the state files that block a new payment, sorted by path.
///
/// A file that cannot be read or parsed is ignored: only a stated `PAYING` or
/// `AMBIGUOUS` blocks, so a stray file cannot wedge the tool forever.
///
/// # Errors
///
/// [`GuardError::Io`] when the directory cannot be read and it exists.
pub fn blocking_files(dir: &Path) -> Result<Vec<BlockingFile>, GuardError> {
    if !dir.exists() {
        return Ok(Vec::new());
    }

    let entries = fs::read_dir(dir).map_err(|_| GuardError::Io)?;
    let mut found = Vec::new();

    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|value| value.to_str()) != Some("state") {
            continue;
        }

        let Ok(text) = fs::read_to_string(&path) else {
            continue;
        };
        let Ok(value) = serde_json::from_str::<serde_json::Value>(&text) else {
            continue;
        };
        let Some(state) = value.get("state").and_then(|value| value.as_str()) else {
            continue;
        };
        if let Some(state) = GuardState::parse(state)
            && state.blocks_payment()
        {
            found.push(BlockingFile { path, state });
        }
    }

    found.sort_by(|left, right| left.path.cmp(&right.path));
    Ok(found)
}

/// Deletes the state files that block a payment.
///
/// # Errors
///
/// [`GuardError::Io`] when a file cannot be removed.
pub fn clear_blocking(dir: &Path) -> Result<usize, GuardError> {
    let blocking = blocking_files(dir)?;
    let mut removed = 0;

    for file in blocking {
        fs::remove_file(&file.path).map_err(|_| GuardError::Io)?;
        removed += 1;
    }

    Ok(removed)
}

/// Creates the `PAYING` state file for one payment hash, atomically.
///
/// # Errors
///
/// [`GuardError::AlreadyExists`] when the file exists,
/// [`GuardError::BadPaymentHash`] for a hash that is not lowercase hex, and
/// [`GuardError::Io`] on any filesystem failure.
pub fn begin(dir: &Path, payment_hash_hex: &str) -> Result<PathBuf, GuardError> {
    if payment_hash_hex.is_empty()
        || payment_hash_hex.len() > 128
        || !payment_hash_hex
            .chars()
            .all(|c| c.is_ascii_hexdigit() || c == '_')
    {
        return Err(GuardError::BadPaymentHash);
    }

    fs::create_dir_all(dir).map_err(|_| GuardError::Io)?;
    let path = dir.join(format!("{}{STATE_SUFFIX}", payment_hash_hex.to_lowercase()));

    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
        .map_err(|error| match error.kind() {
            std::io::ErrorKind::AlreadyExists => GuardError::AlreadyExists,
            _ => GuardError::Io,
        })?;

    let body = format!(
        "{{\"state\":\"{}\",\"timestamp\":\"{}\"}}\n",
        GuardState::Paying.as_str(),
        now_rfc3339()
    );
    file.write_all(body.as_bytes())
        .map_err(|_| GuardError::Io)?;

    Ok(path)
}

/// Replaces the state of a payment file.
///
/// # Errors
///
/// [`GuardError::Io`] when the file cannot be written.
pub fn finish(path: &Path, state: GuardState) -> Result<(), GuardError> {
    let body = format!(
        "{{\"state\":\"{}\",\"timestamp\":\"{}\"}}\n",
        state.as_str(),
        now_rfc3339()
    );

    let mut file = OpenOptions::new()
        .write(true)
        .truncate(true)
        .open(path)
        .map_err(|_| GuardError::Io)?;
    file.write_all(body.as_bytes())
        .map_err(|_| GuardError::Io)?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::{GuardError, GuardState, begin, blocking_files, clear_blocking, finish};

    fn temp_dir(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("tally-guard-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        dir
    }

    const HASH: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

    #[test]
    fn paying_blocks_and_paid_does_not() {
        let dir = temp_dir("states");
        let path = begin(&dir, HASH).unwrap();

        let blocking = blocking_files(&dir).unwrap();
        assert_eq!(blocking.len(), 1);
        assert_eq!(blocking[0].state, GuardState::Paying);

        finish(&path, GuardState::Paid).unwrap();
        assert!(blocking_files(&dir).unwrap().is_empty());

        finish(&path, GuardState::Ambiguous).unwrap();
        assert_eq!(blocking_files(&dir).unwrap().len(), 1);

        finish(&path, GuardState::Failed).unwrap();
        assert!(blocking_files(&dir).unwrap().is_empty());

        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_second_begin_for_the_same_hash_is_refused() {
        let dir = temp_dir("duplicate");
        let path = begin(&dir, HASH).unwrap();

        assert_eq!(begin(&dir, HASH).unwrap_err(), GuardError::AlreadyExists);
        finish(&path, GuardState::Paid).unwrap();
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn clearing_removes_only_blocking_files() {
        let dir = temp_dir("clear");
        let paying = begin(&dir, HASH).unwrap();
        let ambiguous = begin(&dir, &"ab".repeat(32)).unwrap();
        let paid = begin(&dir, &"cd".repeat(32)).unwrap();
        finish(&ambiguous, GuardState::Ambiguous).unwrap();
        finish(&paid, GuardState::Paid).unwrap();

        assert_eq!(clear_blocking(&dir).unwrap(), 2);
        assert!(blocking_files(&dir).unwrap().is_empty());
        assert!(paid.exists());
        assert!(!paying.exists());

        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn unsafe_payment_hashes_are_refused() {
        let dir = temp_dir("unsafe");
        assert_eq!(
            begin(&dir, "../escape").unwrap_err(),
            GuardError::BadPaymentHash
        );
        assert_eq!(begin(&dir, "").unwrap_err(), GuardError::BadPaymentHash);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn state_files_hold_no_secret_and_no_invoice_text() {
        let dir = temp_dir("content");
        let path = begin(&dir, HASH).unwrap();
        let text = fs::read_to_string(&path).unwrap();

        assert!(text.contains("PAYING"));
        assert!(text.contains("timestamp"));
        assert!(!text.contains("lnbc"));
        assert!(!text.contains("nostr+walletconnect"));
        assert!(!text.contains("secret="));

        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn every_state_round_trips() {
        for state in [
            GuardState::Paying,
            GuardState::Paid,
            GuardState::Failed,
            GuardState::Ambiguous,
        ] {
            assert_eq!(GuardState::parse(state.as_str()), Some(state));
        }
        assert_eq!(GuardState::parse("OTHER"), None);
    }
}
