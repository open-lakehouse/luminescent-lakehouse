//! [`AppsProxy`]: callers as a Databricks App's proxy forwards them.
//!
//! A Databricks App is only reachable through the workspace's Apps proxy. The
//! proxy accepts a Databricks credential from any identity with `CAN_USE` on
//! the App (a person's sign-in, or a service principal's OAuth token), and
//! forwards who the caller is in headers:
//!
//! - `X-Forwarded-User`: the identity provider's id of the caller
//! - `X-Forwarded-Email`: the caller's email
//! - `X-Forwarded-Preferred-Username`: the caller's user name
//!
//! These headers are trustworthy only behind the proxy: anyone who reaches
//! the server directly can send them. So [`AppsProxy`] is for a server that
//! runs as an App, where the proxy is the only way in, and a host turns it
//! on explicitly. Outside Apps mode nothing reads them.
//!
//! The caller becomes a [`Principal`]: `user:<email>` (lower case) for a
//! person, whose forwarded email has an `@`; `service-account:<application
//! id>` for a service principal, whose forwarded email (or, without one,
//! user name) is its application id. A request without `X-Forwarded-User`,
//! or with neither of the others, didn't come through the proxy and is
//! refused (401).
//!
//! The proxy forwards no groups. With a [`GroupSource`] (the workspace's
//! SCIM API, say), a caller's groups are looked up when the
//! [`Principals`](super::Principals) map has group entries.

use std::sync::Arc;

use async_trait::async_trait;
use axum::http::HeaderMap;
use axum::http::request::Parts;

use super::{Authenticate, Identity, Principal, Rejection};

/// The caller's id, as the Apps proxy forwards it.
pub const USER_HEADER: &str = "x-forwarded-user";
/// The caller's email: a person's address, a service principal's
/// application id.
pub const EMAIL_HEADER: &str = "x-forwarded-email";
/// The caller's user name.
pub const USERNAME_HEADER: &str = "x-forwarded-preferred-username";
/// The caller's own OAuth token, which the proxy forwards when the App has
/// user authorization scopes, downscoped to them. Never logged.
pub const ACCESS_TOKEN_HEADER: &str = "x-forwarded-access-token";

/// Every header the proxy's identity is in: what a server outside Apps mode
/// must not act on, and what a server in it takes only from the proxy.
pub const IDENTITY_HEADERS: [&str; 4] = [
    USER_HEADER,
    EMAIL_HEADER,
    USERNAME_HEADER,
    ACCESS_TOKEN_HEADER,
];

/// The groups a principal is in, from wherever the workspace keeps them.
#[async_trait]
pub trait GroupSource: Send + Sync + 'static {
    /// The names of `principal`'s groups, or why they couldn't be read.
    ///
    /// `token` is the caller's own token when the proxy forwarded one
    /// ([`ACCESS_TOKEN_HEADER`]), so a source can read the caller's groups
    /// as the caller. A workspace shows a non-admin service principal, such
    /// as an App's own, no one else's groups, but it shows everyone theirs.
    async fn groups(
        &self,
        principal: &Principal,
        token: Option<&str>,
    ) -> Result<Vec<String>, String>;
}

/// An [`Authenticate`] over the Apps proxy's forwarded identity. See the
/// [module docs](self).
#[derive(Clone, Default)]
pub struct AppsProxy {
    groups: Option<Arc<dyn GroupSource>>,
}

impl std::fmt::Debug for AppsProxy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AppsProxy")
            .field("groups", &self.groups.is_some())
            .finish()
    }
}

impl AppsProxy {
    pub fn new() -> Self {
        Self::default()
    }

    /// Look callers' groups up in `source` when the map asks for them.
    #[must_use]
    pub fn with_groups(mut self, source: impl GroupSource) -> Self {
        self.groups = Some(Arc::new(source));
        self
    }

    /// The caller the proxy forwarded in `headers`, or `None` when the
    /// request didn't come through the proxy.
    pub fn principal_of(headers: &HeaderMap) -> Option<Principal> {
        let header = |name: &str| {
            headers
                .get(name)
                .and_then(|v| v.to_str().ok())
                .map(str::trim)
                .filter(|v| !v.is_empty())
        };
        let user = header(USER_HEADER)?;
        let name = header(EMAIL_HEADER).or_else(|| header(USERNAME_HEADER))?;
        let principal = if name.contains('@') {
            Principal::user(name.to_lowercase())
        } else {
            Principal::service_account(name)
        };
        // Principal's own rules (no `=`, `;`, or surrounding space).
        let principal: Principal = principal.to_string().parse().ok()?;
        tracing::debug!(%principal, forwarded_user = user, "apps proxy identity");
        Some(principal)
    }

    fn refused() -> Rejection {
        Rejection::unauthorized("no Databricks Apps proxy identity")
    }
}

#[async_trait]
impl Authenticate for AppsProxy {
    async fn principal(&self, req: &Parts) -> Result<Principal, Rejection> {
        Self::principal_of(&req.headers).ok_or_else(Self::refused)
    }

