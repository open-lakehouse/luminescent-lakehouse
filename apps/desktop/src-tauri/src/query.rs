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

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use buffa::Message;
use bytes::Bytes;
use connectrpc::Router;
use connectrpc::codec::CodecFormat;
use connectrpc::dispatcher::Dispatcher;
use connectrpc::response::RequestContext;
use futures::StreamExt;
use futures::future::{AbortHandle, Abortable};
use futures::stream::BoxStream;
use hydrofoil::generated::buffa::hydrofoil::query::v1::{RunQueryRequest, RunQueryResponse};
use hydrofoil::{FlightSqlServiceImpl, QueryAppState};
use serde::Serialize;
use shell_core::instances::CurrentEnvironment;
use shell_core::query::{
    QueryError, QueryRequest, UNITY_CATALOG, bearer_of, frame_chunk, uc_api_root,
};
use shell_core::services::{ServiceDirectory, Upstream};
use tauri::State;
use tauri::ipc::{Channel, InvokeResponseBody, IpcResponse};
use tokio::sync::Semaphore;
use unitycatalog_object_store::UnityObjectStoreFactory;

use crate::AppState;

/// The engine's procedure, in-process (no HTTP): the router's key for it, without the
/// path's leading slash.
const RUN_QUERY: &str = "hydrofoil.query.v1.QueryService/RunQuery";
/// Rows a query returns without a `limit`.
const DEFAULT_LIMIT: u32 = 1_000;
/// Rows a query returns at most, whatever its `limit`.
const MAX_LIMIT: u32 = 10_000;
/// How long an idle session lives in the engine's store.
const SESSION_TTL: Duration = Duration::from_secs(30 * 60);
/// Queries running at once, over all instances; more wait for a slot.
const MAX_RUNNING: usize = 4;

/// The instance an engine serves. `None`: no instance, only what is bound by hand
/// (`LUMINESCENT_SERVICES`).
pub type Target = Option<String>;

/// One instance's engine.
pub struct Engine {
    /// The Unity Catalog API and credential it was built for, to tell when they change.
    bound_to: Option<(String, Option<String>)>,
    router: Router,
}

impl Engine {
    /// An engine over the Unity Catalog API at `api`, or over no catalog at all (plain
    /// SQL only, `SELECT 1`). Runs on a Tokio runtime: the factory captures its handle
    /// for credential refreshes, and the engine spawns its session sweeper.
    pub async fn build(uc: Option<(String, Option<String>)>) -> Result<Self, QueryError> {
        let failed = |message: String| QueryError::Failed { message };
        let mut service = FlightSqlServiceImpl::try_new()
            .map_err(|e| failed(format!("the query engine didn't start: {e}")))?;
        if let Some((api, token)) = &uc {
            let factory = UnityObjectStoreFactory::builder()
                .with_uri(api.clone())
                .with_token(token.clone())
                // A local instance's Unity Catalog takes anonymous callers.
                .with_allow_unauthenticated(token.is_none())
                .with_io_runtime(tokio::runtime::Handle::current())
                .build()
                .await
                .map_err(|e| failed(format!("Unity Catalog at {api}: {e}")))?;
            service = service.with_unity(Arc::new(factory), Some(api.clone()), None);
        }
        let service = Arc::new(service.build(SESSION_TTL));
        let router = QueryAppState {
            service,
            query_default_limit: DEFAULT_LIMIT,
            query_max_limit: MAX_LIMIT,
        }
        .register(Router::new());
        Ok(Engine {
            bound_to: uc,
            router,
        })
    }

    /// Plan `request`, then stream its result: framed chunks, then the end. A query
    /// that doesn't plan fails here; one that fails while running ends the stream with
    /// its error. Dropping the stream stops the run.
    pub async fn run(
        &self,
        request: QueryRequest,
    ) -> Result<BoxStream<'static, Result<Vec<u8>, QueryError>>, QueryError> {
        let message = RunQueryRequest {
            sql: request.sql,
            limit: request.limit,
            catalog: request.catalog,
            schema: request.schema,
            ..Default::default()
        };
        let ctx = RequestContext::new(Default::default()).with_path(format!("/{RUN_QUERY}"));
        let response = self
            .router
            .call_server_streaming(
                RUN_QUERY,
                ctx,
                Bytes::from(message.encode_to_vec()),
                CodecFormat::Proto,
            )
            .await
            .map_err(|e| QueryError::from_engine(connect_message(&e), true))?;
        Ok(response
            .body
            .map(|item| {
                let bytes =
                    item.map_err(|e| QueryError::from_engine(connect_message(&e), false))?;
                let chunk = RunQueryResponse::decode_from_slice(&bytes).map_err(|e| {
                    QueryError::Failed {
                        message: format!("a result chunk didn't decode: {e}"),
                    }
                })?;
                Ok(frame_chunk(chunk.num_rows, &chunk.arrow_ipc))
            })
            .boxed())
    }
}

