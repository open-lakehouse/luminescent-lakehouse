//! An environment's variables, as the editors set them, and an instance's effective
//! environment, as the view shows it.
//!
//! The editors write the same var sets `trestle var set` does, through
//! [`Store::modify_vars`] (one write transaction, so a concurrent writer's change to another
//! key survives):
//!
//! - the **workspace**'s, which every environment of it inherits;
//! - the **environment**'s, shared with the team when the environment is on a team's store;
//! - the **draft**'s, which the next commit freezes into its revision (a committed
//!   revision's vars never change);
//! - this machine's **local** vars for the environment (`trestle var set --local`), kept in
//!   this machine's `state.db` whichever store holds the environment;
//! - an **instance**'s overrides ([`Store::set_overrides`]; the CLI has no command for them).
//!
//! Any value can be a secret: the value goes to the keychain, the var set holds only a
//! [`SecretRef`] to it. The keychain key is the one `trestle var set --secret` uses for the
//! same scope ([`VarTarget::prefix`]), so the CLI resolves what the app set and the other way
//! round. On a team's store a secret goes to this machine's local vars, and a workspace or
//! draft secret is refused, as the CLI refuses them: no colleague's keychain holds it.
//!
//! The effective view ([`EffectiveView`]) is [`Instances::effective_env`] masked, the same
//! resolution the launch uses, with each layer named as `trestle env show --effective`
//! names it ([`layer_name`]).
//!
//! [`Instances::effective_env`]: olai_stack_runtime::Instances::effective_env

use olai_stack_runtime::neon::{NEON_SOURCE, NeonBinding};
use olai_stack_runtime::{EffectiveEnv, Layer};
use olai_stack_state::{
    Environment, EnvironmentId, Instance, InstanceId, SecretRef, Store, VarScope, VarSet, VarValue,
    Workspace, WorkspaceId,
};
use serde::{Deserialize, Serialize};

use crate::neon::SecretSink;
use crate::state::{SharedState, StateError};

type Result<T, E = StateError> = std::result::Result<T, E>;
use crate::token_endpoint::{self, ProfileBinding};

/// The keychain provider the CLI's secrets are under.
pub const KEYCHAIN: &str = "keychain";

/// The var set an edit goes to.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(
    tag = "scope",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum VarTarget {
    Workspace {
        workspace: String,
    },
    Environment {
        environment: String,
    },
    /// The draft: the next revision's vars.
    Draft {
        environment: String,
    },
    /// This machine's own vars for the environment.
    Local {
        environment: String,
    },
    /// An instance's overrides.
    Instance {
        instance: String,
    },
}

/// What an edit sets a variable to. A secret's value goes to the keychain only.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(tag = "kind", content = "value", rename_all = "camelCase")]
pub enum VarInput {
    Literal(String),
    Secret(String),
}

impl std::fmt::Debug for VarInput {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            VarInput::Literal(v) => f.debug_tuple("Literal").field(v).finish(),
            VarInput::Secret(_) => f.write_str("Secret(<redacted>)"),
        }
    }
}

/// A variable as the editors show it: a literal's value, a secret's reference
/// (`keychain:<key>`), never a secret's value.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct VarEntry {
    pub key: String,
    pub secret: bool,
    /// The literal, or the secret's `provider:key`.
    pub value: String,
}

/// A committed revision's vars, read-only.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct RevisionVars {
    pub number: u32,
    pub vars: Vec<VarEntry>,
}

/// An instance's overrides.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct InstanceVars {
    pub id: String,
    pub revision: u32,
    pub vars: Vec<VarEntry>,
}

/// Every var set of an environment the editors show.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct VarsView {
    pub workspace_id: String,
    pub environment_id: String,
    pub workspace: Vec<VarEntry>,
    pub environment: Vec<VarEntry>,
    pub draft: Vec<VarEntry>,
    pub local: Vec<VarEntry>,
    /// The head revision's, if one was committed.
    pub revision: Option<RevisionVars>,
    /// The asked-for instance's overrides.
    pub instance: Option<InstanceVars>,
    /// Whether the environment is on a team's store: its environment, draft and workspace
    /// vars reach every colleague's machine, its local vars don't.
    pub shared: bool,
}

