//! The Neon AI Gateway of an environment's branch, as the Models choice shows it (#169).
//!
//! Neon has no call that enables a branch's gateway: `GET …/branches/{b}/ai_gateway`
//! answers `enabled: true` on any branch of a region that serves it, free plan or not,
//! and a 404 (`AI_GATEWAY_NOT_ENABLED`) where the region doesn't. What the account may
//! call shows up only when it calls: the branch's `GET /v1/models` marks each model
//! `enabled`, with an `unavailable_reason` (`credits_required`) and a `recovery_url`
//! when it isn't, and the calls answer 429 without credits. So the option reports:
//!
//! - the project's plan and region, from the Neon CLI ([`ProjectInfo`]);
//! - the gateway's host, from `ai_gateway`;
//! - the models the branch serves to this account, listed with a credential minted
//!   for the listing and revoked right after ([`NeonEnvironments::gateway`]).
//!
//! Each problem ([`GatewayProblem`]) links to the branch's AI Gateway page in the Neon
//! Console, where a plan or credits are bought. The fixtures in
//! `tests/fixtures/neon/` say which shapes are real (docs/ai-gateway.md, #194).
//!
//! [`NeonEnvironments::gateway`]: crate::neon::NeonEnvironments::gateway

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

/// The regions that serve the AI Gateway (Neon's docs, 2026-11).
pub const GATEWAY_REGIONS: [&str; 4] = [
    "aws-us-east-1",
    "aws-us-east-2",
    "aws-eu-central-1",
    "aws-ap-southeast-1",
];

/// A model the branch's gateway serves to the account.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct GatewayModel {
    /// Its catalog id, which requests name it by (`claude-opus-5-5`).
    pub id: String,
    /// Its display name.
    pub name: String,
    /// Its provider (`anthropic`, `openai`, …).
    pub owned_by: String,
}

/// Why the gateway can't serve the environment's models.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum GatewayProblem {
    /// The project's region has no AI Gateway.
    Region { region: Option<String> },
    /// The gateway needs a paid Neon plan (Launch or Scale).
    PaidPlan,
    /// The organization has no prepaid AI Gateway credits left.
    Credits { recovery_url: Option<String> },
    /// The account may call none of the branch's models, for `reason`.
    NoModels { reason: Option<String> },
    /// The gateway didn't answer as expected.
    Unreachable { message: String },
}

impl GatewayProblem {
    /// In a sentence, for an error.
    pub fn describe(&self) -> String {
        match self {
            GatewayProblem::Region { region } => format!(
                "the project's region{} has no Neon AI Gateway (it serves {})",
                region
                    .as_deref()
                    .map(|r| format!(" ({r})"))
                    .unwrap_or_default(),
                GATEWAY_REGIONS.join(", ")
            ),
            GatewayProblem::PaidPlan => {
                "the Neon AI Gateway needs a paid Neon plan (Launch or Scale)".to_owned()
            }
            GatewayProblem::Credits { .. } => {
                "the organization has no Neon AI Gateway credits: add credits to call its models"
                    .to_owned()
            }
            GatewayProblem::NoModels { reason } => format!(
                "the account may call none of the gateway's models{}",
                reason
                    .as_deref()
                    .map(|r| format!(" ({r})"))
                    .unwrap_or_default()
            ),
            GatewayProblem::Unreachable { message } => {
                format!("the Neon AI Gateway didn't answer: {message}")
            }
        }
    }
}

/// A branch's AI Gateway, as the Models choice shows it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct NeonGateway {
    pub project_id: String,
    /// The branch's id.
    pub branch: String,
    pub branch_name: String,
    /// Its bare host, when the region serves one.
    pub base_url: Option<String>,
    /// The models it serves to the account (`enabled: true` only).
    pub models: Vec<GatewayModel>,
    /// How many of its models the account may not call yet.
    pub locked: u32,
    /// The models the environment's MLflow is seeded with that it doesn't serve.
    pub missing_seeded: Vec<String>,
    /// Why it can't serve the environment's models, if it can't.
    pub problem: Option<GatewayProblem>,
    /// The branch's AI Gateway page in the Neon Console.
    pub console_url: String,
}

