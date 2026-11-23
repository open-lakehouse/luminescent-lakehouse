//! The standalone server's body: flags, the backend, the registry, a seed,
//! and serving [`router`](crate::router) and the agents' MCP server
//! (`workgraph-mcp`, at `/mcp`). A composition root calls [`run`]
//! with its [`FactoryBuilder`] and seeds; the reference binary passes the
//! built-in extension and the reference demo.

use std::future::Future;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::Arc;

use clap::Parser;
use factory_core::backend::Backend;
use std::time::Duration;

use factory_core::databricks::{self, DatabricksOAuth};
use factory_core::feed::{self, ListenConfig};
use factory_core::pg::{self, Credentials, LakebaseCredentials, PgSource, StaticCredentials};
use factory_core::token::TokenSource;
use factory_core::{Factory, FactoryBuilder, Registry, Schema, olai};
use tower_http::cors::CorsLayer;
use tower_http::services::{ServeDir, ServeFile};
use workgraph_mcp::auth::{AppsProxy, CallerAuth, PrincipalAuth, Principals};

use crate::apps;

/// The GitHub App's private key, as `--github app` reads it.
const GITHUB_APP_KEY_ENV: &str = "FACTORY_GITHUB_APP_KEY";

/// The server's flags.
#[derive(Parser)]
#[command(version, about)]
pub struct Args {
    /// The store: `memory`; a sqlx SQLite URL (`sqlite://factory.db?mode=rwc`);
    /// a Postgres URL (`postgres://…` or `postgresql://…`); or `postgres`, to
    /// connect with libpq's environment variables (PGHOST, PGPORT, PGDATABASE,
    /// PGUSER, PGPASSWORD, PGSSLMODE).
    #[arg(long, env = "FACTORY_DB", default_value = "memory")]
    db: String,
    /// Apply the Postgres store's pending migrations at startup, over a
    /// connection of their own (a direct one: the migrator holds a session
    /// advisory lock). Without it the server only checks the schema, and
    /// `luminescent-migrate factory` applies it. SQLite migrates on connect
    /// regardless; `memory` has nothing to migrate.
    #[arg(
        long,
        env = "FACTORY_MIGRATE",
        value_parser = clap::builder::BoolishValueParser::new(),
        default_value_t = false,
        num_args = 0..=1,
        default_missing_value = "true"
    )]
    migrate: bool,
    /// Where a Postgres store's password comes from: `static` (the URL's,
    /// PGPASSWORD or .pgpass) or `lakebase` (a Databricks service principal's
    /// OAuth token, from DATABRICKS_HOST, DATABRICKS_CLIENT_ID and
    /// DATABRICKS_CLIENT_SECRET, renewed before it expires). `lakebase` needs
    /// a Postgres --db.
    #[arg(
        long,
        env = "FACTORY_DB_CREDENTIALS",
        value_enum,
        default_value_t = DbCredentials::Static
    )]
    db_credentials: DbCredentials,
    /// How watchers (WatchGraph, the reconciler) see writes other processes
    /// make to a Postgres store: `notify` (LISTEN on --watch-db), `poll`
    /// (read the change feed every --watch-interval, through --db), or `off`
    /// (this process's writes only). SQLite and memory only ever see this
    /// process's.
    #[arg(long, env = "FACTORY_WATCH", value_enum, default_value_t = Watch::Notify)]
    watch: Watch,
    /// The Postgres `--watch notify` listens on, in --db's Postgres forms,
    /// with --db-credentials' password: by default --db. It has to be a
    /// direct host: a pooled one (`ep-…-pooler.…`) is refused, since LISTEN
    /// through a pooler never delivers.
    #[arg(long, env = "FACTORY_WATCH_DB")]
    watch_db: Option<String>,
    /// Seconds between change feed reads with `--watch poll`.
    #[arg(long, env = "FACTORY_WATCH_INTERVAL", default_value_t = 2)]
    watch_interval: u64,
    /// Where to listen. By default `0.0.0.0:$DATABRICKS_APP_PORT` in a
    /// Databricks App (the port the platform assigns), else `127.0.0.1:7410`.
    #[arg(long, env = "FACTORY_LISTEN")]
    listen: Option<SocketAddr>,
    /// Run as a Databricks App: authenticate factory.v1 and `/mcp` callers
    /// from the identity the Apps proxy forwards (`X-Forwarded-User`,
    /// `X-Forwarded-Email`), map them to grants with FACTORY_MCP_ROLES,
    /// FACTORY_MCP_PRINCIPALS and FACTORY_MCP_APP_PRINCIPALS (`group:`
    /// entries through the workspace's SCIM API, as the App's own service
    /// principal), and refuse a request without one. Only behind the proxy:
    /// anyone who reaches the port directly can send those headers. Off by
    /// default, and then nothing reads them.
    #[arg(
        long,
        env = "FACTORY_APPS_PROXY",
        value_parser = clap::builder::BoolishValueParser::new(),
        default_value_t = false,
        num_args = 0..=1,
        default_missing_value = "true"
    )]
    apps_proxy: bool,
    /// Seed a sample plan when the store has no graphs (`demo`).
    #[arg(long)]
    seed: Option<String>,
    /// The execution registry (profiles and repos), e.g.
    /// `crates/factory-core/registry.example.yaml`. Without it both lists are empty.
    #[arg(long, env = "FACTORY_REGISTRY")]
    registry: Option<PathBuf>,
    /// Serve a built control-room app from this directory too.
    #[arg(long = "static")]
    static_dir: Option<PathBuf>,
    /// The Omnigent server to execute plans on (`http://localhost:9080/omnigent`).
    /// Without it nothing is dispatched: the run RPCs record, and no
    /// reconciler acts on them.
    #[arg(long, env = "FACTORY_OMNIGENT_URL")]
    omnigent_url: Option<url::Url>,
    /// A bearer token for the Omnigent server.
    #[arg(
        long,
        env = "FACTORY_OMNIGENT_TOKEN",
        hide_env_values = true,
        conflicts_with = "omnigent_client_id"
    )]
    omnigent_token: Option<String>,
    /// The OAuth client id of the service principal the factory calls
    /// Omnigent as (hosted Omnigent, `<workspace>/api/2.0/omnigent`). Its
    /// token comes from `$DATABRICKS_HOST/oidc/v1/token` (client
    /// credentials, scope `all-apis`), renewed before it expires.
    #[arg(
        long,
        env = "FACTORY_OMNIGENT_CLIENT_ID",
        requires = "omnigent_client_secret"
    )]
    omnigent_client_id: Option<String>,
    /// The OAuth client's secret, with --omnigent-client-id.
    #[arg(
        long,
        env = "FACTORY_OMNIGENT_CLIENT_SECRET",
        hide_env_values = true,
        requires = "omnigent_client_id"
    )]
    omnigent_client_secret: Option<String>,
    /// Send `X-Databricks-Omnigent-Slice-Key: <host_id>` on the calls for a
    /// session on a connected host, which a sharded Omnigent (hosted
    /// Omnigent) routes by. An unsharded server ignores it.
    #[arg(
        long,
        env = "FACTORY_OMNIGENT_SLICE_KEYS",
        value_parser = clap::builder::BoolishValueParser::new(),
        default_value_t = true,
        num_args = 0..=1,
        default_missing_value = "true"
    )]
    omnigent_slice_keys: bool,
    /// Where PR status comes from: `gh` (GitHub, with the gh CLI's token),
    /// `token` (GitHub, with GITHUB_TOKEN), `app` (a GitHub App's
    /// installation tokens: --github-app-id, and the key in
    /// FACTORY_GITHUB_APP_KEY or --github-app-key-file), or `off`.
    #[arg(long, env = "FACTORY_GITHUB", default_value = "gh")]
    github: String,
    /// The GitHub App's id (or client id), with `--github app`.
    #[arg(long, env = "FACTORY_GITHUB_APP_ID")]
    github_app_id: Option<String>,
    /// A file with the GitHub App's private key (the PEM GitHub issues),
    /// with `--github app`, when FACTORY_GITHUB_APP_KEY doesn't hold it.
    #[arg(long, env = "FACTORY_GITHUB_APP_KEY_FILE")]
    github_app_key_file: Option<PathBuf>,
    /// Seconds between reconciler passes when nothing happens.
    #[arg(long, env = "FACTORY_TICK", default_value_t = 5)]
    tick: u64,
    /// A development token for the agents' MCP server at `/mcp`, as
    /// `[ROLE:]TOKEN=POLICY` (`dev=*`, `ro=@read`,
    /// `planner:wg_x=@read,add_item`); `TOKEN` may be `sha256:<hex>`. Writes
    /// are attributed to the role, or to `operator` without one. Agent sessions get their own
    /// tokens, minted per session; without any, `/mcp` refuses every request.
    #[arg(long = "mcp-token", env = "FACTORY_MCP_TOKENS", value_delimiter = ';')]
    mcp_tokens: Vec<String>,
    /// A token for an MCP Apps host acting for a person (the desktop's side
    /// channel), in `--mcp-token`'s form: like `--mcp-token`, plus the right to the
    /// app-only tools (plan review's approve and return) and the `ui://`
    /// views. Never give one to an agent.
    #[arg(
        long = "mcp-app-token",
        env = "FACTORY_MCP_APP_TOKENS",
        value_delimiter = ';'
    )]
    mcp_app_tokens: Vec<String>,
    /// A `Host` the MCP endpoint accepts besides loopback (DNS-rebinding guard).
    #[arg(long = "mcp-allowed-host")]
    mcp_allowed_hosts: Vec<String>,
    /// The control room's base URL (`http://localhost:4330`). The MCP tools
    /// link a plan there (`?plan=<root>`) for clients that can't show plan
    /// review, so a person can approve it; without it they say where to go.
    #[arg(long, env = "FACTORY_CONTROL_ROOM_URL")]
    control_room_url: Option<url::Url>,
}

