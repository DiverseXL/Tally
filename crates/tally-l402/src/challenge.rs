//! The two L402 headers: the `WWW-Authenticate` challenge and the
//! `Authorization` credentials.
//!
//! Parsing is strict and fails closed. No error echoes the input.

use std::fmt;

use base64::Engine as _;
use base64::prelude::BASE64_STANDARD;

/// The scheme name. Matching is case-insensitive.
pub const SCHEME: &str = "L402";
/// Largest header value accepted, in bytes.
pub const MAX_HEADER_BYTES: usize = 8192;
/// Largest decoded macaroon accepted, in bytes.
pub const MAX_MACAROON_BYTES: usize = 4096;
/// Number of hex characters in a 32-byte preimage.
pub const PREIMAGE_HEX_LEN: usize = 64;

const CHALLENGE_PARAMETER: &str = "macaroon";
const INVOICE_PARAMETER: &str = "invoice";

/// Why a header was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChallengeErrorKind {
    /// The header value is longer than [`MAX_HEADER_BYTES`].
    TooLarge,
    /// The value contains a control character.
    ControlCharacter,
    /// The scheme is not `L402`.
    SchemeMismatch,
    /// A required parameter is absent.
    MissingParameter,
    /// A parameter appears twice.
    DuplicateParameter,
    /// A parameter value is not quoted.
    UnquotedParameter,
    /// A parameter name is not part of L402.
    UnknownParameter,
    /// Something other than whitespace follows the header.
    TrailingGarbage,
    /// The macaroon is not valid standard base64 within the size bound.
    BadMacaroon,
    /// The part after the colon is not 64 hex characters.
    BadPreimage,
    /// The credential token does not contain exactly one colon.
    ColonCount,
    /// The credential token is empty.
    EmptyToken,
}

impl ChallengeErrorKind {
    /// A stable short name, for logs and reports.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::TooLarge => "header_too_large",
            Self::ControlCharacter => "header_control_character",
            Self::SchemeMismatch => "header_scheme_mismatch",
            Self::MissingParameter => "header_missing_parameter",
            Self::DuplicateParameter => "header_duplicate_parameter",
            Self::UnquotedParameter => "header_unquoted_parameter",
            Self::UnknownParameter => "header_unknown_parameter",
            Self::TrailingGarbage => "header_trailing_garbage",
            Self::BadMacaroon => "macaroon_bad_base64",
            Self::BadPreimage => "preimage_bad_hex",
            Self::ColonCount => "credential_colon_count",
            Self::EmptyToken => "credential_empty_token",
        }
    }
}

/// A sanitized header error. It never holds a copy of the input.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChallengeError {
    kind: ChallengeErrorKind,
}

impl ChallengeError {
    const fn new(kind: ChallengeErrorKind) -> Self {
        Self { kind }
    }

    /// What went wrong, as a kind.
    #[must_use]
    pub const fn kind(&self) -> ChallengeErrorKind {
        self.kind
    }
}

impl fmt::Display for ChallengeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.kind.as_str())
    }
}

impl std::error::Error for ChallengeError {}

/// A parsed challenge: the macaroon to present and the invoice to pay.
///
/// The macaroon and the invoice are sensitive: both `Debug` and `Display`
/// redact them.
#[derive(Clone, PartialEq, Eq)]
pub struct Challenge {
    http_status: u16,
    scheme: String,
    macaroon_b64: String,
    invoice: String,
}

impl Challenge {
    /// Builds a challenge from its parts, for example from a parsed header.
    ///
    /// `scheme` is kept exactly as it arrived on the wire; matching it is
    /// case-insensitive, so it is not normalised.
    #[must_use]
    pub fn new(http_status: u16, scheme: String, macaroon_b64: String, invoice: String) -> Self {
        Self {
            http_status,
            scheme,
            macaroon_b64,
            invoice,
        }
    }

    /// The status the challenge arrived with.
    #[must_use]
    pub const fn http_status(&self) -> u16 {
        self.http_status
    }

    /// The authentication scheme exactly as parsed from the wire.
    #[must_use]
    pub fn scheme(&self) -> &str {
        &self.scheme
    }

    /// The base64 macaroon, sensitive.
    #[must_use]
    pub fn macaroon_b64(&self) -> &str {
        &self.macaroon_b64
    }

