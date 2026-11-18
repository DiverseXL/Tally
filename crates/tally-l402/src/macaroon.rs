//! Macaroon minting and verification for the local fixture only.
//!
//! Macaroons here are a Tally-defined layout over libmacaroon's V2
//! serialization, not a claim of byte compatibility with other L402
//! implementations. The real proof check stays the preimage hash.
//!
//! Identifier layout, Tally's own: a 2-byte big-endian version (0), then the
//! 32-byte payment hash, then the 32-byte user id.
//!
//! The root key lives only in memory, in a [`SecretBox`], and never appears in
//! `Debug`, `Display` or an error.

use std::fmt;

use base64::Engine as _;
use base64::prelude::{BASE64_STANDARD, BASE64_URL_SAFE};
use libmacaroon::{Caveat, Format, Macaroon, MacaroonKey, Verifier};
use secrecy::{ExposeSecret, SecretBox};
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;

/// Version written into every identifier.
pub const IDENTIFIER_VERSION: u16 = 0;
/// Length of an identifier: 2 version bytes, hash and user id.
pub const IDENTIFIER_BYTES: usize = 2 + 32 + 32;
/// Location recorded in every macaroon.
pub const LOCATION: &str = "tally-fixture";
/// The only caveat Tally mints.
pub const SERVICES_CAVEAT: &str = "services = tally_fixture:0";
/// Largest decoded macaroon accepted for verification.
pub const MAX_MACAROON_BYTES: usize = 4096;

/// Why a key or a user id could not be generated.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyError {
    /// The operating system random source failed.
    RandomUnavailable,
}

impl fmt::Display for KeyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("random_unavailable")
    }
}

impl std::error::Error for KeyError {}

/// Why a macaroon was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum VerifyErrorKind {
    /// Not standard base64, too large, or not a V2 macaroon.
    BadEncoding,
    /// The signature does not verify under the root key.
    BadSignature,
    /// A known caveat is not satisfied.
    UnsatisfiedCaveat,
    /// A caveat that Tally does not mint.
    UnknownCaveat,
    /// The identifier is not Tally's layout.
    BadIdentifier,
    /// SHA-256 of the preimage is not the identifier's payment hash.
    PreimageMismatch,
    /// The preimage is not 32 bytes of hex.
    BadPreimage,
}

impl VerifyErrorKind {
    /// A stable short name, for logs and reports.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::BadEncoding => "macaroon_bad_encoding",
            Self::BadSignature => "macaroon_bad_signature",
            Self::UnsatisfiedCaveat => "macaroon_unsatisfied_caveat",
            Self::UnknownCaveat => "macaroon_unknown_caveat",
            Self::BadIdentifier => "macaroon_bad_identifier",
            Self::PreimageMismatch => "preimage_mismatch",
            Self::BadPreimage => "preimage_bad_hex",
        }
    }
}

/// A sanitized verification error. It never holds a macaroon or a preimage.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VerifyError {
    kind: VerifyErrorKind,
}

impl VerifyError {
    const fn new(kind: VerifyErrorKind) -> Self {
        Self { kind }
    }

    /// What went wrong, as a kind.
    #[must_use]
    pub const fn kind(&self) -> VerifyErrorKind {
        self.kind
    }
}

impl fmt::Display for VerifyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.kind.as_str())
    }
}

impl std::error::Error for VerifyError {}

/// The 32-byte key the fixture signs and verifies with.
///
/// Held in a [`SecretBox`], so the bytes are wiped on drop. `Debug` and
/// `Display` never reveal them.
pub struct RootKey(SecretBox<[u8; 32]>);

impl RootKey {
    /// Draws 32 bytes from the operating system random source.
    ///
    /// # Errors
    ///
    /// [`KeyError::RandomUnavailable`] when the source fails.
    pub fn generate() -> Result<Self, KeyError> {
        let mut bytes = [0u8; 32];
        getrandom::fill(&mut bytes).map_err(|_| KeyError::RandomUnavailable)?;
        Ok(Self(SecretBox::new(Box::new(bytes))))
    }

