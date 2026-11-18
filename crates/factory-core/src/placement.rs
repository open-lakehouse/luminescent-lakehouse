//! Where each session of a graph runs.
//!
//! An environment offers capability labels; a session needs the union of its
//! task's, repo's and profile's requirements, and may only run where every
//! one is offered. Within that, the first rule that names an environment
//! wins:
//!
//! 1. the run's environment (`StartRun`), strict;
//! 2. the submission's choice for the node, strict;
//! 3. the submission's environment, a preference: if it doesn't fit, the
//!    defaults below are tried and the placement carries a note;
//! 4. the profile's default environment;
//! 5. the registry's default environment;
//! 6. the first eligible environment, in registry order.
//!
//! A default (4, 5) that doesn't fit is skipped. `decide` at any step leaves
//! the pick to the Decider, among the eligible environments. See
//! docs/factory/execution.md.

use std::collections::BTreeSet;

use factory_proto::workgraph::v1 as wg;

use crate::registry::{DECIDE, Registry};
use crate::schema;

/// A session a graph would start: whose node, in which role, on which
/// profile, needing what.
struct Session<'a> {
    node: &'a str,
    role: &'static str,
    profile: String,
    /// Requirements from the node and its repos; the profile's are added
    /// once it's resolved.
    requires: BTreeSet<String>,
    /// Set when the node's own data is incomplete (an unknown repo).
    problem: Option<String>,
}

fn is_resolved(node: &wg::Node) -> bool {
    matches!(
        node.status.as_option().and_then(|s| s.phase.as_known()),
        Some(wg::Phase::PHASE_DONE | wg::Phase::PHASE_DROPPED)
    )
}

fn or_default(name: &str, default: &str) -> String {
    if name.is_empty() { default } else { name }.to_owned()
}

/// The sessions `graph` would start: the planner on its root, and the
/// executor and reviewer of every open Task.
fn sessions<'a>(registry: &Registry, graph: &'a wg::Graph) -> Vec<Session<'a>> {
    let defaults = registry.defaults();
    let mut out = Vec::new();
    for node in &graph.nodes {
        let Some(any) = node.spec.as_option().and_then(|s| s.payload.as_option()) else {
            continue;
        };
        let repo_needs = |slug: &str, requires: &mut BTreeSet<String>| match registry.repo(slug) {
            Some(r) => {
                requires.extend(r.requires.iter().cloned());
                None
            }
            None => Some(format!("repo {slug} isn't in the registry")),
        };
        match node.kind.as_str() {
            schema::PLAN if node.r#ref == graph.root => {
                let Ok(plan) = any.unpack_unchecked::<wg::Plan>() else {
                    continue;
                };
                let mut requires = BTreeSet::new();
                let mut problem = None;
                for repo in &plan.repos {
                    // The planner reads every repo the plan targets.
                    requires.extend(repo.requires.iter().cloned());
                    if let Some(p) = repo_needs(&repo.slug, &mut requires) {
                        problem.get_or_insert(p);
                    }
                }
                out.push(Session {
                    node: &node.r#ref,
                    role: "planner",
                    profile: or_default(&plan.planner_profile, &defaults.planner),
                    requires,
                    problem,
                });
            }
            schema::TASK if !is_resolved(node) => {
                let Ok(task) = any.unpack_unchecked::<wg::Task>() else {
                    continue;
                };
                let mut requires: BTreeSet<String> = task.requires.iter().cloned().collect();
                let problem = if task.repo.is_empty() {
                    None
                } else {
                    repo_needs(&task.repo, &mut requires)
                };
                for (role, profile, default) in [
                    ("executor", &task.profile, &defaults.executor),
                    ("reviewer", &task.review_profile, &defaults.reviewer),
                ] {
                    out.push(Session {
                        node: &node.r#ref,
                        role,
                        profile: or_default(profile, default),
                        requires: requires.clone(),
                        problem: problem.clone(),
                    });
                }
            }
            _ => {}
        }
    }
    out
}

fn missing(env: &wg::Environment, requires: &BTreeSet<String>) -> Vec<String> {
    requires
        .iter()
        .filter(|r| !env.capabilities.contains(r))
        .cloned()
        .collect()
}