/// The entries of `vars`, by key.
pub fn entries(vars: &VarSet) -> Vec<VarEntry> {
    vars.0
        .iter()
        .map(|(key, value)| match value {
            VarValue::Literal(v) => VarEntry {
                key: key.clone(),
                secret: false,
                value: v.clone(),
            },
            VarValue::Secret(r) => VarEntry {
                key: key.clone(),
                secret: true,
                value: reference(r),
            },
        })
        .collect()
}

/// How a secret's reference shows: `provider:key`.
pub fn reference(r: &SecretRef) -> String {
    format!("{}:{}", r.provider, r.key)
}

/// The keychain key of variable `var` in the scope keyed `prefix`, as `trestle var set
/// --secret` names it.
pub fn keychain_key(prefix: &str, var: &str) -> String {
    format!("{prefix}/{var}")
}

/// Refuse `key` unless it is a variable name: letters, digits and `_`, not starting with
/// a digit (as the CLI checks it).
pub fn check_key(key: &str) -> Result<()> {
    let mut chars = key.chars();
    let valid = matches!(chars.next(), Some(c) if c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_');
    if valid {
        Ok(())
    } else {
        Err(StateError::Invalid {
            message: format!(
                "`{key}` is not a variable name (letters, digits and `_`, not starting with a digit)"
            ),
        })
    }
}

/// Where a target's vars are: a store scope, or an instance's overrides.
enum Resolved {
    Scope(VarScope),
    Instance(InstanceId),
}

impl VarTarget {
    /// The workspace the target belongs to.
    fn workspace(&self, store: &Store) -> Result<Workspace> {
        let env = |id: &str| store.environment(&EnvironmentId(id.to_owned()));
        let ws = match self {
            VarTarget::Workspace { workspace } => WorkspaceId(workspace.clone()),
            VarTarget::Environment { environment }
            | VarTarget::Draft { environment }
            | VarTarget::Local { environment } => env(environment)?.workspace,
            VarTarget::Instance { instance } => {
                let instance = store.instance(&InstanceId(instance.clone()))?;
                store.environment(&instance.environment)?.workspace
            }
        };
        Ok(store.workspace_by_id(&ws)?)
    }

    /// The var set, and the prefix of its secrets' keychain keys. The prefixes are the
    /// CLI's (`trestle var set --secret`), built from ids so a rename never makes two
    /// scopes share keys: `<workspace id>`, `<environment id>`, `<environment id>@r<next>`
    /// for the draft (each revision's secrets keep keys of their own, so editing the draft
    /// never changes what a committed revision resolves to), `<environment id>@local`.
    /// An instance's are `<instance id>@instance`, the app's own (the CLI sets no
    /// override).
    fn prefix(&self, store: &Store) -> Result<(Resolved, String)> {
        let env = |id: &str| store.environment(&EnvironmentId(id.to_owned()));
        Ok(match self {
            VarTarget::Workspace { workspace } => {
                let ws = store.workspace_by_id(&WorkspaceId(workspace.clone()))?;
                let prefix = ws.id.0.clone();
                (Resolved::Scope(VarScope::Workspace(ws.id)), prefix)
            }
            VarTarget::Environment { environment } => {
                let env = env(environment)?;
                let prefix = env.id.0.clone();
                (Resolved::Scope(VarScope::Environment(env.id)), prefix)
            }
            VarTarget::Draft { environment } => {
                let env = env(environment)?;
                let next = env.head.unwrap_or(0) + 1;
                let prefix = format!("{}@r{next}", env.id);
                (Resolved::Scope(VarScope::Draft(env.id)), prefix)
            }
            VarTarget::Local { environment } => {
                let env = env(environment)?;
                let prefix = format!("{}@local", env.id);
                (Resolved::Scope(VarScope::Local(env.id)), prefix)
            }
            VarTarget::Instance { instance } => {
                let instance = store.instance(&InstanceId(instance.clone()))?;
                let prefix = format!("{}@instance", instance.id);
                (Resolved::Instance(instance.id), prefix)
            }
        })
    }

    /// Where a secret for this target goes on a store that is `shared` (a team's), as
    /// `trestle var set --secret` routes it: an environment's goes to this machine's local
    /// vars, a workspace's or draft's is refused (no colleague could resolve it), an
    /// instance's stays (instances are this machine's).
    pub fn for_secret(self, shared: bool) -> Result<VarTarget> {
        if !shared {
            return Ok(self);
        }
        match self {
            VarTarget::Environment { environment } => Ok(VarTarget::Local { environment }),
            VarTarget::Workspace { .. } | VarTarget::Draft { .. } => Err(StateError::Invalid {
                message: "a secret here would be shared with the team, whose keychains don't \
                          hold it: set it on the environment (kept on this machine)"
                    .into(),
            }),
            other => Ok(other),
        }
    }
}

/// Every keychain reference in `vars`.
fn keychain_refs(vars: &VarSet) -> impl Iterator<Item = &SecretRef> {
    vars.0.values().filter_map(|v| match v {
        VarValue::Secret(r) if r.provider == KEYCHAIN => Some(r),
        _ => None,
    })
}

/// Every keychain reference the workspace holds: its vars, and each environment's (its
/// own, this machine's, its draft's, every revision's, its instances' overrides).
fn workspace_refs(store: &Store, ws: &Workspace) -> Result<Vec<SecretRef>> {
    let mut refs: Vec<SecretRef> = keychain_refs(&ws.vars).cloned().collect();
    for env in store.environments(&ws.id)? {
        refs.extend(environment_refs(store, &env)?);
    }
    refs.sort();
    refs.dedup();
    Ok(refs)
}

fn environment_refs(store: &Store, env: &Environment) -> Result<Vec<SecretRef>> {
    let local = store.local_vars(&env.id)?;
    let mut refs: Vec<SecretRef> = keychain_refs(&env.vars)
        .chain(keychain_refs(&local))
        .chain(keychain_refs(&env.draft.vars))
        .cloned()
        .collect();
    for rev in store.revisions(&env.id)? {
        refs.extend(keychain_refs(rev.vars()).cloned());
    }
    for instance in store.instances_of(&env.id)? {
        refs.extend(keychain_refs(&instance.overrides).cloned());
    }
    Ok(refs)
}

impl SharedState {
    /// Whether the store is a team's (an attached store): what the environment, draft
    /// and workspace vars hold reaches every colleague's machine.
    pub fn is_shared(&self) -> bool {
        self.store().attachment_id().is_some()
    }

    /// The var sets of environment `id`, and the overrides of `instance` (one of its).
    pub fn vars(&self, id: &str, instance: Option<&str>) -> Result<VarsView> {
        let store = self.store();
        let env = store.environment(&EnvironmentId(id.to_owned()))?;
        let ws = store.workspace_by_id(&env.workspace)?;
        let revision = match env.head {
            Some(head) => {
                let rev = store.revision(&env.id, head)?;
                Some(RevisionVars {
                    number: head,
                    vars: entries(rev.vars()),
                })
            }
            None => None,
        };
        let instance = match instance {
            Some(i) => {
                let i = store.instance(&InstanceId(i.to_owned()))?;
                if i.environment != env.id {
                    return Err(StateError::Invalid {
                        message: format!("instance {} isn't one of {}", i.id, env.name),
                    });
                }
                Some(InstanceVars {
                    id: i.id.0.clone(),
                    revision: i.revision,
                    vars: entries(&i.overrides),
                })
            }
            None => None,
        };
        Ok(VarsView {
            workspace_id: ws.id.0.clone(),
            environment_id: env.id.0.clone(),
            workspace: entries(&ws.vars),
            environment: entries(&env.vars),
            draft: entries(&env.draft.vars),
            local: entries(&store.local_vars(&env.id)?),
            revision,
            instance,
            shared: self.is_shared(),
        })
    }

    /// Set `key` in `target` to `input`; a secret's value goes to `keychain` (none: refused)
    /// under the CLI's key, the var set gets its reference. Returns where it went (a secret
    /// on a team's environment goes to this machine's local vars, [`VarTarget::for_secret`]).
    /// A secret it replaces is deleted from the keychain unless something still refers to
    /// it (a committed revision may).
    pub fn set_var(
        &self,
        target: VarTarget,
        key: &str,
        input: VarInput,
        keychain: Option<&dyn SecretSink>,
    ) -> Result<VarTarget> {
        check_key(key)?;
        let store = self.store();
        let target = match &input {
            VarInput::Secret(_) => target.for_secret(self.is_shared())?,
            VarInput::Literal(_) => target,
        };
        let (scope, prefix) = target.prefix(&store)?;
        let ws = target.workspace(&store)?;
        let value = match input {
            VarInput::Literal(v) => VarValue::Literal(v),
            VarInput::Secret(secret) => {
                if secret.is_empty() {
                    return Err(StateError::Invalid {
                        message: format!("no value for the secret {key}"),
                    });
                }
                let keychain = keychain.ok_or_else(|| StateError::Store {
                    message: "there is no keychain to keep the secret in".into(),
                })?;
                let reference = SecretRef {
                    provider: keychain.provider().to_owned(),
                    key: keychain_key(&prefix, key),
                };
                // The same key held under another variable or scope (a minted key, say)
                // would change under it.
                let current = current(&store, &scope, key)?;
                if current.as_ref() != Some(&VarValue::Secret(reference.clone()))
                    && workspace_refs(&store, &ws)?.contains(&reference)
                {
                    return Err(StateError::Invalid {
                        message: format!(
                            "the keychain key {} is in use elsewhere: unset that first",
                            reference.key
                        ),
                    });
                }
                keychain
                    .set(&reference.key, &secret)
                    .map_err(|message| StateError::Store {
                        message: format!("the keychain refused {key}: {message}"),
                    })?;
                VarValue::Secret(reference)
            }
        };
        let old = write(&store, &scope, key, Some(value))?;
        forget_unused(&store, &ws, old, keychain)?;
        Ok(target)
    }

    /// Unset `key` in `target`; its secret, if it was one, is deleted from `keychain` unless
    /// something still refers to it.
    pub fn unset_var(
        &self,
        target: &VarTarget,
        key: &str,
        keychain: Option<&dyn SecretSink>,
    ) -> Result<()> {
        let store = self.store();
        let (scope, _) = target.prefix(&store)?;
        let ws = target.workspace(&store)?;
        let old = write(&store, &scope, key, None)?;
        forget_unused(&store, &ws, old, keychain)
    }
}

/// `key`'s value in `scope` now.
fn current(store: &Store, scope: &Resolved, key: &str) -> Result<Option<VarValue>> {
    Ok(match scope {
        Resolved::Scope(VarScope::Workspace(id)) => {
            store.workspace_by_id(id)?.vars.get(key).cloned()
        }
        Resolved::Scope(VarScope::Environment(id)) => store.environment(id)?.vars.get(key).cloned(),
        Resolved::Scope(VarScope::Draft(id)) => store.environment(id)?.draft.vars.get(key).cloned(),
        Resolved::Scope(VarScope::Local(id)) => store.local_vars(id)?.get(key).cloned(),
        Resolved::Instance(id) => store.instance(id)?.overrides.get(key).cloned(),
    })
}

/// Set (or, with `None`, unset) `key` in `scope`; returns what it replaced.
fn write(
    store: &Store,
    scope: &Resolved,
    key: &str,
    value: Option<VarValue>,
) -> Result<Option<VarValue>> {
    let apply = |vars: &mut VarSet| match value.clone() {
        Some(v) => {
            let old = vars.remove(key);
            vars.set(key, v);
            old
        }
        None => vars.remove(key),
    };
    match scope {
        Resolved::Scope(scope) => {
            let mut old = None;
            store.modify_vars(scope, |vars| {
                old = apply(vars);
                Ok::<_, StateError>(())
            })?;
            Ok(old)
        }
        Resolved::Instance(id) => {
            let Instance { mut overrides, .. } = store.instance(id)?;
            let old = apply(&mut overrides);
            store.set_overrides(id, &overrides)?;
            Ok(old)
        }
    }
}

/// Delete the keychain entry of `dropped`, a value just replaced or unset, unless it isn't a
/// keychain secret or something in the workspace still refers to it.
fn forget_unused(
    store: &Store,
    ws: &Workspace,
    dropped: Option<VarValue>,
    keychain: Option<&dyn SecretSink>,
) -> Result<()> {
    let Some(VarValue::Secret(r)) = dropped else {
        return Ok(());
    };
    let Some(keychain) = keychain.filter(|k| k.provider() == r.provider) else {
        return Ok(());
    };
    if workspace_refs(store, ws)?.contains(&r) {
        return Ok(());
    }
    keychain
        .delete(&r.key)
        .map_err(|message| StateError::Store {
            message: format!("the keychain kept {}: {message}", r.key),
        })
}

/// A layer, as the view shows it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct LayerRef {
    /// As `trestle env show --effective` names it ([`layer_name`]).
    pub name: String,
    /// For showing: the name, or what a source is (`Neon branch main`).
    pub label: String,
}

