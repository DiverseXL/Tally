//! NIP-47 capability data from the two advertising sources.
//!
//! The info event (kind 13194) carries a plain-text capability list; `get_info`
//! carries a JSON list. Both are kept verbatim and separately, so a report can
//! state which one was used and where the two disagree.

use serde::{Deserialize, Serialize};

/// Where a capability list came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CapabilitySource {
    /// The kind 13194 info event.
    #[serde(rename = "info_event")]
    InfoEvent,
    /// The `get_info` response.
    #[serde(rename = "get_info")]
    GetInfo,
    /// Neither source listed a method.
    #[serde(rename = "none")]
    None,
}

impl CapabilitySource {
    /// The field value used in reports.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::InfoEvent => "info_event",
            Self::GetInfo => "get_info",
            Self::None => "none",
        }
    }
}

/// How the raw info-event content looks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ContentKind {
    /// Whitespace-separated tokens made only of lowercase letters, digits and
    /// underscores.
    #[serde(rename = "space_separated")]
    SpaceSeparated,
    /// Nothing but whitespace.
    #[serde(rename = "empty")]
    Empty,
    /// Starts with `{` or `[`, i.e. someone published JSON here.
    #[serde(rename = "json_like")]
    JsonLike,
    /// Anything else.
    #[serde(rename = "other")]
    Other,
}

impl ContentKind {
    /// The field value used in reports and console output.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::SpaceSeparated => "space_separated",
            Self::Empty => "empty",
            Self::JsonLike => "json_like",
            Self::Other => "other",
        }
    }
}

/// Classifies the raw info-event content without interpreting it.
#[must_use]
pub fn content_kind(raw_content: &str) -> ContentKind {
    let trimmed = raw_content.trim();

    if trimmed.is_empty() {
        return ContentKind::Empty;
    }

    if trimmed.starts_with('{') || trimmed.starts_with('[') {
        return ContentKind::JsonLike;
    }

    let all_plain_tokens = trimmed.split_whitespace().all(is_method_token);

    if all_plain_tokens {
        ContentKind::SpaceSeparated
    } else {
        ContentKind::Other
    }
}

/// Whether a whitespace-separated token can be a NIP-47 method or extension
/// name: lowercase ASCII letters, digits and underscores only.
///
/// This is what makes the difference between a capability list and a JSON blob
/// someone published in the content field by mistake.
#[must_use]
pub fn is_method_token(token: &str) -> bool {
    !token.is_empty()
        && token
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
}

/// Raw data of the kind 13194 info event.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InfoEventCapabilities {
    /// Whether an info event was received at all.
    pub found: bool,
    /// The content string exactly as received (plain text, never parsed as JSON).
    pub raw_content: String,
    /// Method tokens from the content, split on whitespace, verbatim.
    pub methods: Vec<String>,
    /// Encryption tokens from every `encryption` tag, split on whitespace.
    pub encryption_tokens: Vec<String>,
    /// Extension ids from every `extensions` tag, split on whitespace.
    pub extensions: Vec<String>,
}

impl InfoEventCapabilities {
    /// An info event that was not received.
    #[must_use]
    pub fn missing() -> Self {
        Self {
            found: false,
            raw_content: String::new(),
            methods: Vec::new(),
            encryption_tokens: Vec::new(),
            extensions: Vec::new(),
        }
    }

    /// Builds the record from the raw content and the two tag lists.
    #[must_use]
    pub fn from_content(
        raw_content: String,
        encryption_tokens: Vec<String>,
        extensions: Vec<String>,
    ) -> Self {
        let methods = raw_content
            .split_whitespace()
            .map(str::to_owned)
            .collect::<Vec<String>>();

        Self {
            found: true,
            raw_content,
            methods,
            encryption_tokens,
            extensions,
        }
    }

    /// How the raw content looks.
    #[must_use]
    pub fn content_kind(&self) -> ContentKind {
        content_kind(&self.raw_content)
    }

    /// The entries of [`Self::methods`] that can be method names.
    ///
    /// [`Self::methods`] is always the plain whitespace split of the content;
    /// this view drops tokens such as a whole JSON document, which must never be
    /// mistaken for an advertised capability.
    #[must_use]
    pub fn method_tokens(&self) -> Vec<String> {
        self.methods
            .iter()
            .filter(|token| is_method_token(token))
            .cloned()
            .collect()
    }
}

/// The info-event token that advertises notification support.
///
/// It is a flag, not a method: it is compared with the `get_info` notification
/// list being non-empty, never with the `get_info` method list.
pub const NOTIFICATIONS_FLAG: &str = "notifications";