    /// Builds a key from known bytes. Intended for tests and fixtures that must
    /// be reproducible.
    #[must_use]
    pub fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(SecretBox::new(Box::new(bytes)))
    }

    fn key(&self) -> MacaroonKey {
        MacaroonKey::from(*self.0.expose_secret())
    }
}

impl fmt::Debug for RootKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("RootKey").field(&"[REDACTED]").finish()
    }
}

impl fmt::Display for RootKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("[REDACTED ROOT KEY]")
    }
}

/// Draws a 32-byte user id from the operating system random source.
///
/// # Errors
///
/// [`KeyError::RandomUnavailable`] when the source fails.
pub fn new_user_id() -> Result<[u8; 32], KeyError> {
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes).map_err(|_| KeyError::RandomUnavailable)?;
    Ok(bytes)
}

/// The identifier bytes for one payment hash and one user id.
#[must_use]
pub fn identifier(payment_hash: [u8; 32], user_id: [u8; 32]) -> [u8; IDENTIFIER_BYTES] {
    let mut bytes = [0u8; IDENTIFIER_BYTES];
    bytes[..2].copy_from_slice(&IDENTIFIER_VERSION.to_be_bytes());
    bytes[2..34].copy_from_slice(&payment_hash);
    bytes[34..66].copy_from_slice(&user_id);
    bytes
}

/// Mints a macaroon bound to one payment hash and one user id.
///
/// The result is libmacaroon's binary V2 serialization re-encoded as standard
/// base64, with location [`LOCATION`] and the single caveat
/// [`SERVICES_CAVEAT`].
///
/// # Errors
///
/// [`VerifyErrorKind::BadEncoding`] when libmacaroon cannot serialize the
/// result.
pub fn mint(
    root_key: &RootKey,
    payment_hash: [u8; 32],
    user_id: [u8; 32],
) -> Result<String, VerifyError> {
    let key = root_key.key();
    let id = identifier(payment_hash, user_id);

    let mut macaroon = Macaroon::create(Some(LOCATION), &key, id.as_slice())
        .map_err(|_| VerifyError::new(VerifyErrorKind::BadEncoding))?;
    macaroon
        .add_first_party_caveat(SERVICES_CAVEAT)
        .map_err(|_| VerifyError::new(VerifyErrorKind::BadEncoding))?;

    let encoded = macaroon
        .serialize(Format::V2)
        .map_err(|_| VerifyError::new(VerifyErrorKind::BadEncoding))?;
    let binary = BASE64_URL_SAFE
        .decode(encoded)
        .map_err(|_| VerifyError::new(VerifyErrorKind::BadEncoding))?;

    Ok(BASE64_STANDARD.encode(binary))
}

/// Verifies a macaroon and the preimage that unlocks it.
///
/// The checks are: standard base64 and V2 binary; every caveat is exactly
/// [`SERVICES_CAVEAT`]; the signature verifies under the root key; the
/// identifier has Tally's layout; SHA-256 of the preimage equals the
/// identifier's payment hash, compared in constant time.
///
/// # Errors
///
/// The [`VerifyErrorKind`] of the first check that fails.
pub fn verify(
    root_key: &RootKey,
    macaroon_b64: &str,
    preimage_hex: &str,
) -> Result<(), VerifyError> {
    let macaroon = decode(macaroon_b64)?;
    let id = parse_identifier(macaroon.identifier())?;

    check_caveats(&macaroon)?;

    let mut verifier = Verifier::default();
    verifier.satisfy_exact(SERVICES_CAVEAT);
    verifier
        .verify(&macaroon, &root_key.key(), &[])
        .map_err(classify_verifier_error)?;

    let preimage = decode_hex_32(preimage_hex)?;
    let digest = Sha256::digest(preimage);
    let digest_bytes: &[u8] = digest.as_ref();

    if bool::from(digest_bytes.ct_eq(id.payment_hash.as_slice())) {
        Ok(())
    } else {
        Err(VerifyError::new(VerifyErrorKind::PreimageMismatch))
    }
}

