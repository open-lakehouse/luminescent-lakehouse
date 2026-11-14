//! The Cedar entities of a request, from the branch's Postgres.
//!
//! - The principal, never cached, so a change in the Neon Console ("Make
//!   admin", a ban) or a disabled service account counts from the next request:
//!   - `Lakehouse::User` from `neon_auth."user"` (`neon_role`, `banned`,
//!     `email_verified`), never from token claims;
//!   - `Lakehouse::ServiceAccount` from `authz.service_accounts` (`name`,
//!     `disabled`).
//! - Its groups (`authz.memberships`, transitively) as `Lakehouse::Group`
//!   parents, by name, and its Neon Auth role as a `Lakehouse::Role` parent.
//! - `relations`: the relations of `authz.grants` the principal holds, itself
//!   or through a group, on the request's resource or one of its ancestors (the
//!   parents of the caller-supplied `resource_entity`). Cedar can't quantify
//!   over a set of grants, so the loader resolves them for the request at hand,
//!   and a policy reads `principal.relations.contains("select")`.
//!
//! Groups and relations are cached for a short TTL. Any error is a deny.
//!
//! It connects with the function's `DATABASE_URL`, today as `neondb_owner`, and
//! reads only what the `authz_reader` role may read. A least-privilege login
//! role in `authz_reader` replaces it with #26.

use std::collections::{BTreeSet, HashMap};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use olai_auth::wire::{AuthorizeRequest, EntityRef};
use olai_auth::{BoxError, EntityLoader, async_trait};
use serde_json::{Value, json};
use sqlx::PgPool;
use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
use uuid::Uuid;

pub(crate) const USER: &str = "Lakehouse::User";
pub(crate) const SERVICE_ACCOUNT: &str = "Lakehouse::ServiceAccount";
const GROUP: &str = "Lakehouse::Group";
const ROLE: &str = "Lakehouse::Role";

/// Well under the 2 s a caller's `HttpAuthorizer` waits for a decision.
const LOAD_TIMEOUT: Duration = Duration::from_millis(1500);
const ACQUIRE_TIMEOUT: Duration = Duration::from_secs(1);
const MAX_CONNECTIONS: u32 = 4;
/// The most cached principals before the cache is dropped.
const MAX_CACHED: usize = 10_000;

/// A principal's groups and relations on one resource.
struct Memberships {
    /// `(name, parent group names)` of every group the principal is in.
    groups: Vec<(String, Vec<String>)>,
    /// The names of the groups the principal is directly in.
    direct: Vec<String>,
    relations: Vec<String>,
}

pub(crate) struct PgEntityLoader {
    pool: PgPool,
    ttl: Duration,
    cache: Mutex<HashMap<String, (Instant, Arc<Memberships>)>>,
}

impl PgEntityLoader {
    /// A loader over `database_url`, connected on first use, that reuses
    /// groups and relations for `ttl` (never when zero).
    pub(crate) fn new(database_url: &str, ttl: Duration) -> Result<Self, String> {
        let options: PgConnectOptions = database_url
            .parse()
            .map_err(|e| format!("invalid DATABASE_URL: {e}"))?;
        let pool = PgPoolOptions::new()
            .max_connections(MAX_CONNECTIONS)
            .acquire_timeout(ACQUIRE_TIMEOUT)
            .connect_lazy_with(options);
        Ok(Self {
            pool,
            ttl,
            cache: Mutex::default(),
        })
    }

    async fn principal(&self, principal: &EntityRef) -> Result<(Value, &'static str), BoxError> {
        let id = Uuid::parse_str(&principal.id).map_err(|_| {
            format!(
                "{}::{:?} is not a known principal",
                principal.entity_type, principal.id
            )
        })?;
        match principal.entity_type.as_str() {
            USER => {
                let row: Option<(Option<String>, bool, bool)> = sqlx::query_as(
                    r#"SELECT role,
                              coalesce(banned, false) AND ("banExpires" IS NULL OR "banExpires" > now()),
                              coalesce("emailVerified", false)
                       FROM neon_auth."user" WHERE id = $1"#,
                )
                .bind(id)
                .fetch_optional(&self.pool)
                .await?;
                let (role, banned, email_verified) =
                    row.ok_or_else(|| format!("no Neon Auth user {:?}", principal.id))?;
                let role = role
                    .filter(|r| !r.is_empty())
                    .unwrap_or_else(|| "user".into());
                Ok((
                    json!({ "neon_role": role, "banned": banned, "email_verified": email_verified }),
                    "user",
                ))
            }
            SERVICE_ACCOUNT => {
                let row: Option<(String, bool)> = sqlx::query_as(
                    "SELECT name, disabled_at IS NOT NULL FROM authz.service_accounts WHERE id = $1",
                )
                .bind(id)
                .fetch_optional(&self.pool)
                .await?;
                let (name, disabled) =
                    row.ok_or_else(|| format!("no service account {:?}", principal.id))?;
                Ok((
                    json!({ "name": name, "disabled": disabled }),
                    "service_account",
                ))
            }
            other => Err(format!("unsupported principal type {other:?}").into()),
        }
    }

