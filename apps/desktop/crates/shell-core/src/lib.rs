//! The desktop shell's logic, kept free of Tauri so it builds and tests with plain
//! `cargo test`. `src-tauri` is a thin layer of commands and events over it.
//!
//! - [`agent`]: agent sessions on an instance's Omnigent, folded into chat messages.
//! - [`state`]: trestle's shared state store, as the environment editor sees it.
//! - [`instances`]: running environments, as `trestle up` / `down` run them.
//! - [`neon`]: a Neon branch's functions as the current environment.
//! - [`planner`]: the editor's planner, on the native `olai-stack-topology`.
//! - [`query`]: what the query engine and the UI exchange, and where it finds Unity Catalog.
//! - [`toolchain`]: the doctor (Docker, Compose, the daemon, the Neon and Databricks CLIs,
//!   `PATH`).
//! - [`databricks`]: the Databricks CLI's profiles and the tokens it mints.
//! - [`token_endpoint`]: those tokens for an instance's containers, as the Databricks SDK's
//!   metadata service.
//! - [`secrets`]: the secrets modules declare as minted (Unity Catalog's key, MLflow's
//!   passphrase), minted per environment into the keychain.
//! - [`models`]: an environment's Models choice (provider keys, a Databricks profile, the
//!   Neon AI Gateway of its branch).
//! - [`gateway`]: a branch's Neon AI Gateway: its plan, region and credit problems, its models.
//! - [`factory`]: the reference factory, an in-process service.
//! - [`keys`]: the keys the views address services by (an API, else a module id).
//! - [`services`]: the service directory and the proxy behind the UI's `fetch`.
//! - [`settings`]: the app's own settings (quit policy, switch mode).

pub mod agent;
pub mod databricks;
pub mod directory;
pub mod external;
pub mod factory;
pub mod gateway;
pub mod instances;
mod json;
pub mod keys;
pub mod models;
pub mod neon;
pub mod planner;
pub mod query;
pub mod secrets;
pub mod services;
pub mod settings;
pub mod state;
pub mod token_endpoint;
pub mod toolchain;

pub use json::Json;
