//! The operations behind `luminescent-admin`: service accounts and their
//! credentials, groups, and grants in `authz`.

use std::fmt;
use std::str::FromStr;

use anyhow::{Context, bail};
use chrono::{DateTime, Utc};
use sqlx::PgPool;
use uuid::Uuid;

use crate::api_key;

/// A principal as written on the command line: `user:<neon-auth-user-id>`,
/// `service-account:<name>` or `group:<name>`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PrincipalRef {
    User(Uuid),
    ServiceAccount(String),
    Group(String),
}

impl FromStr for PrincipalRef {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> anyhow::Result<Self> {
        match s.split_once(':') {
            Some(("user", id)) => Ok(Self::User(
                id.parse().context("a user is user:<Neon Auth user id>")?,
            )),
            Some(("service-account", name)) if !name.is_empty() => {
                Ok(Self::ServiceAccount(name.into()))
            }
            Some(("group", name)) if !name.is_empty() => Ok(Self::Group(name.into())),
            _ => bail!("expected user:<id>, service-account:<name> or group:<name>, got '{s}'"),
        }
    }
}

impl fmt::Display for PrincipalRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::User(id) => write!(f, "user:{id}"),
            Self::ServiceAccount(name) => write!(f, "service-account:{name}"),
            Self::Group(name) => write!(f, "group:{name}"),
        }
    }
}

/// The `(principal_type, principal_id)` columns of `principal`.
async fn resolve(
    pool: &PgPool,
    principal: &PrincipalRef,
) -> anyhow::Result<(&'static str, String)> {
    Ok(match principal {
        PrincipalRef::User(id) => ("user", id.to_string()),
        PrincipalRef::ServiceAccount(name) => {
            ("service_account", account_id(pool, name).await?.to_string())
        }
        PrincipalRef::Group(name) => ("group", group_id(pool, name).await?.to_string()),
    })
}

/// The command-line form of stored principal columns.
async fn describe(pool: &PgPool, kind: &str, id: &str) -> anyhow::Result<String> {
    let name = |table: &'static str| async move {
        let id: Uuid = id.parse()?;
        let name: Option<String> =
            sqlx::query_scalar(&format!("SELECT name FROM authz.{table} WHERE id = $1"))
                .bind(id)
                .fetch_optional(pool)
                .await?;
        anyhow::Ok(name.unwrap_or_else(|| format!("<deleted {id}>")))
    };
    Ok(match kind {
        "user" => format!("user:{id}"),
        "service_account" => format!("service-account:{}", name("service_accounts").await?),
        "group" => format!("group:{}", name("groups").await?),
        other => format!("{other}:{id}"),
    })
}

async fn account_id(pool: &PgPool, name: &str) -> anyhow::Result<Uuid> {
    sqlx::query_scalar("SELECT id FROM authz.service_accounts WHERE name = $1")
        .bind(name)
        .fetch_optional(pool)
        .await?
        .with_context(|| format!("no service account '{name}'"))
}

async fn group_id(pool: &PgPool, name: &str) -> anyhow::Result<Uuid> {
    sqlx::query_scalar("SELECT id FROM authz.groups WHERE name = $1")
        .bind(name)
        .fetch_optional(pool)
        .await?
        .with_context(|| format!("no group '{name}'"))
}

/// Fail unless exactly one row was affected.
fn one(result: sqlx::postgres::PgQueryResult, what: impl fmt::Display) -> anyhow::Result<()> {
    if result.rows_affected() == 0 {
        bail!("no {what}");
    }
    Ok(())
}

// Service accounts

pub async fn create_service_account(
    pool: &PgPool,
    name: &str,
    description: Option<&str>,
) -> anyhow::Result<Uuid> {
    sqlx::query_scalar(
        "INSERT INTO authz.service_accounts (name, description) VALUES ($1, $2) RETURNING id",
    )
    .bind(name)
    .bind(description)
    .fetch_one(pool)
    .await
    .with_context(|| format!("could not create service account '{name}'"))
}

#[derive(Debug)]
pub struct ServiceAccount {
    pub name: String,
    pub description: Option<String>,
    pub disabled: bool,
    pub live_keys: i64,
}

pub async fn list_service_accounts(pool: &PgPool) -> anyhow::Result<Vec<ServiceAccount>> {
    let rows: Vec<(String, Option<String>, bool, i64)> = sqlx::query_as(
        "SELECT a.name, a.description, a.disabled_at IS NOT NULL,
                count(k.id) FILTER (WHERE k.revoked_at IS NULL AND (k.expires_at IS NULL OR k.expires_at > now()))
         FROM authz.service_accounts a LEFT JOIN authz.api_keys k ON k.account_id = a.id
         GROUP BY a.id ORDER BY a.name",
    )
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(|(name, description, disabled, live_keys)| ServiceAccount {
            name,
            description,
            disabled,
            live_keys,
        })
        .collect())
}

