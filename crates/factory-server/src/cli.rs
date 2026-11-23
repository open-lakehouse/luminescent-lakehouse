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

use factory_core::feed::{self, ListenConfig};
use factory_core::pg::{self, Credentials, LakebaseCredentials, PgSource, StaticCredentials};
use factory_core::{Factory, FactoryBuilder, Registry, Schema, olai};
use tower_http::cors::CorsLayer;
use tower_http::services::{ServeDir, ServeFile};

#[derive(Parser)]
#[command(version, about)]
struct Args {
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
    #[arg(long, env = "FACTORY_LISTEN", default_value = "127.0.0.1:7410")]
    listen: SocketAddr,
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
    #[arg(long, env = "FACTORY_OMNIGENT_TOKEN", hide_env_values = true)]
    omnigent_token: Option<String>,
    /// Where PR status comes from: `gh` (GitHub, with the gh CLI's token),
    /// `token` (GitHub, with GITHUB_TOKEN), or `off`.
    #[arg(long, env = "FACTORY_GITHUB", default_value = "gh")]
    github: String,
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

/// The MCP endpoint, with the command line's development tokens.
fn mcp(
    factory: Arc<Factory>,
    tokens: &[String],
    app_tokens: &[String],
    allowed_hosts: &[String],
    control_room: Option<&url::Url>,
) -> Result<axum::Router, Box<dyn std::error::Error>> {
    use workgraph_mcp::{Tokens, WorkgraphMcp, http};
    // `--mcp-token` and `--mcp-app-token` in `Tokens::from_config`'s form:
    // `[ROLE:]SECRET=POLICY`. Sessions' own tokens are minted into the same set.
    let live = Tokens::from_config(tokens, app_tokens)?;
    let mut server = WorkgraphMcp::new(factory)?;
    if let Some(url) = control_room {
        server = server.with_control_room(url.as_str());
    }
    Ok(http::router(
        server,
        live,
        http::PATH,
        http::config(allowed_hosts),
    ))
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

    // The control room may be served from another origin (the Vite dev
    // server), so the API allows any. It's bound to localhost by default and
    // has no authorization yet.
    let mut app = crate::router(factory.clone())
        .merge(mcp(
            factory,
            &args.mcp_tokens,
            &args.mcp_app_tokens,
            &args.mcp_allowed_hosts,
            args.control_room_url.as_ref(),
        )?)
        .layer(CorsLayer::very_permissive());
    if let Some(dir) = args.static_dir {
        let index = dir.join("index.html");
        app = app.fallback_service(ServeDir::new(dir).fallback(ServeFile::new(index)));
    }

    let listener = tokio::net::TcpListener::bind(args.listen).await?;
    tracing::info!(addr = %args.listen, db = %describe_db(&args.db), "factory-server listening");
    tracing::info!(
        url = %format!("http://{}{}", args.listen, workgraph_mcp::http::PATH),
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
    use factory_core::codehost::CodeHost;
    use factory_core::codehost::github::{GitHub, GitHubAuth};

    let mut transport = omnigent_client::HttpClient::new(reqwest::Client::new());
    if let Some(token) = &args.omnigent_token {
        transport = transport.with_token(token.clone());
    }
    let client = omnigent_client::OmnigentClient::with_transport(transport, url.clone());
    let driver = Arc::new(factory_core::driver::omnigent::OmnigentAdapter::new(client));
    let code_host: Option<Arc<dyn CodeHost>> = match args.github.as_str() {
        "off" => None,
        "gh" => Some(Arc::new(GitHub::new(GitHubAuth::gh()))),
        "token" => {
            let token =
                std::env::var("GITHUB_TOKEN").map_err(|_| "--github token needs GITHUB_TOKEN")?;
            Some(Arc::new(GitHub::new(GitHubAuth::Token(token))))
        }
        other => return Err(format!("--github is gh, token or off, not {other:?}").into()),
    };
    let tick = std::time::Duration::from_secs(args.tick.max(1));
    let config = crate::reconciler::Config {
        tick,
        lease: tick * 6,
        ..Default::default()
    };
    Ok(Arc::new(crate::reconciler::Reconciler::new(
        factory.clone(),
        driver,
        code_host,
        config,
    )))
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
}
