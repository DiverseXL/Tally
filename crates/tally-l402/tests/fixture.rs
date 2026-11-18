//! Integration tests for the L402 fixture, over loopback, with the real client.
//!
//! The invoice source is the TEST ONLY generator in `tests/common`: it signs
//! invoices offline and remembers their preimages. No payment is made and no
//! network other than 127.0.0.1 is touched.

mod common;

use std::collections::BTreeSet;
use std::time::Duration;

use base64::Engine as _;
use base64::prelude::BASE64_STANDARD;
use libmacaroon::{Format, Macaroon};
use reqwest::header::{AUTHORIZATION, HeaderMap, HeaderValue};
use reqwest::redirect::Policy;

use tally_l402::client::{ClientErrorKind, L402Client};
use tally_l402::fixture::{self, FixtureConfig, FixtureHandle, RESOURCE_BODY};
use tally_l402::invoice;
use tally_l402::macaroon::identifier_payment_hash;

use common::TestInvoiceSource;

const HEADER_ALLOWLIST: [&str; 6] = [
    "cache-control",
    "content-length",
    "content-type",
    "date",
    "www-authenticate",
    "authorization",
];

async fn spawn_fixture() -> (FixtureHandle, TestInvoiceSource) {
    let source = TestInvoiceSource::new();
    let handle = fixture::spawn(FixtureConfig::default(), source.clone())
        .await
        .expect("fixture starts on loopback");
    (handle, source)
}

fn raw_client() -> reqwest::Client {
    reqwest::Client::builder()
        .redirect(Policy::none())
        .connect_timeout(Duration::from_secs(5))
        .timeout(Duration::from_secs(15))
        .build()
        .expect("raw test client")
}

/// Sends one raw `Authorization` value and returns the status, headers and body.
async fn send_raw_header(url: &str, header: Option<HeaderValue>) -> (u16, HeaderMap, String) {
    let mut request = raw_client().get(url);
    if let Some(header) = header {
        request = request.header(AUTHORIZATION, header);
    }

    let response = request.send().await.expect("loopback request");
    let status = response.status().as_u16();
    let headers = response.headers().clone();
    let body = response.text().await.expect("body");

    (status, headers, body)
}

/// Fetches a challenge and resolves the matching preimage from the source.
async fn challenge_and_preimage(
    client: &L402Client,
    url: &str,
    source: &TestInvoiceSource,
) -> (String, String, String) {
    let challenge = client.fetch_challenge(url).await.expect("challenge");
    let hash = identifier_payment_hash(challenge.macaroon_b64()).expect("identifier");
    let preimage = source.preimage_for(hash).expect("issued preimage");

    (
        challenge.macaroon_b64().to_owned(),
        preimage,
        challenge.invoice().to_owned(),
    )
}

#[tokio::test]
async fn scenario_01_no_authorization_gives_a_bound_challenge() {
    let (handle, _source) = spawn_fixture().await;
    let url = handle.protected_url();
    let (status, headers, body) = send_raw_header(&url, None).await;

    assert_eq!(status, 402);
    assert_eq!(body, "{\"error\":\"payment required\"}");
    assert_eq!(
        headers.get("cache-control").and_then(|v| v.to_str().ok()),
        Some("no-store")
    );

    let header = headers
        .get("www-authenticate")
        .and_then(|value| value.to_str().ok())
        .expect("challenge header");
    let challenge = tally_l402::challenge::parse_www_authenticate(header).expect("parseable");

    let facts = invoice::parse(challenge.invoice()).expect("parseable invoice");
    assert_eq!(facts.network, "signet");
    assert_eq!(facts.amount_msat, Some(21_000));
    assert_eq!(
        identifier_payment_hash(challenge.macaroon_b64()).unwrap(),
        facts.payment_hash
    );

    handle.shutdown().await;
}

