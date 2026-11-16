//! SQL against a running instance: hydrofoil's DataFusion engine, embedded in the
//! host, over the instance's Unity Catalog (shell-core's `query`).
//!
//! One [`QueryHost`] holds an engine per instance that was queried, bound to that
//! instance's Unity Catalog with the app's credential for it, the one the proxy sends
//! (a remote one only). Each query gets a session of its own, so vended storage
//! credentials never outlive it. An engine is built on its instance's first query,
//! not on start: each runs a CPU runtime of its own. It goes when its instance stops.
//!
//! Results stream to the UI as raw bytes on a channel, one framed Arrow IPC chunk per
//! record batch ([`shell_core::query::frame_chunk`]).
//!
//! The engine is behind the `query` feature (off by default: it is most of a build).
//! Without it the commands stay, so the bindings don't change, and `query_run` fails
//! with [`QueryError::Unavailable`].

use serde::Serialize;
use shell_core::query::{QueryError, QueryRequest};
use tauri::State;
use tauri::ipc::{Channel, InvokeResponseBody, IpcResponse};

use crate::AppState;

#[cfg(feature = "query")]
mod engine;
#[cfg(feature = "query")]
pub use engine::QueryHost;

/// Without the engine: nothing is ever built, nothing runs.
#[cfg(not(feature = "query"))]
#[derive(Default)]
pub struct QueryHost;

#[cfg(not(feature = "query"))]
impl QueryHost {
    /// The instances with an engine: none.
    pub fn instances(&self) -> Vec<String> {
        Vec::new()
    }

    /// Drop the engines of stopped instances: there are none.
    pub fn forget(&self, _keep: impl Fn(&str) -> bool) {}

    fn cancel(&self, _query_id: u32) {}
}

/// One message on a query's channel: a framed chunk as raw bytes (an `ArrayBuffer` in
/// the UI), then the end or an error as a small JSON object.
pub enum QueryMessage {
    Chunk(Vec<u8>),
    End,
    Error(QueryError),
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
enum QueryControl {
    End,
    Error(QueryError),
}

impl IpcResponse for QueryMessage {
    fn body(self) -> tauri::Result<InvokeResponseBody> {
        match self {
            QueryMessage::Chunk(bytes) => Ok(InvokeResponseBody::Raw(bytes)),
            QueryMessage::End => QueryControl::End.body(),
            QueryMessage::Error(e) => QueryControl::Error(e).body(),
        }
    }
}

/// The UI sees `Channel<QueryMessage>` as `ArrayBuffer | "end" | { error }`, written
/// by hand in `src/host/query.ts`: specta has no `ArrayBuffer`.
#[derive(specta::Type)]
#[specta(remote = QueryMessage)]
#[allow(dead_code)]
struct QueryMessageShape(specta_typescript::Unknown);

/// Run `request` against `instance`'s Unity Catalog (the current environment's when
/// `None`). Resolves once the query planned; its result follows on `on_chunk`, ending
/// with `"end"` (or `{ error }`). Stop it with [`query_cancel`] and the same `query_id`.
/// Without the `query` feature it fails with [`QueryError::Unavailable`].
#[tauri::command]
#[specta::specta]
pub async fn query_run(
    state: State<'_, AppState>,
    query_id: u32,
    instance: Option<String>,
    request: QueryRequest,
    on_chunk: Channel<QueryMessage>,
) -> Result<(), QueryError> {
    #[cfg(feature = "query")]
    return engine::run(&state, query_id, instance, request, on_chunk).await;
    #[cfg(not(feature = "query"))]
    {
        let _ = (state, query_id, instance, request, on_chunk);
        Err(QueryError::Unavailable)
    }
}

/// Stop a query (the UI's `AbortSignal`).
#[tauri::command]
#[specta::specta]
pub fn query_cancel(state: State<'_, AppState>, query_id: u32) {
    state.query.cancel(query_id);
}
