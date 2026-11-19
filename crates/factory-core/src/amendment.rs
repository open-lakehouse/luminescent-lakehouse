//! Changes to a plan while it executes.
//!
//! Plans change during execution: an agent discovers work, splits a task, or
//! reshapes what's left. That mustn't stop the plan, and most changes don't
//! need a person. When the current revision isn't the approved one, the
//! reconciler assesses each changed node with the extensions'
//! [`AmendmentPolicy`]s, in registration order, then [`DefaultAmendments`].
//! The first answer that isn't [`Assessment::Abstain`] decides, so a
//! composition can tighten the default (escalate more) or loosen it (accept
//! what the default would escalate).
//!
//! If every change is accepted, the reconciler approves the new revision
//! itself and the plan carries on. If one is escalated, a person is asked
//! (an `approval` item) and the changed nodes wait; the rest of the plan
//! keeps running.

use std::collections::HashSet;

use factory_proto::workgraph::v1 as wg;
use plan_tree::{Change, DiffEntry};

use crate::backend::Provenance;
use crate::executor::{self, payload};
use crate::registry::Registry;
use crate::schema::{PLAN, TASK};

/// What a policy sees about a change.
pub struct AmendmentContext<'a> {
    /// The plan as it is now (a removed node is gone from it).
    pub graph: &'a wg::Graph,
    pub runs: &'a [wg::Run],
    pub registry: &'a Registry,
    /// Who made the revisions since the approved one, oldest first.
    pub provenance: &'a [Provenance],
}

impl AmendmentContext<'_> {
    pub fn node(&self, key: &str) -> Option<&wg::Node> {
        self.graph.nodes.iter().find(|n| n.r#ref == key)
    }

    pub fn root(&self) -> Option<&wg::Node> {
        self.node(&self.graph.root)
    }
}

/// A policy's answer for one changed node.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Assessment {
    Accept,
    /// A person decides, and why.
    Escalate(String),
    /// No opinion: the next policy decides.
    Abstain,
}

/// Decides which changes to an executing plan go ahead on their own.
pub trait AmendmentPolicy: Send + Sync {
    fn name(&self) -> &str;
    /// The answer for one entry of the compact diff against the approved
    /// revision. `change.key` is the node's ref.
    fn assess(&self, change: &DiffEntry, cx: &AmendmentContext<'_>) -> Assessment;
}

/// The built-in default, consulted last. It accepts every change except:
///
/// - a Task on a repo that isn't one of the plan's `repos`;
/// - a change to the plan root's own content (its repos, its budget);
/// - removing a node that has a run, which would orphan its work.
pub struct DefaultAmendments;

impl AmendmentPolicy for DefaultAmendments {
    fn name(&self) -> &str {
        "default"
    }

    fn assess(&self, change: &DiffEntry, cx: &AmendmentContext<'_>) -> Assessment {
        if change.change == Change::Removed {
            return if cx.runs.iter().any(|r| r.node == change.key) {
                Assessment::Escalate(format!("removes {}, which has a run", change.label))
            } else {
                Assessment::Accept
            };
        }
        let own =
            change.content_changed || matches!(change.change, Change::Added | Change::Modified);
        if !own {
            return Assessment::Accept;
        }
        let Some(node) = cx.node(&change.key) else {
            return Assessment::Accept;
        };
        match node.kind.as_str() {
            PLAN if node.r#ref == cx.graph.root => {
                Assessment::Escalate("changes the plan's own goal, repos or budget".into())
            }
            TASK => {
                let task: wg::Task = payload(node);
                let plan: wg::Plan = cx.root().map(payload).unwrap_or_default();
                let allowed: HashSet<&str> = plan.repos.iter().map(|r| r.slug.as_str()).collect();
                if task.repo.is_empty()
                    || allowed.is_empty()
                    || allowed.contains(task.repo.as_str())
                {
                    Assessment::Accept
                } else {
                    Assessment::Escalate(format!(
                        "{} targets {}, which isn't one of the plan's repos",
                        node.spec.title, task.repo
                    ))
                }
            }
            _ => Assessment::Accept,
        }
    }
}

/// The outcome for a revision's changes.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Outcome {
    pub accepted: Vec<String>,
    /// (node ref, policy, why).
    pub escalated: Vec<(String, String, String)>,
}

impl Outcome {
    pub fn all_accepted(&self) -> bool {
        self.escalated.is_empty()
    }
}

/// Assess `changes` with `policies`, then the default.
pub fn assess(
    policies: &[Box<dyn AmendmentPolicy>],
    changes: &[DiffEntry],
    cx: &AmendmentContext<'_>,
) -> Outcome {
    let mut out = Outcome::default();
    let default = DefaultAmendments;
    for change in changes {
        if change.change == Change::ModifiedDescendants && !change.content_changed {
            // Its descendants are entries of their own.
            continue;
        }
        let decided = policies
            .iter()
            .map(AsRef::as_ref)
            .chain(std::iter::once(&default as &dyn AmendmentPolicy))
            .find_map(|p| match p.assess(change, cx) {
                Assessment::Abstain => None,
                a => Some((p.name().to_owned(), a)),
            });
        match decided {
            Some((policy, Assessment::Escalate(why))) => {
                out.escalated.push((change.key.clone(), policy, why));
            }
            _ => out.accepted.push(change.key.clone()),
        }
    }
    out
}