/// Stop the account authenticating, with any credential. Kept for its history.
pub async fn disable_service_account(pool: &PgPool, name: &str) -> anyhow::Result<()> {
    let r = sqlx::query("UPDATE authz.service_accounts SET disabled_at = now() WHERE name = $1 AND disabled_at IS NULL")
        .bind(name)
        .execute(pool)
        .await?;
    one(r, format_args!("enabled service account '{name}'"))
}

// API keys

/// Issue a key for `account`. The returned key is the only copy.
pub async fn issue_api_key(
    pool: &PgPool,
    account: &str,
    scopes: &[String],
    expires_at: Option<DateTime<Utc>>,
    description: Option<&str>,
) -> anyhow::Result<api_key::NewKey> {
    let account_id = account_id(pool, account).await?;
    let key = api_key::generate()?;
    sqlx::query(
        "INSERT INTO authz.api_keys (id, account_id, sha256, scopes, expires_at, description)
         VALUES ($1, $2, $3, $4, $5, $6)",
    )
    .bind(&key.id)
    .bind(account_id)
    .bind(key.sha256.as_slice())
    .bind(scopes)
    .bind(expires_at)
    .bind(description)
    .execute(pool)
    .await?;
    Ok(key)
}

#[derive(Debug)]
pub struct ApiKey {
    pub id: String,
    pub account: String,
    pub scopes: Vec<String>,
    pub description: Option<String>,
    pub created_at: DateTime<Utc>,
    pub expires_at: Option<DateTime<Utc>>,
    pub revoked_at: Option<DateTime<Utc>>,
}

pub async fn list_api_keys(pool: &PgPool, account: Option<&str>) -> anyhow::Result<Vec<ApiKey>> {
    type Row = (
        String,
        String,
        Vec<String>,
        Option<String>,
        DateTime<Utc>,
        Option<DateTime<Utc>>,
        Option<DateTime<Utc>>,
    );
    let rows: Vec<Row> = sqlx::query_as(
        "SELECT k.id, a.name, k.scopes, k.description, k.created_at, k.expires_at, k.revoked_at
         FROM authz.api_keys k JOIN authz.service_accounts a ON a.id = k.account_id
         WHERE $1::text IS NULL OR a.name = $1
         ORDER BY a.name, k.created_at",
    )
    .bind(account)
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(
            |(id, account, scopes, description, created_at, expires_at, revoked_at)| ApiKey {
                id,
                account,
                scopes,
                description,
                created_at,
                expires_at,
                revoked_at,
            },
        )
        .collect())
}

pub async fn revoke_api_key(pool: &PgPool, id: &str) -> anyhow::Result<()> {
    let r = sqlx::query(
        "UPDATE authz.api_keys SET revoked_at = now() WHERE id = $1 AND revoked_at IS NULL",
    )
    .bind(id)
    .execute(pool)
    .await?;
    one(r, format_args!("unrevoked API key '{id}'"))
}

/// The service account a valid key authenticates as.
#[derive(Debug, PartialEq, Eq)]
pub struct VerifiedKey {
    pub account_id: Uuid,
    pub account: String,
    pub scopes: Vec<String>,
}

/// The reference check `olai-auth` implements: `None` for a malformed, unknown,
/// wrong, revoked or expired key, or one of a disabled account.
pub async fn verify_api_key(pool: &PgPool, key: &str) -> anyhow::Result<Option<VerifiedKey>> {
    let Ok(id) = api_key::parse(key) else {
        return Ok(None);
    };
    let row: Option<(Vec<u8>, Uuid, String, Vec<String>)> = sqlx::query_as(
        "SELECT k.sha256, a.id, a.name, k.scopes
         FROM authz.api_keys k JOIN authz.service_accounts a ON a.id = k.account_id
         WHERE k.id = $1 AND k.revoked_at IS NULL AND (k.expires_at IS NULL OR k.expires_at > now())
           AND a.disabled_at IS NULL",
    )
    .bind(id)
    .fetch_optional(pool)
    .await?;
    Ok(row
        .filter(|(sha256, ..)| api_key::matches(key, sha256))
        .map(|(_, account_id, account, scopes)| VerifiedKey {
            account_id,
            account,
            scopes,
        }))
}

