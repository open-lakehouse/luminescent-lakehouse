//! Unity Catalog's key-encryption key, one per environment.
//!
//! mangrove's server (`unity-catalog-rs`) encrypts the credentials it stores with a
//! key-encryption key it reads from `UC_SERVER_KEK`, and refuses to start without one.
//! In a container, trestle passes the variable through from Compose's process
//! environment by name and never renders it. So the environment needs the variable,
//! and the app mints it on the first Start that runs such a module: 32 bytes from the
//! OS CSPRNG, base64, in the keychain under the key `trestle var set --secret` uses
//! (`<environment id>/UC_SERVER_KEK`). The environment's vars hold only a reference,
//! so `trestle up` from the CLI uses the same key. There is no fallback: when the
//! keychain fails, Start fails (hydrofoil ADR 0016).
//!
//! A Unity Catalog that runs as a Neon Function has its key on the deployment.

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use olai_stack_state::{EnvironmentId, SecretRef, Store, VarScope, VarValue};
use olai_stack_topology::{Catalog, Plan};

use crate::instances::RunError;
use crate::neon::SecretSink;

/// The variable Unity Catalog reads its key-encryption key from.
pub const UC_KEK_VAR: &str = "UC_SERVER_KEK";

/// Whether `plan` runs a module in a container that reads [`UC_KEK_VAR`]: one whose
/// deployment needs it as a secret (`Provides::function_secrets`). A module hosted
/// elsewhere (a Neon Function) has the key on its deployment.
pub fn needs_kek(plan: &Plan, catalog: &Catalog) -> bool {
    let local = plan
        .services
        .keys()
        .filter(|id| !plan.hosted.contains_key(*id));
    local.into_iter().any(|id| {
        catalog.get(id).is_some_and(|m| {
            m.provides()
                .function_secrets
                .iter()
                .any(|s| s == UC_KEK_VAR)
        })
    })
}

/// The keychain key the environment's KEK is stored under, as the CLI names it.
pub fn kek_key(env: &EnvironmentId) -> String {
    format!("{}/{UC_KEK_VAR}", env.0)
}

/// Give `env` a [`UC_KEK_VAR`] unless it has one (set by hand or by an earlier Start):
/// mint the key into `secrets` and set the variable to a reference to it. Returns
/// whether it minted one.
pub fn ensure_kek(
    store: &Store,
    env: &EnvironmentId,
    secrets: &dyn SecretSink,
) -> Result<bool, RunError> {
    if store.environment(env)?.vars.get(UC_KEK_VAR).is_some() {
        return Ok(false);
    }
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes).map_err(|e| RunError::Other {
        message: format!("no randomness for the Unity Catalog key: {e}"),
    })?;
    let key = kek_key(env);
    secrets
        .set(&key, &STANDARD.encode(bytes))
        .map_err(|message| RunError::Other {
            message: format!("the keychain refused the Unity Catalog key: {message}"),
        })?;
    let reference = VarValue::Secret(SecretRef {
        provider: secrets.provider().to_owned(),
        key,
    });
    store.modify_vars(&VarScope::Environment(env.clone()), |vars| {
        // Another writer set it in the meantime: keep theirs.
        if vars.get(UC_KEK_VAR).is_none() {
            vars.set(UC_KEK_VAR, reference);
        }
        Ok::<_, RunError>(())
    })?;
    Ok(true)
}