/// The branch's AI Gateway page in the Neon Console: plan, credits and locked models.
pub fn console_url(project: &str, branch: &str) -> String {
    format!("https://console.neon.tech/app/projects/{project}/branches/{branch}/ai-gateway")
}

/// What the Neon CLI says of a project: where it runs and its organization's plan.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ProjectInfo {
    /// `aws-eu-central-1`, …
    pub region: Option<String>,
    /// The organization's plan (`free`, `launch`, `scale`, …).
    pub plan: Option<String>,
}

impl ProjectInfo {
    /// What is wrong before the gateway is asked: a free plan, a region it doesn't serve.
    pub fn problem(&self) -> Option<GatewayProblem> {
        if self
            .plan
            .as_deref()
            .is_some_and(|p| p.eq_ignore_ascii_case("free"))
        {
            return Some(GatewayProblem::PaidPlan);
        }
        match &self.region {
            Some(region) if !GATEWAY_REGIONS.contains(&region.as_str()) => {
                Some(GatewayProblem::Region {
                    region: Some(region.clone()),
                })
            }
            _ => None,
        }
    }
}

/// Whether an error from `GET …/ai_gateway` says the region has no gateway (its 404,
/// `AI_GATEWAY_NOT_ENABLED` / `ai_gateway_unavailable`), as the Neon CLI prints it.
pub fn says_unavailable(stderr: &str) -> bool {
    let s = stderr.to_ascii_lowercase();
    s.contains("ai_gateway_not_enabled")
        || s.contains("ai_gateway_unavailable")
        || (s.contains("gateway") && (s.contains("not available") || s.contains("not enabled")))
}

/// The models a `/v1/models` answer lists.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ModelList {
    /// `enabled: true`.
    pub enabled: Vec<GatewayModel>,
    /// How many aren't.
    pub locked: u32,
    /// Why they aren't: the answer's own `request_context_availability`, else the most
    /// common `unavailable_reason`.
    pub reason: Option<String>,
    /// Where to fix it (the first locked model's `recovery_url`).
    pub recovery_url: Option<String>,
}

#[derive(Deserialize)]
struct ListBody {
    #[serde(default)]
    data: Vec<ModelEntry>,
    #[serde(default)]
    request_context_availability: Option<serde_json::Value>,
}

#[derive(Deserialize)]
struct ModelEntry {
    id: String,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    display_name: Option<String>,
    #[serde(default)]
    owned_by: String,
    #[serde(default)]
    enabled: bool,
    #[serde(default)]
    unavailable_reason: Option<String>,
    #[serde(default)]
    recovery_url: Option<String>,
}

/// Parse the branch gateway's `GET /v1/models` (an OpenAI-style list).
pub fn parse_models(body: &[u8]) -> Result<ModelList, String> {
    let list: ListBody =
        serde_json::from_slice(body).map_err(|e| format!("`/v1/models` didn't parse: {e}"))?;
    let mut out = ModelList::default();
    let mut reasons: Vec<(String, u32)> = Vec::new();
    for m in list.data {
        if m.enabled {
            out.enabled.push(GatewayModel {
                name: m.display_name.or(m.name).unwrap_or_else(|| m.id.clone()),
                id: m.id,
                owned_by: m.owned_by,
            });
            continue;
        }
        out.locked += 1;
        if out.recovery_url.is_none() {
            out.recovery_url = m.recovery_url;
        }
        if let Some(reason) = m.unavailable_reason {
            match reasons.iter_mut().find(|(r, _)| *r == reason) {
                Some((_, n)) => *n += 1,
                None => reasons.push((reason, 1)),
            }
        }
    }
    let context = list
        .request_context_availability
        .as_ref()
        .and_then(|c| c.get("http"))
        .filter(|h| h.get("status").and_then(|s| s.as_str()) == Some("unavailable"))
        .and_then(|h| h.get("reason"))
        .and_then(|r| r.as_str())
        .map(str::to_owned);
    out.reason = context.or_else(|| {
        reasons
            .into_iter()
            .max_by_key(|(_, n)| *n)
            .map(|(reason, _)| reason)
    });
    out.enabled.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(out)
}