    /// The BOLT11 invoice, sensitive.
    #[must_use]
    pub fn invoice(&self) -> &str {
        &self.invoice
    }
}

impl fmt::Debug for Challenge {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Challenge")
            .field("http_status", &self.http_status)
            .field("scheme", &self.scheme)
            .field("macaroon", &"[REDACTED]")
            .field("invoice", &"[REDACTED]")
            .finish()
    }
}

impl fmt::Display for Challenge {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} challenge (status {}, credentials redacted)",
            self.scheme, self.http_status
        )
    }
}

/// Parsed `Authorization` credentials.
///
/// The macaroon and the preimage are sensitive: both `Debug` and `Display`
/// redact them.
#[derive(Clone, PartialEq, Eq)]
pub struct Credentials {
    macaroon_b64: String,
    preimage_hex: String,
}

impl Credentials {
    /// Builds credentials from validated parts.
    #[must_use]
    pub fn new(macaroon_b64: String, preimage_hex: String) -> Self {
        Self {
            macaroon_b64,
            preimage_hex,
        }
    }

    /// The base64 macaroon, sensitive.
    #[must_use]
    pub fn macaroon_b64(&self) -> &str {
        &self.macaroon_b64
    }

    /// The hex preimage, sensitive.
    #[must_use]
    pub fn preimage_hex(&self) -> &str {
        &self.preimage_hex
    }
}

impl fmt::Debug for Credentials {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Credentials")
            .field("macaroon", &"[REDACTED]")
            .field("preimage", &"[REDACTED]")
            .finish()
    }
}

impl fmt::Display for Credentials {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("L402 credentials (redacted)")
    }
}

/// Formats a `WWW-Authenticate` value.
///
/// # Errors
///
/// [`ChallengeErrorKind::ControlCharacter`], [`ChallengeErrorKind::BadMacaroon`]
/// or [`ChallengeErrorKind::MissingParameter`] for a part that cannot be
/// presented safely.
pub fn format_www_authenticate(
    macaroon_b64: &str,
    invoice: &str,
) -> Result<String, ChallengeError> {
    validate_common(macaroon_b64)?;
    validate_common(invoice)?;

    // The specification's challenge rules do not require the macaroon to be
    // valid base64 here, and its own example is not canonically padded, so the
    // challenge parser only requires both parts to be present and safe.
    if macaroon_b64.is_empty() || invoice.is_empty() {
        return Err(ChallengeError::new(ChallengeErrorKind::MissingParameter));
    }

    if macaroon_b64.len() > MAX_MACAROON_BYTES * 2 {
        return Err(ChallengeError::new(ChallengeErrorKind::BadMacaroon));
    }

    Ok(format!(
        "{SCHEME} {CHALLENGE_PARAMETER}=\"{macaroon_b64}\", {INVOICE_PARAMETER}=\"{invoice}\""
    ))
}

/// Parses a `WWW-Authenticate` value.
///
/// The scheme is case-insensitive. Both `macaroon=` and `invoice=` must be
/// present exactly once and quoted.
///
/// # Errors
///
/// A [`ChallengeError`] naming the rule that failed.
pub fn parse_www_authenticate(value: &str) -> Result<Challenge, ChallengeError> {
    validate_common(value)?;

    let (scheme, rest) = strip_scheme(value)?;
    let mut macaroon = None;
    let mut invoice = None;
    let mut rest = rest;

    loop {
        rest = rest.trim_start_matches([' ', '\t', ',']);
        if rest.is_empty() {
            break;
        }

        let (name, after_name) = match rest.split_once('=') {
            Some((name, after)) => (name.trim_end_matches([' ', '\t']), after),
            None => return Err(ChallengeError::new(ChallengeErrorKind::UnquotedParameter)),
        };

        if name.is_empty() {
            return Err(ChallengeError::new(ChallengeErrorKind::UnquotedParameter));
        }

        let after_name = after_name.trim_start_matches([' ', '\t']);
        let Some(quoted) = after_name.strip_prefix('"') else {
            return Err(ChallengeError::new(ChallengeErrorKind::UnquotedParameter));
        };
        let Some((inner, remainder)) = quoted.split_once('"') else {
            return Err(ChallengeError::new(ChallengeErrorKind::UnquotedParameter));
        };

        let remainder = remainder.trim_start_matches([' ', '\t']);
        if !remainder.is_empty() && !remainder.starts_with(',') {
            return Err(ChallengeError::new(ChallengeErrorKind::TrailingGarbage));
        }

        match name {
            CHALLENGE_PARAMETER => {
                if macaroon.is_some() {
                    return Err(ChallengeError::new(ChallengeErrorKind::DuplicateParameter));
                }
                macaroon = Some(inner.to_owned());
            }
            INVOICE_PARAMETER => {
                if invoice.is_some() {
                    return Err(ChallengeError::new(ChallengeErrorKind::DuplicateParameter));
                }
                invoice = Some(inner.to_owned());
            }
            _ => return Err(ChallengeError::new(ChallengeErrorKind::UnknownParameter)),
        }

        rest = remainder;
    }

    let (Some(macaroon), Some(invoice)) = (macaroon, invoice) else {
        return Err(ChallengeError::new(ChallengeErrorKind::MissingParameter));
    };

    if macaroon.is_empty() || invoice.is_empty() {
        return Err(ChallengeError::new(ChallengeErrorKind::MissingParameter));
    }

    Ok(Challenge::new(402, scheme.to_owned(), macaroon, invoice))
}