/// Data of the `get_info` response.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GetInfoCapabilities {
    /// Methods the wallet reports for this connection, verbatim.
    pub methods: Vec<String>,
    /// Extension ids, when the client library exposes them at all.
    ///
    /// `None` means the library has no such field in its `get_info` response,
    /// which is the case for nostr 0.45.7: the field is not exposed, so it is
    /// not an empty list. Extension ids come from the info event only.
    pub extensions: Option<Vec<String>>,
    /// Supported notification types, verbatim.
    pub notifications: Vec<String>,
    /// Reported network, if any.
    pub network: Option<String>,
    /// Reported alias, if any.
    pub alias: Option<String>,
}

/// Both capability sources of one wallet.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Capabilities {
    /// The info event, present or missing.
    pub info_event: InfoEventCapabilities,
    /// The `get_info` response, when one was obtained.
    pub get_info: Option<GetInfoCapabilities>,
}

impl Capabilities {
    /// Builds the pair from both sources.
    #[must_use]
    pub fn new(info_event: InfoEventCapabilities, get_info: Option<GetInfoCapabilities>) -> Self {
        Self {
            info_event,
            get_info,
        }
    }

    /// The capability list that governs the run.
    ///
    /// The info event wins when its content has at least one method token;
    /// otherwise `get_info` is used; otherwise there is no source. The raw
    /// content is always kept in the record, and [`InfoEventCapabilities::content_kind`]
    /// shows when content that is not a capability list was ignored.
    #[must_use]
    pub fn effective(&self) -> EffectiveCapabilities {
        let info_methods = self.info_event.method_tokens();

        if !info_methods.is_empty() {
            return EffectiveCapabilities {
                source: CapabilitySource::InfoEvent,
                methods: info_methods,
            };
        }

        match &self.get_info {
            Some(get_info) => EffectiveCapabilities {
                source: CapabilitySource::GetInfo,
                methods: get_info.methods.clone(),
            },
            None => EffectiveCapabilities {
                source: CapabilitySource::None,
                methods: Vec::new(),
            },
        }
    }

    /// Capabilities listed by exactly one of the two sources.
    ///
    /// Method names are compared between the info-event content and the
    /// `get_info` method list. The token [`NOTIFICATIONS_FLAG`] is not a method
    /// name: it is compared with the `get_info` notification list being
    /// non-empty, so `notifications` in both places is agreement, not a
    /// discrepancy.
    ///
    /// Empty when there is no `get_info` data or no info-event method token,
    /// because there is nothing to compare against.
    #[must_use]
    pub fn discrepancies(&self) -> Vec<Discrepancy> {
        let Some(get_info) = &self.get_info else {
            return Vec::new();
        };

        let info_methods = self.info_event.method_tokens();

        if info_methods.is_empty() {
            return Vec::new();
        }

        let mut found = Vec::new();

        let info_plain = info_methods
            .iter()
            .filter(|method| method.as_str() != NOTIFICATIONS_FLAG)
            .cloned()
            .collect::<Vec<String>>();

        for method in &info_plain {
            if !contains(&get_info.methods, method) && !contains_method(&found, method) {
                found.push(Discrepancy {
                    method: method.clone(),
                    present_in: CapabilitySource::InfoEvent,
                    absent_in: CapabilitySource::GetInfo,
                });
            }
        }

        for method in &get_info.methods {
            if !contains(&info_plain, method)
                && method.as_str() != NOTIFICATIONS_FLAG
                && !contains_method(&found, method)
            {
                found.push(Discrepancy {
                    method: method.clone(),
                    present_in: CapabilitySource::GetInfo,
                    absent_in: CapabilitySource::InfoEvent,
                });
            }
        }

        let advertised_notifications = contains(&info_methods, NOTIFICATIONS_FLAG);
        let wallet_notifications = !get_info.notifications.is_empty();

        match (advertised_notifications, wallet_notifications) {
            (true, false) => found.push(Discrepancy {
                method: String::from(NOTIFICATIONS_FLAG),
                present_in: CapabilitySource::InfoEvent,
                absent_in: CapabilitySource::GetInfo,
            }),
            (false, true) => found.push(Discrepancy {
                method: String::from(NOTIFICATIONS_FLAG),
                present_in: CapabilitySource::GetInfo,
                absent_in: CapabilitySource::InfoEvent,
            }),
            _ => {}
        }

        found.sort_by(|left, right| left.method.cmp(&right.method));
        found
    }