/// `--db-credentials`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
enum DbCredentials {
    Static,
    Lakebase,
}

/// `--watch`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum Watch {
    Notify,
    Poll,
    Off,
}

/// The credentials `--db-credentials` names, for the store `db` names.
fn credentials(
    kind: DbCredentials,
    db: &str,
) -> Result<Arc<dyn Credentials>, Box<dyn std::error::Error>> {
    match kind {
        DbCredentials::Static => Ok(Arc::new(StaticCredentials)),
        DbCredentials::Lakebase if PgSource::parse(db).is_none() => Err(format!(
            "--db-credentials lakebase needs a Postgres --db, not {}",
            describe_db(db)
        )
        .into()),
        DbCredentials::Lakebase => Ok(Arc::new(LakebaseCredentials::from_env()?)),
    }
}

/// What keeps `--watch` running: hold it for as long as the server runs.
pub enum Watcher {
    /// This process's writes only: a SQLite or memory store, or `--watch off`.
    InProcess,
    Listen(feed::Listener),
    Poll(tokio::task::JoinHandle<()>),
}

impl Drop for Watcher {
    fn drop(&mut self) {
        if let Self::Poll(task) = self {
            task.abort();
        }
    }
}

/// Feed `factory`'s watchers what other processes write to the store `db`
/// names, as `mode` says: over a `LISTEN` connection on `watch_db` (by
/// default `db`) with `credentials`' password, or by polling the store every
/// `interval`. Only a Postgres store has a change feed.
pub async fn watch(
    factory: &Arc<Factory>,
    mode: Watch,
    db: &str,
    watch_db: Option<&str>,
    interval: Duration,
    credentials: Arc<dyn Credentials>,
) -> Result<Watcher, Box<dyn std::error::Error>> {
    if PgSource::parse(db).is_none() {
        if watch_db.is_some() {
            return Err(format!(
                "--watch-db needs a Postgres --db; {} only has this process's writes",
                describe_db(db)
            )
            .into());
        }
        return Ok(Watcher::InProcess);
    }
    match mode {
        Watch::Off => {
            tracing::info!("--watch off: watchers see this process's writes only");
            Ok(Watcher::InProcess)
        }
        Watch::Poll => Ok(Watcher::Poll(
            feed::poll(factory.clone(), interval.max(Duration::from_millis(100))).await?,
        )),
        Watch::Notify => {
            let target = watch_db.unwrap_or(db);
            let source = PgSource::parse(target).ok_or_else(|| {
                format!(
                    "--watch-db is a `postgres://` URL or `postgres`, not {}",
                    describe_db(target)
                )
            })?;
            let listener = feed::listen(
                factory.clone(),
                source,
                credentials,
                ListenConfig::default(),
            )
            .await
            .map_err(|e| format!("--watch notify (--watch-db, default --db): {e}"))?;
            Ok(Watcher::Listen(listener))
        }
    }
}

