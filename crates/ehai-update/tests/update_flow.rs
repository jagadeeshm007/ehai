//! `check` + `stage` against a fake GitHub Releases API (wiremock), using a
//! real archive + a real minisign signature from a throwaway test keypair —
//! not the production release key, so this never needs network or secrets.

use base64::Engine;
use ehai_core::config::Channel;
use ehai_update::{check, stage, CheckResult};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const ARCHIVE_B64: &str = "H4sIAAAAAAAA/+3HwQmAMAxA0Z6dogsEK6LOEyXFIrSQpqDbqwdxgx407/Q/rRjAh10KU0vXmHrcox/fdu7uyQ2dqaJkQbbW/JTHjWAOEfmAJUWhKBl8YhDK0hillFIfdQJm5FkEAAgAAA==";
const TEST_PUBLIC_KEY: &str = "RWRPFNtEDrilkPDM9qUzffLRm1RI8Z8THD6r9yJLtmBBAgAFqY2ndGGr";

/// Every target `ehai_update::current_target()` can return (spec's build
/// matrix) — one `SHA256SUMS` line per target, all pointing at the same
/// archive bytes, so the suite verifies correctly on whichever OS/arch CI
/// actually runs it on, not just the one this fixture happened to be
/// generated on.
const TARGETS: &[&str] = &[
    "x86_64-unknown-linux-musl",
    "aarch64-unknown-linux-musl",
    "x86_64-apple-darwin",
    "aarch64-apple-darwin",
    "x86_64-pc-windows-msvc",
];

fn asset_name(target: &str) -> String {
    format!("ehai-fixture-{target}.tar.gz")
}

// One SHA256SUMS line per TARGETS entry (same hash — same archive bytes for
// all), signed once with a throwaway test keypair (`minisign -G`/`-S`), not
// the production release key.
const SUMS: &str = "d1e7ee682e285d28700a8f6352988dddc16c18ea7009d68a4a96edc2d9a70aa5  ehai-fixture-x86_64-unknown-linux-musl.tar.gz\nd1e7ee682e285d28700a8f6352988dddc16c18ea7009d68a4a96edc2d9a70aa5  ehai-fixture-aarch64-unknown-linux-musl.tar.gz\nd1e7ee682e285d28700a8f6352988dddc16c18ea7009d68a4a96edc2d9a70aa5  ehai-fixture-x86_64-apple-darwin.tar.gz\nd1e7ee682e285d28700a8f6352988dddc16c18ea7009d68a4a96edc2d9a70aa5  ehai-fixture-aarch64-apple-darwin.tar.gz\nd1e7ee682e285d28700a8f6352988dddc16c18ea7009d68a4a96edc2d9a70aa5  ehai-fixture-x86_64-pc-windows-msvc.tar.gz\n";
const SIG: &str = "untrusted comment: signature from minisign secret key\nRURPFNtEDrilkFNy0rbjpAtg9ZNwVWqH2+dXW+lBQ93chzlkyXwDlDQbjwPLp57RvN959CzOqAOASWMebZRT0YDTKqHZuu+OJgA=\ntrusted comment: timestamp:1794943763\tfile:SHA256SUMS\thashed\nBnvEMRCjg6zQgP48yhgLuGEjOtBBJNu9u6EsdiEmjnaKPH+WyFYbOqdv8OjNvixcc07OPfOBouSNWsCqWM/qDQ==\n";

fn archive_bytes() -> Vec<u8> {
    base64::engine::general_purpose::STANDARD
        .decode(ARCHIVE_B64)
        .unwrap()
}

fn assets_for(server: &MockServer) -> serde_json::Value {
    let mut assets: Vec<serde_json::Value> = TARGETS
        .iter()
        .map(|t| {
            serde_json::json!({ "name": asset_name(t), "url": format!("{}/download/archive", server.uri()) })
        })
        .collect();
    assets.push(serde_json::json!({ "name": "SHA256SUMS", "url": format!("{}/download/sums", server.uri()) }));
    assets.push(serde_json::json!({ "name": "SHA256SUMS.minisig", "url": format!("{}/download/sig", server.uri()) }));
    serde_json::Value::Array(assets)
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