    /// Whether `method` is advertised by the effective source.
    #[must_use]
    pub fn advertises(&self, method: &str) -> Advertisement {
        let effective = self.effective();

        match effective.source {
            CapabilitySource::None => Advertisement::Unknown,
            source => {
                if contains(&effective.methods, method) {
                    Advertisement::Yes(source)
                } else {
                    Advertisement::No(source)
                }
            }
        }
    }
}

fn contains(list: &[String], method: &str) -> bool {
    list.iter().any(|candidate| candidate == method)
}

fn contains_method(list: &[Discrepancy], method: &str) -> bool {
    list.iter().any(|item| item.method == method)
}

/// The capability list that governs the run, with its source.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EffectiveCapabilities {
    /// Where the list came from.
    pub source: CapabilitySource,
    /// Methods in that list, verbatim.
    pub methods: Vec<String>,
}

/// A method that only one source lists.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Discrepancy {
    /// The method name, verbatim.
    pub method: String,
    /// The source that lists it.
    pub present_in: CapabilitySource,
    /// The source that does not.
    pub absent_in: CapabilitySource,
}

/// Answer to "does the wallet advertise this method".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Advertisement {
    /// Advertised, by the named source.
    Yes(CapabilitySource),
    /// Not advertised, according to the named source.
    No(CapabilitySource),
    /// No source listed any method, so nothing can be said.
    Unknown,
}

impl Advertisement {
    /// True only for [`Advertisement::Yes`].
    #[must_use]
    pub const fn is_yes(self) -> bool {
        matches!(self, Self::Yes(_))
    }