/// Formats an `Authorization` value for one macaroon and one preimage.
///
/// # Errors
///
/// [`ChallengeErrorKind::ControlCharacter`], [`ChallengeErrorKind::BadMacaroon`]
/// or [`ChallengeErrorKind::BadPreimage`] for parts that cannot be presented.
pub fn format_authorization(
    macaroon_b64: &str,
    preimage_hex: &str,
) -> Result<String, ChallengeError> {
    validate_common(macaroon_b64)?;
    validate_common(preimage_hex)?;
    validate_preimage(preimage_hex)?;
    validate_macaroon(macaroon_b64)?;

    Ok(format!("{SCHEME} {macaroon_b64}:{preimage_hex}"))
}

/// Parses an `Authorization` value.
///
/// Exactly one colon separates the base64 macaroon from the hex preimage, the
/// macaroon must be valid standard base64 within [`MAX_MACAROON_BYTES`], and
/// the preimage must be exactly 64 hex characters.
///
/// # Errors
///
/// A [`ChallengeError`] naming the rule that failed.
pub fn parse_authorization(value: &str) -> Result<Credentials, ChallengeError> {
    validate_common(value)?;

    let (_, rest) = strip_scheme(value)?;
    let rest = rest.trim();

    if rest.is_empty() {
        return Err(ChallengeError::new(ChallengeErrorKind::EmptyToken));
    }

    if rest.chars().any(|c| c.is_whitespace()) {
        return Err(ChallengeError::new(ChallengeErrorKind::TrailingGarbage));
    }

    let colons = rest.matches(':').count();
    if colons != 1 {
        return Err(ChallengeError::new(ChallengeErrorKind::ColonCount));
    }

    let Some((macaroon, preimage)) = rest.split_once(':') else {
        return Err(ChallengeError::new(ChallengeErrorKind::ColonCount));
    };

    // The preimage is checked first, so the specification's own credential
    // example is refused for its 12-byte preimage rather than for the padding
    // of its macaroon.
    validate_preimage(preimage)?;
    validate_macaroon(macaroon)?;

    Ok(Credentials::new(macaroon.to_owned(), preimage.to_owned()))
}

fn validate_common(value: &str) -> Result<(), ChallengeError> {
    if value.len() > MAX_HEADER_BYTES {
        return Err(ChallengeError::new(ChallengeErrorKind::TooLarge));
    }

    if value.chars().any(char::is_control) {
        return Err(ChallengeError::new(ChallengeErrorKind::ControlCharacter));
    }

    Ok(())
}

fn strip_scheme(value: &str) -> Result<(&str, &str), ChallengeError> {
    let trimmed = value.trim_start();
    let (scheme, rest) = match trimmed.split_once([' ', '\t']) {
        Some((scheme, rest)) => (scheme, rest),
        None => (trimmed, ""),
    };

    if !scheme.eq_ignore_ascii_case(SCHEME) {
        return Err(ChallengeError::new(ChallengeErrorKind::SchemeMismatch));
    }

    if rest.is_empty() {
        return Err(ChallengeError::new(ChallengeErrorKind::MissingParameter));
    }

    Ok((scheme, rest))
}

