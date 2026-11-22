//! Callers as principals of the host's identity provider, and what each may
//! do, from configuration.
//!
//! An [`Authenticate`] says who a request's caller is (a [`Principal`]);
//! [`Principals`] says what that principal may do (a [`Grant`]). Neither
//! knows about the other's host, so the same map serves Neon Auth's users and
//! service accounts ([`super::olai`]) and, later, a Databricks App's.

use std::collections::HashMap;
use std::fmt;
use std::str::FromStr;

use async_trait::async_trait;
use axum::http::request::Parts;

use super::{CallerAuth, Grant, Rejection, env_list, valid_role};
use crate::policy::ToolPolicy;

/// What kind of caller a [`Principal`] is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum PrincipalKind {
    /// A person (`user:<id>`, the identity provider's stable user id).
    User,
    /// A non-human caller, such as an agent behind a gateway
    /// (`service-account:<name>`).
    ServiceAccount,
}

impl PrincipalKind {
    fn prefix(self) -> &'static str {
        match self {
            Self::User => "user",
            Self::ServiceAccount => "service-account",
        }
    }
}

/// A caller the host's identity provider vouched for, written
/// `user:<id>` or `service-account:<name>`, as `luminescent-admin` names
/// them.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Principal {
    pub kind: PrincipalKind,
    pub name: String,
}

impl Principal {
    pub fn user(id: impl Into<String>) -> Self {
        Self {
            kind: PrincipalKind::User,
            name: id.into(),
        }
    }

    pub fn service_account(name: impl Into<String>) -> Self {
        Self {
            kind: PrincipalKind::ServiceAccount,
            name: name.into(),
        }
    }
}

impl fmt::Display for Principal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.kind.prefix(), self.name)
    }
}

impl FromStr for Principal {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let principal = match s.split_once(':') {
            Some(("user", id)) => Self::user(id),
            Some(("service-account", name)) => Self::service_account(name),
            _ => {
                return Err(format!(
                    "expected user:<id> or service-account:<name>, got {s:?}"
                ));
            }
        };
        if principal.name.is_empty()
            || principal.name.contains(['=', ';'])
            || principal.name.trim() != principal.name
        {
            return Err(format!("{s:?} has an invalid name"));
        }
        Ok(principal)
    }
}

/// Finds out who a request's caller is.
#[async_trait]
pub trait Authenticate: Send + Sync + 'static {
    /// The caller, or why there is none: a 401 for a missing or invalid
    /// credential, a 403 for a caller the provider refuses (banned), a 503
    /// when it can't be asked.
    async fn principal(&self, req: &Parts) -> Result<Principal, Rejection>;
}

/// The principals' variables, as [`Principals::from_env`] reads them.
pub const PRINCIPALS_ENV: &str = "FACTORY_MCP_PRINCIPALS";
/// The people who get the right to MCP Apps.
pub const APP_PRINCIPALS_ENV: &str = "FACTORY_MCP_APP_PRINCIPALS";

/// What each principal may do: `PRINCIPAL=ROLE:POLICY` entries.
///
/// - `PRINCIPAL` is `user:<id>` or `service-account:<name>`.
/// - `ROLE` labels the grant (`planner`); the actor writes are attributed to
///   is the principal itself.
/// - `POLICY` is a [`ToolPolicy`] (`@read,add_item`).
///
/// A principal without an entry may do nothing (a 403). The app principals
/// are people who also get [`Grant::apps`]: each needs an entry of its own,
/// and a service account can never be one, so no agent can approve its own
/// work.
#[derive(Debug, Clone, Default)]
pub struct Principals {
    grants: HashMap<Principal, Grant>,
}

