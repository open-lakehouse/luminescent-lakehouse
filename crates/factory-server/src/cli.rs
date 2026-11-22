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
use factory_core::{Factory, FactoryBuilder, Registry, olai};
use tower_http::cors::CorsLayer;
use tower_http::services::{ServeDir, ServeFile};

#[derive(Parser)]
#[command(version, about)]
struct Args {
    /// `memory`, or a sqlx SQLite URL such as `sqlite://factory.db?mode=rwc`.
    #[arg(long, env = "FACTORY_DB", default_value = "memory")]
    db: String,
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
    let backend: Arc<dyn Backend> = if args.db == "memory" {
        Arc::new(olai::in_memory(&schema))
    } else {
        Arc::new(olai::sqlite(&args.db, &schema).await?)
    };
    let registry = match &args.registry {
        Some(path) => Registry::from_yaml(&std::fs::read_to_string(path)?)?,
        None => Registry::default(),
    };
    let factory = Arc::new(builder.build(backend)?.with_registry(registry));
    tracing::info!(extensions = ?factory.extensions().names(), "factory built");

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
    tracing::info!(addr = %args.listen, db = %args.db, "factory-server listening");
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
