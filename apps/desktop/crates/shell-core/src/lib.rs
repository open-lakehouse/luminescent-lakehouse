//! The desktop shell's logic, kept free of Tauri so it builds and tests with plain
//! `cargo test`. `src-tauri` is a thin layer of commands and events over it.
//!
//! - [`toolchain`]: the doctor (Docker, Compose, the daemon, the Neon CLI, `PATH`).
//! - [`services`]: the service directory and the proxy behind the UI's `fetch`.

pub mod services;
pub mod toolchain;