// GitHub Actions OIDC trust

/// A rule letting GitHub Actions OIDC tokens act as a service account.
#[derive(Debug, Clone, Default)]
pub struct GithubTrust {
    pub repository_owner_id: i64,
    pub repository_id: Option<i64>,
    pub ref_pattern: Option<String>,
    pub environment: Option<String>,
}

pub async fn add_github_trust(
    pool: &PgPool,
    account: &str,
    rule: &GithubTrust,
) -> anyhow::Result<Uuid> {
    let account_id = account_id(pool, account).await?;
    Ok(sqlx::query_scalar(
        "INSERT INTO authz.github_oidc_trust (account_id, repository_owner_id, repository_id, ref_pattern, environment)
         VALUES ($1, $2, $3, $4, $5) RETURNING id",
    )
    .bind(account_id)
    .bind(rule.repository_owner_id)
    .bind(rule.repository_id)
    .bind(&rule.ref_pattern)
    .bind(&rule.environment)
    .fetch_one(pool)
    .await?)
}

pub async fn list_github_trust(pool: &PgPool) -> anyhow::Result<Vec<(Uuid, String, GithubTrust)>> {
    type Row = (
        Uuid,
        String,
        i64,
        Option<i64>,
        Option<String>,
        Option<String>,
    );
    let rows: Vec<Row> = sqlx::query_as(
        "SELECT t.id, a.name, t.repository_owner_id, t.repository_id, t.ref_pattern, t.environment
         FROM authz.github_oidc_trust t JOIN authz.service_accounts a ON a.id = t.account_id
         ORDER BY a.name, t.created_at",
    )
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(
            |(id, account, repository_owner_id, repository_id, ref_pattern, environment)| {
                (
                    id,
                    account,
                    GithubTrust {
                        repository_owner_id,
                        repository_id,
                        ref_pattern,
                        environment,
                    },
                )
            },
        )
        .collect())
}

pub async fn remove_github_trust(pool: &PgPool, id: Uuid) -> anyhow::Result<()> {
    let r = sqlx::query("DELETE FROM authz.github_oidc_trust WHERE id = $1")
        .bind(id)
        .execute(pool)
        .await?;
    one(r, format_args!("GitHub trust rule {id}"))
}

// Groups and memberships

pub async fn create_group(
    pool: &PgPool,
    name: &str,
    description: Option<&str>,
) -> anyhow::Result<Uuid> {
    sqlx::query_scalar("INSERT INTO authz.groups (name, description) VALUES ($1, $2) RETURNING id")
        .bind(name)
        .bind(description)
        .fetch_one(pool)
        .await
        .with_context(|| format!("could not create group '{name}'"))
}

/// Delete a group, its memberships, and every membership and grant naming it.
pub async fn delete_group(pool: &PgPool, name: &str) -> anyhow::Result<()> {
    let id = group_id(pool, name).await?.to_string();
    let mut tx = pool.begin().await?;
    sqlx::query(
        "DELETE FROM authz.memberships WHERE principal_type = 'group' AND principal_id = $1",
    )
    .bind(&id)
    .execute(&mut *tx)
    .await?;
    sqlx::query("DELETE FROM authz.grants WHERE subject_type = 'group' AND subject_id = $1")
        .bind(&id)
        .execute(&mut *tx)
        .await?;
    sqlx::query("DELETE FROM authz.groups WHERE name = $1")
        .bind(name)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(())
}

pub async fn list_groups(pool: &PgPool) -> anyhow::Result<Vec<(String, Option<String>, i64)>> {
    Ok(sqlx::query_as(
        "SELECT g.name, g.description, count(m.group_id)
         FROM authz.groups g LEFT JOIN authz.memberships m ON m.group_id = g.id
         GROUP BY g.id ORDER BY g.name",
    )
    .fetch_all(pool)
    .await?)
}

pub async fn add_member(pool: &PgPool, group: &str, member: &PrincipalRef) -> anyhow::Result<()> {
    let group_id = group_id(pool, group).await?;
    let (kind, id) = resolve(pool, member).await?;
    sqlx::query(
        "INSERT INTO authz.memberships (group_id, principal_type, principal_id) VALUES ($1, $2, $3)
         ON CONFLICT DO NOTHING",
    )
    .bind(group_id)
    .bind(kind)
    .bind(id)
    .execute(pool)
    .await
    .with_context(|| format!("could not add {member} to group '{group}'"))?;
    Ok(())
}