/// A reason Neon gives for a locked model or request, as a problem.
fn reason_problem(reason: &str, recovery_url: Option<String>) -> GatewayProblem {
    let r = reason.to_ascii_lowercase();
    if r.contains("credit") {
        GatewayProblem::Credits { recovery_url }
    } else if r.contains("plan") {
        GatewayProblem::PaidPlan
    } else {
        GatewayProblem::NoModels {
            reason: Some(reason.to_owned()),
        }
    }
}

/// What a `/v1/models` answer means for the option: the list, or the problem.
pub fn interpret(status: u16, body: &[u8]) -> Result<ModelList, GatewayProblem> {
    if (200..300).contains(&status) {
        let list = parse_models(body).map_err(|message| GatewayProblem::Unreachable { message })?;
        if list.enabled.is_empty() {
            return Err(match &list.reason {
                Some(reason) => reason_problem(reason, list.recovery_url.clone()),
                None => GatewayProblem::NoModels { reason: None },
            });
        }
        return Ok(list);
    }
    // An error body: Neon's `{error_code, message, unavailable_reason, recovery_url}`, or
    // `{error: {message, recovery_url}}`.
    let json: serde_json::Value = serde_json::from_slice(body).unwrap_or_default();
    let field = |key: &str| {
        json.get(key)
            .or_else(|| json.get("error").and_then(|e| e.get(key)))
            .and_then(|v| v.as_str())
            .map(str::to_owned)
    };
    if let Some(reason) = field("unavailable_reason") {
        return Err(reason_problem(&reason, field("recovery_url")));
    }
    let message = field("message").unwrap_or_else(|| String::from_utf8_lossy(body).into_owned());
    Err(match status {
        402 => GatewayProblem::PaidPlan,
        403 if message.to_ascii_lowercase().contains("plan") => GatewayProblem::PaidPlan,
        429 if message.to_ascii_lowercase().contains("credit") => GatewayProblem::Credits {
            recovery_url: field("recovery_url"),
        },
        _ => GatewayProblem::Unreachable {
            message: format!("HTTP {status}: {}", message.trim()),
        },
    })
}

/// The seeded endpoints (MLflow's `ai_gateway_models`: `provider:model` pairs) the
/// gateway doesn't serve, by the model id the `neon` upstream asks it for.
pub fn missing_seeded(seeded: &[String], served: &[GatewayModel]) -> Vec<String> {
    seeded
        .iter()
        .filter(|id| !served.iter().any(|m| &m.id == *id))
        .cloned()
        .collect()
}

/// `GET <base_url>/v1/models` with a bearer: the status and the body.
#[async_trait]
pub trait ModelsApi: Send + Sync {
    async fn get_models(&self, base_url: &str, token: &str) -> Result<(u16, Vec<u8>), String>;
}

/// [`ModelsApi`] over HTTP.
pub struct HttpModels(reqwest::Client);

impl Default for HttpModels {
    fn default() -> Self {
        HttpModels(
            reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(20))
                .build()
                .unwrap_or_default(),
        )
    }
}

