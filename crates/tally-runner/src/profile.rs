//! The test profiles and the exact order of their tests.
//!
//! The profile is the only place that decides which tests a run selects and
//! which of them are required.

use std::fmt;

/// One test of a profile.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TestSpec {
    /// Stable id, also used in logs and reports.
    pub id: &'static str,
    /// Human-readable name.
    pub name: &'static str,
    /// Whether this test decides the exit code.
    pub required: bool,
    /// The capability the test needs, when it needs one.
    pub capability: Option<&'static str>,
}

/// The tests a run executes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Profile {
    /// The wallet-only profile: no protected resource and no payment.
    Basic,
    /// The flagship profile: challenge, payment, preimage check, unlock.
    L402E2e,
    /// One wallet inspected on its own; no fixture and no payment.
    Wallet,
}

impl Profile {
    /// The field value used in reports and on the command line.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Basic => "basic",
            Self::L402E2e => "l402-e2e",
            Self::Wallet => "wallet",
        }
    }

    /// Parses a profile name.
    #[must_use]
    pub fn parse(name: &str) -> Option<Self> {
        match name {
            "basic" => Some(Self::Basic),
            "l402-e2e" => Some(Self::L402E2e),
            "wallet" => Some(Self::Wallet),
            _ => None,
        }
    }

    /// The tests of this profile, in run order.
    #[must_use]
    pub const fn tests(self) -> &'static [TestSpec] {
        match self {
            Self::Basic => BASIC_TESTS,
            Self::L402E2e => L402_TESTS,
            Self::Wallet => WALLET_TESTS,
        }
    }

    /// The spec for one test id, if the profile selects it.
    #[must_use]
    pub fn spec(self, id: &str) -> Option<&'static TestSpec> {
        self.tests().iter().find(|spec| spec.id == id)
    }
}

impl fmt::Display for Profile {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

const BASIC_TESTS: &[TestSpec] = &[
    TestSpec {
        id: "connect",
        name: "Connect both wallets",
        required: true,
        capability: None,
    },
    TestSpec {
        id: "info_event",
        name: "Fetch the info event",
        required: true,
        capability: None,
    },
    TestSpec {
        id: "get_info",
        name: "Get wallet info",
        required: true,
        capability: Some("get_info"),
    },
    TestSpec {
        id: "get_balance",
        name: "Get balance",
        required: true,
        capability: Some("get_balance"),
    },
    TestSpec {
        id: "make_invoice",
        name: "Make an invoice",
        required: true,
        capability: Some("make_invoice"),
    },
    TestSpec {
        id: "lookup_invoice",
        name: "Look up the invoice",
        required: true,
        capability: Some("lookup_invoice"),
    },
    TestSpec {
        id: "sign_message",
        name: "Sign a message",
        required: false,
        capability: Some("sign_message"),
    },
];

/// The six fixed tests of the wallet profile. The capability checks that follow
/// them are built from the required-method list of each run and are appended by
/// the single-wallet runner, never stored here.
const WALLET_TESTS: &[TestSpec] = &[
    TestSpec {
        id: "connect",
        name: "Connect the wallet",
        required: true,
        capability: None,
    },
    TestSpec {
        id: "info_event",
        name: "Fetch the info event",
        required: true,
        capability: None,
    },
    TestSpec {
        id: "get_info",
        name: "Get wallet info",
        required: true,
        capability: Some("get_info"),
    },
    TestSpec {
        id: "get_balance",
        name: "Get balance",
        required: true,
        capability: Some("get_balance"),
    },
    TestSpec {
        id: "make_invoice",
        name: "Make an invoice",
        required: true,
        capability: Some("make_invoice"),
    },
    TestSpec {
        id: "lookup_invoice",
        name: "Look up the invoice",
        required: true,
        capability: Some("lookup_invoice"),
    },
];

const L402_TESTS: &[TestSpec] = &[
    TestSpec {
        id: "connect",
        name: "Connect both wallets",
        required: true,
        capability: None,
    },
    TestSpec {
        id: "info_event",
        name: "Fetch the info event",
        required: true,
        capability: None,
    },
    TestSpec {
        id: "get_info",
        name: "Get wallet info",
        required: true,
        capability: Some("get_info"),
    },
    TestSpec {
        id: "get_balance",
        name: "Get balance",
        required: true,
        capability: Some("get_balance"),
    },
    TestSpec {
        id: "challenge",
        name: "Fetch the L402 challenge",
        required: true,
        capability: Some("make_invoice"),
    },
    TestSpec {
        id: "invoice_validation",
        name: "Validate the invoice",
        required: true,
        capability: None,
    },
    TestSpec {
        id: "challenge_binding",
        name: "Check the challenge binding",
        required: true,
        capability: None,
    },
    TestSpec {
        id: "pay_invoice",
        name: "Pay the invoice",
        required: true,
        capability: Some("pay_invoice"),
    },
    TestSpec {
        id: "preimage_verification",
        name: "Verify the preimage",
        required: true,
        capability: None,
    },
    TestSpec {
        id: "protected_resource",
        name: "Unlock the protected resource",
        required: true,
        capability: None,
    },
    TestSpec {
        id: "negative_wrong_proof",
        name: "Reject a wrong proof",
        required: true,
        capability: None,
    },
    TestSpec {
        id: "negative_no_proof",
        name: "Reject a macaroon without a preimage",
        required: true,
        capability: None,
    },
    TestSpec {
        id: "lookup_invoice",
        name: "Look up the invoice",
        required: true,
        capability: Some("lookup_invoice"),
    },
    TestSpec {
        id: "sign_message",
        name: "Sign a message",
        required: false,
        capability: Some("sign_message"),
    },
];

#[cfg(test)]
mod tests {
    use super::Profile;