/// Place one session. `choices` are the rules in order, each with whether
/// it's strict.
fn place(
    registry: &Registry,
    session: Session<'_>,
    rules: &[(wg::PlacementSource, &str, Strictness)],
) -> wg::NodePlacement {
    let mut out = wg::NodePlacement {
        node: session.node.to_owned(),
        role: session.role.to_owned(),
        profile: session.profile.clone(),
        ..Default::default()
    };
    let mut requires = session.requires;
    let profile = registry.profile(&session.profile);
    match profile {
        Some(p) => requires.extend(p.requires.iter().cloned()),
        None => {
            out.problem = if session.profile.is_empty() {
                format!(
                    "no {} profile, and the registry names no default",
                    session.role
                )
            } else {
                format!("profile {} isn't defined", session.profile)
            };
        }
    }
    out.requires = requires.iter().cloned().collect();
    out.eligible = registry
        .environments()
        .iter()
        .filter(|e| missing(e, &requires).is_empty())
        .map(|e| e.name.clone())
        .collect();
    if let Some(p) = session.problem {
        out.problem = p;
    }
    if !out.problem.is_empty() {
        return out;
    }

    let profile_env = profile.map_or("", |p| p.environment.as_str());
    let defaults = [
        (
            wg::PlacementSource::PLACEMENT_SOURCE_PROFILE,
            profile_env,
            Strictness::Default,
        ),
        (
            wg::PlacementSource::PLACEMENT_SOURCE_REGISTRY,
            registry.defaults().environment.as_str(),
            Strictness::Default,
        ),
    ];
    let source = |out: &mut wg::NodePlacement, s: wg::PlacementSource| out.source = s.into();
    for &(rule, name, strictness) in rules.iter().chain(defaults.iter()) {
        if name.is_empty() {
            continue;
        }
        if name == DECIDE {
            source(&mut out, wg::PlacementSource::PLACEMENT_SOURCE_DECIDE);
            if out.eligible.is_empty() {
                out.problem = no_environment(&requires);
            }
            return out;
        }
        let Some(env) = registry.environment(name) else {
            if strictness == Strictness::Preference {
                out.note = format!("environment {name} isn't defined");
                continue;
            }
            out.problem = format!("environment {name} isn't defined");
            return out;
        };
        let lacks = missing(env, &requires);
        if lacks.is_empty() {
            out.environment = env.name.clone();
            source(&mut out, rule);
            return out;
        }
        let why = format!("{name} lacks {}", lacks.join(", "));
        match strictness {
            Strictness::Strict => {
                out.problem = why;
                return out;
            }
            Strictness::Preference => out.note = why,
            Strictness::Default => {}
        }
    }
    match out.eligible.first() {
        Some(first) => {
            out.environment = first.clone();
            source(&mut out, wg::PlacementSource::PLACEMENT_SOURCE_ELIGIBLE);
        }
        None => out.problem = no_environment(&requires),
    }
    out
}

