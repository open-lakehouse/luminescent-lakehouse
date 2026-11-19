//! `workgraph-mcp`: the factory's MCP server for agents.
//!
//! Agents read and write the work graph through these tools only, never
//! through a tracker's CLI, so the factory stays the one place that checks
//! their writes. It serves the same [`Factory`](factory_core::Factory) as
//! `factory.v1`, over Streamable HTTP ([`http::router`], mounted by
//! factory-server at `/mcp`), or stdio with the `stdio` feature.
//!
//! - [`auth`]: per-session bearer tokens, each with a [`Grant`]: who the
//!   caller is, what it works on, and its [`ToolPolicy`].
//! - [`policy`]: which tools a token may use. There are no built-in roles:
//!   the composition decides who may do what.
//! - [`tools`]: the built-in tools. Extensions add theirs as
//!   [`ToolSpec`](factory_core::tool::ToolSpec)s
//!   ([`Extension::mcp_tools`](factory_core::Extension::mcp_tools)).
//! - [`server`]: the MCP side: listing per grant and per client, MCP Apps
//!   gating, results without embedded resources.
//!
//! `docs/factory/workgraph-mcp.md` has the whole picture.

pub mod auth;
pub mod http;
pub mod policy;
pub mod server;
#[cfg(feature = "stdio")]
pub mod stdio;
pub mod tools;

pub use auth::{Grant, Tokens};
pub use policy::{Selector, ToolPolicy};
pub use server::{UI_EXTENSION, WorkgraphMcp};