/// One variable of an instance's effective environment.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct EffectiveEntry {
    pub key: String,
    /// The value, or [`MASK`](olai_stack_runtime::MASK) for a secret.
    pub value: String,
    pub secret: bool,
    /// A secret's reference (`keychain:<key>`), when a var set holds it.
    pub reference: Option<String>,
    /// The layer that set it.
    pub layer: LayerRef,
    /// The lower layers that set it too, lowest first.
    pub shadowed: Vec<LayerRef>,
}

/// An instance's effective environment from the host's vantage, secrets masked.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct EffectiveView {
    pub instance: String,
    pub revision: u32,
    /// By key.
    pub entries: Vec<EffectiveEntry>,
    /// The entries as a dotenv file, secrets masked ([`dotenv`]).
    pub dotenv: String,
}

/// `layer` as `trestle env show --effective` names it (trestle's `layer_name`, in
/// `crates/trestle/src/cli/env_store.rs`).
pub fn layer_name(layer: &Layer) -> String {
    match layer {
        Layer::CatalogDefaults => "catalog defaults".into(),
        Layer::RevisionManifest => "revision manifest".into(),
        Layer::Placement => "placement".into(),
        Layer::ClientEnv => "client env".into(),
        Layer::WorkspaceVars => "workspace vars".into(),
        Layer::EnvironmentVars => "environment vars".into(),
        Layer::RevisionVars => "revision vars".into(),
        Layer::LocalVars => "local vars".into(),
        Layer::Source(name) => format!("source:{name}"),
        Layer::InstanceOverrides => "instance overrides".into(),
    }
}

