//! Checksum and minisign signature verification for a downloaded release
//! asset, against the `SHA256SUMS` file and its `.minisig` signature.

use std::collections::HashMap;

use minisign_verify::{PublicKey, Signature};
use sha2::{Digest, Sha256};

use crate::{Error, Result};

pub fn sha256_hex(data: &[u8]) -> String {
    let mut h = Sha256::new();
    h.update(data);
    hex(&h.finalize())
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// `sha256sum`-format lines: `<hex>  <filename>`.
fn parse_sums(text: &str) -> HashMap<&str, &str> {
    text.lines()
        .filter_map(|l| l.split_once(char::is_whitespace))
        .map(|(hash, name)| (name.trim(), hash.trim()))
        .collect()
}

pub fn verify_checksum(sums: &[u8], filename: &str, data: &[u8]) -> Result<()> {
    let sums_text = String::from_utf8_lossy(sums);
    let table = parse_sums(&sums_text);
    let expected = table.get(filename).ok_or_else(|| Error::ChecksumMissing {
        name: filename.into(),
    })?;
    let actual = sha256_hex(data);
    if actual.eq_ignore_ascii_case(expected) {
        Ok(())
    } else {
        Err(Error::ChecksumMismatch {
            name: filename.into(),
            expected: expected.to_string(),
            actual,
        })
    }
}

pub fn verify_signature(sums: &[u8], sig: &[u8], public_key_b64: &str) -> Result<()> {
    let pk = PublicKey::from_base64(public_key_b64)
        .map_err(|e| Error::SignatureInvalid(e.to_string()))?;
    let sig_text = String::from_utf8_lossy(sig);
    let signature =
        Signature::decode(&sig_text).map_err(|e| Error::SignatureInvalid(e.to_string()))?;
    pk.verify(sums, &signature, false)
        .map_err(|e| Error::SignatureInvalid(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checksum_matches_and_mismatches() {
        let data = b"hello world";
        let hash = sha256_hex(data);
        let sums = format!("{hash}  ehai-linux.tar.gz\nabc  other-file\n");
        assert!(verify_checksum(sums.as_bytes(), "ehai-linux.tar.gz", data).is_ok());

        let wrong = verify_checksum(sums.as_bytes(), "ehai-linux.tar.gz", b"tampered");
        assert!(matches!(wrong, Err(Error::ChecksumMismatch { .. })));

        let missing = verify_checksum(sums.as_bytes(), "nope.tar.gz", data);
        assert!(matches!(missing, Err(Error::ChecksumMissing { .. })));
    }

    // Real keypair and signature from an actual `minisign -G` / `minisign -S`
    // run (a disposable test key, not the shipped release key).
    const TEST_PUBLIC_KEY: &str = "RWRPFNtEDrilkPDM9qUzffLRm1RI8Z8THD6r9yJLtmBBAgAFqY2ndGGr";
    const TEST_SUMS: &str =
        "ef95342fe3b460aa24b114b79cfd489100afeb09bde254ecc378320242a3ecf8  ehai-test\n";
    const TEST_SIG: &str = "untrusted comment: signature from minisign secret key\nRURPFNtEDrilkHyTK/v/sguKgLO3ivO4+nzX9xlUKkTqb0XrUwdD76t6hY0dH2xvkWW2/gCTke1XwTkv0rG6yXzfzhcDbRq/EgI=\ntrusted comment: timestamp:1794940344\tfile:test_sums.txt\thashed\noGhJxZUS6IW73uEXriKs2pvpsAxQd3ENCrdXcxXo3MOliiExoOl94X78WuJv2zyS9iQ8wVK9hXT4u6LCyWlwAA==\n";

    #[test]
    fn verifies_a_real_minisign_signature() {
        verify_signature(TEST_SUMS.as_bytes(), TEST_SIG.as_bytes(), TEST_PUBLIC_KEY).unwrap();
    }

    #[test]
    fn rejects_a_tampered_sums_file() {
        let tampered = TEST_SUMS.replace("ehai-test", "ehai-evil");
        let err = verify_signature(tampered.as_bytes(), TEST_SIG.as_bytes(), TEST_PUBLIC_KEY);
        assert!(matches!(err, Err(Error::SignatureInvalid(_))), "{err:?}");
    }

    #[test]
    fn rejects_the_wrong_public_key() {
        // a different, unrelated real minisign public key
        let other_key = "RWTKHW5dGCGJOywz9PXOM27FdVwe+Eqj9qrgNrFh0faYS9LKS0kL6uhC";
        let err = verify_signature(TEST_SUMS.as_bytes(), TEST_SIG.as_bytes(), other_key);
        assert!(matches!(err, Err(Error::SignatureInvalid(_))), "{err:?}");
    }
}