impl Principals {
    /// The map of a configuration: `principals` are `PRINCIPAL=ROLE:POLICY`,
    /// `app_principals` are `user:<id>`.
    pub fn from_config(principals: &[String], app_principals: &[String]) -> Result<Self, String> {
        let mut grants = HashMap::new();
        for spec in principals {
            let (principal, rest) = spec
                .split_once('=')
                .ok_or_else(|| format!("{spec:?} wants PRINCIPAL=ROLE:POLICY, got no `=`"))?;
            let principal: Principal = principal.trim().parse()?;
            let (role, policy) = rest
                .split_once(':')
                .ok_or_else(|| format!("{principal}'s entry wants ROLE:POLICY, got no `:`"))?;
            if !valid_role(role) {
                return Err(format!("{principal} has an invalid role {role:?}"));
            }
            let policy: ToolPolicy = policy
                .parse()
                .map_err(|e| format!("{principal}'s policy: {e}"))?;
            let grant = Grant::new(principal.to_string(), policy).role(role);
            if grants.insert(principal.clone(), grant).is_some() {
                return Err(format!("{principal} has two entries"));
            }
        }
        for spec in app_principals {
            let principal: Principal = spec.trim().parse()?;
            if principal.kind != PrincipalKind::User {
                return Err(format!(
                    "{principal} can't get MCP Apps: only people (user:<id>) can"
                ));
            }
            let Some(grant) = grants.get_mut(&principal) else {
                return Err(format!(
                    "{principal} is an app principal without an entry of its own"
                ));
            };
            grant.apps = true;
        }
        Ok(Self { grants })
    }

    /// [`from_config`](Self::from_config) over `FACTORY_MCP_PRINCIPALS` and
    /// `FACTORY_MCP_APP_PRINCIPALS`, `;`-separated. Unset is empty: every
    /// caller is refused.
    pub fn from_env() -> Result<Self, String> {
        Self::from_config(&env_list(PRINCIPALS_ENV), &env_list(APP_PRINCIPALS_ENV))
            .map_err(|e| format!("{PRINCIPALS_ENV}/{APP_PRINCIPALS_ENV}: {e}"))
    }

    /// `principal`'s grant, if it has an entry. Only a person's carries
    /// [`Grant::apps`].
    pub fn lookup(&self, principal: &Principal) -> Option<Grant> {
        let mut grant = self.grants.get(principal)?.clone();
        grant.apps &= principal.kind == PrincipalKind::User;
        Some(grant)
    }

    pub fn len(&self) -> usize {
        self.grants.len()
    }

    pub fn is_empty(&self) -> bool {
        self.grants.is_empty()
    }
}

/// An [`Authenticate`] and the [`Principals`] map behind it: an
/// authenticated caller without an entry is a 403.
pub struct PrincipalAuth<A> {
    pub authn: A,
    pub principals: Principals,
}

impl<A> PrincipalAuth<A> {
    pub fn new(authn: A, principals: Principals) -> Self {
        Self { authn, principals }
    }
}