/// What a source is, from the instance's binding to it: the Neon branch, the Databricks
/// profile.
fn source_label(instance: &Instance, name: &str) -> Option<String> {
    let config = &instance.sources.iter().find(|b| b.name == name)?.config;
    match name {
        NEON_SOURCE => {
            let binding: NeonBinding = serde_json::from_value(config.clone()).ok()?;
            let branch = if binding.branch_name.is_empty() {
                binding.branch
            } else {
                binding.branch_name
            };
            Some(format!("Neon branch {branch}"))
        }
        token_endpoint::SOURCE => {
            let binding: ProfileBinding = serde_json::from_value(config.clone()).ok()?;
            Some(format!("Databricks profile {}", binding.profile))
        }
        _ => None,
    }
}

/// The view of `instance`'s effective environment `env`. A secret a var set holds shows its
/// reference; a source's credential (a Neon branch's password) only its mask. `env`'s values
/// stay here: only [`EffectiveEnv::masked`] reaches the view.
pub fn effective_view(
    store: &Store,
    instance: &Instance,
    env: &EffectiveEnv,
) -> Result<EffectiveView> {
    let environment = store.environment(&instance.environment)?;
    let ws = store.workspace_by_id(&environment.workspace)?;
    let revision = store.revision(&environment.id, instance.revision)?;
    let local = store.local_vars(&environment.id)?;
    let layer = |layer: &Layer| {
        let name = layer_name(layer);
        let label = match layer {
            Layer::Source(source) => source_label(instance, source).unwrap_or_else(|| name.clone()),
            _ => name.clone(),
        };
        LayerRef { name, label }
    };
    let set_of = |layer: &Layer| -> Option<&VarSet> {
        match layer {
            Layer::WorkspaceVars => Some(&ws.vars),
            Layer::EnvironmentVars => Some(&environment.vars),
            Layer::RevisionVars => Some(revision.vars()),
            Layer::LocalVars => Some(&local),
            Layer::InstanceOverrides => Some(&instance.overrides),
            _ => None,
        }
    };
    let entries: Vec<EffectiveEntry> = env
        .masked()
        .into_iter()
        .map(|(key, var)| {
            let reference = match set_of(&var.provenance).and_then(|vars| vars.get(&key)) {
                Some(VarValue::Secret(r)) if var.secret => Some(reference(r)),
                _ => None,
            };
            EffectiveEntry {
                layer: layer(&var.provenance),
                shadowed: var.shadowed.iter().map(layer).collect(),
                key,
                value: var.value,
                secret: var.secret,
                reference,
            }
        })
        .collect();
    let header = format!(
        "{}/{}@{} ({}), from the host; secrets masked",
        ws.name, environment.name, instance.revision, instance.id
    );
    Ok(EffectiveView {
        instance: instance.id.0.clone(),
        revision: instance.revision,
        dotenv: dotenv(&header, &entries),
        entries,
    })
}