/// The MCP endpoint: behind `apps` (the Apps proxy's callers) in Apps mode,
/// else the command line's development tokens.
fn mcp(
    factory: Arc<Factory>,
    args: &Args,
    apps: Option<Arc<dyn CallerAuth>>,
) -> Result<axum::Router, Box<dyn std::error::Error>> {
    use workgraph_mcp::{Tokens, WorkgraphMcp, http};
    let mut server = WorkgraphMcp::new(factory)?;
    if let Some(url) = &args.control_room_url {
        server = server.with_control_room(url.as_str());
    }
    let Some(auth) = apps else {
        // `--mcp-token` and `--mcp-app-token` in `Tokens::from_config`'s form:
        // `[ROLE:]SECRET=POLICY`.
        let live = Tokens::from_config(&args.mcp_tokens, &args.mcp_app_tokens)?;
        return Ok(http::router(
            server,
            live,
            http::PATH,
            http::config(&args.mcp_allowed_hosts),
        ));
    };
    if !args.mcp_tokens.is_empty() || !args.mcp_app_tokens.is_empty() {
        return Err(
            "--apps-proxy authenticates /mcp from the proxy's identity: \
                    drop --mcp-token and --mcp-app-token (a token can't pass the proxy)"
                .into(),
        );
    }
    // The Host is the App's own domain, which the proxy has already vetted:
    // no loopback check, unless hosts are named.
    let config = if args.mcp_allowed_hosts.is_empty() {
        http::config(&[]).disable_allowed_hosts()
    } else {
        http::config(&args.mcp_allowed_hosts)
    };
    Ok(http::router(server, auth, http::PATH, config))
}