#[async_trait]
impl<A: Authenticate> CallerAuth for PrincipalAuth<A> {
    async fn grant(&self, req: &Parts) -> Result<Grant, Rejection> {
        let principal = self.authn.principal(req).await?;
        self.principals.lookup(&principal).ok_or_else(|| {
            tracing::debug!(%principal, "no grant for the principal");
            Rejection::forbidden("the caller has no grant on this server")
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth::{AnyOf, Tokens};
    use axum::http::{Request, StatusCode, header};

    fn own(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| (*s).to_owned()).collect()
    }

    fn cfg(principals: &[&str], apps: &[&str]) -> Result<Principals, String> {
        Principals::from_config(&own(principals), &own(apps))
    }

    #[test]
    fn principals_print_and_parse() {
        let sa: Principal = "service-account:planner".parse().unwrap();
        assert_eq!(sa, Principal::service_account("planner"));
        assert_eq!(sa.to_string(), "service-account:planner");
        let user: Principal = "user:5e1f".parse().unwrap();
        assert_eq!(user.kind, PrincipalKind::User);
        for bad in ["planner", "group:x", "user:", "service-account: x", ":x"] {
            assert!(bad.parse::<Principal>().is_err(), "{bad}");
        }
    }

    #[test]
    fn entries_grant_their_role_and_policy_to_the_principal() {
        let set = cfg(
            &[
                "service-account:planner=planner:@read,add_item",
                "user:u1=operator:*",
            ],
            &[],
        )
        .unwrap();
        assert_eq!(set.len(), 2);
        let planner = set.lookup(&Principal::service_account("planner")).unwrap();
        assert_eq!(planner.actor, "service-account:planner");
        assert_eq!(planner.role, "planner");
        assert_eq!(planner.policy.to_string(), "@read,add_item");
        assert!(!planner.apps);
        assert!(set.lookup(&Principal::service_account("other")).is_none());
        // The kind is part of the name.
        assert!(set.lookup(&Principal::user("planner")).is_none());
        assert!(!set.lookup(&Principal::user("u1")).unwrap().apps);
    }

    #[test]
    fn bad_entries_are_config_errors() {
        assert!(
            cfg(&["service-account:p"], &[])
                .unwrap_err()
                .contains("`=`")
        );
        assert!(
            cfg(&["service-account:p=@read"], &[])
                .unwrap_err()
                .contains("`:`")
        );
        assert!(
            cfg(&["service-account:p=a b:@read"], &[])
                .unwrap_err()
                .contains("role")
        );
        assert!(cfg(&["service-account:p=:@read"], &[]).is_err());
        assert!(cfg(&["service-account:p=r:@nope"], &[]).is_err());
        assert!(cfg(&["group:g=r:*"], &[]).is_err());
        let twice = cfg(&["user:u=a:*", "user:u=b:@read"], &[]).unwrap_err();
        assert!(twice.contains("two entries"), "{twice}");
    }

    #[test]
    fn only_people_with_an_entry_get_apps() {
        let set = cfg(&["user:u1=operator:*", "user:u2=operator:*"], &["user:u1"]).unwrap();
        assert!(set.lookup(&Principal::user("u1")).unwrap().apps);
        assert!(!set.lookup(&Principal::user("u2")).unwrap().apps);

        let sa = cfg(&["service-account:p=planner:*"], &["service-account:p"]).unwrap_err();
        assert!(sa.contains("only people"), "{sa}");
        let missing = cfg(&["user:u1=operator:*"], &["user:u3"]).unwrap_err();
        assert!(missing.contains("without an entry"), "{missing}");
    }

    /// Authenticates the bearer `sa:<name>` or `user:<id>`, refuses the rest.
    struct Fake;

    #[async_trait]
    impl Authenticate for Fake {
        async fn principal(&self, req: &Parts) -> Result<Principal, Rejection> {
            match crate::auth::bearer(&req.headers) {
                Some(t) if t.starts_with("sa:") => Ok(Principal::service_account(&t[3..])),
                Some(t) if t.starts_with("user:") => Ok(Principal::user(&t[5..])),
                _ => Err(Rejection::unauthorized("no")),
            }
        }
    }

    fn parts(bearer: &str) -> Parts {
        Request::builder()
            .header(header::AUTHORIZATION, format!("Bearer {bearer}"))
            .body(())
            .unwrap()
            .into_parts()
            .0
    }

    #[tokio::test]
    async fn principal_auth_maps_or_refuses() {
        let auth = PrincipalAuth::new(
            Fake,
            cfg(
                &["service-account:p=planner:@read", "user:u=operator:*"],
                &["user:u"],
            )
            .unwrap(),
        );
        let grant = auth.grant(&parts("sa:p")).await.unwrap();
        assert_eq!(
            (grant.actor.as_str(), grant.apps),
            ("service-account:p", false)
        );
        assert!(auth.grant(&parts("user:u")).await.unwrap().apps);
        let unknown = auth.grant(&parts("sa:stranger")).await.unwrap_err();
        assert_eq!(unknown.status(), StatusCode::FORBIDDEN);
        let invalid = auth.grant(&parts("garbage")).await.unwrap_err();
        assert_eq!(invalid.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn static_tokens_go_first_and_the_rest_falls_through() {
        let tokens = Tokens::from_config(&[], &own(&["wg_desk=*"])).unwrap();
        let auth = AnyOf {
            tokens,
            rest: PrincipalAuth::new(
                Fake,
                cfg(&["service-account:p=planner:@read"], &[]).unwrap(),
            ),
        };
        let desk = auth.grant(&parts("wg_desk")).await.unwrap();
        assert!(desk.apps && desk.role == "app");
        assert_eq!(auth.grant(&parts("sa:p")).await.unwrap().role, "planner");
        let unknown = auth.grant(&parts("wg_nope")).await.unwrap_err();
        assert_eq!(unknown.status(), StatusCode::UNAUTHORIZED);
    }
}
