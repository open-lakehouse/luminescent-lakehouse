//! Callers as principals of the host's identity provider, and what each may
//! do, from configuration.
//!
//! An [`Authenticate`] says who a request's caller is (a [`Principal`], and
//! the groups it is in); [`Principals`] says what that caller may do (a
//! [`Grant`]). Neither knows about the other's host, so the same map serves
//! Neon Auth's users and service accounts ([`super::olai`]) and, later, a
//! Databricks App's.
//!
//! The roles the map hands out are [`Roles`]: named tool policies, the one
//! declaration both the map and the [role catalog](Roles::catalog) read. A
//! role is what the server enforces, so a gateway route or an agent's tool
//! allowlist derived from the catalog can't ask for more than the server
//! grants.

use std::collections::{BTreeMap, HashMap};
use std::fmt;
use std::str::FromStr;

use async_trait::async_trait;
use axum::http::request::Parts;
use factory_core::tool::{ToolSpec, Visibility};
use serde::Serialize;

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

/// An authenticated caller: the [`Principal`], and the names of the groups it
/// is in, directly or through other groups, where they were asked for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Identity {
    pub principal: Principal,
    pub groups: Vec<String>,
}

impl From<Principal> for Identity {
    fn from(principal: Principal) -> Self {
        Self {
            principal,
            groups: Vec::new(),
        }
    }
}

/// Finds out who a request's caller is.
#[async_trait]
pub trait Authenticate: Send + Sync + 'static {
    /// The caller, or why there is none: a 401 for a missing or invalid
    /// credential, a 403 for a caller the provider refuses (banned), a 503
    /// when it can't be asked.
    async fn principal(&self, req: &Parts) -> Result<Principal, Rejection>;

    /// The caller and, with `groups`, the groups it is in. A provider without
    /// groups answers none, so only direct entries can match.
    async fn identity(&self, req: &Parts, groups: bool) -> Result<Identity, Rejection> {
        let _ = groups;
        Ok(self.principal(req).await?.into())
    }
}

/// The roles' variable, as [`Roles::from_env`] reads it.
pub const ROLES_ENV: &str = "FACTORY_MCP_ROLES";

/// Named tool policies: `NAME=POLICY` entries (`planner=@read,add_item`).
///
/// The framework still has no roles of its own: a composition names them in
/// configuration. [`Principals`] entries refer to them by name, and the
/// [catalog](Self::catalog) publishes them, so the server's enforcement and
/// what a host builds on it (a gateway route per role, an agent's tool
/// allowlist) come from one declaration.
#[derive(Debug, Clone, Default)]
pub struct Roles {
    roles: BTreeMap<String, ToolPolicy>,
}

impl Roles {
    /// The roles of a configuration: `NAME=POLICY` entries.
    pub fn from_config(roles: &[String]) -> Result<Self, String> {
        let mut out = BTreeMap::new();
        for spec in roles {
            let (name, policy) = spec
                .split_once('=')
                .ok_or_else(|| format!("{spec:?} wants NAME=POLICY, got no `=`"))?;
            let name = name.trim();
            if !valid_role(name) {
                return Err(format!("invalid role name {name:?}"));
            }
            let policy: ToolPolicy = policy
                .parse()
                .map_err(|e| format!("role {name}'s policy: {e}"))?;
            if out.insert(name.to_owned(), policy).is_some() {
                return Err(format!("role {name} is defined twice"));
            }
        }
        Ok(Self { roles: out })
    }

    /// [`from_config`](Self::from_config) over `FACTORY_MCP_ROLES`,
    /// `;`-separated. Unset is none.
    pub fn from_env() -> Result<Self, String> {
        Self::from_config(&env_list(ROLES_ENV)).map_err(|e| format!("{ROLES_ENV}: {e}"))
    }

    pub fn get(&self, name: &str) -> Option<&ToolPolicy> {
        self.roles.get(name)
    }

    pub fn len(&self) -> usize {
        self.roles.len()
    }

    pub fn is_empty(&self) -> bool {
        self.roles.is_empty()
    }

