//! The factory's work graph, behind one seam.
//!
//! - [`FactoryBuilder`] is the composition root's seam: it merges the
//!   built-in [`Extension`] (the reference kind set) with the binary's own,
//!   and builds the [`Factory`]. See [`extension`].
//! - [`Factory`] is the only way in: it validates every write against the
//!   [`Schema`], derives node conditions (`Ready`, `Blocked`, …) on read,
//!   records a Merkle revision whenever plan content changes, and notifies
//!   watchers.
//! - [`backend`] is the seam itself: the [`WorkGraph`](backend::WorkGraph) and
//!   [`RecordStore`](backend::RecordStore) traits a backend implements.
//! - [`olai`] is the olai-store backend, and [`pg`] connects it to Postgres
//!   (feature `postgres`). [`feed`] carries changes between processes on
//!   one Postgres store.
//! - [`driver`] is the factory's way to Omnigent: the
//!   [`OmnigentDriver`](driver::OmnigentDriver) trait, a fake, and the
//!   adapter over `omnigent-client` (feature `omnigent`).
//! - [`codehost`] is where PRs live ([`CodeHost`](codehost::CodeHost), a
//!   GitHub adapter with feature `github`), and [`artifact`] merges a
//!   run's PRs from Omnigent, reports and the code host by URL.
//! - [`canonical`] turns a graph into the tree a revision hashes and the text
//!   a diff shows.
//!
//! The shape is specified in `docs/factory/work-graph.md`.

pub mod amendment;
pub mod artifact;
pub mod backend;
pub mod builtin;
pub mod canonical;
pub mod codehost;
pub mod demo;
pub mod driver;
mod error;
pub mod executor;
pub mod extension;
mod factory;
pub mod feed;
pub mod gates;
pub mod olai;
pub mod payload;
#[cfg(feature = "postgres")]
pub mod pg;
pub mod placement;
pub mod registry;
pub mod schema;
pub mod tool;
pub mod wire;

#[cfg(any(test, feature = "testing"))]
pub mod conformance;

pub use backend::{FeedEntry, Provenance};
pub use error::{Error, Result};
pub use extension::{Extension, Extensions, FactoryBuilder};
pub use factory::{Factory, GraphChange, GraphState, GraphSummary, condition};
pub use registry::Registry;
pub use schema::Schema;