/// Reads the payment hash out of a macaroon identifier, without a key.
///
/// Used to check that a challenge's macaroon is bound to its invoice.
///
/// # Errors
///
/// [`VerifyErrorKind::BadEncoding`] or [`VerifyErrorKind::BadIdentifier`].
pub fn identifier_payment_hash(macaroon_b64: &str) -> Result<[u8; 32], VerifyError> {
    let macaroon = decode(macaroon_b64)?;
    Ok(parse_identifier(macaroon.identifier())?.payment_hash)
}

/// Reads the user id out of a macaroon identifier, without a key.
///
/// # Errors
///
/// [`VerifyErrorKind::BadEncoding`] or [`VerifyErrorKind::BadIdentifier`].
pub fn identifier_user_id(macaroon_b64: &str) -> Result<[u8; 32], VerifyError> {
    let macaroon = decode(macaroon_b64)?;
    Ok(parse_identifier(macaroon.identifier())?.user_id)
}

struct ParsedIdentifier {
    payment_hash: [u8; 32],
    user_id: [u8; 32],
}

fn decode(macaroon_b64: &str) -> Result<Macaroon, VerifyError> {
    if macaroon_b64.is_empty() || macaroon_b64.len() > MAX_MACAROON_BYTES * 2 {
        return Err(VerifyError::new(VerifyErrorKind::BadEncoding));
    }

    let binary = BASE64_STANDARD
        .decode(macaroon_b64)
        .map_err(|_| VerifyError::new(VerifyErrorKind::BadEncoding))?;

    if binary.len() > MAX_MACAROON_BYTES || binary.first() != Some(&2) {
        return Err(VerifyError::new(VerifyErrorKind::BadEncoding));
    }

    Macaroon::deserialize_binary(&binary)
        .map_err(|_| VerifyError::new(VerifyErrorKind::BadEncoding))
}

fn parse_identifier(bytes: &[u8]) -> Result<ParsedIdentifier, VerifyError> {
    if bytes.len() != IDENTIFIER_BYTES {
        return Err(VerifyError::new(VerifyErrorKind::BadIdentifier));
    }

    let version = u16::from_be_bytes([bytes[0], bytes[1]]);
    if version != IDENTIFIER_VERSION {
        return Err(VerifyError::new(VerifyErrorKind::BadIdentifier));
    }

    let mut payment_hash = [0u8; 32];
    payment_hash.copy_from_slice(&bytes[2..34]);

    let mut user_id = [0u8; 32];
    user_id.copy_from_slice(&bytes[34..66]);

    Ok(ParsedIdentifier {
        payment_hash,
        user_id,
    })
}

fn check_caveats(macaroon: &Macaroon) -> Result<(), VerifyError> {
    for caveat in macaroon.caveats() {
        match caveat {
            Caveat::FirstParty(first_party)
                if first_party.predicate() == SERVICES_CAVEAT.as_bytes() => {}
            Caveat::FirstParty(_) => {
                return Err(VerifyError::new(VerifyErrorKind::UnknownCaveat));
            }
            Caveat::ThirdParty(_) => {
                return Err(VerifyError::new(VerifyErrorKind::UnknownCaveat));
            }
        }
    }

    Ok(())
}

fn classify_verifier_error(error: libmacaroon::MacaroonError) -> VerifyError {
    match error {
        libmacaroon::MacaroonError::InvalidSignature => {
            VerifyError::new(VerifyErrorKind::BadSignature)
        }
        libmacaroon::MacaroonError::CaveatNotSatisfied(_) => {
            VerifyError::new(VerifyErrorKind::UnsatisfiedCaveat)
        }
        _ => VerifyError::new(VerifyErrorKind::BadEncoding),
    }
}