fn connect_message(e: &connectrpc::ConnectError) -> String {
    e.message.clone().unwrap_or_else(|| e.to_string())
}

/// The engines, the queries running, and the slots they take.
pub struct QueryHost {
    engines: Mutex<HashMap<Target, Arc<Engine>>>,
    running: Arc<Mutex<HashMap<u32, AbortHandle>>>,
    slots: Arc<Semaphore>,
}

impl Default for QueryHost {
    fn default() -> Self {
        QueryHost {
            engines: Mutex::default(),
            running: Arc::default(),
            slots: Arc::new(Semaphore::new(MAX_RUNNING)),
        }
    }
}

impl QueryHost {
    /// The engine built for `target`, if any.
    fn bound(&self, target: &Target) -> Option<Arc<Engine>> {
        self.engines
            .lock()
            .expect("engines lock")
            .get(target)
            .cloned()
    }

    /// `target`'s engine over `upstream`'s Unity Catalog, built if there is none yet or
    /// the one there was built for another catalog or credential.
    async fn engine_for(
        &self,
        target: Target,
        upstream: &Upstream,
    ) -> Result<Arc<Engine>, QueryError> {
        let wanted = (uc_api_root(upstream)?.to_string(), bearer_of(upstream));
        if let Some(engine) = self.bound(&target)
            && engine.bound_to.as_ref() == Some(&wanted)
        {
            return Ok(engine);
        }
        let engine = Arc::new(Engine::build(Some(wanted)).await?);
        let replaced = self
            .engines
            .lock()
            .expect("engines lock")
            .insert(target, engine.clone());
        retire(replaced);
        Ok(engine)
    }

    /// The instances with an engine.
    pub fn instances(&self) -> Vec<String> {
        self.engines
            .lock()
            .expect("engines lock")
            .keys()
            .filter_map(Clone::clone)
            .collect()
    }

    /// Drop the engines of instances `keep` says no to (they stopped).
    pub fn forget(&self, keep: impl Fn(&str) -> bool) {
        let gone: Vec<Arc<Engine>> = {
            let mut engines = self.engines.lock().expect("engines lock");
            let stale: Vec<Target> = engines
                .keys()
                .filter(|t| t.as_deref().is_some_and(|id| !keep(id)))
                .cloned()
                .collect();
            stale
                .into_iter()
                .filter_map(|t| engines.remove(&t))
                .collect()
        };
        for engine in gone {
            retire(Some(engine));
        }
    }

    /// Run `request` on `engine`, sending its chunks to `on_chunk` from a task of its
    /// own (cancelled by [`QueryHost::cancel`] with `query_id`). Waits for a slot, then
    /// for the plan: a query that doesn't plan fails here.
    async fn start(
        &self,
        engine: Arc<Engine>,
        query_id: u32,
        request: QueryRequest,
        on_chunk: Channel<QueryMessage>,
    ) -> Result<(), QueryError> {
        let slot = self
            .slots
            .clone()
            .acquire_owned()
            .await
            .expect("the query slots are never closed");
        let mut chunks = engine.run(request).await?;
        let (handle, registration) = AbortHandle::new_pair();
        let running = self.running.clone();
        running
            .lock()
            .expect("running lock")
            .insert(query_id, handle);
        tauri::async_runtime::spawn(async move {
            let pump = async {
                while let Some(chunk) = chunks.next().await {
                    let message = match chunk {
                        Ok(frame) => QueryMessage::Chunk(frame),
                        Err(e) => {
                            let _ = on_chunk.send(QueryMessage::Error(e));
                            return;
                        }
                    };
                    // The webview is gone (or reloaded): stop the run.
                    if on_chunk.send(message).is_err() {
                        return;
                    }
                }
                let _ = on_chunk.send(QueryMessage::End);
            };
            let _ = Abortable::new(pump, registration).await;
            running.lock().expect("running lock").remove(&query_id);
            // `pump` (and with it the run's stream) is gone: the slot is free.
            drop(slot);
        });
        Ok(())
    }

    fn cancel(&self, query_id: u32) {
        if let Some(handle) = self.running.lock().expect("running lock").remove(&query_id) {
            handle.abort();
        }
    }
}

