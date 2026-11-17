//! `check` + `stage` against a fake GitHub Releases API (wiremock), using a
//! real archive + a real minisign signature from a throwaway test keypair —
//! not the production release key, so this never needs network or secrets.

use base64::Engine;
use ehai_core::config::Channel;
use ehai_update::{check, stage, CheckResult};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const ARCHIVE_B64: &str = "H4sIAAAAAAAA/+3HwQmAMAxA0Z6dogsEK6LOEyXFIrSQpqDbqwdxgx407/Q/rRjAh10KU0vXmHrcox/fdu7uyQ2dqaJkQbbW/JTHjWAOEfmAJUWhKBl8YhDK0hillFIfdQJm5FkEAAgAAA==";
const ASSET_NAME: &str = "ehai-fixture-x86_64-unknown-linux-musl.tar.gz";
const SUMS: &str = "d1e7ee682e285d28700a8f6352988dddc16c18ea7009d68a4a96edc2d9a70aa5  ehai-fixture-x86_64-unknown-linux-musl.tar.gz\n";
const SIG: &str = "untrusted comment: signature from minisign secret key\nRURPFNtEDrilkCSuLrfz08efY1bgDHCEOEV2aS8bcd3vj/PRntI61mwZoxoiB7lU+JAkZDhf1Ycy/r1wVcGdDNjx76RPA4lq/gs=\ntrusted comment: timestamp:1794940580\tfile:fixture_sums.txt\thashed\nlfe5jNqnt/xRxk3bwBfSrbfNUKPa7eoH1pVlkVO8tc0c5G/fxWd48QFxUaLYx4EPkvyZV9K1ZgtXZleZ0+F9DQ==\n";
const TEST_PUBLIC_KEY: &str = "RWRPFNtEDrilkPDM9qUzffLRm1RI8Z8THD6r9yJLtmBBAgAFqY2ndGGr";

fn archive_bytes() -> Vec<u8> {
    base64::engine::general_purpose::STANDARD
        .decode(ARCHIVE_B64)
        .unwrap()
}

fn assets_for(server: &MockServer) -> serde_json::Value {
    serde_json::json!([
        { "name": ASSET_NAME, "browser_download_url": format!("{}/download/archive", server.uri()) },
        { "name": "SHA256SUMS", "browser_download_url": format!("{}/download/sums", server.uri()) },
        { "name": "SHA256SUMS.minisig", "browser_download_url": format!("{}/download/sig", server.uri()) },
    ])
}

/// Mounts `/releases` (tagged `v0.2.0`, pointing back at this same server's
/// own download endpoints) plus the three download endpoints.
async fn mock_server() -> MockServer {
    let server = MockServer::start().await;
    let assets = assets_for(&server);
    let releases = serde_json::json!([{ "tag_name": "v0.2.0", "draft": false, "assets": assets }]);
    Mock::given(method("GET"))
        .and(path("/releases"))
        .respond_with(ResponseTemplate::new(200).set_body_json(releases))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/download/archive"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(archive_bytes()))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/download/sums"))
        .respond_with(ResponseTemplate::new(200).set_body_string(SUMS))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/download/sig"))
        .respond_with(ResponseTemplate::new(200).set_body_string(SIG))
        .mount(&server)
        .await;
    server
}

#[tokio::test]
async fn check_finds_a_newer_release_then_stage_verifies_and_extracts() {
    let server = mock_server().await;
    let client = ehai_update::Client::new(&server.uri(), None).unwrap();
    let found = check(&client, Channel::Stable, "0.1.0").await.unwrap();
    let release = match found {
        CheckResult::Available(r) => r,
        CheckResult::UpToDate => panic!("v0.2.0 should be newer than 0.1.0"),
    };
    assert_eq!(release.tag_name, "v0.2.0");

    let binary = stage(&client, &release, TEST_PUBLIC_KEY).await.unwrap();
    assert_eq!(binary, b"fake-binary-contents-for-test\n");
}

#[tokio::test]
async fn stage_rejects_a_tampered_archive() {
    let server = MockServer::start().await;
    let assets = assets_for(&server);
    Mock::given(method("GET"))
        .and(path("/download/archive"))
        // wrong bytes: checksum in SUMS won't match
        .respond_with(ResponseTemplate::new(200).set_body_bytes(b"not-the-real-archive".to_vec()))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/download/sums"))
        .respond_with(ResponseTemplate::new(200).set_body_string(SUMS))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/download/sig"))
        .respond_with(ResponseTemplate::new(200).set_body_string(SIG))
        .mount(&server)
        .await;

    let client = ehai_update::Client::new(&server.uri(), None).unwrap();
    let release = ehai_update::Release {
        tag_name: "v0.2.0".into(),
        draft: false,
        assets: serde_json::from_value(assets).unwrap(),
    };
    let err = stage(&client, &release, TEST_PUBLIC_KEY).await.unwrap_err();
    assert!(
        matches!(err, ehai_update::Error::ChecksumMismatch { .. }),
        "{err:?}"
    );
}

#[tokio::test]
async fn up_to_date_when_no_release_is_newer() {
    let server = mock_server().await;
    let client = ehai_update::Client::new(&server.uri(), None).unwrap();
    // the mocked release is tagged v0.2.0; a caller already on 0.2.0 or newer
    // gets UpToDate, not the same release again.
    let result = check(&client, Channel::Stable, "0.2.0").await.unwrap();
    assert_eq!(result, CheckResult::UpToDate);
}