/// Whether a run of the node is live (keeps going against the approved
/// revision while a change waits).
pub fn has_live_run(runs: &[wg::Run], node: &str) -> bool {
    executor::latest_run(runs, node).is_some_and(|r| {
        matches!(
            r.state.as_known(),
            Some(
                wg::RunState::RUN_STATE_QUEUED
                    | wg::RunState::RUN_STATE_WORKING
                    | wg::RunState::RUN_STATE_INPUT_REQUIRED
            )
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::payload::pack;

    fn n(r: &str, kind: &str, payload: buffa_types::google::protobuf::Any) -> wg::Node {
        wg::Node {
            r#ref: r.into(),
            kind: kind.into(),
            spec: wg::NodeSpec {
                title: r.into(),
                payload: payload.into(),
                ..Default::default()
            }
            .into(),
            ..Default::default()
        }
    }

    fn entry(key: &str, change: Change) -> DiffEntry {
        DiffEntry {
            key: key.into(),
            kind: String::new(),
            change,
            label: key.into(),
            content_changed: matches!(change, Change::Modified),
            from_parent: None,
            to_parent: None,
        }
    }

    fn graph() -> wg::Graph {
        let plan = wg::Plan {
            repos: vec![wg::Repo {
                slug: "o/a".into(),
                ..Default::default()
            }],
            ..Default::default()
        };
        let task = |repo: &str| {
            pack(&wg::Task {
                repo: repo.into(),
                ..Default::default()
            })
        };
        wg::Graph {
            root: "root".into(),
            nodes: vec![
                n("root", PLAN, pack(&plan)),
                n("same", TASK, task("o/a")),
                n("other", TASK, task("o/b")),
                n("norepo", TASK, task("")),
            ],
            ..Default::default()
        }
    }

    struct LetOtherIn;
    impl AmendmentPolicy for LetOtherIn {
        fn name(&self) -> &str {
            "let-other-in"
        }
        fn assess(&self, c: &DiffEntry, _: &AmendmentContext<'_>) -> Assessment {
            if c.key == "other" {
                Assessment::Accept
            } else {
                Assessment::Abstain
            }
        }
    }

    struct Strict;
    impl AmendmentPolicy for Strict {
        fn name(&self) -> &str {
            "strict"
        }
        fn assess(&self, c: &DiffEntry, _: &AmendmentContext<'_>) -> Assessment {
            if c.change == Change::Added {
                Assessment::Escalate("no new work".into())
            } else {
                Assessment::Abstain
            }
        }
    }

    /// (changes, policies) → (accepted, escalated by whom).
    #[test]
    fn assessments() {
        let g = graph();
        let run = wg::Run {
            node: "gone".into(),
            ..Default::default()
        };
        let runs = [run];
        let reg = Registry::default();
        let cx = AmendmentContext {
            graph: &g,
            runs: &runs,
            registry: &reg,
            provenance: &[],
        };
        type Case = (
            &'static str,
            Vec<DiffEntry>,
            Vec<Box<dyn AmendmentPolicy>>,
            Vec<&'static str>,
            Vec<&'static str>,
        );
        let cases: Vec<Case> = vec![
            (
                "new work in the plan's repo",
                vec![entry("same", Change::Added)],
                vec![],
                vec!["same"],
                vec![],
            ),
            (
                "new work without a repo",
                vec![entry("norepo", Change::Added)],
                vec![],
                vec!["norepo"],
                vec![],
            ),
            (
                "another repo escalates",
                vec![entry("other", Change::Added)],
                vec![],
                vec![],
                vec!["other:default"],
            ),
            (
                "the root's content escalates",
                vec![entry("root", Change::Modified)],
                vec![],
                vec![],
                vec!["root:default"],
            ),
            (
                "the root's descendants don't",
                vec![entry("root", Change::ModifiedDescendants)],
                vec![],
                vec![],
                vec![],
            ),
            (
                "removing run work escalates",
                vec![entry("gone", Change::Removed)],
                vec![],
                vec![],
                vec!["gone:default"],
            ),
            (
                "removing idle work doesn't",
                vec![entry("idle", Change::Removed)],
                vec![],
                vec!["idle"],
                vec![],
            ),
            (
                "a move is accepted",
                vec![entry("same", Change::Moved)],
                vec![],
                vec!["same"],
                vec![],
            ),
            (
                "a policy loosens",
                vec![entry("other", Change::Added)],
                vec![Box::new(LetOtherIn)],
                vec!["other"],
                vec![],
            ),
            (
                "a policy tightens",
                vec![
                    entry("same", Change::Added),
                    entry("norepo", Change::Modified),
                ],
                vec![Box::new(Strict)],
                vec!["norepo"],
                vec!["same:strict"],
            ),
            (
                "first decisive answer wins",
                vec![entry("other", Change::Added)],
                vec![Box::new(LetOtherIn), Box::new(Strict)],
                vec!["other"],
                vec![],
            ),
        ];
        for (name, changes, policies, accepted, escalated) in cases {
            let out = assess(&policies, &changes, &cx);
            assert_eq!(out.accepted, accepted, "{name}");
            let esc: Vec<String> = out
                .escalated
                .iter()
                .map(|(k, p, _)| format!("{k}:{p}"))
                .collect();
            assert_eq!(esc, escalated, "{name}");
        }
    }
}