    async fn memberships(
        &self,
        subject_type: &str,
        subject_id: &str,
        objects: &BTreeSet<(String, String)>,
    ) -> Result<Arc<Memberships>, BoxError> {
        let key = format!("{subject_type}\u{0}{subject_id}\u{0}{objects:?}");
        if !self.ttl.is_zero() {
            let cache = self.cache.lock().map_err(|e| e.to_string())?;
            if let Some((at, cached)) = cache.get(&key)
                && at.elapsed() < self.ttl
            {
                return Ok(cached.clone());
            }
        }

        let rows: Vec<(String, String, bool, Vec<String>)> = sqlx::query_as(
            "WITH RECURSIVE member_of (group_id) AS (
                 SELECT group_id FROM authz.memberships
                 WHERE principal_type = $1 AND principal_id = $2
                 UNION
                 SELECT m.group_id FROM authz.memberships m
                 JOIN member_of g ON m.principal_type = 'group' AND m.principal_id = g.group_id::text
             )
             SELECT g.id::text, g.name,
                    EXISTS (SELECT 1 FROM authz.memberships d
                            WHERE d.group_id = g.id AND d.principal_type = $1 AND d.principal_id = $2),
                    ARRAY(SELECT p.name FROM authz.memberships m JOIN authz.groups p ON p.id = m.group_id
                          WHERE m.principal_type = 'group' AND m.principal_id = g.id::text ORDER BY p.name)
             FROM authz.groups g WHERE g.id IN (SELECT group_id FROM member_of)
             ORDER BY g.name",
        )
        .bind(subject_type)
        .bind(subject_id)
        .fetch_all(&self.pool)
        .await?;
        let group_ids: Vec<String> = rows.iter().map(|(id, ..)| id.clone()).collect();
        let (object_types, object_ids): (Vec<String>, Vec<String>) =
            objects.iter().cloned().unzip();
        let relations: Vec<String> = sqlx::query_scalar(
            "SELECT DISTINCT relation FROM authz.grants
             WHERE ((subject_type = $1 AND subject_id = $2)
                    OR (subject_type = 'group' AND subject_id = ANY($3)))
               AND (object_type, object_id) IN (SELECT * FROM unnest($4::text[], $5::text[]))
             ORDER BY relation",
        )
        .bind(subject_type)
        .bind(subject_id)
        .bind(&group_ids)
        .bind(&object_types)
        .bind(&object_ids)
        .fetch_all(&self.pool)
        .await?;

        let memberships = Arc::new(Memberships {
            direct: rows
                .iter()
                .filter(|(_, _, direct, _)| *direct)
                .map(|(_, name, ..)| name.clone())
                .collect(),
            groups: rows
                .into_iter()
                .map(|(_, name, _, parents)| (name, parents))
                .collect(),
            relations,
        });
        if !self.ttl.is_zero() {
            let mut cache = self.cache.lock().map_err(|e| e.to_string())?;
            if cache.len() >= MAX_CACHED {
                cache.clear();
            }
            cache.insert(key, (Instant::now(), memberships.clone()));
        }
        Ok(memberships)
    }

    async fn entities(&self, request: &AuthorizeRequest) -> Result<Value, BoxError> {
        let principal = &request.principal;
        let (mut attrs, subject_type) = self.principal(principal).await?;
        let memberships = self
            .memberships(subject_type, &principal.id, &ancestors(request))
            .await?;

        attrs["relations"] = json!(memberships.relations);
        let mut parents: Vec<Value> = memberships
            .direct
            .iter()
            .map(|name| json!({ "type": GROUP, "id": name }))
            .collect();
        let mut entities = Vec::new();
        if principal.entity_type == USER {
            let role = attrs["neon_role"].clone();
            parents.push(json!({ "type": ROLE, "id": role }));
            entities
                .push(json!({ "uid": { "type": ROLE, "id": role }, "attrs": {}, "parents": [] }));
        }
        entities.push(json!({
            "uid": { "type": principal.entity_type, "id": principal.id },
            "attrs": attrs,
            "parents": parents,
        }));
        for (name, group_parents) in &memberships.groups {
            entities.push(json!({
                "uid": { "type": GROUP, "id": name },
                "attrs": {},
                "parents": group_parents
                    .iter()
                    .map(|parent| json!({ "type": GROUP, "id": parent }))
                    .collect::<Vec<_>>(),
            }));
        }
        Ok(Value::Array(entities))
    }
}

/// The request's resource and its ancestors: the parents of the
/// caller-supplied `resource_entity` (Unity Catalog passes every ancestor).
fn ancestors(request: &AuthorizeRequest) -> BTreeSet<(String, String)> {
    let mut objects = BTreeSet::new();
    objects.insert((
        request.resource.entity_type.clone(),
        request.resource.id.clone(),
    ));
    let parents = request
        .resource_entity
        .as_ref()
        .and_then(|entity| entity.get("parents"))
        .and_then(Value::as_array);
    for parent in parents.into_iter().flatten() {
        // `{"type", "id"}`, or the escaped `{"__entity": {"type", "id"}}`.
        let parent = parent.get("__entity").unwrap_or(parent);
        if let (Some(t), Some(id)) = (
            parent.get("type").and_then(Value::as_str),
            parent.get("id").and_then(Value::as_str),
        ) {
            objects.insert((t.to_owned(), id.to_owned()));
        }
    }
    objects
}

#[async_trait]
impl EntityLoader for PgEntityLoader {
    async fn load(&self, request: &AuthorizeRequest) -> Result<Value, BoxError> {
        match tokio::time::timeout(LOAD_TIMEOUT, self.entities(request)).await {
            Ok(entities) => entities,
            Err(_) => Err(format!("loading entities timed out after {LOAD_TIMEOUT:?}").into()),
        }
    }
}
