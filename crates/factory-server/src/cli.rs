//! The standalone server's body: flags, the backend, the registry, a seed,
//! and serving [`router`](crate::router). A composition root calls [`run`]
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

    // The control room may be served from another origin (the Vite dev
    // server), so the API allows any. It's bound to localhost by default and
    // has no authorization yet.
    let mut app = crate::router(factory).layer(CorsLayer::very_permissive());
    if let Some(dir) = args.static_dir {
        let index = dir.join("index.html");
        app = app.fallback_service(ServeDir::new(dir).fallback(ServeFile::new(index)));
    }

    let listener = tokio::net::TcpListener::bind(args.listen).await?;
    tracing::info!(addr = %args.listen, db = %args.db, "factory-server listening");
    axum::serve(listener, app)
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await?;
    Ok(())
}
