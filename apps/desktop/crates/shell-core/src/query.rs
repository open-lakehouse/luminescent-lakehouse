//! The query engine's side of the shell that needs no engine: the request the UI
//! sends, the errors it gets back, how a result chunk travels to it, and where an
//! instance's Unity Catalog API is. `src-tauri`'s `query` runs hydrofoil's engine
//! over these.

use reqwest::Url;
use serde::{Deserialize, Serialize};

use crate::services::{ProxyError, Upstream, join_under};

/// The key the views address Unity Catalog by ([`crate::keys::ServiceKeys`]).
pub const UNITY_CATALOG: &str = "unity-catalog";

/// Where Unity Catalog's REST API is under its module's root.
const UC_API: &str = "/api/2.1/unity-catalog/";

/// A query to run (the `open_lakehouse.query.v1` contract's `RunQueryRequest`).
#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct QueryRequest {
    pub sql: String,
    /// A row cap; the engine clamps it to its maximum.
    pub limit: Option<u32>,
    /// The default namespace for bare table names.
    pub catalog: Option<String>,
    pub schema: Option<String>,
}

/// Why a query failed: before its rows started (the command's error) or while they
/// came (the channel's last message).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, thiserror::Error)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum QueryError {
    /// No instance was named and none is current.
    #[error("no environment is running")]
    NoEnvironment,
    /// The instance serves no Unity Catalog, so there is nothing to query.
    #[error("the environment has no Unity Catalog")]
    NoCatalog,
    /// The app was built without its query engine (src-tauri's `query` feature).
    #[error("this build of the app has no query engine (build it with the `query` feature)")]
    Unavailable,
    /// Unity Catalog didn't accept the app's credential (none, or a bad one).
    #[error("Unity Catalog refused the app's credential: {message}")]
    Unauthenticated { message: String },
    /// Unity Catalog knows the caller, but its grants don't allow this.
    #[error("not allowed: {message}")]
    Forbidden { message: String },
    /// The SQL didn't plan: a syntax error, an unknown table, a statement the engine
    /// doesn't run.
    #[error("{message}")]
    Invalid { message: String },
    /// Anything else: the catalog or storage couldn't be reached, the run broke off.
    #[error("{message}")]
    Failed { message: String },
}

impl QueryError {
    /// The error for an engine message. Unity Catalog's refusals come back as text
    /// inside the engine's error, so they are recognized by their status.
    pub fn from_engine(message: impl Into<String>, planning: bool) -> Self {
        let message = message.into();
        let lower = message.to_ascii_lowercase();
        let has = |needles: &[&str]| needles.iter().any(|n| lower.contains(n));
        if has(&["401", "unauthenticated", "unauthorized"]) {
            QueryError::Unauthenticated { message }
        } else if has(&["403", "forbidden", "permission_denied", "permission denied"]) {
            QueryError::Forbidden { message }
        } else if planning {
            QueryError::Invalid { message }
        } else {
            QueryError::Failed { message }
        }
    }
}

impl From<ProxyError> for QueryError {
    fn from(e: ProxyError) -> Self {
        QueryError::Failed {
            message: e.to_string(),
        }
    }
}

/// Unity Catalog's REST root under the module's upstream (`…/api/2.1/unity-catalog/`).
pub fn uc_api_root(upstream: &Upstream) -> Result<Url, QueryError> {
    Ok(join_under(&upstream.base_url, UC_API)?)
}

/// The bearer token the upstream carries ([`crate::services::ServiceTokens`] adds it to
/// remote ones only), for a client that authenticates itself rather than through the
/// proxy.
pub fn bearer_of(upstream: &Upstream) -> Option<String> {
    upstream
        .headers
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case("authorization"))
        .and_then(|(_, value)| value.strip_prefix("Bearer "))
        .map(|token| token.trim().to_owned())
        .filter(|token| !token.is_empty())
}

/// One result chunk as it goes to the UI: the chunk's row count (8 bytes, little
/// endian), then its Arrow IPC stream (schema, one batch, end of stream). Raw bytes on
/// the channel, so the UI reads the count without decoding the batch.
pub fn frame_chunk(num_rows: u64, arrow_ipc: &[u8]) -> Vec<u8> {
    let mut frame = Vec::with_capacity(8 + arrow_ipc.len());
    frame.extend_from_slice(&num_rows.to_le_bytes());
    frame.extend_from_slice(arrow_ipc);
    frame
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::{Placement, ServiceTokens};

    fn upstream(url: &str, placement: Placement) -> Upstream {
        Upstream::new(Url::parse(url).unwrap(), placement)
    }

    #[test]
    fn finds_the_api_under_the_module_root() {
        let gateway = upstream("http://localhost:9080/", Placement::Local);
        assert_eq!(
            uc_api_root(&gateway).unwrap().as_str(),
            "http://localhost:9080/api/2.1/unity-catalog/"
        );
        let fronted = upstream("http://localhost:9080/uc/", Placement::Remote);
        assert_eq!(
            uc_api_root(&fronted).unwrap().as_str(),
            "http://localhost:9080/uc/api/2.1/unity-catalog/"
        );
    }

    #[test]
    fn takes_the_bearer_of_a_remote_upstream_only() {
        let tokens = ServiceTokens::parse("unity-catalog=olk_ab").unwrap();
        let remote = tokens.apply(
            UNITY_CATALOG,
            upstream("https://br-x.neon.tech/", Placement::Remote),
        );
        assert_eq!(bearer_of(&remote).as_deref(), Some("olk_ab"));
        let local = tokens.apply(
            UNITY_CATALOG,
            upstream("http://localhost:9080/", Placement::Local),
        );
        assert_eq!(bearer_of(&local), None);
    }

    #[test]
    fn recognizes_unity_catalog_s_refusals() {
        assert!(matches!(
            QueryError::from_engine("UC returned 401 Unauthorized", true),
            QueryError::Unauthenticated { .. }
        ));
        assert!(matches!(
            QueryError::from_engine("status 403: PERMISSION_DENIED on demo.raw", false),
            QueryError::Forbidden { .. }
        ));
        assert!(matches!(
            QueryError::from_engine("table 'x' not found", true),
            QueryError::Invalid { .. }
        ));
        assert!(matches!(
            QueryError::from_engine("connection refused", false),
            QueryError::Failed { .. }
        ));
    }

    #[test]
    fn frames_a_chunk_with_its_row_count() {
        let frame = frame_chunk(3, b"ipc");
        assert_eq!(&frame[..8], &3u64.to_le_bytes());
        assert_eq!(&frame[8..], b"ipc");
    }

    #[test]
    fn reads_the_ui_s_request() {
        let req: QueryRequest =
            serde_json::from_str(r#"{"sql":"SELECT 1","limit":10,"catalog":null,"schema":null}"#)
                .unwrap();
        assert_eq!(req.sql, "SELECT 1");
        assert_eq!(req.limit, Some(10));
    }
}