    /// Each role with the tools its policy allows out of `tools`, by name, in
    /// `tools`' order. App-only tools are left out: a role never carries the
    /// right to MCP Apps, which only a person's own entry can.
    pub fn catalog(&self, tools: &[ToolSpec]) -> RoleCatalog {
        RoleCatalog {
            roles: self
                .roles
                .iter()
                .map(|(name, policy)| CatalogRole {
                    name: name.clone(),
                    policy: policy.to_string(),
                    tools: tools
                        .iter()
                        .filter(|t| t.visibility != Visibility::App && policy.allows(t))
                        .map(|t| t.name.clone())
                        .collect(),
                })
                .collect(),
        }
    }
}

/// What [`Roles::catalog`] publishes: no principal and no credential, only
/// which roles the server grants and the tools each reaches.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct RoleCatalog {
    pub roles: Vec<CatalogRole>,
}

/// One role of a [`RoleCatalog`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CatalogRole {
    pub name: String,
    /// The policy as configured (`@read,annotate`).
    pub policy: String,
    /// The tools the policy allows, by name.
    pub tools: Vec<String>,
}

/// The principals' variables, as [`Principals::from_env`] reads them.
pub const PRINCIPALS_ENV: &str = "FACTORY_MCP_PRINCIPALS";
/// The people who get the right to MCP Apps.
pub const APP_PRINCIPALS_ENV: &str = "FACTORY_MCP_APP_PRINCIPALS";

/// Who an entry is for: one principal, or every member of a group.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum Subject {
    Principal(Principal),
    Group(String),
}

impl fmt::Display for Subject {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Principal(p) => p.fmt(f),
            Self::Group(g) => write!(f, "group:{g}"),
        }
    }
}

impl FromStr for Subject {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.split_once(':') {
            Some(("group", name)) => {
                if name.is_empty() || name.contains(['=', ';']) || name.trim() != name {
                    return Err(format!("{s:?} has an invalid name"));
                }
                Ok(Self::Group(name.into()))
            }
            _ => s.parse().map(Self::Principal).map_err(|_| {
                format!("expected user:<id>, service-account:<name> or group:<name>, got {s:?}")
            }),
        }
    }
}

/// What each principal may do: `SUBJECT=ROLE` or `SUBJECT=ROLE:POLICY`
/// entries.
///
/// - `SUBJECT` is `user:<id>`, `service-account:<name>`, or `group:<name>`
///   for every member of the group, directly or through other groups.
/// - `ROLE` names one of the [`Roles`]; or, with `:POLICY`, labels a policy
///   given inline (a name the roles define can't be redefined that way).
/// - The actor writes are attributed to is always the principal itself, never
///   the group.
///
/// A principal's own entry comes first; without one, its groups' entries
/// decide, and they must agree on the grant. A principal with neither may do
/// nothing (a 403). The app principals are people who also get
/// [`Grant::apps`]: each needs an entry of its own, never only a group's, and
/// a service account can never be one, so no agent can approve its own work.
#[derive(Debug, Clone, Default)]
pub struct Principals {
    grants: HashMap<Principal, Grant>,
    groups: HashMap<String, Grant>,
    roles: Roles,
}

impl Principals {
    /// The map of a configuration without named roles: `principals` are
    /// `SUBJECT=ROLE:POLICY`, `app_principals` are `user:<id>`.
    pub fn from_config(principals: &[String], app_principals: &[String]) -> Result<Self, String> {
        Self::with_roles(Roles::default(), principals, app_principals)
    }

