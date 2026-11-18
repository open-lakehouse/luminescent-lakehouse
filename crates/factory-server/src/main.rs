//! `factory-server`, the reference binary: the factory.v1 API on a port,
//! over an olai-store work graph in memory or in SQLite, with the built-in
//! extension alone. A composition with its own extensions has its own
//! binary that calls [`factory_server::cli::run`] the same way.

use factory_core::FactoryBuilder;
use factory_server::cli::{Seed, run};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    run(
        FactoryBuilder::new(),
        &[Seed {
            name: "demo",
            run: |f| Box::pin(async move { factory_core::demo::seed(&f).await }),
        }],
    )
    .await
}
