//! The execution registry: the profiles sessions start from, the repos plans
//! target, and the environments sessions run in. Loaded from YAML (see
//! `crates/factory-core/registry.example.yaml`) and served read-only.
//!
//! A profile is a thin pointer into an Omnigent agent spec: the spec's name
//! plus the per-session overrides Omnigent accepts on create. Everything else
//! stays in the spec. Its environment is only a default: the submission
//! chooses where work runs, within what the work requires (see
//! [`crate::placement`] and docs/factory/execution.md).

use std::collections::{BTreeMap, HashSet};

use factory_proto::workgraph::v1 as wg;
use serde::Deserialize;

use crate::{Error, Result};

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct File {
    #[serde(default)]
    defaults: Defaults,
    #[serde(default)]
    repos: Vec<RepoEntry>,
    #[serde(default)]
    environments: Vec<EnvironmentEntry>,
    #[serde(default)]
    profiles: BTreeMap<String, ProfileEntry>,
}

/// The profiles a node uses when it names none.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Defaults {
    #[serde(default)]
    pub planner: String,
    #[serde(default)]
    pub executor: String,
    #[serde(default)]
    pub reviewer: String,
    /// The environment of a profile that names none.
    #[serde(default)]
    pub environment: String,
    /// Whether a plan's planner session hears about its finished task
    /// stages, unless the plan says otherwise (`Plan.stage_reports`).
    #[serde(default = "yes")]
    pub stage_reports: bool,
}

impl Default for Defaults {
    fn default() -> Self {
        Self {
            planner: String::new(),
            executor: String::new(),
            reviewer: String::new(),
            environment: String::new(),
            stage_reports: true,
        }
    }
}

