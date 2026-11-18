//! The Databricks CLI: its profiles, and the access tokens it mints for them.
//!
//! People sign in to a workspace with `databricks auth login`, which leaves a U2M profile
//! (`auth_type: databricks-cli`) in `~/.databrickscfg` and a refresh token in the CLI's
//! cache. An environment that takes its models from a workspace names one of those
//! profiles; its containers have neither the CLI nor the cache, so the app asks the CLI
//! for a token whenever they need one ([`crate::token_endpoint`]).
//!
//! Every call runs through [`System`], so the parsing is tested against canned output.

use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::instances::RunError;
use crate::toolchain::System;

/// The literal variable naming an environment's profile.
pub const PROFILE_VAR: &str = "DATABRICKS_CONFIG_PROFILE";
/// The literal variable holding the profile's workspace URL.
pub const HOST_VAR: &str = "DATABRICKS_HOST";
/// The secret variable the Databricks SDK (in MLflow, in Omnigent's token helper) fetches
/// tokens from: the app's [`crate::token_endpoint`], for the launch only.
pub const METADATA_SERVICE_URL_VAR: &str = "DATABRICKS_METADATA_SERVICE_URL";

/// A profile of the Databricks CLI.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct DatabricksProfile {
    pub name: String,
    /// The workspace (or account) URL.
    pub host: String,
    /// How it signs in: `databricks-cli` (U2M), `pat`, `oauth-m2m`, …
    pub auth_type: Option<String>,
}

#[derive(Deserialize)]
struct Profiles {
    #[serde(default)]
    profiles: Vec<CliProfile>,
}

#[derive(Deserialize)]
struct CliProfile {
    name: String,
    #[serde(default)]
    host: String,
    auth_type: Option<String>,
}

/// What `databricks auth token` printed, as the SDK's metadata service answers it.
#[derive(Clone, PartialEq, Eq, Serialize)]
pub struct Token {
    pub access_token: String,
    pub token_type: String,
    /// When it expires, in seconds since the epoch.
    pub expires_on: u64,
}

impl std::fmt::Debug for Token {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Token")
            .field("token_type", &self.token_type)
            .field("expires_on", &self.expires_on)
            .finish_non_exhaustive()
    }
}

#[derive(Deserialize)]
struct CliToken {
    access_token: String,
    #[serde(default = "bearer")]
    token_type: String,
    /// RFC 3339.
    expiry: Option<String>,
    expires_in: Option<u64>,
}

fn bearer() -> String {
    "Bearer".into()
}

fn cli(system: &dyn System) -> Result<PathBuf, RunError> {
    system
        .which("databricks")
        .ok_or_else(|| RunError::Databricks {
            message: "the Databricks CLI isn't installed (see Doctor)".into(),
        })
}

/// Run `databricks args…` and return what it printed, or why it failed.
fn run(system: &dyn System, args: &[&str]) -> Result<String, RunError> {
    let databricks = cli(system)?;
    let out = system
        .run(&databricks, args)
        .map_err(|e| RunError::Databricks {
            message: format!("`databricks {}`: {e}", args.join(" ")),
        })?;
    if !out.success {
        let said = if out.stderr.trim().is_empty() {
            out.stdout.trim()
        } else {
            out.stderr.trim()
        };
        return Err(RunError::Databricks {
            message: format!("`databricks {}`: {said}", args.join(" ")),
        });
    }
    Ok(out.stdout)
}

/// The CLI's profiles (`databricks auth profiles`, without asking each workspace whether
/// it's still valid), sorted by name.
pub fn list_profiles(system: &dyn System) -> Result<Vec<DatabricksProfile>, RunError> {
    let args = ["auth", "profiles", "--skip-validate", "-o", "json"];
    let out = run(system, &args)?;
    let parsed: Profiles = serde_json::from_str(&out).map_err(|e| RunError::Databricks {
        message: format!("`databricks {}` printed no list: {e}", args.join(" ")),
    })?;
    let mut profiles: Vec<DatabricksProfile> = parsed
        .profiles
        .into_iter()
        .map(|p| DatabricksProfile {
            name: p.name,
            host: normalize_host(&p.host),
            auth_type: p.auth_type,
        })
        .collect();
    profiles.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(profiles)
}

/// A token for `profile`: `databricks auth token`, which refreshes from the CLI's cache
/// when the last one has expired.
pub fn token(system: &dyn System, profile: &str) -> Result<Token, RunError> {
    let out = run(system, &["auth", "token", "-p", profile, "-o", "json"])?;
    let parsed: CliToken = serde_json::from_str(&out).map_err(|e| RunError::Databricks {
        message: format!("`databricks auth token -p {profile}` printed no token: {e}"),
    })?;
    let expires_on = match (&parsed.expiry, parsed.expires_in) {
        (Some(expiry), _) => chrono::DateTime::parse_from_rfc3339(expiry)
            .map_err(|e| RunError::Databricks {
                message: format!("`databricks auth token -p {profile}`: bad expiry: {e}"),
            })?
            .timestamp()
            .max(0) as u64,
        (None, Some(seconds)) => now() + seconds,
        (None, None) => {
            return Err(RunError::Databricks {
                message: format!("`databricks auth token -p {profile}` printed no expiry"),
            });
        }
    };
    if parsed.access_token.is_empty() {
        return Err(RunError::Databricks {
            message: format!("`databricks auth token -p {profile}` printed an empty token"),
        });
    }
    Ok(Token {
        access_token: parsed.access_token,
        token_type: parsed.token_type,
        expires_on,
    })
}