#[tokio::test]
async fn scenario_02_correct_credentials_give_the_resource() {
    let (handle, source) = spawn_fixture().await;
    let url = handle.protected_url();
    let client = L402Client::new().unwrap();

    let (macaroon, preimage, _invoice) = challenge_and_preimage(&client, &url, &source).await;
    let response = client
        .fetch_with_credentials(&url, &macaroon, &preimage)
        .await
        .expect("credentialed request");

    assert_eq!(response.status, 200);
    assert_eq!(response.body, RESOURCE_BODY);

    handle.shutdown().await;
}

#[tokio::test]
async fn scenario_03_wrong_preimage_gives_402() {
    let (handle, source) = spawn_fixture().await;
    let url = handle.protected_url();
    let client = L402Client::new().unwrap();

    let (macaroon, _preimage, _invoice) = challenge_and_preimage(&client, &url, &source).await;
    let wrong = "aa".repeat(32);
    let response = client
        .fetch_with_credentials(&url, &macaroon, &wrong)
        .await
        .expect("credentialed request");

    assert_eq!(response.status, 402);

    handle.shutdown().await;
}

#[tokio::test]
async fn scenario_04_preimage_from_another_challenge_gives_402() {
    let (handle, source) = spawn_fixture().await;
    let url = handle.protected_url();
    let client = L402Client::new().unwrap();

    let (_macaroon_a, preimage_a, _invoice_a) =
        challenge_and_preimage(&client, &url, &source).await;
    let (macaroon_b, _preimage_b, _invoice_b) =
        challenge_and_preimage(&client, &url, &source).await;

    let response = client
        .fetch_with_credentials(&url, &macaroon_b, &preimage_a)
        .await
        .expect("credentialed request");

    assert_eq!(response.status, 402);

    handle.shutdown().await;
}

#[tokio::test]
async fn scenario_05_macaroon_only_gives_402() {
    let (handle, source) = spawn_fixture().await;
    let url = handle.protected_url();
    let client = L402Client::new().unwrap();

    let (macaroon, _preimage, _invoice) = challenge_and_preimage(&client, &url, &source).await;

    let no_colon = HeaderValue::from_str(&format!("L402 {macaroon}")).unwrap();
    let empty_preimage = HeaderValue::from_str(&format!("L402 {macaroon}:")).unwrap();

    assert_eq!(send_raw_header(&url, Some(no_colon)).await.0, 402);
    assert_eq!(send_raw_header(&url, Some(empty_preimage)).await.0, 402);

    handle.shutdown().await;
}

#[tokio::test]
async fn scenario_06_tampered_macaroon_gives_402() {
    let (handle, source) = spawn_fixture().await;
    let url = handle.protected_url();
    let client = L402Client::new().unwrap();

    let (macaroon, preimage, _invoice) = challenge_and_preimage(&client, &url, &source).await;

    let mut binary = BASE64_STANDARD.decode(&macaroon).expect("decodable");
    let last = binary.len() - 1;
    binary[last] ^= 0x01;
    let tampered = BASE64_STANDARD.encode(&binary);

    let response = client
        .fetch_with_credentials(&url, &tampered, &preimage)
        .await
        .expect("credentialed request");

    assert_eq!(response.status, 402);

    handle.shutdown().await;
}

#[tokio::test]
async fn scenario_07_extra_caveat_gives_402() {
    let (handle, source) = spawn_fixture().await;
    let url = handle.protected_url();
    let client = L402Client::new().unwrap();

    let (macaroon, preimage, _invoice) = challenge_and_preimage(&client, &url, &source).await;

    let binary = BASE64_STANDARD.decode(&macaroon).expect("decodable");
    let mut appended = Macaroon::deserialize_binary(&binary).expect("real macaroon");
    appended
        .add_first_party_caveat("user = mallory")
        .expect("append caveat");
    let encoded = appended.serialize(Format::V2).expect("serialize");
    let with_caveat = BASE64_STANDARD.encode(
        base64::prelude::BASE64_URL_SAFE
            .decode(encoded)
            .expect("url-safe base64"),
    );

    let response = client
        .fetch_with_credentials(&url, &with_caveat, &preimage)
        .await
        .expect("credentialed request");

    assert_eq!(response.status, 402);

    handle.shutdown().await;
}

