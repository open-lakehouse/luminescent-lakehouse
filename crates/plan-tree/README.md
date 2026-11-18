# plan-tree

Structural Merkle trees and a stable-key diff between two of them.

A caller describes a tree as nodes with a stable key, a kind, a label and its
own content as ordered string parts. `build` hashes it bottom-up:

- `node_hash = sha256(parts joined with NUL)`, hex, kind tag first
- `subtree_hash = sha256(node_hash, …child subtree hashes)`, or `node_hash`
  for a leaf

`diff` matches two trees by key and classifies each changed node as `added`,
`removed`, `modified`, `modified-descendants` or `moved`; `compact` keeps only
review-level changes. Children are either ordered (a changed position is a
move) or a set sorted by key (only a new parent is a move).

The construction, the JSON shape (`MerkleNodeJson`: camelCase `nodeHash`,
`subtreeHash`, flattened attributes) and the diff follow docs-factory's
`site/src/content-core/tree.mjs` and `server/src/tree-diff.ts`.
`tests/docs_factory_golden.rs` checks trees hashed there; see that file for
how to refresh its fixture.

Unlike docs-factory, a moved node whose own content also changed carries
`contentChanged: true`, and moves carry `fromParent` and `toParent`.

The crate is pure and synchronous, so it builds for wasm as it is.
