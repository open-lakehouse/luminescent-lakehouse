//! The operations on a branch's `authz` data, behind `luminescent-admin` and
//! the desktop's provisioning of an environment's agents: service accounts,
//! their API keys and GitHub Actions trust, groups and their members, and
//! grants.
//!
//! The schema itself is `luminescent-migrate authz`'s (olai-auth's tables and
//! the lakehouse's own). Every operation takes a pool on the branch's main
//! database, where `authz` lives. This crate links only what the operations
//! need, so a host can provision accounts without the services
//! `luminescent-migrate` migrates.

pub mod admin;
pub mod api_key;