/// Drop an engine off the async runtime: its CPU runtime joins its thread on drop.
fn retire(engine: Option<Arc<Engine>>) {
    if let Some(engine) = engine {
        std::thread::spawn(move || drop(engine));
    }
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
#[tauri::command]
#[specta::specta]
pub async fn query_run(
    state: State<'_, AppState>,
    query_id: u32,
    instance: Option<String>,
    request: QueryRequest,
    on_chunk: Channel<QueryMessage>,
) -> Result<(), QueryError> {
    let target = instance.or_else(|| {
        let current = state.current.lock().expect("current lock");
        current.as_ref().map(|c| {
            let CurrentEnvironment::Instance { instance_id, .. } = c;
            instance_id.clone()
        })
    });
    let engine = match &target {
        // Bound on its first query, until it stops: its roots don't change meanwhile.
        Some(id) => match state.query.bound(&target) {
            Some(engine) => engine,
            None => {
                let upstream = instance_catalog(&state, id).await?;
                state.query.engine_for(target.clone(), &upstream).await?
            }
        },
        None => {
            let upstream = state
                .directory
                .resolve(UNITY_CATALOG)
                .ok_or(QueryError::NoEnvironment)?;
            state.query.engine_for(None, &upstream).await?
        }
    };
    state.query.start(engine, query_id, request, on_chunk).await
}

/// Where instance `id`'s Unity Catalog is, with the app's credential for it.
async fn instance_catalog(state: &AppState, id: &str) -> Result<Upstream, QueryError> {
    let failed = |e: shell_core::instances::RunError| QueryError::Failed {
        message: e.to_string(),
    };
    let runner = state.runner.clone().map_err(failed)?;
    let local = runner.bindings(id).map_err(failed)?;
    let remote = runner.function_roots(id).await.map_err(failed)?;
    state
        .directory
        .resolve_instance(&local, &remote, UNITY_CATALOG)
        .map_err(|message| QueryError::Failed { message })?
        .ok_or(QueryError::NoCatalog)
}

/// Stop a query (the UI's `AbortSignal`).
#[tauri::command]
#[specta::specta]
pub fn query_cancel(state: State<'_, AppState>, query_id: u32) {
    state.query.cancel(query_id);
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn collect(engine: &Engine, request: QueryRequest) -> Vec<Vec<u8>> {
        let chunks = engine.run(request).await.expect("the query plans");
        chunks
            .map(|c| c.expect("the query runs"))
            .collect::<Vec<_>>()
            .await
    }

    fn rows(frame: &[u8]) -> u64 {
        u64::from_le_bytes(frame[..8].try_into().unwrap())
    }

    fn sql(sql: &str, limit: Option<u32>) -> QueryRequest {
        QueryRequest {
            sql: sql.into(),
            limit,
            ..Default::default()
        }
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn runs_plain_sql_without_a_catalog() {
        let engine = Engine::build(None).await.unwrap();
        let frames = collect(&engine, sql("SELECT 1 AS one", None)).await;
        assert_eq!(frames.iter().map(|f| rows(f)).sum::<u64>(), 1);
        // An Arrow IPC stream follows the count: its continuation marker first.
        assert_eq!(&frames[0][8..12], &[0xff, 0xff, 0xff, 0xff]);
        retire(Some(Arc::new(engine)));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn clamps_to_the_limit() {
        let engine = Engine::build(None).await.unwrap();
        let values = (0..50)
            .map(|i| format!("({i})"))
            .collect::<Vec<_>>()
            .join(",");
        let frames = collect(&engine, sql(&format!("VALUES {values}"), Some(7))).await;
        assert_eq!(frames.iter().map(|f| rows(f)).sum::<u64>(), 7);
        // An empty result still says what its columns are.
        let frames = collect(&engine, sql("SELECT 1 AS one WHERE false", None)).await;
        assert_eq!(frames.len(), 1);
        assert_eq!(rows(&frames[0]), 0);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn refuses_what_does_not_plan() {
        let engine = Engine::build(None).await.unwrap();
        let err = match engine.run(sql("SELEC 1", None)).await {
            Err(e) => e,
            Ok(_) => panic!("a typo planned"),
        };
        assert!(matches!(err, QueryError::Invalid { .. }), "{err:?}");
        // Without a catalog there is nowhere to create a table: refused, when it plans
        // or when it runs.
        let refused = match engine.run(sql("CREATE TABLE t (a INT)", None)).await {
            Err(_) => true,
            Ok(chunks) => chunks.any(|c| async move { c.is_err() }).await,
        };
        assert!(refused, "DDL ran without a catalog");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn forgets_the_engines_of_stopped_instances() {
        let host = QueryHost::default();
        for id in ["a", "b"] {
            let engine = Arc::new(Engine::build(None).await.unwrap());
            host.engines.lock().unwrap().insert(Some(id.into()), engine);
        }
        let by_hand = Arc::new(Engine::build(None).await.unwrap());
        host.engines.lock().unwrap().insert(None, by_hand);
        host.forget(|id| id == "b");
        assert!(host.bound(&Some("a".into())).is_none());
        assert!(host.bound(&Some("b".into())).is_some());
        // What is bound by hand isn't an instance's.
        assert!(host.bound(&None).is_some());
    }
}