fn yes() -> bool {
    true
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RepoEntry {
    slug: String,
    #[serde(default = "main")]
    default_branch: String,
    #[serde(default)]
    criteria: String,
    #[serde(default)]
    release_tool: String,
    #[serde(default)]
    requires: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct EnvironmentEntry {
    name: String,
    placement: PlacementEntry,
    #[serde(default)]
    capabilities: Vec<String>,
    #[serde(default)]
    description: String,
}

fn main() -> String {
    "main".into()
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ProfileEntry {
    agent: String,
    #[serde(default)]
    harness: String,
    #[serde(default)]
    model: String,
    #[serde(default)]
    reasoning_effort: String,
    #[serde(default)]
    environment: String,
    #[serde(default)]
    requires: Vec<String>,
    #[serde(default)]
    description: String,
    /// Omnigent's `mcp_registry_services`: the registry's MCP services the
    /// sessions may call, by id (an environment's egress routes).
    #[serde(default)]
    mcp_registry_services: Vec<String>,
}

/// `{host, workspace_root}` or `{managed: {provider, sandbox_profile}}`.
#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum PlacementEntry {
    Managed { managed: ManagedEntry },
    Host(HostEntry),
}

/// The keyword that leaves an environment to the Decider.
pub const DECIDE: &str = "decide";

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct HostEntry {
    host: String,
    #[serde(default)]
    workspace_root: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ManagedEntry {
    provider: String,
    #[serde(default)]
    sandbox_profile: String,
}

const EFFORTS: &[&str] = &[
    "none", "minimal", "low", "medium", "high", "xhigh", "max", "ultra",
];

/// The loaded registry.
#[derive(Debug, Clone, Default)]
pub struct Registry {
    profiles: Vec<wg::Profile>,
    repos: Vec<wg::Repo>,
    environments: Vec<wg::Environment>,
    defaults: Defaults,
}

fn labels(field: &str, owner: &str, values: Vec<String>) -> Result<Vec<String>> {
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for v in values {
        let v = v.trim().to_owned();
        if v.is_empty() || v.contains(char::is_whitespace) {
            return Err(Error::invalid(format!(
                "registry: {owner} has {field} label {v:?}; labels are non-empty and have no spaces"
            )));
        }
        if seen.insert(v.clone()) {
            out.push(v);
        }
    }
    Ok(out)
}

/// Omnigent's rule for a registry service id: lowercase letters, digits, `-`
/// and `_`, starting with a letter, at most 40 characters, no `__`; and at
/// most 40 of them on a session.
fn registry_services(profile: &str, ids: Vec<String>) -> Result<Vec<String>> {
    if ids.len() > 40 {
        return Err(Error::invalid(format!(
            "registry: profile {profile} names {} MCP registry services; at most 40",
            ids.len()
        )));
    }
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for id in ids {
        let ok = id.bytes().next().is_some_and(|b| b.is_ascii_lowercase())
            && id.len() <= 40
            && id
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'_')
            && !id.contains("__");
        if !ok {
            return Err(Error::invalid(format!(
                "registry: profile {profile} names MCP registry service {id:?}; an id is \
                 lowercase letters, digits, `-` and `_`, starts with a letter, has at most 40 \
                 characters and no `__`"
            )));
        }
        if seen.insert(id.clone()) {
            out.push(id);
        }
    }
    Ok(out)
}

fn placement(p: PlacementEntry) -> wg::Placement {
    let target = match p {
        PlacementEntry::Host(h) => wg::placement::Target::Host(Box::new(wg::HostPlacement {
            host: h.host,
            workspace_root: h.workspace_root,
            ..Default::default()
        })),
        PlacementEntry::Managed { managed } => {
            wg::placement::Target::Managed(Box::new(wg::ManagedPlacement {
                provider: managed.provider,
                sandbox_profile: managed.sandbox_profile,
                ..Default::default()
            }))
        }
    };
    wg::Placement {
        target: Some(target),
        ..Default::default()
    }
}

impl Registry {
    /// Parse and check a registry. Profile, environment and repo names must
    /// be unique, every profile names an agent, and every environment a
    /// profile or the defaults name exists.
    pub fn from_yaml(text: &str) -> Result<Self> {
        let file: File =
            serde_norway::from_str(text).map_err(|e| Error::invalid(format!("registry: {e}")))?;

        let mut slugs = HashSet::new();
        let mut repos = Vec::new();
        for r in file.repos {
            if r.slug.split('/').filter(|p| !p.is_empty()).count() != 2 {
                return Err(Error::invalid(format!(
                    "registry: repo {:?} isn't owner/name",
                    r.slug
                )));
            }
            if !slugs.insert(r.slug.clone()) {
                return Err(Error::invalid(format!(
                    "registry: repo {} is listed twice",
                    r.slug
                )));
            }
            let requires = labels("requires", &format!("repo {}", r.slug), r.requires)?;
            repos.push(wg::Repo {
                slug: r.slug,
                default_branch: r.default_branch,
                criteria: r.criteria,
                release_tool: r.release_tool,
                requires,
                ..Default::default()
            });
        }

        let mut names = HashSet::new();
        let mut environments = Vec::new();
        for e in file.environments {
            if e.name.trim().is_empty() || e.name == DECIDE {
                return Err(Error::invalid(format!(
                    "registry: {:?} can't name an environment",
                    e.name
                )));
            }
            if !names.insert(e.name.clone()) {
                return Err(Error::invalid(format!(
                    "registry: environment {} is listed twice",
                    e.name
                )));
            }
            let capabilities = labels(
                "capability",
                &format!("environment {}", e.name),
                e.capabilities,
            )?;
            environments.push(wg::Environment {
                name: e.name,
                placement: placement(e.placement).into(),
                capabilities,
                description: e.description,
                ..Default::default()
            });
        }
        let known_env = |name: &str| name.is_empty() || name == DECIDE || names.contains(name);

        let mut profiles = Vec::new();
        for (name, p) in file.profiles {
            if p.agent.trim().is_empty() {
                return Err(Error::invalid(format!(
                    "registry: profile {name} names no agent"
                )));
            }
            if !p.reasoning_effort.is_empty() && !EFFORTS.contains(&p.reasoning_effort.as_str()) {
                return Err(Error::invalid(format!(
                    "registry: profile {name} has reasoning_effort {:?}; one of {}",
                    p.reasoning_effort,
                    EFFORTS.join(", ")
                )));
            }
            if !known_env(&p.environment) {
                return Err(Error::invalid(format!(
                    "registry: profile {name} names environment {:?}, which isn't defined",
                    p.environment
                )));
            }
            let requires = labels("requires", &format!("profile {name}"), p.requires)?;
            let mcp_registry_services = registry_services(&name, p.mcp_registry_services)?;
            profiles.push(wg::Profile {
                name,
                agent: p.agent,
                harness: p.harness,
                model: p.model,
                reasoning_effort: p.reasoning_effort,
                environment: p.environment,
                requires,
                description: p.description,
                mcp_registry_services,
                ..Default::default()
            });
        }

        if !known_env(&file.defaults.environment) {
            return Err(Error::invalid(format!(
                "registry: the default environment {} isn't defined",
                file.defaults.environment
            )));
        }
        let registry = Self {
            profiles,
            repos,
            environments,
            defaults: file.defaults,
        };
        for (role, name) in [
            ("planner", &registry.defaults.planner),
            ("executor", &registry.defaults.executor),
            ("reviewer", &registry.defaults.reviewer),
        ] {
            if !name.is_empty() && registry.profile(name).is_none() {
                return Err(Error::invalid(format!(
                    "registry: the default {role} profile {name} isn't defined"
                )));
            }
        }
        Ok(registry)
    }

    /// Sorted by name.
    pub fn profiles(&self) -> &[wg::Profile] {
        &self.profiles
    }

    pub fn profile(&self, name: &str) -> Option<&wg::Profile> {
        self.profiles.iter().find(|p| p.name == name)
    }

    /// In file order.
    pub fn repos(&self) -> &[wg::Repo] {
        &self.repos
    }

    pub fn repo(&self, slug: &str) -> Option<&wg::Repo> {
        self.repos.iter().find(|r| r.slug == slug)
    }

    /// In file order, which is also the order eligible environments are
    /// tried in.
    pub fn environments(&self) -> &[wg::Environment] {
        &self.environments
    }

    pub fn environment(&self, name: &str) -> Option<&wg::Environment> {
        self.environments.iter().find(|e| e.name == name)
    }

    pub fn defaults(&self) -> &Defaults {
        &self.defaults
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const EXAMPLE: &str = include_str!("../registry.example.yaml");

    #[test]
    fn the_example_loads() {
        let r = Registry::from_yaml(EXAMPLE).unwrap();
        assert!(r.profile(&r.defaults().executor).is_some());
        assert!(r.profile(&r.defaults().reviewer).is_some());
        assert!(r.profile(&r.defaults().planner).is_some());
        assert!(!r.repos().is_empty());
    }

    #[test]
    fn environments_parse() {
        let r = Registry::from_yaml(
            "defaults: { environment: local }
environments:
  - { name: local, placement: { host: local, workspace_root: /w }, capabilities: [gh, gh] }
  - { name: box, placement: { managed: { provider: example, sandbox_profile: rust } } }
profiles:
  a: { agent: x, environment: decide, requires: [gh] }
  b: { agent: x, environment: box }
  c: { agent: x }
",
        )
        .unwrap();
        let env = |n: &str| r.environment(n).unwrap();
        assert_eq!(env("local").capabilities, vec!["gh"], "labels are deduped");
        match env("local").placement.target.clone() {
            Some(wg::placement::Target::Host(h)) => {
                assert_eq!(
                    (h.host.as_str(), h.workspace_root.as_str()),
                    ("local", "/w")
                );
            }
            other => panic!("{other:?}"),
        }
        match env("box").placement.target.clone() {
            Some(wg::placement::Target::Managed(m)) => assert_eq!(m.provider, "example"),
            other => panic!("{other:?}"),
        }
        assert_eq!(r.profile("a").unwrap().environment, DECIDE);
        assert_eq!(r.profile("a").unwrap().requires, vec!["gh"]);
        assert_eq!(r.profile("c").unwrap().environment, "");
        assert_eq!(r.defaults().environment, "local");
    }

    #[test]
    fn profiles_name_registry_services() {
        let r = Registry::from_yaml(
            "profiles: { p: { agent: x, mcp_registry_services: [workgraph-planner, workgraph-planner, uc] } }",
        )
        .unwrap();
        assert_eq!(
            r.profile("p").unwrap().mcp_registry_services,
            ["workgraph-planner", "uc"]
        );
    }

    #[test]
    fn bad_registries_are_refused() {
        for (yaml, needle) in [
            ("profiles: { a: { agent: '' } }", "names no agent"),
            (
                "profiles: { a: { agent: x, environment: somewhere } }",
                "isn't defined",
            ),
            (
                "environments: [ { name: e, placement: somewhere } ]",
                "PlacementEntry",
            ),
            (
                "environments: [ { name: decide, placement: { host: h } } ]",
                "can't name",
            ),
            (
                "environments: [ { name: e, placement: { host: h } }, { name: e, placement: { host: h } } ]",
                "twice",
            ),
            (
                "environments: [ { name: e, placement: { host: h }, capabilities: ['a b'] } ]",
                "no spaces",
            ),
            ("defaults: { environment: nowhere }", "isn't defined"),
            (
                "profiles: { a: { agent: x, reasoning_effort: huge } }",
                "reasoning_effort",
            ),
            ("repos: [ { slug: nope } ]", "owner/name"),
            ("repos: [ { slug: a/b }, { slug: a/b } ]", "twice"),
            ("defaults: { executor: missing }", "isn't defined"),
            (
                "profiles: { a: { agent: x, colour: red } }",
                "unknown field",
            ),
            (
                "profiles: { a: { agent: x, mcp_registry_services: [Work_Graph] } }",
                "MCP registry service",
            ),
            (
                "profiles: { a: { agent: x, mcp_registry_services: [a__b] } }",
                "no `__`",
            ),
        ] {
            let err = Registry::from_yaml(yaml).unwrap_err().to_string();
            assert!(err.contains(needle), "{yaml}: {err}");
        }
    }
}
