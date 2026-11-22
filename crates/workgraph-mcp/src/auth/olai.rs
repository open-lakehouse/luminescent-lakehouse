//! [`Authenticate`] through [`olai_auth`]: the hosted `workgraph` function's
//! callers, as `unitycatalog`'s are.
//!
//! A request presents `Authorization: Bearer` with one of:
//!
//! - an OAuth access token of the branch's Neon Auth for the server's
//!   resource (CLI, MCP clients), or a web session token (its `iss` and `aud`
//!   are the issuer's origin), both EdDSA JWTs: the principal is
//!   `user:<sub>`, if Neon Auth's user table knows the user and hasn't banned
//!   them (a 403);
//! - a service account's `olk_` API key from `authz.api_keys`: the principal
//!   is `service-account:<name>`. A revoked or expired key, or one of a
//!   disabled account, is a 401.
//!
//! What the principal may do is the [`Principals`](super::Principals) map's to
//! say, not the token's: its scopes and claims are never consulted. Where the
//! map has group entries, the caller's groups come from `authz`'s
//! memberships, transitively (olai-auth's [`GrantStore::groups`]), as
//! `luminescent-admin group add` writes them.

use std::time::Duration;

use async_trait::async_trait;
use axum::Router;
use axum::http::request::Parts;
use std::sync::Arc;

use olai_auth::grants::{GrantStore, Subject};
use olai_auth::{
    ApiKeyStore, Authenticator, HttpKeySource, JwtVerifier, KeySource, PgApiKeyStore, PgGrantStore,
    PgNeonUserStatus, PrincipalKind as OlaiKind, ProtectedResourceMetadata, UserStatusStore,
};
use sqlx::PgPool;
use sqlx::postgres::PgPoolOptions;

use super::{Authenticate, Identity, Principal, Rejection};

/// The connections the identity pool keeps open at most. Each request looks up
/// at most one user or API key, and the answers are cached for seconds.
const MAX_IDENTITY_CONNECTIONS: u32 = 4;

/// How long a request waits for an identity-database connection before it is
/// answered with a 503.
const IDENTITY_ACQUIRE_TIMEOUT: Duration = Duration::from_secs(5);

/// The issuer and audiences to accept, and where the users and keys are.
#[derive(Debug, Clone)]
pub struct OlaiConfig {
    /// The issuer of the access tokens (Neon Auth's base URL).
    pub issuer: String,
    /// Its JWKS document.
    pub jwks_url: String,
    /// The audiences an access token may be for: the server's resource.
    pub audiences: Vec<String>,
    /// Also accept the issuer's web session tokens, whose `iss` and `aud` are
    /// the issuer's origin.
    pub accept_session_tokens: bool,
    /// The schema of Neon Auth's `user` table.
    pub users_schema: String,
    /// The schema of `service_accounts` and `api_keys`.
    pub authz_schema: String,
}

impl OlaiConfig {
    /// A branch's Neon Auth (`issuer`, `jwks_url`) for `resource`, with web
    /// session tokens, and the users and keys in `neon_auth` and `authz`.
    pub fn neon(
        issuer: impl Into<String>,
        jwks_url: impl Into<String>,
        resource: impl Into<String>,
    ) -> Self {
        Self {
            issuer: issuer.into(),
            jwks_url: jwks_url.into(),
            audiences: vec![resource.into()],
            accept_session_tokens: true,
            users_schema: "neon_auth".into(),
            authz_schema: "authz".into(),
        }
    }
}

/// A lazy pool on the identity database at `url`: where Neon Auth's users
/// and `authz` are (the branch's `neondb`).
pub fn identity_pool(url: &str) -> Result<PgPool, String> {
    PgPoolOptions::new()
        .max_connections(MAX_IDENTITY_CONNECTIONS)
        .acquire_timeout(IDENTITY_ACQUIRE_TIMEOUT)
        .connect_lazy(url)
        .map_err(|e| format!("the identity database: {e}"))
}

/// Callers authenticated by an [`olai_auth::Authenticator`].
#[derive(Clone)]
pub struct OlaiAuth {
    authenticator: Authenticator,
    groups: Option<Arc<dyn GrantStore>>,
}

impl OlaiAuth {
    pub fn new(authenticator: Authenticator) -> Self {
        Self {
            authenticator,
            groups: None,
        }
    }

    /// Callers' groups from `store`, when the map asks for them.
    pub fn with_groups(mut self, store: impl GrantStore) -> Self {
        self.groups = Some(Arc::new(store));
        self
    }

    /// The JWKS fetched over HTTPS, and the users, keys and group memberships
    /// read from `pool`.
    pub fn postgres(config: &OlaiConfig, pool: PgPool) -> Result<Self, String> {
        let api_keys = PgApiKeyStore::new(pool.clone()).with_schema(&config.authz_schema);
        let groups = PgGrantStore::new(pool.clone()).with_schema(&config.authz_schema);
        Ok(Self::build(
            config,
            HttpKeySource::new(&config.jwks_url),
            PgNeonUserStatus::new(pool).with_schema(&config.users_schema),
            Some(api_keys),
        )?
        .with_groups(groups))
    }

