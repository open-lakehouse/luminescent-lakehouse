//! The desktop shell's logic, kept free of Tauri so it builds and tests with plain
//! `cargo test`. `src-tauri` is a thin layer of commands and events over it.
//!
//! - [`agent`]: agent sessions on an instance's Omnigent, folded into chat messages.
//! - [`state`]: trestle's shared state store, as the environment editor sees it.
//! - [`instances`]: running environments, as `trestle up` / `down` run them.
//! - [`neon`]: a Neon branch's functions as the current environment.
//! - [`planner`]: the editor's planner, on the native `olai-stack-topology`.
//! - [`query`]: what the query engine and the UI exchange, and where it finds Unity Catalog.
//! - [`toolchain`]: the doctor (Docker, Compose, the daemon, the Neon CLI, `PATH`).
//! - [`kek`]: Unity Catalog's key-encryption key, minted per environment into the keychain.
//! - [`keys`]: the keys the views address services by (an API, else a module id).
//! - [`services`]: the service directory and the proxy behind the UI's `fetch`.
//! - [`settings`]: the app's own settings (quit policy, switch mode).

pub mod agent;
pub mod directory;
pub mod external;
pub mod instances;
mod json;
pub mod kek;
pub mod keys;
pub mod neon;
pub mod planner;
pub mod query;
pub mod services;
pub mod settings;
pub mod state;
pub mod toolchain;

pub use json::Json;