    /// The source that answered, if any.
    #[must_use]
    pub const fn source(self) -> Option<CapabilitySource> {
        match self {
            Self::Yes(source) | Self::No(source) => Some(source),
            Self::Unknown => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        Advertisement, Capabilities, CapabilitySource, ContentKind, GetInfoCapabilities,
        InfoEventCapabilities, content_kind,
    };

    fn get_info(methods: &[&str]) -> GetInfoCapabilities {
        GetInfoCapabilities {
            methods: methods.iter().map(|method| (*method).to_owned()).collect(),
            // The library exposes no extension list, so the field is absent.
            extensions: None,
            notifications: Vec::new(),
            network: Some(String::from("signet")),
            alias: Some(String::from("test-wallet")),
        }
    }

    fn with_notifications(mut get_info: GetInfoCapabilities) -> GetInfoCapabilities {
        get_info.notifications = vec![String::from("payment_received")];
        get_info
    }

    #[test]
    fn notifications_flag_agrees_with_a_non_empty_notification_list() {
        let info = InfoEventCapabilities::from_content(
            String::from("pay_invoice notifications"),
            Vec::new(),
            Vec::new(),
        );
        let capabilities =
            Capabilities::new(info, Some(with_notifications(get_info(&["pay_invoice"]))));

        assert!(capabilities.discrepancies().is_empty());
    }

    #[test]
    fn notifications_flag_without_wallet_notifications_is_a_discrepancy() {
        let info = InfoEventCapabilities::from_content(
            String::from("pay_invoice notifications"),
            Vec::new(),
            Vec::new(),
        );
        let capabilities = Capabilities::new(info, Some(get_info(&["pay_invoice"])));

        let discrepancies = capabilities.discrepancies();
        assert_eq!(discrepancies.len(), 1);
        assert_eq!(discrepancies[0].method, "notifications");
        assert_eq!(discrepancies[0].present_in, CapabilitySource::InfoEvent);
        assert_eq!(discrepancies[0].absent_in, CapabilitySource::GetInfo);
    }

    #[test]
    fn wallet_notifications_without_the_info_flag_is_a_discrepancy() {
        let info = InfoEventCapabilities::from_content(
            String::from("pay_invoice"),
            Vec::new(),
            Vec::new(),
        );
        let capabilities =
            Capabilities::new(info, Some(with_notifications(get_info(&["pay_invoice"]))));

        let discrepancies = capabilities.discrepancies();
        assert_eq!(discrepancies.len(), 1);
        assert_eq!(discrepancies[0].method, "notifications");
        assert_eq!(discrepancies[0].present_in, CapabilitySource::GetInfo);
        assert_eq!(discrepancies[0].absent_in, CapabilitySource::InfoEvent);
    }

    #[test]
    fn get_info_extensions_are_absent_not_empty() {
        let get_info = get_info(&["pay_invoice"]);
        assert_eq!(get_info.extensions, None);
    }

    fn no_get_info() -> Option<GetInfoCapabilities> {
        None
    }

    #[test]
    fn content_is_split_on_whitespace() {
        let info = InfoEventCapabilities::from_content(
            String::from("pay_invoice get_balance"),
            Vec::new(),
            Vec::new(),
        );

        assert_eq!(info.methods, vec!["pay_invoice", "get_balance"]);
        assert!(info.found);
        assert_eq!(info.content_kind(), ContentKind::SpaceSeparated);
    }

    #[test]
    fn empty_content_falls_back_to_get_info() {
        let info = InfoEventCapabilities::from_content(String::new(), Vec::new(), Vec::new());
        assert_eq!(info.content_kind(), ContentKind::Empty);

        let capabilities = Capabilities::new(info, Some(get_info(&["get_info", "make_invoice"])));
        let effective = capabilities.effective();

        assert_eq!(effective.source, CapabilitySource::GetInfo);
        assert_eq!(effective.methods, vec!["get_info", "make_invoice"]);
        assert_eq!(
            capabilities.advertises("make_invoice"),
            Advertisement::Yes(CapabilitySource::GetInfo)
        );
        assert_eq!(
            capabilities.advertises("pay_invoice"),
            Advertisement::No(CapabilitySource::GetInfo)
        );
    }

    #[test]
    fn no_source_at_all_is_unknown() {
        let info = InfoEventCapabilities::from_content(String::new(), Vec::new(), Vec::new());
        let capabilities = Capabilities::new(info, no_get_info());

        assert_eq!(capabilities.effective().source, CapabilitySource::None);
        assert_eq!(
            capabilities.advertises("pay_invoice"),
            Advertisement::Unknown
        );
        assert!(capabilities.discrepancies().is_empty());
    }

    #[test]
    fn discrepancies_list_methods_present_in_one_source_only() {
        let info = InfoEventCapabilities::from_content(
            String::from("pay_invoice get_balance lookup_invoice"),
            Vec::new(),
            Vec::new(),
        );
        let capabilities = Capabilities::new(
            info,
            Some(get_info(&["get_balance", "make_invoice", "lookup_invoice"])),
        );

        let discrepancies = capabilities.discrepancies();
        let methods = discrepancies
            .iter()
            .map(|item| item.method.as_str())
            .collect::<Vec<&str>>();
        assert_eq!(methods, vec!["make_invoice", "pay_invoice"]);

        assert_eq!(discrepancies[0].present_in, CapabilitySource::GetInfo);
        assert_eq!(discrepancies[0].absent_in, CapabilitySource::InfoEvent);
        assert_eq!(discrepancies[1].present_in, CapabilitySource::InfoEvent);
        assert_eq!(discrepancies[1].absent_in, CapabilitySource::GetInfo);
    }

    #[test]
    fn json_like_content_is_never_parsed_as_capabilities() {
        let raw = String::from("{\"methods\":[\"pay_invoice\"]}");
        let info = InfoEventCapabilities::from_content(raw, Vec::new(), Vec::new());

        assert_eq!(info.content_kind(), ContentKind::JsonLike);
        // The raw split is kept verbatim, but it is not a method name.
        assert_eq!(info.methods.len(), 1);
        assert_eq!(info.methods[0], "{\"methods\":[\"pay_invoice\"]}");
        assert!(info.method_tokens().is_empty());

        let capabilities = Capabilities::new(info, Some(get_info(&["get_info"])));
        assert_eq!(capabilities.effective().source, CapabilitySource::GetInfo);
        assert_eq!(
            capabilities.advertises("get_info"),
            Advertisement::Yes(CapabilitySource::GetInfo)
        );
    }

    #[test]
    fn unknown_and_extension_method_names_stay_verbatim() {
        let info = InfoEventCapabilities::from_content(
            String::from("pay_invoice custom_vendor_method"),
            vec![String::from("nip44_v2")],
            vec![String::from("vendor_extension")],
        );

        let capabilities = Capabilities::new(info, no_get_info());
        assert_eq!(
            capabilities.advertises("custom_vendor_method"),
            Advertisement::Yes(CapabilitySource::InfoEvent)
        );
        assert_eq!(capabilities.info_event.encryption_tokens, vec!["nip44_v2"]);
        assert_eq!(capabilities.info_event.extensions, vec!["vendor_extension"]);
    }

    #[test]
    fn content_kind_classification() {
        assert_eq!(content_kind("  "), ContentKind::Empty);
        assert_eq!(content_kind("pay_invoice"), ContentKind::SpaceSeparated);
        assert_eq!(content_kind("[1, 2]"), ContentKind::JsonLike);
        assert_eq!(content_kind("pay_invoice,get_balance"), ContentKind::Other);
        assert_eq!(
            content_kind("pay_invoice\n get_balance"),
            ContentKind::SpaceSeparated
        );
    }
}