/// What to tell the user when the CLI can't mint a token for `profile`.
pub fn login_hint(profile: &str) -> String {
    format!("run `databricks auth login -p {profile}`")
}

/// A workspace URL as the SDK sends it: no trailing slash, lowercase, `https://` when it
/// has no scheme.
pub fn normalize_host(host: &str) -> String {
    let host = host.trim().trim_end_matches('/').to_ascii_lowercase();
    if host.is_empty() || host.contains("://") {
        host
    } else {
        format!("https://{host}")
    }
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or_default()
}

#[cfg(test)]
pub(crate) mod tests {
    use std::collections::HashMap;
    use std::io;
    use std::path::{Path, PathBuf};
    use std::sync::Mutex;

    use super::*;
    use crate::toolchain::CommandOutput;

    /// A machine with the Databricks CLI, answering from canned output keyed by the joined
    /// arguments; it counts the calls.
    #[derive(Default)]
    pub(crate) struct FakeCli {
        pub installed: bool,
        pub answers: Mutex<HashMap<String, io::Result<CommandOutput>>>,
        pub calls: Mutex<Vec<String>>,
    }

    impl FakeCli {
        pub fn new() -> Self {
            FakeCli {
                installed: true,
                ..Default::default()
            }
        }

        pub fn answer(&self, args: &str, out: io::Result<CommandOutput>) {
            self.answers.lock().unwrap().insert(args.to_owned(), out);
        }

        pub fn token(&self, profile: &str, token: &str, expiry: &str) {
            self.answer(
                &format!("auth token -p {profile} -o json"),
                ok(&format!(
                    r#"{{"access_token":"{token}","token_type":"Bearer","expiry":"{expiry}","expires_in":86400}}"#
                )),
            );
        }
    }

    impl System for FakeCli {
        fn which(&self, name: &str) -> Option<PathBuf> {
            (self.installed && name == "databricks").then(|| "/opt/homebrew/bin/databricks".into())
        }
        fn locate_neon(&self) -> Option<PathBuf> {
            None
        }
        fn run(&self, _program: &Path, args: &[&str]) -> io::Result<CommandOutput> {
            let key = args.join(" ");
            self.calls.lock().unwrap().push(key.clone());
            match self.answers.lock().unwrap().get(&key) {
                Some(Ok(out)) => Ok(out.clone()),
                Some(Err(e)) => Err(io::Error::new(e.kind(), e.to_string())),
                None => Ok(failed(&format!("Error: no canned answer for {key}"))),
            }
        }
    }

    pub(crate) fn ok(stdout: &str) -> io::Result<CommandOutput> {
        Ok(CommandOutput {
            success: true,
            stdout: stdout.to_owned(),
            stderr: String::new(),
        })
    }

    pub(crate) fn failed(stderr: &str) -> CommandOutput {
        CommandOutput {
            success: false,
            stdout: String::new(),
            stderr: stderr.to_owned(),
        }
    }

    #[test]
    fn profiles_come_sorted_with_their_host_and_auth_type() {
        let cli = FakeCli::new();
        cli.answer(
            "auth profiles --skip-validate -o json",
            ok(r#"{"profiles":[
                {"name":"work","host":"https://dbc-1.cloud.databricks.com/","cloud":"aws","auth_type":"databricks-cli","valid":false},
                {"name":"acct","host":"https://accounts.cloud.databricks.com","account_id":"a","auth_type":"oauth-m2m","valid":false},
                {"name":"bare","valid":false}
            ]}"#),
        );
        let profiles = list_profiles(&cli).unwrap();
        assert_eq!(
            profiles,
            [
                DatabricksProfile {
                    name: "acct".into(),
                    host: "https://accounts.cloud.databricks.com".into(),
                    auth_type: Some("oauth-m2m".into()),
                },
                DatabricksProfile {
                    name: "bare".into(),
                    host: String::new(),
                    auth_type: None,
                },
                DatabricksProfile {
                    name: "work".into(),
                    host: "https://dbc-1.cloud.databricks.com".into(),
                    auth_type: Some("databricks-cli".into()),
                },
            ]
        );
    }

    #[test]
    fn no_cli_no_profiles() {
        let cli = FakeCli::default();
        let err = list_profiles(&cli).unwrap_err();
        assert!(err.to_string().contains("isn't installed"), "{err}");
    }

    #[test]
    fn a_cli_failure_says_what_the_cli_said() {
        let cli = FakeCli::new();
        cli.answer(
            "auth profiles --skip-validate -o json",
            Ok(failed("Error: cannot parse config file")),
        );
        let err = list_profiles(&cli).unwrap_err();
        assert!(matches!(err, RunError::Databricks { .. }), "{err:?}");
        assert!(
            err.to_string().contains("cannot parse config file"),
            "{err}"
        );
    }

    #[test]
    fn a_token_expires_when_the_cli_says() {
        let cli = FakeCli::new();
        cli.token("work", "dapi-secret", "2026-11-18T18:00:26.862835+01:00");
        let token = token(&cli, "work").unwrap();
        assert_eq!(token.access_token, "dapi-secret");
        assert_eq!(token.token_type, "Bearer");
        assert_eq!(token.expires_on, 1_795_021_226);
        assert!(!format!("{token:?}").contains("dapi-secret"));
    }

    #[test]
    fn hosts_compare_as_the_sdk_sends_them() {
        assert_eq!(
            normalize_host("https://DBC-1.cloud.databricks.com/"),
            "https://dbc-1.cloud.databricks.com"
        );
        assert_eq!(
            normalize_host("dbc-1.cloud.databricks.com"),
            "https://dbc-1.cloud.databricks.com"
        );
    }
}
