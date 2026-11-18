//! Structural Merkle trees and a stable-key diff between two of them.
//!
//! A caller describes a tree as [`NodeInput`]s: each node has a stable `key`,
//! a `kind`, a display `label`, and its own content as ordered string `parts`.
//! [`build`] hashes it bottom-up into [`MerkleNode`]s:
//!
//! - `node_hash` is SHA-256 (hex) over the parts joined with NUL. The caller
//!   puts a kind tag first, e.g. `["item", title, …]`.
//! - `subtree_hash` folds the node hash with its children's subtree hashes,
//!   and equals `node_hash` for a leaf.
//!
//! [`diff`] matches two trees by key and classifies every changed node;
//! [`compact`] reduces that to review-level changes.
//!
//! The construction, the JSON shape and the diff are those of docs-factory's
//! `site/src/content-core/tree.mjs` and `server/src/tree-diff.ts`, so trees
//! hashed there verify here. The crate knows nothing about what the nodes
//! mean: normalizing content into parts stays with the caller.

mod diff;
mod hash;
mod tree;

pub use diff::{Change, DiffEntry, compact, diff};
pub use hash::{fold_subtree, hash_parts};
pub use tree::{ChildOrder, MerkleNode, NodeInput, TreeError, build};
