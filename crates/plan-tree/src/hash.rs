use sha2::{Digest, Sha256};

use crate::tree::TreeError;

const NUL: u8 = 0;

/// SHA-256, hex, over `parts` joined with NUL: a node's own hash.
///
/// A part that contains NUL is rejected rather than escaped, which keeps the
/// construction identical to docs-factory's `sha256(...parts)`.
pub fn hash_parts<I, S>(parts: I) -> Result<String, TreeError>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let mut hasher = Sha256::new();
    for (i, part) in parts.into_iter().enumerate() {
        let part = part.as_ref();
        if part.as_bytes().contains(&NUL) {
            return Err(TreeError::NulInPart { index: i });
        }
        if i > 0 {
            hasher.update([NUL]);
        }
        hasher.update(part.as_bytes());
    }
    Ok(hex::encode(hasher.finalize()))
}

/// A node's subtree hash: `sha256(node_hash, ...child_subtree_hashes)`, or the
/// node hash itself for a leaf.
pub fn fold_subtree<S: AsRef<str>>(node_hash: &str, child_subtree_hashes: &[S]) -> String {
    if child_subtree_hashes.is_empty() {
        return node_hash.to_owned();
    }
    let mut hasher = Sha256::new();
    hasher.update(node_hash.as_bytes());
    for child in child_subtree_hashes {
        hasher.update([NUL]);
        hasher.update(child.as_ref().as_bytes());
    }
    hex::encode(hasher.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_node_crypto_construction() {
        // sha256("doc"), as docs-factory's doc root.
        assert_eq!(
            hash_parts(["doc"]).unwrap(),
            "139d544b821b13ebea14f1b0fe18577222e415c2966e3a3511c4196055232202"
        );
    }

    #[test]
    fn rejects_nul() {
        assert!(matches!(
            hash_parts(["a", "b\0c"]),
            Err(TreeError::NulInPart { index: 1 })
        ));
    }

    #[test]
    fn leaf_subtree_is_node_hash() {
        assert_eq!(fold_subtree::<&str>("abc", &[]), "abc");
        assert_ne!(fold_subtree("abc", &["d"]), "abc");
    }
}
