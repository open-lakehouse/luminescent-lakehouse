//! The `authz` migrations and admin operations against a real Postgres.
//!
//! `LUMINESCENT_TEST_DATABASE_URL` points at a server the tests may create
//! databases on (`just test-migrate` starts one); each test gets its own. Unset,
//! the tests skip.

use chrono::{Duration, Utc};
use luminescent_migrate::admin::{self, GithubTrust, PrincipalRef};
use luminescent_migrate::{api_key, connect, migrate};
use sqlx::PgPool;
use uuid::Uuid;

/// A fresh database on the test server, and its URL.
async fn fresh_db() -> Option<(String, PgPool)> {
    let Ok(base) = std::env::var("LUMINESCENT_TEST_DATABASE_URL") else {
        eprintln!("LUMINESCENT_TEST_DATABASE_URL is not set; skipping");
        return None;
    };
    let server = connect(&base).await.unwrap();
    let name = format!("authz_test_{}", Uuid::new_v4().simple());
    sqlx::query(&format!("CREATE DATABASE {name}"))
        .execute(&server)
        .await
        .unwrap();
    server.close().await;
    let url = format!("{}/{name}", base.rsplit_once('/').unwrap().0);
    let pool = connect(&url).await.unwrap();
    Some((url, pool))
}

async fn scalar<T>(pool: &PgPool, sql: &str) -> T
where
    T: for<'r> sqlx::Decode<'r, sqlx::Postgres> + sqlx::Type<sqlx::Postgres> + Send + Unpin,
{
    sqlx::query_scalar(sql).fetch_one(pool).await.unwrap()
}

#[tokio::test]
async fn keeps_its_own_ledger_and_reapplies_as_a_no_op() {
    let Some((url, pool)) = fresh_db().await else {
        return;
    };
    // A service's ledger in `public`, as headwaters and the sharing server keep it.
    sqlx::query("CREATE TABLE public._sqlx_migrations (version bigint PRIMARY KEY)")
        .execute(&pool)
        .await
        .unwrap();

    migrate(&url).await.unwrap();
    migrate(&url).await.unwrap();

    let applied: i64 = scalar(
        &pool,
        "SELECT count(*) FROM authz._sqlx_migrations WHERE success",
    )
    .await;
    assert_eq!(applied, luminescent_migrate::MIGRATOR.iter().count() as i64);
    let foreign: i64 = scalar(&pool, "SELECT count(*) FROM public._sqlx_migrations").await;
    assert_eq!(foreign, 0, "the service ledger must stay untouched");
}