fn validate_macaroon(macaroon: &str) -> Result<(), ChallengeError> {
    if macaroon.is_empty() {
        return Err(ChallengeError::new(ChallengeErrorKind::MissingParameter));
    }

    let decoded = BASE64_STANDARD
        .decode(macaroon)
        .map_err(|_| ChallengeError::new(ChallengeErrorKind::BadMacaroon))?;

    if decoded.is_empty() || decoded.len() > MAX_MACAROON_BYTES {
        return Err(ChallengeError::new(ChallengeErrorKind::BadMacaroon));
    }

    Ok(())
}

fn validate_preimage(preimage: &str) -> Result<(), ChallengeError> {
    if preimage.len() != PREIMAGE_HEX_LEN || !preimage.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(ChallengeError::new(ChallengeErrorKind::BadPreimage));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        ChallengeErrorKind, MAX_HEADER_BYTES, format_authorization, format_www_authenticate,
        parse_authorization, parse_www_authenticate,
    };

    const SPEC_MACAROON: &str = "AGIAJEemVQUTEyNCR0exk7ek90Cg==";
    const SPEC_INVOICE: &str = "lnbc1500n1pw5kjhmpp";
    // The specification's macaroon literal above is 30 characters, so it is not
    // valid standard base64 (30 is not a multiple of 4). A challenge header is
    // parsed for syntax only, but a credential must carry a decodable macaroon,
    // so the round-trip tests use this valid token instead.
    const VALID_MACAROON: &str = "AgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgI=";
    const PREIMAGE: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

    #[test]
    fn parses_the_specification_challenge_example() {
        let header = format!("L402 macaroon=\"{SPEC_MACAROON}\", invoice=\"{SPEC_INVOICE}\"");
        let challenge = parse_www_authenticate(&header).unwrap();

        assert_eq!(challenge.macaroon_b64(), SPEC_MACAROON);
        assert_eq!(challenge.invoice(), SPEC_INVOICE);
        assert_eq!(challenge.http_status(), 402);
        assert_eq!(challenge.scheme(), "L402");
    }

    #[test]
    fn scheme_matching_is_case_insensitive() {
        let header = format!("l402 macaroon=\"{SPEC_MACAROON}\", invoice=\"{SPEC_INVOICE}\"");
        let challenge = parse_www_authenticate(&header).unwrap();
        assert_eq!(challenge.scheme(), "l402");

        let upper = format!("L402 MACAROON=\"{SPEC_MACAROON}\", INVOICE=\"{SPEC_INVOICE}\"");
        assert_eq!(
            parse_www_authenticate(&upper).unwrap_err().kind(),
            ChallengeErrorKind::UnknownParameter
        );
    }

    #[test]
    fn challenge_round_trips_through_its_format() {
        let header = format_www_authenticate(SPEC_MACAROON, SPEC_INVOICE).unwrap();
        assert_eq!(
            header,
            format!("L402 macaroon=\"{SPEC_MACAROON}\", invoice=\"{SPEC_INVOICE}\"")
        );
        assert!(parse_www_authenticate(&header).is_ok());
    }

    #[test]
    fn challenge_rejects_missing_duplicate_and_unquoted_parameters() {
        assert_eq!(
            parse_www_authenticate(&format!("L402 macaroon=\"{SPEC_MACAROON}\""))
                .unwrap_err()
                .kind(),
            ChallengeErrorKind::MissingParameter
        );
        assert_eq!(
            parse_www_authenticate(&format!(
                "L402 macaroon=\"{SPEC_MACAROON}\", macaroon=\"{SPEC_MACAROON}\", invoice=\"{SPEC_INVOICE}\""
            ))
            .unwrap_err()
            .kind(),
            ChallengeErrorKind::DuplicateParameter
        );
        assert_eq!(
            parse_www_authenticate(&format!(
                "L402 macaroon={SPEC_MACAROON}, invoice=\"{SPEC_INVOICE}\""
            ))
            .unwrap_err()
            .kind(),
            ChallengeErrorKind::UnquotedParameter
        );
        assert_eq!(
            parse_www_authenticate("Basic abc").unwrap_err().kind(),
            ChallengeErrorKind::SchemeMismatch
        );
    }

    #[test]
    fn challenge_rejects_control_characters_and_oversized_values() {
        assert_eq!(
            parse_www_authenticate("L402 macaroon=\"a\tb\", invoice=\"c\"")
                .unwrap_err()
                .kind(),
            ChallengeErrorKind::ControlCharacter
        );

        let oversized = "L".repeat(MAX_HEADER_BYTES + 1);
        assert_eq!(
            parse_www_authenticate(&oversized).unwrap_err().kind(),
            ChallengeErrorKind::TooLarge
        );
    }

    #[test]
    fn parses_the_specification_authorization_example() {
        // Syntactically an L402 header, but the preimage is 12 bytes, so it is
        // not a valid credential.
        let error =
            parse_authorization("L402 AGIAJEemVQUTEyNCR0exk7ek90Cg==:1234abcd1234abcd1234abcd")
                .unwrap_err();
        assert_eq!(error.kind(), ChallengeErrorKind::BadPreimage);
    }

    #[test]
    fn authorization_round_trips_through_its_format() {
        let header = format_authorization(VALID_MACAROON, PREIMAGE).unwrap();
        assert_eq!(header, format!("L402 {VALID_MACAROON}:{PREIMAGE}"));

        let credentials = parse_authorization(&header).unwrap();
        assert_eq!(credentials.macaroon_b64(), VALID_MACAROON);
        assert_eq!(credentials.preimage_hex(), PREIMAGE);
    }

    #[test]
    fn authorization_scheme_is_case_insensitive() {
        assert!(parse_authorization(&format!("l402 {VALID_MACAROON}:{PREIMAGE}")).is_ok());
    }

    #[test]
    fn authorization_rejects_wrong_colon_counts() {
        assert_eq!(
            parse_authorization(&format!("L402 {SPEC_MACAROON}"))
                .unwrap_err()
                .kind(),
            ChallengeErrorKind::ColonCount
        );
        assert_eq!(
            parse_authorization(&format!("L402 {SPEC_MACAROON}:{PREIMAGE}:{PREIMAGE}"))
                .unwrap_err()
                .kind(),
            ChallengeErrorKind::ColonCount
        );
        assert_eq!(
            parse_authorization("L402 ").unwrap_err().kind(),
            ChallengeErrorKind::MissingParameter
        );
        assert_eq!(
            parse_authorization("").unwrap_err().kind(),
            ChallengeErrorKind::SchemeMismatch
        );
    }

    #[test]
    fn authorization_rejects_empty_preimage_and_bad_macaroon() {
        assert_eq!(
            parse_authorization(&format!("L402 {VALID_MACAROON}:"))
                .unwrap_err()
                .kind(),
            ChallengeErrorKind::BadPreimage
        );
        assert_eq!(
            parse_authorization(&format!("L402 not-base64!:{PREIMAGE}"))
                .unwrap_err()
                .kind(),
            ChallengeErrorKind::BadMacaroon
        );
        assert_eq!(
            parse_authorization(&format!("L402 {SPEC_MACAROON}:{PREIMAGE}"))
                .unwrap_err()
                .kind(),
            ChallengeErrorKind::BadMacaroon
        );
    }

    #[test]
    fn authorization_rejects_control_characters_and_oversized_values() {
        assert_eq!(
            parse_authorization(&format!("L402 {VALID_MACAROON}:{PREIMAGE}\t"))
                .unwrap_err()
                .kind(),
            ChallengeErrorKind::ControlCharacter
        );

        let oversized = "L".repeat(MAX_HEADER_BYTES + 1);
        assert_eq!(
            parse_authorization(&oversized).unwrap_err().kind(),
            ChallengeErrorKind::TooLarge
        );
    }

    #[test]
    fn errors_never_echo_the_input() {
        let error = parse_authorization("L402 sentinel-macaroon:sentinel-preimage").unwrap_err();
        let text = format!("{error} {error:?}");
        assert!(!text.contains("sentinel"));
    }

    #[test]
    fn debug_and_display_redact_macaroon_and_invoice() {
        let challenge = parse_www_authenticate(&format!(
            "L402 macaroon=\"{SPEC_MACAROON}\", invoice=\"{SPEC_INVOICE}\""
        ))
        .unwrap();
        let text = format!("{challenge} {challenge:?}");
        assert!(!text.contains(SPEC_MACAROON));
        assert!(!text.contains(SPEC_INVOICE));

        let credentials =
            parse_authorization(&format!("L402 {VALID_MACAROON}:{PREIMAGE}")).unwrap();
        let text = format!("{credentials} {credentials:?}");
        assert!(!text.contains(VALID_MACAROON));
        assert!(!text.contains(PREIMAGE));
    }
}
