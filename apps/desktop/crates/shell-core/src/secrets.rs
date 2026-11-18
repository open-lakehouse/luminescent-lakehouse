//! The secrets an environment's modules declare as minted, one of each per environment.
//!
//! A module declares the secrets it reads (`Provides::secrets`); some of them are
//! `minted`, which means the launcher may generate the value once and keep it. Two do
//! today: mangrove's server (`unity-catalog-rs`) encrypts the credentials it stores
//! with a key from `UC_SERVER_KEK`, and MLflow encrypts its AI Gateway's provider keys
//! with `MLFLOW_CRYPTO_KEK_PASSPHRASE`. Both refuse to start without theirs. In a
//! container, trestle passes such a variable through from Compose's process environment
//! by name and never renders it.
//!
//! So on the first Start of a revision whose plan declares one, the app mints it: 32
//! bytes from the OS CSPRNG, base64, in the keychain under the key `trestle var set
//! --secret` uses (`<environment id>/<VAR>`). The environment's vars hold only a
//! reference, so `trestle up` from the CLI uses the same value. There is no fallback:
//! when the keychain fails, Start fails (hydrofoil ADR 0016).
//!
//! A module hosted elsewhere (a Neon Function) declares nothing here: its key is on its
//! deployment.

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use olai_stack_state::{EnvironmentId, SecretRef, Store, VarScope, VarValue};
use olai_stack_topology::{DeclaredSecret, Plan};

use crate::instances::RunError;
use crate::neon::SecretSink;

/// The variable Unity Catalog reads its key-encryption key from.
pub const UC_KEK_VAR: &str = "UC_SERVER_KEK";

/// The variable MLflow derives the key that seals its AI Gateway secrets from.
pub const MLFLOW_PASSPHRASE_VAR: &str = "MLFLOW_CRYPTO_KEK_PASSPHRASE";

/// The passphrase trestle's `mlflow` module hard-coded before it declared
/// [`MLFLOW_PASSPHRASE_VAR`] (olai-stack-topology 0.0.21 and older). An environment
/// whose MLflow ran under it gets it as its minted value, so the keys MLflow stored keep
/// decrypting; `docs/desktop.md` ("Minted secrets") says how to rotate it.
pub const LEGACY_MLFLOW_PASSPHRASE: &str = "lakehouse-dev-kek";

/// The secrets `plan` declares as minted, by name.
pub fn minted(plan: &Plan) -> Vec<DeclaredSecret> {
    plan.declared_secrets()
        .into_iter()
        .filter(|s| s.minted)
        .collect()
}

/// The keychain key `env`'s `var` is stored under, as the CLI names it.
pub fn secret_key(env: &EnvironmentId, var: &str) -> String {
    format!("{}/{var}", env.0)
}

/// What to mint a secret as.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Seed {
    /// 32 random bytes, base64.
    Random,
    /// A value fixed by an older release, which data already depends on.
    Legacy(&'static str),
}

/// Give `env` each of `secrets` it has no variable for (set by hand, in the environment
/// or the workspace, or by an earlier Start): mint the value `seed` picks into `sink`
/// and set the environment variable to a reference to it. Returns the names it minted.
pub fn ensure_minted(
    store: &Store,
    env: &EnvironmentId,
    sink: &dyn SecretSink,
    secrets: &[DeclaredSecret],
    seed: impl Fn(&str) -> Seed,
) -> Result<Vec<String>, RunError> {
    let environment = store.environment(env)?;
    let workspace = store.workspace_by_id(&environment.workspace)?;
    let mut minted = Vec::new();
    for secret in secrets {
        let name = secret.name.as_str();
        if environment.vars.get(name).is_some() || workspace.vars.get(name).is_some() {
            continue;
        }
        let value = match seed(name) {
            Seed::Random => random_value(name)?,
            Seed::Legacy(value) => value.to_owned(),
        };
        let key = secret_key(env, name);
        sink.set(&key, &value).map_err(|message| RunError::Other {
            message: format!("the keychain refused {name}: {message}"),
        })?;
        let reference = VarValue::Secret(SecretRef {
            provider: sink.provider().to_owned(),
            key,
        });
        store.modify_vars(&VarScope::Environment(env.clone()), |vars| {
            // Another writer set it in the meantime: keep theirs.
            if vars.get(name).is_none() {
                vars.set(name, reference);
            }
            Ok::<_, RunError>(())
        })?;
        minted.push(name.to_owned());
    }
    Ok(minted)
}

fn random_value(name: &str) -> Result<String, RunError> {
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes).map_err(|e| RunError::Other {
        message: format!("no randomness for {name}: {e}"),
    })?;
    Ok(STANDARD.encode(bytes))
}