/// The caller auth of Apps mode: the proxy's identity, the principals from
/// the environment, and their groups from SCIM when an entry names one.
fn apps_auth() -> Result<Arc<dyn CallerAuth>, Box<dyn std::error::Error>> {
    let principals = Principals::from_env()?;
    if principals.is_empty() {
        tracing::warn!(
            "--apps-proxy without FACTORY_MCP_PRINCIPALS: every caller is refused (403)"
        );
    }
    let mut proxy = AppsProxy::new();
    if principals.has_groups() {
        // The App's own service principal, which the platform injects.
        let oauth = DatabricksOAuth::from_env()
            .map_err(|e| format!("group: entries read SCIM as the App: {e}"))?;
        let workspace = databricks::workspace_url(&std::env::var(databricks::HOST_ENV)?);
        proxy = proxy.with_groups(apps::ScimGroups::new(workspace, Arc::new(oauth)));
    }
    if std::env::var_os(apps::APP_PORT_ENV).is_none() {
        tracing::warn!(
            "--apps-proxy outside a Databricks App (no DATABRICKS_APP_PORT): the forwarded \
             identity is only as good as whatever sits in front of this port"
        );
    }
    Ok(Arc::new(PrincipalAuth::new(proxy, principals)))
}

/// Everything the server serves: factory.v1 (behind `apps` in Apps mode),
/// `/mcp`, and the control room's static files.
pub fn routes(
    factory: Arc<Factory>,
    args: &Args,
    apps: Option<Arc<dyn CallerAuth>>,
) -> Result<axum::Router, Box<dyn std::error::Error>> {
    let mut api = crate::router(factory.clone());
    if let Some(auth) = apps.clone() {
        api = api.layer(axum::middleware::from_fn_with_state(
            auth,
            apps::require_caller,
        ));
    }
    let mut app = api.merge(mcp(factory, args, apps.clone())?);
    if apps.is_none() {
        // The control room may be served from another origin (the Vite dev
        // server), so the API allows any. It's bound to localhost by default.
        // Behind the Apps proxy a browser carries the App's session, so no
        // other origin may read it: the control room is served from here.
        app = app.layer(CorsLayer::very_permissive());
    }
    if let Some(dir) = &args.static_dir {
        let index = dir.join("index.html");
        app = app.fallback_service(ServeDir::new(dir).fallback(ServeFile::new(index)));
    }
    Ok(app)
}

/// The store `db` names (`--db`'s forms), on `schema`. A Postgres store is
/// migrated first with `migrate`, over its own connection, and its schema
/// checked before it's used; `credentials` supply its password.
pub async fn open_backend(
    db: &str,
    migrate: bool,
    schema: &Schema,
    credentials: &dyn Credentials,
) -> Result<Arc<dyn Backend>, Box<dyn std::error::Error>> {
    if db == "memory" {
        if migrate {
            return Err("--migrate needs a Postgres --db; `memory` has no schema".into());
        }
        return Ok(Arc::new(olai::in_memory(schema)));
    }
    if db.starts_with("sqlite:") {
        return Ok(Arc::new(olai::sqlite(db, schema).await?));
    }
    let Some(source) = PgSource::parse(db) else {
        return Err(format!(
            "--db is `memory`, a `sqlite:` URL, a `postgres://` URL or `postgres` (PG* variables), not {:?}",
            describe_db(db)
        )
        .into());
    };
    if migrate {
        pg::migrate(&source, credentials).await?;
        tracing::info!(db = %source.redacted(), "migrations applied");
    }
    let pool = pg::connect(&source, credentials).await?;
    olai::postgres_check(&pool).await?;
    Ok(Arc::new(olai::postgres(pool, schema)))
}

/// `db` for logs and errors: a Postgres URL without its password.
fn describe_db(db: &str) -> String {
    match PgSource::parse(db) {
        Some(source) => format!("postgres {}", source.redacted()),
        None if db.starts_with("sqlite:") || db == "memory" => db.to_owned(),
        None => "<unrecognized>".to_owned(),
    }
}

/// What a seed returns: the root ref of the plan it created.
pub type SeedFuture = Pin<Box<dyn Future<Output = factory_core::Result<String>> + Send>>;

/// A named sample plan for `--seed <name>`.
pub struct Seed {
    pub name: &'static str,
    pub run: fn(Arc<Factory>) -> SeedFuture,
}

