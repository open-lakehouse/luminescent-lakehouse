//! API keys: `olk_<id>_<secret>`.
//!
//! `id` is 8 random bytes and the secret 32, both lowercase hex. `authz.api_keys`
//! stores the id and the SHA-256 of the whole key string, never the key. The
//! services verify keys through `olai-auth`, which follows [`verify`]: look the
//! id up, compare the digest in constant time, and accept the key only while it
//! and its service account are live.

use anyhow::bail;
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;

const PREFIX: &str = "olk_";

/// A freshly generated key. [`Self::key`] is shown once, then discarded.
pub struct NewKey {
    pub id: String,
    pub key: String,
    pub sha256: [u8; 32],
}

/// Generate a key from the OS's randomness.
pub fn generate() -> anyhow::Result<NewKey> {
    let mut id = [0u8; 8];
    let mut secret = [0u8; 32];
    getrandom::fill(&mut id).map_err(|e| anyhow::anyhow!("no randomness: {e}"))?;
    getrandom::fill(&mut secret).map_err(|e| anyhow::anyhow!("no randomness: {e}"))?;
    let id = hex::encode(id);
    let key = format!("{PREFIX}{id}_{}", hex::encode(secret));
    let sha256 = digest(&key);
    Ok(NewKey { id, key, sha256 })
}

/// The id of a well-formed key.
pub fn parse(key: &str) -> anyhow::Result<&str> {
    let Some(rest) = key.strip_prefix(PREFIX) else {
        bail!("not an API key");
    };
    let Some((id, secret)) = rest.split_once('_') else {
        bail!("not an API key");
    };
    let hex =
        |s: &str, len| s.len() == len && s.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'));
    if !hex(id, 16) || !hex(secret, 64) {
        bail!("not an API key");
    }
    Ok(id)
}

/// SHA-256 of the whole key string.
pub fn digest(key: &str) -> [u8; 32] {
    Sha256::digest(key.as_bytes()).into()
}

/// Whether `key` hashes to `stored`, in constant time.
pub fn matches(key: &str, stored: &[u8]) -> bool {
    digest(key).ct_eq(stored).into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_keys_parse_and_match() {
        let k = generate().unwrap();
        assert_eq!(parse(&k.key).unwrap(), k.id);
        assert!(matches(&k.key, &k.sha256));
        assert!(!matches(
            &k.key.replace(&k.id, "0000000000000000"),
            &k.sha256
        ));
        assert_ne!(generate().unwrap().key, k.key);
    }

    #[test]
    fn rejects_malformed_keys() {
        let good = format!("olk_{}_{}", "a".repeat(16), "b".repeat(64));
        assert!(parse(&good).is_ok());
        for bad in [
            "",
            "olk_",
            &good.replacen("olk_", "olx_", 1),
            &good[..good.len() - 1],
            &good.to_uppercase(),
            &format!("olk_{}_{}", "a".repeat(15), "b".repeat(65)),
            &format!("{good}_extra"),
        ] {
            assert!(parse(bad).is_err(), "{bad}");
        }
    }
}
