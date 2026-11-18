//! Trees hashed by docs-factory (`site/src/content-core/tree.mjs`) verify
//! here, so docs-factory can adopt this crate without re-hashing what it has
//! stored. The fixture is a few entries of its generated
//! `content-versions.json`; to refresh it, run `just version-manifest` in
//! docs-factory and copy entries (slug, contentHash, rootHash, tree) over.

use plan_tree::{ChildOrder, MerkleNode, diff, hash_parts};
use serde::Deserialize;

#[derive(Deserialize)]
struct Fixture {
    versions: Vec<Version>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Version {
    slug: String,
    root_hash: String,
    tree: MerkleNode,
}

fn fixture() -> Fixture {
    serde_json::from_str(include_str!("fixtures/docs-factory-versions.json")).unwrap()
}

fn check_own_hashes(node: &MerkleNode) {
    match node.kind.as_str() {
        "doc" => assert_eq!(node.node_hash, hash_parts(["doc"]).unwrap()),
        "heading" => {
            let level = node.attrs["level"].as_u64().unwrap();
            assert_eq!(
                node.node_hash,
                hash_parts([format!("h{level}"), node.label.clone()]).unwrap(),
                "heading {}",
                node.key
            );
        }
        // Leaves hash text the tree doesn't carry (prose bodies, code).
        _ => {}
    }
    for child in &node.children {
        check_own_hashes(child);
    }
}

#[test]
fn subtree_hashes_fold_identically() {
    let f = fixture();
    assert!(!f.versions.is_empty());
    for v in &f.versions {
        assert_eq!(v.tree.subtree_hash, v.root_hash, "{}", v.slug);
        assert_eq!(v.tree.recompute_subtree_hash(), v.root_hash, "{}", v.slug);
        for n in v.tree.descendants() {
            assert_eq!(
                n.recompute_subtree_hash(),
                n.subtree_hash,
                "{} {}",
                v.slug,
                n.key
            );
        }
    }
}

#[test]
fn own_hashes_use_the_same_construction() {
    for v in &fixture().versions {
        check_own_hashes(&v.tree);
    }
}

#[test]
fn json_round_trips() {
    let raw: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/docs-factory-versions.json")).unwrap();
    for (v, raw) in fixture()
        .versions
        .iter()
        .zip(raw["versions"].as_array().unwrap())
    {
        assert_eq!(
            serde_json::to_value(&v.tree).unwrap(),
            raw["tree"],
            "{}",
            v.slug
        );
    }
}

#[test]
fn a_tree_diffs_empty_against_itself_and_full_against_nothing() {
    for v in &fixture().versions {
        assert!(diff(Some(&v.tree), Some(&v.tree), ChildOrder::Ordered).is_empty());
        assert_eq!(
            diff(None, Some(&v.tree), ChildOrder::Ordered).len(),
            v.tree.descendants().count()
        );
    }
}