    /// Tokens of `config.issuer` for any of its audiences, plus its session
    /// tokens when `accept_session_tokens`, with keys from `keys`; and API
    /// keys from `api_keys`, when given.
    pub fn build<K: KeySource + Clone>(
        config: &OlaiConfig,
        keys: K,
        user_status: impl UserStatusStore,
        api_keys: Option<impl ApiKeyStore>,
    ) -> Result<Self, String> {
        let mut access = JwtVerifier::new(&config.issuer, keys.clone());
        for audience in &config.audiences {
            access = access.with_audience(audience);
        }
        let mut builder = Authenticator::builder(user_status);
        if config.accept_session_tokens {
            let origin = origin(&config.issuer)?;
            if origin == config.issuer {
                // The issuer is an origin already: one verifier takes both.
                access = access.with_audience(origin);
            } else {
                builder = builder.issuer(JwtVerifier::new(&origin, keys).with_audience(&origin));
            }
        }
        builder = builder.issuer(access);
        if let Some(store) = api_keys {
            builder = builder.api_keys(store);
        }
        let authenticator = builder.build().map_err(|e| format!("olai-auth: {e}"))?;
        Ok(Self::new(authenticator))
    }
}

impl OlaiAuth {
    /// The caller as olai-auth knows it (with the account's id), and as the map
    /// names it.
    // The same `Rejection` `Authenticate` answers with, built once per refusal.
    #[allow(clippy::result_large_err)]
    async fn authenticate(&self, req: &Parts) -> Result<(Subject, Principal), Rejection> {
        match self
            .authenticator
            .authenticate_request(&req.headers, &req.extensions)
            .await
        {
            Ok(principal) => Ok(match principal.kind {
                OlaiKind::User { sub, .. } => (Subject::user(&sub), Principal::user(sub)),
                OlaiKind::ServiceAccount { id, name } => (
                    Subject::service_account(id),
                    Principal::service_account(name),
                ),
            }),
            Err(error) => {
                match &error {
                    olai_auth::AuthError::Unavailable(source) => {
                        tracing::warn!(error = %source, "authentication backend unavailable");
                    }
                    _ => tracing::debug!(%error, "request not authenticated"),
                }
                Err(Rejection::from_response(self.authenticator.reject(&error)))
            }
        }
    }
}

#[async_trait]
impl Authenticate for OlaiAuth {
    async fn principal(&self, req: &Parts) -> Result<Principal, Rejection> {
        Ok(self.authenticate(req).await?.1)
    }

    async fn identity(&self, req: &Parts, groups: bool) -> Result<Identity, Rejection> {
        let (subject, principal) = self.authenticate(req).await?;
        let Some(store) = self.groups.as_ref().filter(|_| groups) else {
            return Ok(principal.into());
        };
        let groups = store.groups(&subject).await.map_err(|error| {
            tracing::warn!(%error, "group memberships unavailable");
            Rejection::unavailable("the identity database is unavailable")
        })?;
        Ok(Identity {
            principal,
            groups: groups.into_iter().map(|g| g.name).collect(),
        })
    }
}

/// The OAuth protected resource metadata document (RFC 9728) for
/// `resource`, served without a token at `/.well-known/oauth-protected-resource`,
/// so MCP clients find the authorization server.
pub fn metadata_router(resource: &str, issuer: &str) -> Router {
    ProtectedResourceMetadata::new(resource, issuer).router()
}

/// The origin (`scheme://host[:port]`) of `url`.
fn origin(url: &str) -> Result<String, String> {
    let parsed = url::Url::parse(url).map_err(|e| format!("issuer `{url}`: {e}"))?;
    match parsed.origin() {
        origin @ url::Origin::Tuple(..) => Ok(origin.ascii_serialization()),
        url::Origin::Opaque(_) => Err(format!("issuer `{url}` has no origin")),
    }
}

#[cfg(test)]
mod tests {
    use axum::http::{Request, StatusCode, header};
    use olai_auth::{ApiKeyRecord, BoxError, PushKeySource, UserStatus, api_key_digest};

    use super::*;

    const LIVE: &str =
        "olk_0000000000000001_0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

    struct Keys;

    #[async_trait]
    impl ApiKeyStore for Keys {
        async fn lookup(&self, id: &str) -> Result<Option<ApiKeyRecord>, BoxError> {
            Ok((id == "0000000000000001").then(|| ApiKeyRecord {
                sha256: api_key_digest(LIVE),
                account_id: "a1".into(),
                account_name: "planner".into(),
                scopes: vec![],
            }))
        }
    }

    struct NoUsers;

    #[async_trait]
    impl UserStatusStore for NoUsers {
        async fn status(&self, _: &str, _: &str) -> Result<Option<UserStatus>, BoxError> {
            Ok(None)
        }
    }

    fn parts(bearer: Option<&str>) -> Parts {
        let mut req = Request::builder();
        if let Some(b) = bearer {
            req = req.header(header::AUTHORIZATION, format!("Bearer {b}"));
        }
        req.body(()).unwrap().into_parts().0
    }

    #[tokio::test]
    async fn api_keys_are_service_accounts_by_name() {
        let config = OlaiConfig::neon(
            "https://auth.example/neondb/auth",
            "https://auth.example/neondb/auth/.well-known/jwks.json",
            "https://workgraph.example",
        );
        let auth = OlaiAuth::build(&config, PushKeySource::new(), NoUsers, Some(Keys)).unwrap();
        assert_eq!(
            auth.principal(&parts(Some(LIVE))).await.unwrap(),
            Principal::service_account("planner")
        );
        let wrong = format!("{}0", &LIVE[..LIVE.len() - 1]);
        for bad in [
            None,
            Some(wrong.as_str()),
            Some("olk_0000000000000002_00"),
            Some("a.b.c"),
        ] {
            let rejected = auth.principal(&parts(bad)).await.unwrap_err();
            assert_eq!(rejected.status(), StatusCode::UNAUTHORIZED, "{bad:?}");
        }
    }

    #[test]
    fn session_tokens_are_verified_on_the_issuers_origin() {
        assert_eq!(
            origin("https://a.example/neondb/auth").unwrap(),
            "https://a.example"
        );
        assert!(origin("data:x").is_err());
    }
}
