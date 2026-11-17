//! Reading a wallet connection string from a single `KEY=VALUE` line.
//!
//! The value is a spend-capable secret: it is returned as [`SecretString`] and
//! never appears in an error.

use std::error::Error;
use std::fmt;
use std::fs;
use std::io;
use std::path::Path;

use secrecy::SecretString;
use zeroize::Zeroize;

/// Why a secret line could not be read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineErrorKind {
    /// The file could not be read; carries the standard library error kind only.
    Io(io::ErrorKind),
    /// The file has no non-empty line.
    Empty,
    /// The file has more than one non-empty line.
    MultipleLines,
    /// The single line does not start with the requested key.
    WrongKey,
    /// The line has the requested key but no value.
    EmptyValue,
}

impl fmt::Display for LineErrorKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(kind) => write!(f, "io error ({kind})"),
            Self::Empty => f.write_str("file is empty"),
            Self::MultipleLines => f.write_str("file has more than one line"),
            Self::WrongKey => f.write_str("line does not start with the requested key"),
            Self::EmptyValue => f.write_str("value is empty"),
        }
    }
}

/// Error returned by [`read_secret_line`].
///
/// It carries the key name and an error kind only: never the path, the file
/// content or the value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SecretLineError {
    kind: LineErrorKind,
}

impl SecretLineError {
    /// The kind of failure.
    #[must_use]
    pub fn kind(&self) -> LineErrorKind {
        self.kind
    }
}

impl fmt::Display for SecretLineError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "secret line error: {}", self.kind)
    }
}

impl Error for SecretLineError {}

/// Reads a file holding exactly one non-empty `key=value` line.
///
/// The value is returned as [`SecretString`]; the intermediate buffer is
/// zeroized before returning.
pub fn read_secret_line(path: &Path, key: &str) -> Result<SecretString, SecretLineError> {
    let mut content = fs::read_to_string(path).map_err(|error| SecretLineError {
        kind: LineErrorKind::Io(error.kind()),
    })?;

    let parsed = parse_single_line(&content, key);
    content.zeroize();

    match parsed {
        Ok(value) => Ok(SecretString::from(value)),
        Err(kind) => Err(SecretLineError { kind }),
    }
}

/// Splits `content` into the value of its single `key=` line.
fn parse_single_line(content: &str, key: &str) -> Result<String, LineErrorKind> {
    let mut lines = content.lines().filter(|line| !line.trim().is_empty());

    let line = lines.next().ok_or(LineErrorKind::Empty)?;
    if lines.next().is_some() {
        return Err(LineErrorKind::MultipleLines);
    }

    let value = line
        .strip_prefix(key)
        .and_then(|rest| rest.strip_prefix('='))
        .ok_or(LineErrorKind::WrongKey)?;

    if value.is_empty() {
        return Err(LineErrorKind::EmptyValue);
    }

    Ok(value.to_owned())
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;

    use secrecy::ExposeSecret;

    use super::{LineErrorKind, read_secret_line};

    const SENTINEL: &str = "sentinel-secret-value-0123456789";

    fn temp_path(name: &str) -> PathBuf {
        let mut path = std::env::temp_dir();
        path.push(format!("tally-connection-test-{name}.env"));
        path
    }

    #[test]
    fn reads_single_line() {
        let path = temp_path("ok");
        fs::write(&path, format!("TALLY_TEST_KEY={SENTINEL}\n")).unwrap();

        let secret = read_secret_line(&path, "TALLY_TEST_KEY").unwrap();
        assert_eq!(secret.expose_secret(), SENTINEL);

        fs::remove_file(&path).ok();
    }

    #[test]
    fn errors_do_not_contain_content_or_value() {
        let path = temp_path("wrong-key");
        fs::write(&path, format!("OTHER_KEY={SENTINEL}\n")).unwrap();

        let error = read_secret_line(&path, "TALLY_TEST_KEY").unwrap_err();
        assert_eq!(error.kind(), LineErrorKind::WrongKey);

        let text = format!("{error} {error:?}");
        assert!(!text.contains(SENTINEL));
        assert!(!text.contains("OTHER_KEY="));

        fs::remove_file(&path).ok();
    }

    #[test]
    fn rejects_empty_and_multiple_lines() {
        let empty = temp_path("empty");
        fs::write(&empty, "\n\n").unwrap();
        assert_eq!(
            read_secret_line(&empty, "TALLY_TEST_KEY")
                .unwrap_err()
                .kind(),
            LineErrorKind::Empty
        );

        let multiple = temp_path("multiple");
        fs::write(
            &multiple,
            format!("TALLY_TEST_KEY={SENTINEL}\nTALLY_TEST_KEY={SENTINEL}\n"),
        )
        .unwrap();
        assert_eq!(
            read_secret_line(&multiple, "TALLY_TEST_KEY")
                .unwrap_err()
                .kind(),
            LineErrorKind::MultipleLines
        );

        fs::remove_file(&empty).ok();
        fs::remove_file(&multiple).ok();
    }

    #[test]
    fn missing_file_reports_io_kind_only() {
        let path = temp_path("missing");
        let error = read_secret_line(&path, "TALLY_TEST_KEY").unwrap_err();
        assert!(matches!(error.kind(), LineErrorKind::Io(_)));
        assert!(!format!("{error}").contains("tally-connection-test"));
    }
}