/// Parse the flags, build the factory with `builder`'s extensions, seed it,
/// and serve until Ctrl-C.
pub async fn run(
    builder: FactoryBuilder,
    seeds: &[Seed],
) -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info,factory_core=debug".into()),
        )
        .init();
    let args = Args::parse();

    let schema = builder.schema()?;
    let credentials = credentials(args.db_credentials, &args.db)?;
    let backend = open_backend(&args.db, args.migrate, &schema, credentials.as_ref()).await?;
    let registry = match &args.registry {
        Some(path) => Registry::from_yaml(&std::fs::read_to_string(path)?)?,
        None => Registry::default(),
    };
    let factory = Arc::new(builder.build(backend)?.with_registry(registry));
    tracing::info!(extensions = ?factory.extensions().names(), "factory built");
    let _watcher = watch(
        &factory,
        args.watch,
        &args.db,
        args.watch_db.as_deref(),
        Duration::from_secs(args.watch_interval),
        credentials.clone(),
    )
    .await?;

    if let Some(name) = args.seed.as_deref() {
        let Some(seed) = seeds.iter().find(|s| s.name == name) else {
            let known: Vec<&str> = seeds.iter().map(|s| s.name).collect();
            return Err(format!("unknown seed {name:?}; try one of {known:?}").into());
        };
        if factory.list_graphs().await?.is_empty() {
            let root = (seed.run)(factory.clone()).await?;
            tracing::info!(%root, seed = name, "seeded a sample plan");
        }
    }

    match &args.omnigent_url {
        Some(url) => {
            let reconciler = reconciler(&factory, url, &args)?;
            reconciler.spawn();
            tracing::info!(omnigent = %url, github = %args.github, "reconciler running");
        }
        None => tracing::info!("no --omnigent-url: nothing will be dispatched"),
    }

    let apps = if args.apps_proxy {
        Some(apps_auth()?)
    } else {
        None
    };
    let app = routes(factory, &args, apps)?;

    let addr = apps::listen_addr(
        args.listen,
        std::env::var(apps::APP_PORT_ENV).ok().as_deref(),
    )?;
    let listener = tokio::net::TcpListener::bind(addr).await?;
    tracing::info!(%addr, db = %describe_db(&args.db), apps_proxy = args.apps_proxy, "factory-server listening");
    tracing::info!(
        url = %format!("http://{addr}{}", workgraph_mcp::http::PATH),
        tokens = args.mcp_tokens.len(),
        app_tokens = args.mcp_app_tokens.len(),
        "workgraph-mcp"
    );
    axum::serve(listener, app)
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await?;
    Ok(())
}

fn reconciler(
    factory: &Arc<Factory>,
    url: &url::Url,
    args: &Args,
) -> Result<Arc<crate::reconciler::Reconciler>, Box<dyn std::error::Error>> {
    let mut driver = factory_core::driver::omnigent::OmnigentAdapter::new(url.clone())
        .with_slice_keys(args.omnigent_slice_keys);
    if let Some(token) = omnigent_token(args)? {
        driver = driver.with_token(token);
    }
    let code_host = code_host(args)?;
    let tick = std::time::Duration::from_secs(args.tick.max(1));
    let config = crate::reconciler::Config {
        tick,
        lease: tick * 6,
        ..Default::default()
    };
    Ok(Arc::new(crate::reconciler::Reconciler::new(
        factory.clone(),
        Arc::new(driver),
        code_host,
        config,
    )))
}

/// Omnigent's bearer token: a service principal's OAuth token
/// (--omnigent-client-id/secret, from DATABRICKS_HOST), the static
/// --omnigent-token, or none.
fn omnigent_token(args: &Args) -> Result<Option<Arc<dyn TokenSource>>, Box<dyn std::error::Error>> {
    match (
        &args.omnigent_client_id,
        &args.omnigent_client_secret,
        &args.omnigent_token,
    ) {
        (Some(id), Some(secret), _) => {
            let host = std::env::var(databricks::HOST_ENV)
                .ok()
                .filter(|h| !h.is_empty())
                .ok_or("--omnigent-client-id mints at $DATABRICKS_HOST/oidc/v1/token: set DATABRICKS_HOST")?;
            Ok(Some(Arc::new(DatabricksOAuth::new(&host, id, secret)?)))
        }
        (_, _, Some(token)) => Ok(Some(Arc::new(factory_core::token::StaticToken::new(
            token.clone(),
        )))),
        _ => Ok(None),
    }
}