/// `entries` as a dotenv file under a `# header` comment, one `KEY=value` line each (a
/// secret's value is its mask), each line after a comment naming its layer. Values with
/// spaces, quotes, `#` or line breaks are double-quoted.
pub fn dotenv(header: &str, entries: &[EffectiveEntry]) -> String {
    let mut out = format!("# {header}\n");
    for e in entries {
        out.push_str(&format!(
            "# {}\n{}={}\n",
            e.layer.label,
            e.key,
            quote(&e.value)
        ));
    }
    out
}

fn quote(value: &str) -> String {
    let plain = !value.is_empty()
        && value
            .chars()
            .all(|c| !c.is_whitespace() && !matches!(c, '"' | '\'' | '#' | '\\' | '$' | '`'));
    if plain || value.is_empty() {
        return value.to_owned();
    }
    let mut out = String::from('"');
    for c in value.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '$' => out.push_str("\\$"),
            '`' => out.push_str("\\`"),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// The layers' names in resolution order, lowest first (`source:<name>` stands for each
/// source, in the order the instance binds them): the view's legend.
pub const LAYER_ORDER: [&str; 10] = [
    "catalog defaults",
    "revision manifest",
    "placement",
    "client env",
    "workspace vars",
    "environment vars",
    "revision vars",
    "local vars",
    "source:<name>",
    "instance overrides",
];

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::sync::Arc;

    use super::*;
    use crate::neon::tests::MemorySecrets;

    fn fixture() -> (
        tempfile::TempDir,
        SharedState,
        String,
        String,
        Arc<MemorySecrets>,
    ) {
        let dir = tempfile::tempdir().unwrap();
        let state = SharedState::open(Some(dir.path())).unwrap();
        let ws = state.create_workspace("local", false, "dev").unwrap();
        let env = state.workspace(&ws.id).unwrap().environments.remove(0).id;
        (dir, state, ws.id, env, Arc::new(MemorySecrets::default()))
    }

    fn keys(k: &MemorySecrets) -> BTreeMap<String, String> {
        k.0.lock().unwrap().clone()
    }

    #[test]
    fn layers_are_named_as_the_cli_names_them_in_resolution_order() {
        let layers = [
            Layer::CatalogDefaults,
            Layer::RevisionManifest,
            Layer::Placement,
            Layer::ClientEnv,
            Layer::WorkspaceVars,
            Layer::EnvironmentVars,
            Layer::RevisionVars,
            Layer::LocalVars,
            Layer::Source("neon".into()),
            Layer::InstanceOverrides,
        ];
        let mut sorted = layers.to_vec();
        sorted.sort();
        assert_eq!(sorted, layers, "the runtime's order");
        let names: Vec<String> = layers.iter().map(layer_name).collect();
        let mut expected: Vec<String> = LAYER_ORDER.iter().map(|n| (*n).to_owned()).collect();
        expected[8] = "source:neon".into();
        assert_eq!(names, expected);
    }

    #[test]
    fn each_scope_gets_the_cli_s_keychain_key_and_holds_only_a_reference() {
        let (_dir, state, ws, env, keychain) = fixture();
        let sink: &dyn SecretSink = keychain.as_ref();
        let id = EnvironmentId(env.clone());
        let cases = [
            (
                VarTarget::Workspace {
                    workspace: ws.clone(),
                },
                format!("{ws}/A"),
            ),
            (
                VarTarget::Environment {
                    environment: env.clone(),
                },
                format!("{env}/A"),
            ),
            (
                VarTarget::Draft {
                    environment: env.clone(),
                },
                format!("{env}@r1/A"),
            ),
            (
                VarTarget::Local {
                    environment: env.clone(),
                },
                format!("{env}@local/A"),
            ),
        ];
        for (target, key) in cases {
            let wrote = state
                .set_var(
                    target.clone(),
                    "A",
                    VarInput::Secret("s3cret".into()),
                    Some(sink),
                )
                .unwrap();
            assert_eq!(wrote, target);
            assert_eq!(
                keys(&keychain).get(&key).map(String::as_str),
                Some("s3cret")
            );
        }
        let view = state.vars(&env, None).unwrap();
        for set in [&view.workspace, &view.environment, &view.draft, &view.local] {
            assert_eq!(set.len(), 1);
            assert!(set[0].secret);
            assert!(set[0].value.starts_with("keychain:"), "{set:?}");
        }
        // The store holds references only.
        let store = state.store();
        assert!(matches!(
            store.local_vars(&id).unwrap().get("A"),
            Some(VarValue::Secret(SecretRef { provider, .. })) if provider == KEYCHAIN
        ));
        assert!(!format!("{view:?}").contains("s3cret"));
    }

    #[test]
    fn a_literal_replacing_a_secret_drops_its_key_unless_a_revision_holds_it() {
        let (_dir, state, _ws, env, keychain) = fixture();
        let sink: &dyn SecretSink = keychain.as_ref();
        let draft = VarTarget::Draft {
            environment: env.clone(),
        };
        state
            .set_var(
                draft.clone(),
                "TOKEN",
                VarInput::Secret("one".into()),
                Some(sink),
            )
            .unwrap();
        // Committed: revision 1 refers to it, so it stays.
        state.commit(&env).unwrap();
        state
            .set_var(
                draft.clone(),
                "TOKEN",
                VarInput::Literal("plain".into()),
                Some(sink),
            )
            .unwrap();
        assert!(keys(&keychain).contains_key(&format!("{env}@r1/TOKEN")));
        // Not committed: dropped with its last reference.
        let local = VarTarget::Local {
            environment: env.clone(),
        };
        state
            .set_var(
                local.clone(),
                "TOKEN",
                VarInput::Secret("two".into()),
                Some(sink),
            )
            .unwrap();
        state.unset_var(&local, "TOKEN", Some(sink)).unwrap();
        assert!(!keys(&keychain).contains_key(&format!("{env}@local/TOKEN")));
        assert!(state.vars(&env, None).unwrap().local.is_empty());
    }

    #[test]
    fn a_key_another_variable_holds_is_refused() {
        let (_dir, state, _ws, env, keychain) = fixture();
        let sink: &dyn SecretSink = keychain.as_ref();
        // A minted key: `<env>/<VAR>` in the local vars, the environment scope's key.
        let id = EnvironmentId(env.clone());
        state
            .store()
            .modify_vars(&VarScope::Local(id.clone()), |vars| {
                vars.set(
                    "UC_SERVER_KEK",
                    VarValue::Secret(SecretRef {
                        provider: KEYCHAIN.into(),
                        key: crate::secrets::secret_key(&id, "UC_SERVER_KEK"),
                    }),
                );
                Ok::<_, StateError>(())
            })
            .unwrap();
        let err = state
            .set_var(
                VarTarget::Environment {
                    environment: env.clone(),
                },
                "UC_SERVER_KEK",
                VarInput::Secret("other".into()),
                Some(sink),
            )
            .unwrap_err();
        assert!(matches!(err, StateError::Invalid { .. }), "{err:?}");
        assert!(keys(&keychain).is_empty());
    }

    #[test]
    fn on_a_team_s_store_secrets_stay_on_this_machine() {
        let env = || "e".to_owned();
        assert_eq!(
            VarTarget::Environment { environment: env() }
                .for_secret(true)
                .unwrap(),
            VarTarget::Local { environment: env() }
        );
        assert!(
            VarTarget::Workspace {
                workspace: "w".into()
            }
            .for_secret(true)
            .is_err()
        );
        assert!(
            VarTarget::Draft { environment: env() }
                .for_secret(true)
                .is_err()
        );
        assert_eq!(
            VarTarget::Local { environment: env() }
                .for_secret(true)
                .unwrap(),
            VarTarget::Local { environment: env() }
        );
        assert_eq!(
            VarTarget::Environment { environment: env() }
                .for_secret(false)
                .unwrap(),
            VarTarget::Environment { environment: env() }
        );
    }

    #[test]
    fn names_and_empty_secrets_are_checked() {
        let (_dir, state, _ws, env, keychain) = fixture();
        let sink: &dyn SecretSink = keychain.as_ref();
        let target = VarTarget::Environment { environment: env };
        for bad in ["", "1A", "A-B", "A B"] {
            assert!(
                state
                    .set_var(
                        target.clone(),
                        bad,
                        VarInput::Literal("x".into()),
                        Some(sink)
                    )
                    .is_err(),
                "{bad}"
            );
        }
        assert!(
            state
                .set_var(
                    target.clone(),
                    "A",
                    VarInput::Secret(String::new()),
                    Some(sink)
                )
                .is_err()
        );
        assert!(
            state
                .set_var(target, "A", VarInput::Secret("x".into()), None)
                .is_err()
        );
    }

    #[test]
    fn dotenv_masks_and_quotes() {
        let layer = |label: &str| LayerRef {
            name: label.into(),
            label: label.into(),
        };
        let entry = |key: &str, value: &str, secret: bool| EffectiveEntry {
            key: key.into(),
            value: value.into(),
            secret,
            reference: None,
            layer: layer("local vars"),
            shadowed: vec![],
        };
        let text = dotenv(
            "ws/dev@1 (i)",
            &[
                entry("A", "plain", false),
                entry("B", "two words", false),
                entry("C", "say \"hi\" $HOME", false),
                entry("D", olai_stack_runtime::MASK, true),
                entry("E", "", false),
            ],
        );
        assert_eq!(
            text,
            "# ws/dev@1 (i)\n# local vars\nA=plain\n# local vars\nB=\"two words\"\n\
             # local vars\nC=\"say \\\"hi\\\" \\$HOME\"\n# local vars\nD=••••••\n# local vars\nE=\n"
        );
    }

    #[test]
    fn debug_never_shows_a_secret_input() {
        assert_eq!(
            format!("{:?}", VarInput::Secret("s3cret".into())),
            "Secret(<redacted>)"
        );
    }
}