pub async fn remove_member(
    pool: &PgPool,
    group: &str,
    member: &PrincipalRef,
) -> anyhow::Result<()> {
    let group_id = group_id(pool, group).await?;
    let (kind, id) = resolve(pool, member).await?;
    let r = sqlx::query("DELETE FROM authz.memberships WHERE group_id = $1 AND principal_type = $2 AND principal_id = $3")
        .bind(group_id)
        .bind(kind)
        .bind(id)
        .execute(pool)
        .await?;
    one(r, format_args!("{member} in group '{group}'"))
}

pub async fn list_members(pool: &PgPool, group: &str) -> anyhow::Result<Vec<String>> {
    let group_id = group_id(pool, group).await?;
    let rows: Vec<(String, String)> = sqlx::query_as(
        "SELECT principal_type, principal_id FROM authz.memberships WHERE group_id = $1 ORDER BY created_at",
    )
    .bind(group_id)
    .fetch_all(pool)
    .await?;
    let mut members = Vec::with_capacity(rows.len());
    for (kind, id) in rows {
        members.push(describe(pool, &kind, &id).await?);
    }
    Ok(members)
}

// Grants

/// `subject` holds `relation` on the Cedar entity `object_type::"object_id"`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Grant {
    pub subject: String,
    pub relation: String,
    pub object_type: String,
    pub object_id: String,
}

pub async fn add_grant(
    pool: &PgPool,
    subject: &PrincipalRef,
    relation: &str,
    object_type: &str,
    object_id: &str,
) -> anyhow::Result<()> {
    let (kind, id) = resolve(pool, subject).await?;
    sqlx::query(
        "INSERT INTO authz.grants (subject_type, subject_id, relation, object_type, object_id)
         VALUES ($1, $2, $3, $4, $5) ON CONFLICT DO NOTHING",
    )
    .bind(kind)
    .bind(id)
    .bind(relation)
    .bind(object_type)
    .bind(object_id)
    .execute(pool)
    .await
    .with_context(|| {
        format!("could not grant {relation} on {object_type}::\"{object_id}\" to {subject}")
    })?;
    Ok(())
}

pub async fn remove_grant(
    pool: &PgPool,
    subject: &PrincipalRef,
    relation: &str,
    object_type: &str,
    object_id: &str,
) -> anyhow::Result<()> {
    let (kind, id) = resolve(pool, subject).await?;
    let r = sqlx::query(
        "DELETE FROM authz.grants
         WHERE subject_type = $1 AND subject_id = $2 AND relation = $3 AND object_type = $4 AND object_id = $5",
    )
    .bind(kind)
    .bind(id)
    .bind(relation)
    .bind(object_type)
    .bind(object_id)
    .execute(pool)
    .await?;
    one(
        r,
        format_args!("grant of {relation} on {object_type}::\"{object_id}\" to {subject}"),
    )
}

/// Grants, optionally only those of `subject` (directly, not through groups).
pub async fn list_grants(
    pool: &PgPool,
    subject: Option<&PrincipalRef>,
) -> anyhow::Result<Vec<Grant>> {
    let filter = match subject {
        Some(s) => Some(resolve(pool, s).await?),
        None => None,
    };
    let rows: Vec<(String, String, String, String, String)> = sqlx::query_as(
        "SELECT subject_type, subject_id, relation, object_type, object_id FROM authz.grants
         WHERE $1::text IS NULL OR (subject_type = $1 AND subject_id = $2)
         ORDER BY object_type, object_id, relation, created_at",
    )
    .bind(filter.as_ref().map(|f| f.0))
    .bind(filter.as_ref().map(|f| f.1.as_str()))
    .fetch_all(pool)
    .await?;
    let mut grants = Vec::with_capacity(rows.len());
    for (kind, id, relation, object_type, object_id) in rows {
        grants.push(Grant {
            subject: describe(pool, &kind, &id).await?,
            relation,
            object_type,
            object_id,
        });
    }
    Ok(grants)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_principals() {
        let id = Uuid::nil();
        assert_eq!(
            format!("user:{id}").parse::<PrincipalRef>().unwrap(),
            PrincipalRef::User(id)
        );
        assert_eq!(
            "service-account:ci".parse::<PrincipalRef>().unwrap(),
            PrincipalRef::ServiceAccount("ci".into())
        );
        assert_eq!(
            "group:data".parse::<PrincipalRef>().unwrap().to_string(),
            "group:data"
        );
        for bad in ["", "ci", "user:not-a-uuid", "group:", "role:admin"] {
            assert!(bad.parse::<PrincipalRef>().is_err(), "{bad}");
        }
    }
}