/// The code host `--github` names.
fn code_host(
    args: &Args,
) -> Result<Option<Arc<dyn factory_core::codehost::CodeHost>>, Box<dyn std::error::Error>> {
    use factory_core::codehost::github::{GitHub, GitHubApp, GitHubAuth};
    let auth = match args.github.as_str() {
        "off" => return Ok(None),
        "gh" => GitHubAuth::gh(),
        "token" => GitHubAuth::Token(
            std::env::var("GITHUB_TOKEN").map_err(|_| "--github token needs GITHUB_TOKEN")?,
        ),
        "app" => {
            let id = args
                .github_app_id
                .as_deref()
                .ok_or("--github app needs --github-app-id (FACTORY_GITHUB_APP_ID)")?;
            let key = match (
                std::env::var(GITHUB_APP_KEY_ENV)
                    .ok()
                    .filter(|k| !k.is_empty()),
                &args.github_app_key_file,
            ) {
                (Some(key), _) => key,
                (None, Some(path)) => std::fs::read_to_string(path)
                    .map_err(|e| format!("--github-app-key-file {}: {e}", path.display()))?,
                (None, None) => {
                    return Err(format!(
                        "--github app needs the App's private key: {GITHUB_APP_KEY_ENV} or \
                         --github-app-key-file"
                    )
                    .into());
                }
            };
            GitHubAuth::App(Arc::new(GitHubApp::new(id, &key)?))
        }
        other => return Err(format!("--github is gh, token, app or off, not {other:?}").into()),
    };
    Ok(Some(Arc::new(GitHub::new(auth))))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn control_room_url_parses() {
        let args = Args::try_parse_from([
            "factory-server",
            "--control-room-url",
            "http://localhost:4330",
        ])
        .unwrap();
        assert_eq!(
            args.control_room_url.map(|u| u.to_string()),
            Some("http://localhost:4330/".to_owned())
        );
        assert!(Args::try_parse_from(["factory-server", "--control-room-url", "nope"]).is_err());
    }

    #[test]
    fn db_and_migrate_parse() {
        let args =
            Args::try_parse_from(["factory-server", "--db", "postgres", "--migrate"]).unwrap();
        assert_eq!(args.db, "postgres");
        assert!(args.migrate);
        let args = Args::try_parse_from(["factory-server"]).unwrap();
        assert_eq!(args.db, "memory");
        assert!(!args.migrate);
        let args = Args::try_parse_from(["factory-server", "--migrate", "false"]).unwrap();
        assert!(!args.migrate);
    }

    #[test]
    fn db_credentials_parse_and_lakebase_needs_postgres() {
        let args = Args::try_parse_from(["factory-server"]).unwrap();
        assert_eq!(args.db_credentials, DbCredentials::Static);
        let args = Args::try_parse_from([
            "factory-server",
            "--db",
            "postgres",
            "--db-credentials",
            "lakebase",
        ])
        .unwrap();
        assert_eq!(args.db_credentials, DbCredentials::Lakebase);
        assert!(Args::try_parse_from(["factory-server", "--db-credentials", "vault"]).is_err());

        for db in ["memory", "sqlite::memory:"] {
            let err = credentials(DbCredentials::Lakebase, db).err().unwrap();
            assert!(err.to_string().contains("needs a Postgres --db"), "{err}");
        }
        assert!(credentials(DbCredentials::Static, "memory").is_ok());
    }

    #[test]
    fn watch_flags_parse() {
        let args = Args::try_parse_from(["factory-server"]).unwrap();
        assert_eq!(args.watch, Watch::Notify);
        assert_eq!(args.watch_db, None);
        assert_eq!(args.watch_interval, 2);
        let args = Args::try_parse_from([
            "factory-server",
            "--watch",
            "poll",
            "--watch-interval",
            "5",
            "--watch-db",
            "postgres://h/db",
        ])
        .unwrap();
        assert_eq!(args.watch, Watch::Poll);
        assert_eq!(args.watch_db.as_deref(), Some("postgres://h/db"));
        assert!(Args::try_parse_from(["factory-server", "--watch", "sometimes"]).is_err());
    }

    #[tokio::test]
    async fn watch_refuses_a_pooler_and_needs_postgres_for_watch_db() {
        let schema = Schema::builtin();
        let backend = open_backend("memory", false, &schema, &StaticCredentials)
            .await
            .unwrap();
        let factory = Arc::new(Factory::new(backend, factory_core::Extensions::builtin()));
        let every = Duration::from_secs(1);
        let creds: Arc<dyn Credentials> = Arc::new(StaticCredentials);

        // SQLite and memory stay in-process, whatever --watch says.
        let w = watch(
            &factory,
            Watch::Notify,
            "memory",
            None,
            every,
            creds.clone(),
        )
        .await
        .unwrap();
        assert!(matches!(w, Watcher::InProcess));
        let err = watch(
            &factory,
            Watch::Notify,
            "memory",
            Some("postgres://h/db"),
            every,
            creds.clone(),
        )
        .await
        .err()
        .unwrap();
        assert!(err.to_string().contains("needs a Postgres --db"), "{err}");

        // A pooled --db is refused before connecting, naming the direct host.
        let pooled = "postgres://u@ep-a-1-pooler.eu-west-2.aws.neon.tech/wg";
        let err = watch(&factory, Watch::Notify, pooled, None, every, creds.clone())
            .await
            .err()
            .unwrap()
            .to_string();
        assert!(err.contains("ep-a-1.eu-west-2.aws.neon.tech"), "{err}");
        assert!(err.contains("--watch-db"), "{err}");
        let err = watch(
            &factory,
            Watch::Notify,
            "postgres://u@h/db",
            Some("sqlite::memory:"),
            every,
            creds,
        )
        .await
        .err()
        .unwrap();
        assert!(err.to_string().contains("--watch-db is"), "{err}");
    }

    #[test]
    fn describe_db_hides_the_password() {
        let url = format!("postgres://factory:{}@db.internal:5432/wg", "hunter2");
        assert_eq!(describe_db(&url), "postgres factory@db.internal:5432/wg");
        assert_eq!(describe_db("memory"), "memory");
        assert_eq!(describe_db("mysql://u:p@h/db"), "<unrecognized>");
    }

    #[tokio::test]
    async fn open_backend_refuses_unknown_forms_and_migrating_memory() {
        let schema = Schema::builtin();
        let err = open_backend("mysql://h/db", false, &schema, &StaticCredentials)
            .await
            .err()
            .unwrap();
        assert!(err.to_string().contains("--db is"), "{err}");
        let err = open_backend("memory", true, &schema, &StaticCredentials)
            .await
            .err()
            .unwrap();
        assert!(err.to_string().contains("--migrate"), "{err}");
        assert!(
            open_backend("memory", false, &schema, &StaticCredentials)
                .await
                .is_ok()
        );
        assert!(
            open_backend("sqlite::memory:", true, &schema, &StaticCredentials)
                .await
                .is_ok()
        );
    }

    #[test]
    fn mcp_tokens_reach_the_token_set() {
        let args = Args::try_parse_from([
            "factory-server",
            "--mcp-token",
            "dev=*",
            "--mcp-app-token",
            "desk=*,!submit",
        ])
        .unwrap();
        let tokens =
            workgraph_mcp::Tokens::from_config(&args.mcp_tokens, &args.mcp_app_tokens).unwrap();
        assert!(!tokens.grant("dev").unwrap().apps);
        assert!(tokens.grant("desk").unwrap().apps);
        assert!(workgraph_mcp::Tokens::from_config(&["dev".into()], &[]).is_err());
    }

    fn apps_auth_for(principals: &[&str]) -> Arc<dyn CallerAuth> {
        let own: Vec<String> = principals.iter().map(|s| (*s).to_owned()).collect();
        let principals = Principals::from_config(&own, &[]).unwrap();
        Arc::new(PrincipalAuth::new(AppsProxy::new(), principals))
    }

    fn proxied(email: &str) -> reqwest::header::HeaderMap {
        let mut h = reqwest::header::HeaderMap::new();
        h.insert("x-forwarded-user", "4711@1234".parse().unwrap());
        h.insert("x-forwarded-email", email.parse().unwrap());
        h
    }

    async fn serve(app: axum::Router) -> String {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        format!("http://{addr}")
    }

    #[tokio::test]
    async fn the_caller_replaces_the_clients_actor() {
        let echo = axum::Router::new()
            .route(
                "/echo",
                axum::routing::post(|h: axum::http::HeaderMap| async move {
                    h.get("x-factory-actor")
                        .and_then(|v| v.to_str().ok())
                        .unwrap_or("-")
                        .to_owned()
                }),
            )
            .layer(axum::middleware::from_fn_with_state(
                apps_auth_for(&["user:ada@example.com=operator:*"]),
                apps::require_caller,
            ));
        let base = serve(echo).await;
        let http = reqwest::Client::new();
        let spoofed = http
            .post(format!("{base}/echo"))
            .headers(proxied("ada@example.com"))
            .header("x-factory-actor", "someone-else")
            .send()
            .await
            .unwrap();
        assert_eq!(spoofed.text().await.unwrap(), "user:ada@example.com");
        let anonymous = http
            .post(format!("{base}/echo"))
            .header("x-factory-actor", "someone-else")
            .send()
            .await
            .unwrap();
        assert_eq!(anonymous.status(), 401);
    }

    async fn routes_for(apps: Option<Arc<dyn CallerAuth>>, flags: &[&str]) -> String {
        let schema = Schema::builtin();
        let backend = open_backend("memory", false, &schema, &StaticCredentials)
            .await
            .unwrap();
        let factory = Arc::new(Factory::new(backend, factory_core::Extensions::builtin()));
        let args =
            Args::try_parse_from(std::iter::once("factory-server").chain(flags.iter().copied()))
                .unwrap();
        serve(routes(factory, &args, apps).unwrap()).await
    }

    /// The status of a factory.v1 `ListGraphs` and of an MCP `initialize`
    /// with `headers`.
    async fn statuses(base: &str, headers: reqwest::header::HeaderMap) -> (u16, u16) {
        let http = reqwest::Client::new();
        let api = http
            .post(format!("{base}{}/ListGraphs", crate::SERVICE_PATH))
            .headers(headers.clone())
            .header("content-type", "application/json")
            .body("{}")
            .send()
            .await
            .unwrap()
            .status()
            .as_u16();
        let init = serde_json::json!({
            "jsonrpc": "2.0", "id": 1, "method": "initialize",
            "params": {"protocolVersion": "2025-06-18", "capabilities": {},
                       "clientInfo": {"name": "t", "version": "0"}},
        });
        let mcp = http
            .post(format!("{base}{}", workgraph_mcp::http::PATH))
            .headers(headers)
            .header("host", "factory.example.databricksapps.com")
            .header("accept", "application/json, text/event-stream")
            .json(&init)
            .send()
            .await
            .unwrap()
            .status()
            .as_u16();
        (api, mcp)
    }

    #[tokio::test]
    async fn apps_mode_admits_only_granted_proxy_callers() {
        let auth = apps_auth_for(&["user:ada@example.com=operator:*"]);
        let base = routes_for(Some(auth), &["--apps-proxy"]).await;
        assert_eq!(statuses(&base, Default::default()).await, (401, 401));
        assert_eq!(
            statuses(&base, proxied("eve@example.com")).await,
            (403, 403)
        );
        assert_eq!(
            statuses(&base, proxied("Ada@example.com")).await,
            (200, 200)
        );
    }

    #[tokio::test]
    async fn outside_apps_mode_forwarded_headers_count_for_nothing() {
        let base = routes_for(None, &[]).await;
        // factory.v1 is open as before; /mcp still wants a token, and a
        // spoofed proxy identity isn't one.
        let (api, mcp) = statuses(&base, proxied("ada@example.com")).await;
        assert_eq!(api, 200);
        assert!(mcp == 401 || mcp == 403, "{mcp}");
    }

    #[tokio::test]
    async fn apps_mode_refuses_tokens_and_cross_origin_reads() {
        let auth = apps_auth_for(&[]);
        let schema = Schema::builtin();
        let backend = open_backend("memory", false, &schema, &StaticCredentials)
            .await
            .unwrap();
        let factory = Arc::new(Factory::new(backend, factory_core::Extensions::builtin()));
        let args = Args::try_parse_from(["factory-server", "--apps-proxy", "--mcp-token", "dev=*"])
            .unwrap();
        let err = routes(factory, &args, Some(auth.clone())).err().unwrap();
        assert!(err.to_string().contains("--mcp-token"), "{err}");

        let preflight = |base: String| async move {
            reqwest::Client::new()
                .request(
                    reqwest::Method::OPTIONS,
                    format!("{base}{}/ListGraphs", crate::SERVICE_PATH),
                )
                .header("origin", "https://evil.example")
                .header("access-control-request-method", "POST")
                .send()
                .await
                .unwrap()
                .headers()
                .contains_key("access-control-allow-origin")
        };
        assert!(preflight(routes_for(None, &[]).await).await);
        assert!(!preflight(routes_for(Some(auth), &["--apps-proxy"]).await).await);
    }

    #[test]
    fn omnigent_and_github_flags_parse() {
        let args = Args::try_parse_from([
            "factory-server",
            "--omnigent-client-id",
            "sp",
            "--omnigent-client-secret",
            "s",
        ])
        .unwrap();
        assert!(args.omnigent_slice_keys);
        assert!(
            Args::try_parse_from(["factory-server", "--omnigent-client-id", "sp"]).is_err(),
            "the secret is required"
        );
        assert!(
            Args::try_parse_from([
                "factory-server",
                "--omnigent-token",
                "t",
                "--omnigent-client-id",
                "sp",
                "--omnigent-client-secret",
                "s",
            ])
            .is_err(),
            "a static token or an OAuth client, not both"
        );
        let args =
            Args::try_parse_from(["factory-server", "--omnigent-slice-keys", "false"]).unwrap();
        assert!(!args.omnigent_slice_keys);
        let args = Args::try_parse_from(["factory-server", "--omnigent-token", "t"]).unwrap();
        assert!(omnigent_token(&args).unwrap().is_some());

        let args = Args::try_parse_from(["factory-server", "--github", "app"]).unwrap();
        let err = code_host(&args).err().unwrap().to_string();
        assert!(err.contains("--github-app-id"), "{err}");
        let missing = Args::try_parse_from([
            "factory-server",
            "--github",
            "app",
            "--github-app-id",
            "1",
            "--github-app-key-file",
            "/nonexistent/app.pem",
        ])
        .unwrap();
        let err = code_host(&missing).err().unwrap().to_string();
        assert!(err.contains("--github-app-key-file"), "{err}");
        let args = Args::try_parse_from(["factory-server", "--github", "nope"]).unwrap();
        assert!(code_host(&args).is_err());
    }
}