    /// The map of a configuration whose entries may name `roles`.
    pub fn with_roles(
        roles: Roles,
        principals: &[String],
        app_principals: &[String],
    ) -> Result<Self, String> {
        let mut grants = HashMap::new();
        let mut groups = HashMap::new();
        for spec in principals {
            let (subject, rest) = spec
                .split_once('=')
                .ok_or_else(|| format!("{spec:?} wants SUBJECT=ROLE, got no `=`"))?;
            let subject: Subject = subject.trim().parse()?;
            let (role, policy) = match rest.split_once(':') {
                Some((role, policy)) => {
                    if roles.get(role).is_some() {
                        return Err(format!(
                            "{subject} redefines role {role}, which {ROLES_ENV} defines: \
                             name it without a policy"
                        ));
                    }
                    let policy: ToolPolicy = policy
                        .parse()
                        .map_err(|e| format!("{subject}'s policy: {e}"))?;
                    (role, policy)
                }
                None => {
                    let role = rest.trim();
                    let policy = roles.get(role).cloned().ok_or_else(|| {
                        format!(
                            "{subject} names role {role:?}, which {ROLES_ENV} doesn't define \
                             (or give ROLE:POLICY)"
                        )
                    })?;
                    (role, policy)
                }
            };
            if !valid_role(role) {
                return Err(format!("{subject} has an invalid role {role:?}"));
            }
            let twice = match &subject {
                Subject::Principal(p) => grants
                    .insert(p.clone(), Grant::new(p.to_string(), policy).role(role))
                    .is_some(),
                // The actor is filled in with the member's own name.
                Subject::Group(g) => groups
                    .insert(g.clone(), Grant::new("", policy).role(role))
                    .is_some(),
            };
            if twice {
                return Err(format!("{subject} has two entries"));
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
        Ok(Self {
            grants,
            groups,
            roles,
        })
    }

    /// [`with_roles`](Self::with_roles) over `FACTORY_MCP_ROLES`,
    /// `FACTORY_MCP_PRINCIPALS` and `FACTORY_MCP_APP_PRINCIPALS`,
    /// `;`-separated. Unset is empty: every caller is refused.
    pub fn from_env() -> Result<Self, String> {
        Self::with_roles(
            Roles::from_env()?,
            &env_list(PRINCIPALS_ENV),
            &env_list(APP_PRINCIPALS_ENV),
        )
        .map_err(|e| format!("{PRINCIPALS_ENV}/{APP_PRINCIPALS_ENV}: {e}"))
    }

    /// `principal`'s grant, if it has an entry of its own. Only a person's
    /// carries [`Grant::apps`].
    pub fn lookup(&self, principal: &Principal) -> Option<Grant> {
        let mut grant = self.grants.get(principal)?.clone();
        grant.apps &= principal.kind == PrincipalKind::User;
        Some(grant)
    }

    /// The caller's grant: its own entry, or else its groups'. Groups whose
    /// entries disagree grant nothing, as no entry does: membership alone
    /// mustn't decide between two roles.
    pub fn resolve(&self, identity: &Identity) -> Option<Grant> {
        if let Some(grant) = self.lookup(&identity.principal) {
            return Some(grant);
        }
        let mut found: Option<&Grant> = None;
        for group in &identity.groups {
            let Some(grant) = self.groups.get(group) else {
                continue;
            };
            match found {
                Some(seen) if seen != grant => {
                    tracing::warn!(
                        principal = %identity.principal,
                        "the principal's groups map to different grants; refusing"
                    );
                    return None;
                }
                _ => found = Some(grant),
            }
        }
        let mut grant = found?.clone();
        grant.actor = identity.principal.to_string();
        Some(grant)
    }

    /// Whether any entry is a group's, so a caller's groups are worth asking for.
    pub fn has_groups(&self) -> bool {
        !self.groups.is_empty()
    }

    /// The named roles the entries may refer to.
    pub fn roles(&self) -> &Roles {
        &self.roles
    }

    /// The number of entries, principals' and groups'.
    pub fn len(&self) -> usize {
        self.grants.len() + self.groups.len()
    }

    pub fn is_empty(&self) -> bool {
        self.grants.is_empty() && self.groups.is_empty()
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
        let identity = self
            .authn
            .identity(req, self.principals.has_groups())
            .await?;
        self.principals.resolve(&identity).ok_or_else(|| {
            tracing::debug!(principal = %identity.principal, "no grant for the principal");
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
        // Without `:`, the entry names a role, which must be defined.
        assert!(
            cfg(&["service-account:p=@read"], &[])
                .unwrap_err()
                .contains("doesn't define")
        );
        assert!(
            cfg(&["service-account:p=a b:@read"], &[])
                .unwrap_err()
                .contains("role")
        );
        assert!(cfg(&["service-account:p=:@read"], &[]).is_err());
        assert!(cfg(&["service-account:p=r:@nope"], &[]).is_err());
        assert!(cfg(&["group:=r:*"], &[]).is_err());
        assert!(cfg(&["team:g=r:*"], &[]).is_err());
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

    const ROLES: &[&str] = &[
        "planner=@read,add_item,add_edge,annotate,submit",
        "executor=@read,add_item,add_edge,annotate,propose,report_change",
        "reviewer=@read,annotate",
    ];

    fn roles() -> Roles {
        Roles::from_config(&own(ROLES)).unwrap()
    }

    fn identity(principal: &str, groups: &[&str]) -> Identity {
        Identity {
            principal: principal.parse().unwrap(),
            groups: own(groups),
        }
    }

    #[test]
    fn roles_parse_and_refuse_bad_entries() {
        assert_eq!(roles().len(), 3);
        for (bad, needle) in [
            ("planner", "`=`"),
            ("a b=*", "invalid role name"),
            ("p=@nope", "policy"),
        ] {
            let err = Roles::from_config(&own(&[bad])).unwrap_err();
            assert!(err.contains(needle), "{bad}: {err}");
        }
        let twice = Roles::from_config(&own(&["p=*", "p=@read"])).unwrap_err();
        assert!(twice.contains("twice"), "{twice}");
    }

    #[test]
    fn entries_name_roles_and_cant_redefine_them() {
        let set = Principals::with_roles(
            roles(),
            &own(&[
                "service-account:factory-planner-e1=planner",
                "user:u1=operator:*",
            ]),
            &[],
        )
        .unwrap();
        let planner = set
            .lookup(&Principal::service_account("factory-planner-e1"))
            .unwrap();
        assert_eq!(planner.role, "planner");
        assert_eq!(
            planner.policy.to_string(),
            "@read,add_item,add_edge,annotate,submit"
        );
        let redefine = Principals::with_roles(roles(), &own(&["service-account:p=planner:*"]), &[])
            .unwrap_err();
        assert!(redefine.contains("redefines role planner"), "{redefine}");
        let unknown =
            Principals::with_roles(roles(), &own(&["service-account:p=admin"]), &[]).unwrap_err();
        assert!(unknown.contains("doesn't define"), "{unknown}");
    }

    #[test]
    fn groups_grant_their_members_as_themselves() {
        let set = Principals::with_roles(
            roles(),
            &own(&[
                "group:factory-planners=planner",
                "group:factory-reviewers=reviewer",
                "group:planners-too=planner",
                "service-account:special=reviewer",
            ]),
            &[],
        )
        .unwrap();
        assert!(set.has_groups());
        let member = set
            .resolve(&identity(
                "service-account:factory-planner-e1",
                &["factory-planners", "unmapped"],
            ))
            .unwrap();
        assert_eq!(
            (member.actor.as_str(), member.role.as_str()),
            ("service-account:factory-planner-e1", "planner")
        );
        // Two groups with the same grant agree.
        let both = identity("service-account:x", &["factory-planners", "planners-too"]);
        assert_eq!(set.resolve(&both).unwrap().role, "planner");
        // Groups that disagree grant nothing.
        let torn = identity(
            "service-account:x",
            &["factory-planners", "factory-reviewers"],
        );
        assert!(set.resolve(&torn).is_none());
        // A principal's own entry comes first.
        let own_entry = identity("service-account:special", &["factory-planners"]);
        assert_eq!(set.resolve(&own_entry).unwrap().role, "reviewer");
        assert!(set.resolve(&identity("service-account:x", &[])).is_none());
    }

    #[test]
    fn apps_come_only_from_a_persons_own_entry() {
        let set = Principals::with_roles(
            Roles::default(),
            &own(&["group:operators=operator:*", "user:u1=operator:*"]),
            &own(&["user:u1"]),
        )
        .unwrap();
        assert!(
            !set.resolve(&identity("user:u2", &["operators"]))
                .unwrap()
                .apps
        );
        assert!(
            set.resolve(&identity("user:u1", &["operators"]))
                .unwrap()
                .apps
        );
        let group_app = Principals::with_roles(
            Roles::default(),
            &own(&["group:operators=operator:*"]),
            &own(&["group:operators"]),
        )
        .unwrap_err();
        assert!(
            group_app.contains("user:<id>") || group_app.contains("only people"),
            "{group_app}"
        );
    }

    #[test]
    fn the_catalog_lists_each_roles_tools_and_no_app_tools() {
        let mut tools = crate::tools::builtin();
        tools.push(
            ToolSpec::new(
                "approve_plan",
                "",
                serde_json::json!({"type": "object"}),
                Nop,
            )
            .app_only(),
        );
        let catalog = roles().catalog(&tools);
        let names: Vec<_> = catalog.roles.iter().map(|r| r.name.as_str()).collect();
        assert_eq!(names, ["executor", "planner", "reviewer"]);
        // `@read` as the server enforces it: every read-only tool a model sees,
        // the plan review's opener included (it approves nothing).
        let read = [
            "plan_read",
            "item_read",
            "ready",
            "list_repos",
            "list_profiles",
            "open_plan_review",
        ];
        let tools_of = |name: &str| {
            let mut t = catalog
                .roles
                .iter()
                .find(|r| r.name == name)
                .unwrap()
                .tools
                .clone();
            t.sort();
            t
        };
        let expect = |extra: &[&str]| {
            let mut t: Vec<String> = read.iter().chain(extra).map(|s| (*s).to_owned()).collect();
            t.sort();
            t
        };
        assert_eq!(
            tools_of("planner"),
            expect(&["add_item", "add_edge", "annotate", "submit"])
        );
        assert_eq!(
            tools_of("executor"),
            expect(&[
                "add_item",
                "add_edge",
                "annotate",
                "propose",
                "report_change"
            ])
        );
        assert_eq!(tools_of("reviewer"), expect(&["annotate"]));
        let star = Roles::from_config(&own(&["all=*"]))
            .unwrap()
            .catalog(&tools);
        assert!(!star.roles[0].tools.iter().any(|t| t == "approve_plan"));
        let json = serde_json::to_value(&catalog).unwrap();
        assert_eq!(
            json["roles"][1]["policy"],
            "@read,add_item,add_edge,annotate,submit"
        );
    }

    struct Nop;

    #[async_trait]
    impl factory_core::tool::ToolHandler for Nop {
        async fn call(
            &self,
            _: factory_core::tool::ToolCall<'_>,
        ) -> factory_core::Result<factory_core::tool::ToolOutput> {
            Ok(factory_core::tool::ToolOutput::default())
        }
    }

    /// Authenticates the bearer `sa:<name>` or `user:<id>`, refuses the rest;
    /// `sa:<name>` is in the groups `in-<name>`, when asked.
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

        async fn identity(&self, req: &Parts, groups: bool) -> Result<Identity, Rejection> {
            let principal = self.principal(req).await?;
            let groups = match (&principal.kind, groups) {
                (PrincipalKind::ServiceAccount, true) => vec![format!("in-{}", principal.name)],
                _ => Vec::new(),
            };
            Ok(Identity { principal, groups })
        }
    }

    #[tokio::test]
    async fn principal_auth_asks_for_groups_and_maps_members() {
        let auth = PrincipalAuth::new(Fake, cfg(&["group:in-agent=planner:@read"], &[]).unwrap());
        let grant = auth.grant(&parts("sa:agent")).await.unwrap();
        assert_eq!(
            (grant.actor.as_str(), grant.role.as_str()),
            ("service-account:agent", "planner")
        );
        let other = auth.grant(&parts("sa:other")).await.unwrap_err();
        assert_eq!(other.status(), StatusCode::FORBIDDEN);
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