    async fn identity(&self, req: &Parts, groups: bool) -> Result<Identity, Rejection> {
        let principal = Self::principal_of(&req.headers).ok_or_else(Self::refused)?;
        let Some(source) = self.groups.as_ref().filter(|_| groups) else {
            return Ok(principal.into());
        };
        let token = req
            .headers
            .get(ACCESS_TOKEN_HEADER)
            .and_then(|v| v.to_str().ok())
            .map(str::trim)
            .filter(|v| !v.is_empty());
        let groups = source.groups(&principal, token).await.map_err(|error| {
            tracing::warn!(%principal, %error, "reading the caller's groups");
            Rejection::unavailable("the caller's groups couldn't be read")
        })?;
        Ok(Identity { principal, groups })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth::{CallerAuth, PrincipalAuth, Principals, Roles};
    use axum::http::{Request, StatusCode};

    fn parts(headers: &[(&str, &str)]) -> Parts {
        let mut req = Request::builder().uri("/mcp");
        for (k, v) in headers {
            req = req.header(*k, *v);
        }
        req.body(()).unwrap().into_parts().0
    }

    fn person(email: &str) -> Vec<(&'static str, String)> {
        vec![
            (USER_HEADER, "4711@1234".to_owned()),
            (EMAIL_HEADER, email.to_owned()),
            (USERNAME_HEADER, email.to_owned()),
        ]
    }

    fn of(headers: &[(&str, String)]) -> Result<Principal, StatusCode> {
        let headers: Vec<(&str, &str)> = headers.iter().map(|(k, v)| (*k, v.as_str())).collect();
        AppsProxy::principal_of(&parts(&headers).headers).ok_or(StatusCode::UNAUTHORIZED)
    }

    #[test]
    fn people_and_service_principals_become_principals() {
        assert_eq!(
            of(&person("Ada@Example.com")),
            Ok(Principal::user("ada@example.com"))
        );
        let sp = "6f0e5b9c-1d2a-4c3b-9e8f-0a1b2c3d4e5f";
        assert_eq!(
            of(&[(USER_HEADER, "99@1234".into()), (EMAIL_HEADER, sp.into())]),
            Ok(Principal::service_account(sp))
        );
        // Without an email, the user name.
        assert_eq!(
            of(&[
                (USER_HEADER, "99@1234".into()),
                (USERNAME_HEADER, sp.into())
            ]),
            Ok(Principal::service_account(sp))
        );
    }

    #[test]
    fn a_request_without_the_proxy_headers_is_refused() {
        assert_eq!(of(&[]), Err(StatusCode::UNAUTHORIZED));
        // An email alone isn't the proxy's: it always sends the user id.
        assert_eq!(
            of(&[(EMAIL_HEADER, "ada@example.com".into())]),
            Err(StatusCode::UNAUTHORIZED)
        );
        assert_eq!(
            of(&[(USER_HEADER, "4711@1234".into())]),
            Err(StatusCode::UNAUTHORIZED)
        );
        assert_eq!(
            of(&[(USER_HEADER, "1".into()), (EMAIL_HEADER, "a=b".into())]),
            Err(StatusCode::UNAUTHORIZED)
        );
    }

    struct Groups;

    #[async_trait]
    impl GroupSource for Groups {
        async fn groups(
            &self,
            principal: &Principal,
            token: Option<&str>,
        ) -> Result<Vec<String>, String> {
            match (principal.name.as_str(), token) {
                ("ada@example.com", _) => Ok(vec!["operators".into()]),
                ("down@example.com", _) => Err("scim: 503".into()),
                // Only readable as the caller.
                ("own@example.com", Some("own-token")) => Ok(vec!["operators".into()]),
                _ => Ok(Vec::new()),
            }
        }
    }

    #[tokio::test]
    async fn the_callers_forwarded_token_reaches_the_group_source() {
        let principals = Principals::with_roles(
            Roles::from_config(&["operator=*".into()]).unwrap(),
            &["group:operators=operator".into()],
            &[],
        )
        .unwrap();
        let auth = PrincipalAuth::new(AppsProxy::new().with_groups(Groups), principals);
        let mut headers = person("own@example.com");
        let without: Vec<(&str, &str)> = headers.iter().map(|(k, v)| (*k, v.as_str())).collect();
        assert_eq!(
            auth.grant(&parts(&without))
                .await
                .map_err(|r| r.status())
                .err(),
            Some(StatusCode::FORBIDDEN),
            "without its own token, the caller's groups aren't readable"
        );
        headers.push((ACCESS_TOKEN_HEADER, " own-token ".to_owned()));
        let with: Vec<(&str, &str)> = headers.iter().map(|(k, v)| (*k, v.as_str())).collect();
        assert!(auth.grant(&parts(&with)).await.is_ok());
    }

    async fn grant(auth: &impl CallerAuth, email: &str) -> Result<crate::Grant, StatusCode> {
        let headers = person(email);
        let headers: Vec<(&str, &str)> = headers.iter().map(|(k, v)| (*k, v.as_str())).collect();
        auth.grant(&parts(&headers)).await.map_err(|r| r.status())
    }

    #[tokio::test]
    async fn the_principals_map_grants_by_entry_role_and_group() {
        let roles = Roles::from_config(&["operator=*".into(), "reader=@read".into()]).unwrap();
        let principals = Principals::with_roles(
            roles,
            &[
                "user:bob@example.com=reader".into(),
                "group:operators=operator".into(),
            ],
            &["user:bob@example.com".into()],
        )
        .unwrap();
        let auth = PrincipalAuth::new(AppsProxy::new().with_groups(Groups), principals);

        let bob = grant(&auth, "Bob@example.com").await.unwrap();
        assert_eq!(
            (bob.actor.as_str(), bob.role.as_str()),
            ("user:bob@example.com", "reader")
        );
        assert!(bob.apps, "an app principal");
        let ada = grant(&auth, "ada@example.com").await.unwrap();
        assert_eq!(
            (ada.actor.as_str(), ada.role.as_str()),
            ("user:ada@example.com", "operator"),
            "a group's grant, attributed to the member"
        );
        assert_eq!(
            grant(&auth, "eve@example.com").await.unwrap_err(),
            StatusCode::FORBIDDEN
        );
        assert_eq!(
            grant(&auth, "down@example.com").await.unwrap_err(),
            StatusCode::SERVICE_UNAVAILABLE
        );
    }
}
