//! The five test statuses.

use std::fmt;

use serde::{Deserialize, Serialize};

/// Outcome of one test.
///
/// Serialized with the exact uppercase names used in reports; [`fmt::Display`]
/// prints the human label, which differs only for `NOT TESTED`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Status {
    /// The expected behaviour was observed.
    #[serde(rename = "PASS")]
    Pass,
    /// The test ran but the expected behaviour was not observed.
    #[serde(rename = "FAIL")]
    Fail,
    /// The wallet does not offer what the test needs; no request was sent.
    #[serde(rename = "UNSUPPORTED")]
    Unsupported,
    /// The test was not selected by the profile.
    #[serde(rename = "NOT_TESTED")]
    NotTested,
    /// The test could not run because something outside the wallet blocked it.
    #[serde(rename = "BLOCKED")]
    Blocked,
}

impl Status {
    /// The machine-readable name, identical to the serialized form.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Pass => "PASS",
            Self::Fail => "FAIL",
            Self::Unsupported => "UNSUPPORTED",
            Self::NotTested => "NOT_TESTED",
            Self::Blocked => "BLOCKED",
        }
    }

    /// Returns true for [`Status::Pass`].
    #[must_use]
    pub const fn is_pass(self) -> bool {
        matches!(self, Self::Pass)
    }
}

impl fmt::Display for Status {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let label = match self {
            Self::Pass => "PASS",
            Self::Fail => "FAIL",
            Self::Unsupported => "UNSUPPORTED",
            Self::NotTested => "NOT TESTED",
            Self::Blocked => "BLOCKED",
        };
        f.write_str(label)
    }
}

#[cfg(test)]
mod tests {
    use super::Status;

    #[test]
    fn serializes_with_exact_names() {
        let cases = [
            (Status::Pass, "\"PASS\""),
            (Status::Fail, "\"FAIL\""),
            (Status::Unsupported, "\"UNSUPPORTED\""),
            (Status::NotTested, "\"NOT_TESTED\""),
            (Status::Blocked, "\"BLOCKED\""),
        ];

        for (status, expected) in cases {
            assert_eq!(serde_json::to_string(&status).unwrap(), expected);
            assert_eq!(status.as_str(), status.to_string().replace(' ', "_"));
        }
    }

    #[test]
    fn display_uses_space_for_not_tested() {
        assert_eq!(Status::NotTested.to_string(), "NOT TESTED");
        assert_eq!(Status::Pass.to_string(), "PASS");
    }
}