#[async_trait]
impl ModelsApi for HttpModels {
    async fn get_models(&self, base_url: &str, token: &str) -> Result<(u16, Vec<u8>), String> {
        let url = format!("{}/v1/models", base_url.trim_end_matches('/'));
        let response = self
            .0
            .get(&url)
            .bearer_auth(token)
            .send()
            .await
            .map_err(|e| format!("GET {url}: {e}"))?;
        let status = response.status().as_u16();
        let body = response
            .bytes()
            .await
            .map_err(|e| format!("GET {url}: {e}"))?;
        Ok((status, body.to_vec()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CREDITS: &[u8] = include_bytes!("../tests/fixtures/neon/v1-models-credits-required.json");
    const ENABLED: &[u8] = include_bytes!("../tests/fixtures/neon/v1-models-enabled.json");
    const UNAUTHORIZED: &[u8] =
        include_bytes!("../tests/fixtures/neon/v1-models-unauthorized.json");

    #[test]
    fn only_enabled_models_are_listed() {
        let list = parse_models(ENABLED).unwrap();
        let ids: Vec<&str> = list.enabled.iter().map(|m| m.id.as_str()).collect();
        assert!(ids.contains(&"claude-opus-5-5"));
        assert_eq!(list.locked, 0);
        let opus = list
            .enabled
            .iter()
            .find(|m| m.id == "claude-opus-5-5")
            .unwrap();
        assert_eq!(opus.name, "Claude Opus 5.5");
        assert_eq!(opus.owned_by, "anthropic");

        let body = br#"{"object":"list","data":[
            {"id":"a","owned_by":"x","enabled":true},
            {"id":"b","owned_by":"x","enabled":false,"unavailable_reason":"model_restricted"}]}"#;
        let list = parse_models(body).unwrap();
        assert_eq!(list.enabled.len(), 1);
        assert_eq!(list.locked, 1);
        assert_eq!(list.reason.as_deref(), Some("model_restricted"));
    }

    #[test]
    fn no_credits_is_the_credits_problem_with_its_billing_link() {
        // The real answer of a branch whose organization has no credits.
        match interpret(200, CREDITS) {
            Err(GatewayProblem::Credits { recovery_url }) => {
                assert!(recovery_url.unwrap().contains("billing#ai_gateway_credits"));
            }
            other => panic!("{other:?}"),
        }
        // The documented 429 on a call.
        let body = br#"{"error_code":"REQUEST_LIMIT_EXCEEDED","message":"AI Gateway credit balance is below the required minimum.","unavailable_reason":"credits_required","recovery_url":"https://console.neon.tech/x"}"#;
        assert_eq!(
            interpret(429, body),
            Err(GatewayProblem::Credits {
                recovery_url: Some("https://console.neon.tech/x".into())
            })
        );
    }

    #[test]
    fn a_paid_plan_problem_reads_from_the_reason_or_the_status() {
        let body = br#"{"object":"list","data":[{"id":"a","enabled":false,"unavailable_reason":"paid_plan_required"}]}"#;
        assert_eq!(interpret(200, body), Err(GatewayProblem::PaidPlan));
        assert_eq!(interpret(402, b"{}"), Err(GatewayProblem::PaidPlan));
        assert_eq!(
            interpret(403, br#"{"message":"upgrade to a paid plan"}"#),
            Err(GatewayProblem::PaidPlan)
        );
        let info = ProjectInfo {
            region: Some("aws-eu-central-1".into()),
            plan: Some("free".into()),
        };
        assert_eq!(info.problem(), Some(GatewayProblem::PaidPlan));
    }

    #[test]
    fn a_region_without_the_gateway_is_the_region_problem() {
        let info = ProjectInfo {
            region: Some("azure-eastus2".into()),
            plan: Some("scale".into()),
        };
        assert_eq!(
            info.problem(),
            Some(GatewayProblem::Region {
                region: Some("azure-eastus2".into())
            })
        );
        assert_eq!(
            ProjectInfo {
                region: Some("aws-eu-central-1".into()),
                plan: Some("scale".into())
            }
            .problem(),
            None
        );
        assert!(says_unavailable(
            r#"{"code":"AI_GATEWAY_NOT_ENABLED","reason":"ai_gateway_unavailable"}"#
        ));
        assert!(says_unavailable(
            "ERROR: AI gateway is not available in this region"
        ));
        assert!(!says_unavailable("ERROR: branch not found in this project"));
    }

    #[test]
    fn a_refused_credential_is_unreachable_with_neons_message() {
        match interpret(401, UNAUTHORIZED) {
            Err(GatewayProblem::Unreachable { message }) => {
                assert!(message.starts_with("HTTP 401: invalid or missing credential"));
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn seeded_models_the_gateway_lacks_are_named() {
        let served = parse_models(ENABLED).unwrap().enabled;
        let seeded = vec!["claude-opus-5-5".to_owned(), "claude-opus-9".to_owned()];
        assert_eq!(missing_seeded(&seeded, &served), ["claude-opus-9"]);
    }
}