#[tokio::test]
async fn reader_role_sees_only_what_the_services_need() {
    let Some((url, pool)) = fresh_db().await else {
        return;
    };
    // Neon Auth's layout: tables owned by `neon_auth`, in schema `neon_auth`.
    for sql in [
        "DO $$ BEGIN IF NOT EXISTS (SELECT FROM pg_roles WHERE rolname = 'neon_auth') THEN CREATE ROLE neon_auth NOLOGIN; END IF; END $$",
        "CREATE SCHEMA neon_auth AUTHORIZATION neon_auth",
        r#"CREATE TABLE neon_auth."user" (id uuid PRIMARY KEY, email text, "emailVerified" bool, role text, banned bool, "banReason" text, "banExpires" timestamptz)"#,
        r#"CREATE TABLE neon_auth.account ("userId" uuid, "providerId" text, "accountId" text, "accessToken" text, password text)"#,
        r#"ALTER TABLE neon_auth."user" OWNER TO neon_auth"#,
        "ALTER TABLE neon_auth.account OWNER TO neon_auth",
        r#"INSERT INTO neon_auth."user" VALUES ('00000000-0000-0000-0000-000000000001', 'a@example.com', true, 'admin', false, NULL, NULL)"#,
        "DO $$ BEGIN IF NOT EXISTS (SELECT FROM pg_roles WHERE rolname = 'authz_test_login') THEN CREATE ROLE authz_test_login LOGIN PASSWORD 'authz-test-login'; END IF; END $$",
    ] {
        sqlx::query(sql).execute(&pool).await.unwrap();
    }
    migrate(&url).await.unwrap();
    sqlx::query("GRANT authz_reader TO authz_test_login")
        .execute(&pool)
        .await
        .unwrap();
    admin::create_group(&pool, "data", None).await.unwrap();

    let (scheme, rest) = url.split_once("://").unwrap();
    let host = rest.split_once('@').unwrap().1;
    let reader = connect(&format!(
        "{scheme}://authz_test_login:authz-test-login@{host}"
    ))
    .await
    .unwrap();

    let role: String = scalar(&reader, r#"SELECT role FROM neon_auth."user""#).await;
    assert_eq!(role, "admin");
    let _: i64 = scalar(
        &reader,
        r#"SELECT count("accountId") FROM neon_auth.account"#,
    )
    .await;
    let groups: i64 = scalar(&reader, "SELECT count(*) FROM authz.groups").await;
    assert_eq!(groups, 1);
    for denied in [
        r#"SELECT email FROM neon_auth."user""#,
        r#"SELECT "accessToken" FROM neon_auth.account"#,
        r#"SELECT password FROM neon_auth.account"#,
        r#"UPDATE neon_auth."user" SET role = 'admin'"#,
        "INSERT INTO authz.groups (name) VALUES ('mine')",
        "DELETE FROM authz.api_keys",
    ] {
        let err = sqlx::query(denied).execute(&reader).await.unwrap_err();
        assert!(
            err.to_string().contains("permission denied"),
            "{denied}: {err}"
        );
    }
}

#[tokio::test]
async fn api_keys_authenticate_only_while_live() {
    let Some((url, pool)) = fresh_db().await else {
        return;
    };
    migrate(&url).await.unwrap();
    admin::create_service_account(&pool, "airflow", Some("lineage from Airflow"))
        .await
        .unwrap();

    let scopes = vec!["ingest".to_string()];
    let key = admin::issue_api_key(&pool, "airflow", &scopes, None, None)
        .await
        .unwrap();
    let verified = admin::verify_api_key(&pool, &key.key)
        .await
        .unwrap()
        .expect("valid key");
    assert_eq!(
        (verified.account.as_str(), verified.scopes),
        ("airflow", scopes)
    );

    let wrong = format!("olk_{}_{}", key.id, "0".repeat(64));
    assert_eq!(admin::verify_api_key(&pool, &wrong).await.unwrap(), None);
    assert_eq!(
        admin::verify_api_key(&pool, "not-a-key").await.unwrap(),
        None
    );
    let unknown = api_key::generate().unwrap();
    assert_eq!(
        admin::verify_api_key(&pool, &unknown.key).await.unwrap(),
        None
    );

    admin::revoke_api_key(&pool, &key.id).await.unwrap();
    assert_eq!(admin::verify_api_key(&pool, &key.key).await.unwrap(), None);
    assert!(
        admin::revoke_api_key(&pool, &key.id).await.is_err(),
        "already revoked"
    );

    let expired = admin::issue_api_key(
        &pool,
        "airflow",
        &[],
        Some(Utc::now() - Duration::minutes(1)),
        None,
    )
    .await
    .unwrap();
    assert_eq!(
        admin::verify_api_key(&pool, &expired.key).await.unwrap(),
        None
    );

    let live = admin::issue_api_key(&pool, "airflow", &[], None, None)
        .await
        .unwrap();
    assert!(
        admin::verify_api_key(&pool, &live.key)
            .await
            .unwrap()
            .is_some()
    );
    admin::disable_service_account(&pool, "airflow")
        .await
        .unwrap();
    assert_eq!(admin::verify_api_key(&pool, &live.key).await.unwrap(), None);

    let keys = admin::list_api_keys(&pool, Some("airflow")).await.unwrap();
    assert_eq!(keys.len(), 3);
    // Only the digest is stored.
    let rows: Vec<(String, Vec<u8>)> =
        sqlx::query_as("SELECT id, sha256 FROM authz.api_keys WHERE id = $1")
            .bind(&live.id)
            .fetch_all(&pool)
            .await
            .unwrap();
    assert_eq!(
        rows,
        [(live.id.clone(), api_key::digest(&live.key).to_vec())]
    );
}

#[tokio::test]
async fn groups_and_grants() {
    let Some((url, pool)) = fresh_db().await else {
        return;
    };
    migrate(&url).await.unwrap();
    admin::create_service_account(&pool, "ci", None)
        .await
        .unwrap();
    admin::create_group(&pool, "data", None).await.unwrap();
    admin::create_group(&pool, "platform", None).await.unwrap();

    let user = PrincipalRef::User(Uuid::new_v4());
    let ci: PrincipalRef = "service-account:ci".parse().unwrap();
    let platform: PrincipalRef = "group:platform".parse().unwrap();
    for member in [&user, &ci, &platform] {
        admin::add_member(&pool, "data", member).await.unwrap();
        admin::add_member(&pool, "data", member).await.unwrap(); // idempotent
    }
    let members = admin::list_members(&pool, "data").await.unwrap();
    assert_eq!(
        members,
        [
            user.to_string(),
            "service-account:ci".into(),
            "group:platform".into()
        ]
    );
    assert!(
        admin::add_member(&pool, "data", &"group:data".parse().unwrap())
            .await
            .is_err(),
        "no self-membership"
    );
    assert!(admin::add_member(&pool, "nope", &ci).await.is_err());

    let data: PrincipalRef = "group:data".parse().unwrap();
    admin::add_grant(&pool, &data, "reader", "Headwaters::Namespace", "prod")
        .await
        .unwrap();
    admin::add_grant(&pool, &data, "reader", "Headwaters::Namespace", "prod")
        .await
        .unwrap(); // idempotent
    admin::add_grant(&pool, &ci, "writer", "Headwaters::Namespace", "prod")
        .await
        .unwrap();
    assert!(
        admin::add_grant(&pool, &ci, "Writer", "Headwaters::Namespace", "prod")
            .await
            .is_err(),
        "relation syntax"
    );
    assert!(
        admin::add_grant(&pool, &ci, "writer", "namespace", "prod")
            .await
            .is_err(),
        "Cedar entity type syntax"
    );
    admin::add_grant(
        &pool,
        &ci,
        "external_use",
        "UnityCatalog::Schema",
        "main.sales",
    )
    .await
    .unwrap();
    assert!(
        admin::add_grant(&pool, &ci, "reader", "UnityCatalog::Schema", "main.sales")
            .await
            .is_err(),
        "not a Unity Catalog relation"
    );
    assert!(
        admin::add_grant(&pool, &ci, "select", "UnityCatalog::Table", "main.sales")
            .await
            .is_err(),
        "not a table's full name"
    );
    admin::remove_grant(
        &pool,
        &ci,
        "external_use",
        "UnityCatalog::Schema",
        "main.sales",
    )
    .await
    .unwrap();

    assert_eq!(admin::list_grants(&pool, None).await.unwrap().len(), 2);
    let ci_grants = admin::list_grants(&pool, Some(&ci)).await.unwrap();
    assert_eq!(
        ci_grants,
        [admin::Grant {
            subject: "service-account:ci".into(),
            relation: "writer".into(),
            object_type: "Headwaters::Namespace".into(),
            object_id: "prod".into(),
        }]
    );

    admin::remove_grant(&pool, &ci, "writer", "Headwaters::Namespace", "prod")
        .await
        .unwrap();
    assert!(
        admin::remove_grant(&pool, &ci, "writer", "Headwaters::Namespace", "prod")
            .await
            .is_err()
    );

    // Deleting a group takes the memberships and grants naming it along.
    admin::add_member(&pool, "platform", &user).await.unwrap();
    admin::add_grant(&pool, &platform, "admin", "Headwaters::Namespace", "prod")
        .await
        .unwrap();
    admin::delete_group(&pool, "platform").await.unwrap();
    assert_eq!(admin::list_members(&pool, "data").await.unwrap().len(), 2);
    assert_eq!(admin::list_grants(&pool, None).await.unwrap().len(), 1);
    let dangling: i64 = scalar(&pool, "SELECT count(*) FROM authz.memberships m LEFT JOIN authz.groups g ON g.id = m.group_id WHERE g.id IS NULL").await;
    assert_eq!(dangling, 0);
}

#[tokio::test]
async fn github_trust_rules() {
    let Some((url, pool)) = fresh_db().await else {
        return;
    };
    migrate(&url).await.unwrap();
    admin::create_service_account(&pool, "deploy", None)
        .await
        .unwrap();

    let rule = GithubTrust {
        repository_owner_id: 1234,
        repository_id: Some(5678),
        ref_pattern: Some("refs/heads/main".into()),
        environment: None,
    };
    let id = admin::add_github_trust(&pool, "deploy", &rule)
        .await
        .unwrap();
    assert!(admin::add_github_trust(&pool, "nope", &rule).await.is_err());
    let rules = admin::list_github_trust(&pool).await.unwrap();
    assert_eq!(rules.len(), 1);
    assert_eq!(
        (rules[0].0, rules[0].1.as_str(), rules[0].2.repository_id),
        (id, "deploy", Some(5678))
    );

    admin::remove_github_trust(&pool, id).await.unwrap();
    assert!(admin::remove_github_trust(&pool, id).await.is_err());
}

#[tokio::test]
async fn ensuring_a_service_account_is_idempotent_but_respects_disabling() {
    let Some((url, pool)) = fresh_db().await else {
        return;
    };
    migrate(&url).await.unwrap();

    let id = admin::ensure_service_account(&pool, "uc-storage-seed", Some("first"))
        .await
        .unwrap();
    let again = admin::ensure_service_account(&pool, "uc-storage-seed", Some("second"))
        .await
        .unwrap();
    assert_eq!(id, again);
    let description: Option<String> = scalar(
        &pool,
        "SELECT description FROM authz.service_accounts WHERE name = 'uc-storage-seed'",
    )
    .await;
    assert_eq!(description.as_deref(), Some("first"), "left as it was");

    admin::disable_service_account(&pool, "uc-storage-seed")
        .await
        .unwrap();
    let err = admin::ensure_service_account(&pool, "uc-storage-seed", None)
        .await
        .unwrap_err();
    assert!(err.to_string().contains("disabled"), "{err}");
}
