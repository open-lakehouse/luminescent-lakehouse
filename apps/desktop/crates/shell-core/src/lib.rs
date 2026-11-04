//! The desktop shell's logic, kept free of Tauri so it builds and tests with plain
//! `cargo test`. `src-tauri` is a thin layer of commands and events over it.
//!
//! - [`state`]: trestle's shared state store, as the environment editor sees it.
//! - [`instances`]: running environments, as `trestle up` / `down` run them.
//! - [`neon`]: a Neon branch's functions as the current environment.
//! - [`planner`]: the editor's planner, on the native `olai-stack-topology`.
//! - [`toolchain`]: the doctor (Docker, Compose, the daemon, the Neon CLI, `PATH`).
//! - [`services`]: the service directory and the proxy behind the UI's `fetch`.
//! - [`settings`]: the app's own settings (quit policy, switch mode).

pub mod directory;
pub mod instances;
mod json;
pub mod neon;
pub mod planner;
pub mod services;
pub mod settings;
pub mod state;
pub mod toolchain;

pub use json::Json;