#[tokio::test]
async fn scenario_08_macaroon_from_another_fixture_gives_402() {
    let (handle_a, _source_a) = spawn_fixture().await;
    let (handle_b, source_b) = spawn_fixture().await;
    let client = L402Client::new().unwrap();

    let (macaroon_b, preimage_b, _invoice_b) =
        challenge_and_preimage(&client, &handle_b.protected_url(), &source_b).await;

    let response = client
        .fetch_with_credentials(&handle_a.protected_url(), &macaroon_b, &preimage_b)
        .await
        .expect("credentialed request");

    assert_eq!(response.status, 402);

    handle_a.shutdown().await;
    handle_b.shutdown().await;
}

#[tokio::test]
async fn scenario_09_malformed_authorization_gives_402_never_500() {
    let (handle, source) = spawn_fixture().await;
    let url = handle.protected_url();
    let client = L402Client::new().unwrap();

    let (macaroon, preimage, _invoice) = challenge_and_preimage(&client, &url, &source).await;

    let cases: Vec<HeaderValue> = vec![
        // A tab is the only control character a header value may carry.
        HeaderValue::from_bytes(format!("L402 {macaroon}\t:{preimage}").as_bytes()).unwrap(),
        HeaderValue::from_str(&format!("L402 {macaroon}:{preimage}:{preimage}")).unwrap(),
        HeaderValue::from_str(&format!("L402 {macaroon}:{}", "zz".repeat(32))).unwrap(),
        // The specification's own credential example: a 12-byte preimage.
        HeaderValue::from_str("L402 AGIAJEemVQUTEyNCR0exk7ek90Cg==:1234abcd1234abcd1234abcd")
            .unwrap(),
        HeaderValue::from_str(&format!("Basic {macaroon}:{preimage}")).unwrap(),
        HeaderValue::from_str(&format!("L402 {}", "A".repeat(9_000))).unwrap(),
        HeaderValue::from_str("L402 :").unwrap(),
        HeaderValue::from_str("L402").unwrap(),
    ];

    for header in cases {
        let (status, _headers, body) = send_raw_header(&url, Some(header)).await;
        assert_eq!(status, 402);
        assert_eq!(body, "{\"error\":\"payment required\"}");
    }

    handle.shutdown().await;
}

#[tokio::test]
async fn scenario_10_the_same_credentials_work_twice() {
    let (handle, source) = spawn_fixture().await;
    let url = handle.protected_url();
    let client = L402Client::new().unwrap();

    let (macaroon, preimage, _invoice) = challenge_and_preimage(&client, &url, &source).await;

    let first = client
        .fetch_with_credentials(&url, &macaroon, &preimage)
        .await
        .expect("first use");
    let second = client
        .fetch_with_credentials(&url, &macaroon, &preimage)
        .await
        .expect("second use");

    assert_eq!(first.status, 200);
    assert_eq!(second.status, 200);
    assert_eq!(first.body, RESOURCE_BODY);
    assert_eq!(second.body, RESOURCE_BODY);

    handle.shutdown().await;
}