    #[test]
    fn profiles_list_their_tests_in_order() {
        let ids = Profile::L402E2e
            .tests()
            .iter()
            .map(|spec| spec.id)
            .collect::<Vec<&str>>();
        assert_eq!(
            ids,
            vec![
                "connect",
                "info_event",
                "get_info",
                "get_balance",
                "challenge",
                "invoice_validation",
                "challenge_binding",
                "pay_invoice",
                "preimage_verification",
                "protected_resource",
                "negative_wrong_proof",
                "negative_no_proof",
                "lookup_invoice",
                "sign_message",
            ]
        );

        let basic = Profile::Basic
            .tests()
            .iter()
            .map(|spec| spec.id)
            .collect::<Vec<&str>>();
        assert_eq!(
            basic,
            vec![
                "connect",
                "info_event",
                "get_info",
                "get_balance",
                "make_invoice",
                "lookup_invoice",
                "sign_message",
            ]
        );

        let wallet = Profile::Wallet
            .tests()
            .iter()
            .map(|spec| spec.id)
            .collect::<Vec<&str>>();
        assert_eq!(
            wallet,
            vec![
                "connect",
                "info_event",
                "get_info",
                "get_balance",
                "make_invoice",
                "lookup_invoice",
            ]
        );
    }

    #[test]
    fn only_sign_message_is_not_required() {
        for profile in [Profile::Basic, Profile::L402E2e] {
            for spec in profile.tests() {
                assert_eq!(spec.required, spec.id != "sign_message", "{}", spec.id);
            }
        }
    }

    #[test]
    fn names_round_trip() {
        assert_eq!(Profile::parse("basic"), Some(Profile::Basic));
        assert_eq!(Profile::parse("l402-e2e"), Some(Profile::L402E2e));
        assert_eq!(Profile::parse("wallet"), Some(Profile::Wallet));
        assert_eq!(Profile::parse("other"), None);
        assert_eq!(Profile::Basic.to_string(), "basic");
        assert_eq!(Profile::Wallet.to_string(), "wallet");
    }
}