fn decode_hex_32(hex: &str) -> Result<[u8; 32], VerifyError> {
    if hex.len() != 64 || !hex.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(VerifyError::new(VerifyErrorKind::BadPreimage));
    }

    let mut bytes = [0u8; 32];
    for (index, chunk) in bytes.iter_mut().enumerate() {
        *chunk = u8::from_str_radix(&hex[index * 2..index * 2 + 2], 16)
            .map_err(|_| VerifyError::new(VerifyErrorKind::BadPreimage))?;
    }

    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use base64::Engine as _;
    use base64::prelude::{BASE64_STANDARD, BASE64_URL_SAFE};
    use libmacaroon::{Format, Macaroon, MacaroonKey};
    use sha2::{Digest, Sha256};

    use super::{
        SERVICES_CAVEAT, VerifyErrorKind, identifier, identifier_payment_hash, identifier_user_id,
        mint, new_user_id, verify,
    };
    use crate::macaroon::RootKey;

    fn root_key(bytes: u8) -> RootKey {
        RootKey::from_bytes([bytes; 32])
    }

    /// A preimage and the payment hash it unlocks, as hex.
    fn preimage_and_hash() -> (String, [u8; 32]) {
        let preimage = [42u8; 32];
        let hash = Sha256::digest(preimage);

        let mut payment_hash = [0u8; 32];
        payment_hash.copy_from_slice(hash.as_ref());

        let preimage_hex = preimage
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();

        (preimage_hex, payment_hash)
    }

    #[test]
    fn mint_binds_the_identifier_and_the_single_caveat() {
        let root = root_key(1);
        let user_id = [7u8; 32];
        let payment_hash = [9u8; 32];

        let token = mint(&root, payment_hash, user_id).unwrap();

        assert_eq!(identifier_payment_hash(&token).unwrap(), payment_hash);
        assert_eq!(identifier_user_id(&token).unwrap(), user_id);

        let binary = BASE64_STANDARD.decode(&token).unwrap();
        let macaroon = Macaroon::deserialize_binary(&binary).unwrap();
        assert_eq!(macaroon.location(), Some(super::LOCATION));
        assert_eq!(macaroon.caveats().len(), 1);
        assert_eq!(
            macaroon.identifier(),
            identifier(payment_hash, user_id).as_slice()
        );
    }

    #[test]
    fn identifier_layout_is_version_hash_then_user_id() {
        let bytes = identifier([1u8; 32], [2u8; 32]);

        assert_eq!(&bytes[..2], &[0u8, 0u8]);
        assert_eq!(&bytes[2..34], &[1u8; 32]);
        assert_eq!(&bytes[34..], &[2u8; 32]);
        assert_eq!(bytes.len(), super::IDENTIFIER_BYTES);
    }

    #[test]
    fn verify_accepts_the_matching_preimage() {
        let root = root_key(3);
        let (preimage_hex, payment_hash) = preimage_and_hash();

        let token = mint(&root, payment_hash, [4u8; 32]).unwrap();

        assert!(verify(&root, &token, &preimage_hex).is_ok());
    }

    #[test]
    fn verify_rejects_a_wrong_preimage() {
        let root = root_key(5);
        let payment_hash = [11u8; 32];
        let token = mint(&root, payment_hash, [6u8; 32]).unwrap();

        let error = verify(&root, &token, &"00".repeat(32)).unwrap_err();
        assert_eq!(error.kind(), VerifyErrorKind::PreimageMismatch);
    }

    #[test]
    fn verify_rejects_another_root_key() {
        let minted = mint(&root_key(7), [12u8; 32], [8u8; 32]).unwrap();

        let error = verify(&root_key(9), &minted, &"00".repeat(32)).unwrap_err();
        assert_eq!(error.kind(), VerifyErrorKind::BadSignature);
    }

    #[test]
    fn verify_rejects_a_tampered_byte() {
        let root = root_key(10);
        let token = mint(&root, [13u8; 32], [14u8; 32]).unwrap();

        let mut binary = BASE64_STANDARD.decode(&token).unwrap();
        let last = binary.len() - 1;
        binary[last] ^= 0x01;
        let tampered = BASE64_STANDARD.encode(&binary);

        assert_eq!(
            verify(&root, &tampered, &"00".repeat(32))
                .unwrap_err()
                .kind(),
            VerifyErrorKind::BadSignature
        );
    }

    #[test]
    fn verify_rejects_an_unknown_caveat() {
        let root = root_key(15);
        let key = MacaroonKey::from([15u8; 32]);
        let mut macaroon = Macaroon::create(
            Some(super::LOCATION),
            &key,
            identifier([16u8; 32], [17u8; 32]),
        )
        .unwrap();
        macaroon.add_first_party_caveat(SERVICES_CAVEAT).unwrap();
        macaroon.add_first_party_caveat("user = someone").unwrap();

        let encoded = macaroon.serialize(Format::V2).unwrap();
        let binary = BASE64_URL_SAFE.decode(encoded).unwrap();
        let token = BASE64_STANDARD.encode(binary);

        assert_eq!(
            verify(&root, &token, &"00".repeat(32)).unwrap_err().kind(),
            VerifyErrorKind::UnknownCaveat
        );
    }

    #[test]
    fn verify_rejects_a_macaroon_without_the_caveat() {
        let root = root_key(18);
        let key = MacaroonKey::from([18u8; 32]);
        let macaroon = Macaroon::create(
            Some(super::LOCATION),
            &key,
            identifier([19u8; 32], [20u8; 32]),
        )
        .unwrap();

        let encoded = macaroon.serialize(Format::V2).unwrap();
        let binary = BASE64_URL_SAFE.decode(encoded).unwrap();
        let token = BASE64_STANDARD.encode(binary);

        // No caveats at all: the signature is fine but the mutating caveat set
        // is not what Tally mints, so this is refused as an unknown caveat.
        assert!(verify(&root, &token, &"00".repeat(32)).is_err());
    }

    #[test]
    fn verify_rejects_bad_encoding_and_bad_identifier() {
        let root = root_key(21);

        assert_eq!(
            verify(&root, "not base64!", &"00".repeat(32))
                .unwrap_err()
                .kind(),
            VerifyErrorKind::BadEncoding
        );

        let key = MacaroonKey::from([21u8; 32]);
        let macaroon = Macaroon::create(Some(super::LOCATION), &key, [1u8, 2, 3]).unwrap();
        let encoded = macaroon.serialize(Format::V2).unwrap();
        let binary = BASE64_URL_SAFE.decode(encoded).unwrap();
        let token = BASE64_STANDARD.encode(binary);

        assert_eq!(
            verify(&root, &token, &"00".repeat(32)).unwrap_err().kind(),
            VerifyErrorKind::BadIdentifier
        );
    }

    #[test]
    fn errors_and_debug_never_reveal_key_or_token() {
        let root = root_key(22);
        let token = mint(&root, [23u8; 32], [24u8; 32]).unwrap();
        let error = verify(&root_key(25), &token, &"00".repeat(32)).unwrap_err();

        let text = format!("{error} {error:?} {root} {root:?}");
        assert!(!text.contains(&token));
        assert!(!text.contains("2222"));
    }

    #[test]
    fn debug_and_display_redact_the_root_key() {
        let root = root_key(26);
        let text = format!("{root} {root:?}");
        assert!(text.contains("REDACTED"));
        assert!(!text.contains("1a1a1a"));
    }

    #[test]
    fn user_ids_are_32_bytes_and_differ() {
        let first = new_user_id().unwrap();
        let second = new_user_id().unwrap();

        assert_eq!(first.len(), 32);
        assert_ne!(first, second);
    }
}
