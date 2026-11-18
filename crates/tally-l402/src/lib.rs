//! The L402 engine: invoice facts, the two headers, macaroon mint and verify,
//! the loopback test fixture and a client.
//!
//! The fixture is a local test service. It proves Tally's flow, not
//! compatibility with third-party L402 services. Macaroons use Tally's own
//! identifier layout over libmacaroon's V2 serialization; the real proof check
//! is that SHA-256 of the preimage equals the invoice payment hash.
//!
//! Nothing in this crate pays: it holds no payment code at all.

pub mod challenge;
pub mod client;
pub mod fixture;
pub mod invoice;
pub mod macaroon;
#[cfg(feature = "test-support")]
pub mod test_support;

pub use challenge::{
    Challenge, ChallengeError, ChallengeErrorKind, Credentials, MAX_HEADER_BYTES,
    format_authorization, format_www_authenticate, parse_authorization, parse_www_authenticate,
};
pub use client::{
    CONNECT_TIMEOUT, ClientError, ClientErrorKind, ClientResponse, L402Client, REQUEST_TIMEOUT,
    is_allowed_url,
};
pub use fixture::{
    FixtureConfig, FixtureError, FixtureHandle, INVOICE_DESCRIPTION, InvoiceSource,
    InvoiceSourceError, NwcInvoiceSource, PAYMENT_REQUIRED_BODY, RESOURCE_BODY, SourcedInvoice,
    spawn,
};
pub use invoice::{InvoiceError, InvoiceErrorKind, InvoiceFacts, check, parse};
pub use macaroon::{
    IDENTIFIER_BYTES, IDENTIFIER_VERSION, KeyError, LOCATION, RootKey, SERVICES_CAVEAT,
    VerifyError, VerifyErrorKind, identifier_payment_hash, identifier_user_id, mint, new_user_id,
    verify,
};