#[tokio::test]
async fn scenario_11_responses_never_contain_the_preimage_or_the_invoice() {
    let (handle, source) = spawn_fixture().await;
    let url = handle.protected_url();
    let client = L402Client::new().unwrap();

    let challenge = client.fetch_challenge(&url).await.expect("challenge");
    let hash = identifier_payment_hash(challenge.macaroon_b64()).expect("identifier");
    let preimage = source.preimage_for(hash).expect("issued preimage");
    let invoice_text = source.invoice_for(hash).expect("issued invoice").invoice;

    let (status, headers, body) = send_raw_header(&url, None).await;
    assert_eq!(status, 402);

    let response = client
        .fetch_with_credentials(&url, challenge.macaroon_b64(), &preimage)
        .await
        .expect("credentialed request");
    assert_eq!(response.status, 200);

    let preimage_bytes = hex_to_bytes(&preimage);
    let mut haystacks: Vec<String> = vec![body.clone()];
    let mut header_names: BTreeSet<String> = BTreeSet::new();

    for (name, value) in headers.iter() {
        header_names.insert(name.as_str().to_ascii_lowercase());
        if let Ok(text) = value.to_str() {
            haystacks.push(text.to_owned());
        }
    }

    haystacks.push(response.body.clone());

    let forbidden = [
        preimage.clone(),
        BASE64_STANDARD.encode(&preimage_bytes),
        base64::prelude::BASE64_URL_SAFE.encode(&preimage_bytes),
        String::from_utf8_lossy(&preimage_bytes).into_owned(),
    ];

    for haystack in &haystacks {
        for needle in &forbidden {
            assert!(
                !haystack.contains(needle.as_str()),
                "a response leaked the preimage"
            );
        }
        assert!(
            !haystack.contains(invoice_text.as_str()),
            "a response body leaked the invoice outside the challenge"
        );
    }

    // The only body a 402 ever carries is the fixed error document.
    assert_eq!(body, "{\"error\":\"payment required\"}");

    for name in &header_names {
        assert!(
            HEADER_ALLOWLIST.contains(&name.as_str()),
            "unexpected response header name"
        );
    }

    handle.shutdown().await;
}

#[tokio::test]
async fn scenario_12_client_refuses_non_loopback_and_https() {
    let (handle, _source) = spawn_fixture().await;
    let client = L402Client::new().unwrap();

    assert_eq!(
        client
            .fetch_challenge("http://example.com/protected")
            .await
            .unwrap_err()
            .kind(),
        ClientErrorKind::UrlNotAllowed
    );
    assert_eq!(
        client
            .fetch_challenge(&format!("https://{}/protected", handle.addr()))
            .await
            .unwrap_err()
            .kind(),
        ClientErrorKind::UrlNotAllowed
    );
    assert_eq!(
        client
            .fetch_with_credentials("http://192.168.0.2/protected", "unused", "00",)
            .await
            .unwrap_err()
            .kind(),
        ClientErrorKind::UrlNotAllowed
    );

    handle.shutdown().await;
}

#[tokio::test]
async fn scenario_13_every_failure_has_the_same_response_shape() {
    let (handle, source) = spawn_fixture().await;
    let url = handle.protected_url();
    let client = L402Client::new().unwrap();

    let (macaroon, preimage, _invoice) = challenge_and_preimage(&client, &url, &source).await;

    let failures: Vec<Option<HeaderValue>> = vec![
        None,
        Some(HeaderValue::from_str(&format!("L402 {macaroon}:{}", "bb".repeat(32))).unwrap()),
        Some(HeaderValue::from_str(&format!("L402 {macaroon}")).unwrap()),
        Some(HeaderValue::from_str("Basic abc").unwrap()),
        Some(HeaderValue::from_str(&format!("L402 not-base64!:{preimage}")).unwrap()),
        Some(HeaderValue::from_str(&format!("L402 {}", "A".repeat(9_000))).unwrap()),
    ];

    let mut shapes = Vec::new();

    for header in failures {
        let (status, headers, body) = send_raw_header(&url, header).await;
        let names: Vec<String> = headers
            .keys()
            .map(|name| name.as_str().to_ascii_lowercase())
            .collect();
        shapes.push((status, names, body));
    }

    let first = &shapes[0];
    for shape in &shapes {
        assert_eq!(shape, first, "a failure kind produced a different shape");
    }
    assert_eq!(first.0, 402);
    assert_eq!(first.2, "{\"error\":\"payment required\"}");

    handle.shutdown().await;
}

fn hex_to_bytes(hex: &str) -> Vec<u8> {
    hex.as_bytes()
        .chunks(2)
        .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
        .collect()
}