fn no_environment(requires: &BTreeSet<String>) -> String {
    let needs: Vec<&str> = requires.iter().map(String::as_str).collect();
    format!("no environment provides {}", needs.join(", "))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Strictness {
    /// A per-node or per-run choice: a miss is a problem.
    Strict,
    /// The submission's environment: a miss falls through, with a note.
    Preference,
    /// A profile's or the registry's default: a miss falls through quietly.
    Default,
}

/// Where every session of `graph` would run under `choice`, with `run`
/// overriding the environment of one node's sessions (`StartRun`).
pub fn resolve(
    registry: &Registry,
    graph: &wg::Graph,
    choice: &wg::PlacementChoice,
    run: Option<(&str, &str)>,
) -> Vec<wg::NodePlacement> {
    sessions(registry, graph)
        .into_iter()
        .map(|s| {
            let run_env = run.filter(|(n, _)| *n == s.node).map_or("", |(_, e)| e);
            let node_env = choice.nodes.get(s.node).map_or("", String::as_str);
            let rules = [
                (
                    wg::PlacementSource::PLACEMENT_SOURCE_RUN,
                    run_env,
                    Strictness::Strict,
                ),
                (
                    wg::PlacementSource::PLACEMENT_SOURCE_NODE,
                    node_env,
                    Strictness::Strict,
                ),
                (
                    wg::PlacementSource::PLACEMENT_SOURCE_SUBMISSION,
                    choice.environment.as_str(),
                    Strictness::Preference,
                ),
            ];
            place(registry, s, &rules)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::payload;

    const REGISTRY: &str = "
defaults: { executor: exec, reviewer: review, planner: plan, environment: local }
repos:
  - { slug: o/public }
  - { slug: o/private, requires: [net:corp] }
environments:
  - { name: local, placement: { host: local }, capabilities: [gh] }
  - { name: corp, placement: { host: corp-box }, capabilities: [gh, net:corp] }
  - { name: gpu, placement: { managed: { provider: example, sandbox_profile: gpu } }, capabilities: [gh, gpu] }
profiles:
  exec: { agent: e, requires: [gh] }
  review: { agent: r, environment: decide }
  plan: { agent: p }
  sandboxed: { agent: e, environment: gpu }
";

    fn node(r: &str, kind: &str, payload: wg::NodeSpec) -> wg::Node {
        wg::Node {
            r#ref: r.into(),
            kind: kind.into(),
            spec: payload.into(),
            ..Default::default()
        }
    }

    fn task(r: &str, t: wg::Task) -> wg::Node {
        node(r, schema::TASK, payload::spec(r, "", &t))
    }

    fn graph(nodes: Vec<wg::Node>) -> wg::Graph {
        let mut all = vec![node(
            "root",
            schema::PLAN,
            payload::spec(
                "P",
                "",
                &wg::Plan {
                    repos: vec![wg::Repo {
                        slug: "o/public".into(),
                        ..Default::default()
                    }],
                    ..Default::default()
                },
            ),
        )];
        all.extend(nodes);
        wg::Graph {
            root: "root".into(),
            nodes: all,
            ..Default::default()
        }
    }

    fn find<'a>(ps: &'a [wg::NodePlacement], node: &str, role: &str) -> &'a wg::NodePlacement {
        ps.iter()
            .find(|p| p.node == node && p.role == role)
            .unwrap_or_else(|| panic!("no {role} placement for {node}"))
    }

    fn src(p: &wg::NodePlacement) -> wg::PlacementSource {
        p.source.as_known().unwrap()
    }

    fn public() -> wg::Task {
        wg::Task {
            repo: "o/public".into(),
            ..Default::default()
        }
    }

    #[test]
    fn defaults_place_everything_locally() {
        let r = Registry::from_yaml(REGISTRY).unwrap();
        let ps = resolve(
            &r,
            &graph(vec![task("a", public())]),
            &Default::default(),
            None,
        );
        assert_eq!(ps.len(), 3, "planner, executor, reviewer");
        let exec = find(&ps, "a", "executor");
        assert_eq!(exec.environment, "local");
        assert_eq!(src(exec), wg::PlacementSource::PLACEMENT_SOURCE_REGISTRY);
        assert_eq!(exec.requires, vec!["gh"]);
        let review = find(&ps, "a", "reviewer");
        assert_eq!(src(review), wg::PlacementSource::PLACEMENT_SOURCE_DECIDE);
        assert_eq!(review.environment, "");
        assert_eq!(review.eligible, vec!["local", "corp", "gpu"]);
        assert_eq!(find(&ps, "root", "planner").environment, "local");
    }

    #[test]
    fn requirements_route_past_defaults() {
        let r = Registry::from_yaml(REGISTRY).unwrap();
        let private = wg::Task {
            repo: "o/private".into(),
            ..Default::default()
        };
        let ps = resolve(
            &r,
            &graph(vec![task("a", private)]),
            &Default::default(),
            None,
        );
        let exec = find(&ps, "a", "executor");
        assert_eq!(
            exec.environment, "corp",
            "local can't reach the private repo"
        );
        assert_eq!(src(exec), wg::PlacementSource::PLACEMENT_SOURCE_ELIGIBLE);
        assert_eq!(exec.eligible, vec!["corp"]);
    }

    #[test]
    fn the_submission_is_a_preference() {
        let r = Registry::from_yaml(REGISTRY).unwrap();
        let needs_gpu = wg::Task {
            requires: vec!["gpu".into()],
            ..public()
        };
        let choice = wg::PlacementChoice {
            environment: "corp".into(),
            ..Default::default()
        };
        let ps = resolve(
            &r,
            &graph(vec![task("a", public()), task("b", needs_gpu)]),
            &choice,
            None,
        );
        let a = find(&ps, "a", "executor");
        assert_eq!(
            (a.environment.as_str(), src(a)),
            ("corp", wg::PlacementSource::PLACEMENT_SOURCE_SUBMISSION)
        );
        let b = find(&ps, "b", "executor");
        assert_eq!(b.environment, "gpu");
        assert_eq!(b.note, "corp lacks gpu");
        assert!(b.problem.is_empty());
    }

    #[test]
    fn node_and_run_choices_are_strict() {
        let r = Registry::from_yaml(REGISTRY).unwrap();
        let choice = wg::PlacementChoice {
            nodes: [("a".to_owned(), "gpu".to_owned())].into(),
            ..Default::default()
        };
        let g = graph(vec![task(
            "a",
            wg::Task {
                repo: "o/private".into(),
                ..Default::default()
            },
        )]);
        let ps = resolve(&r, &g, &choice, None);
        assert_eq!(find(&ps, "a", "executor").problem, "gpu lacks net:corp");

        let ps = resolve(&r, &g, &Default::default(), Some(("a", "corp")));
        let exec = find(&ps, "a", "executor");
        assert_eq!(
            (exec.environment.as_str(), src(exec)),
            ("corp", wg::PlacementSource::PLACEMENT_SOURCE_RUN)
        );
        let ps = resolve(&r, &g, &Default::default(), Some(("a", "nowhere")));
        assert_eq!(
            find(&ps, "a", "executor").problem,
            "environment nowhere isn't defined"
        );
    }

    #[test]
    fn a_profile_default_beats_the_registry_default() {
        let r = Registry::from_yaml(REGISTRY).unwrap();
        let t = wg::Task {
            profile: "sandboxed".into(),
            ..public()
        };
        let ps = resolve(&r, &graph(vec![task("a", t)]), &Default::default(), None);
        let exec = find(&ps, "a", "executor");
        assert_eq!(
            (exec.environment.as_str(), src(exec)),
            ("gpu", wg::PlacementSource::PLACEMENT_SOURCE_PROFILE)
        );
    }

    #[test]
    fn what_cant_be_placed_says_why() {
        let r = Registry::from_yaml(REGISTRY).unwrap();
        let ps = resolve(
            &r,
            &graph(vec![
                task(
                    "a",
                    wg::Task {
                        requires: vec!["tpu".into()],
                        ..public()
                    },
                ),
                task(
                    "b",
                    wg::Task {
                        repo: "o/unknown".into(),
                        ..Default::default()
                    },
                ),
                task(
                    "c",
                    wg::Task {
                        profile: "missing".into(),
                        ..public()
                    },
                ),
            ]),
            &Default::default(),
            None,
        );
        assert_eq!(
            find(&ps, "a", "executor").problem,
            "no environment provides gh, tpu"
        );
        assert_eq!(
            find(&ps, "a", "reviewer").problem,
            "no environment provides tpu",
            "decide with nothing eligible"
        );
        assert_eq!(
            find(&ps, "b", "executor").problem,
            "repo o/unknown isn't in the registry"
        );
        assert_eq!(
            find(&ps, "c", "executor").problem,
            "profile missing isn't defined"
        );
    }

    #[test]
    fn resolved_tasks_and_other_kinds_start_nothing() {
        let r = Registry::from_yaml(REGISTRY).unwrap();
        let mut done = task("a", public());
        done.status = wg::NodeStatus {
            phase: wg::Phase::PHASE_DONE.into(),
            ..Default::default()
        }
        .into();
        let gate = node(
            "g",
            schema::GATE,
            payload::spec("g", "", &wg::Gate::default()),
        );
        let ps = resolve(&r, &graph(vec![done, gate]), &Default::default(), None);
        assert_eq!(ps.len(), 1, "only the planner");
    }
}
