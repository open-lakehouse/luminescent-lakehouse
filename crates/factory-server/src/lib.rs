//! The factory.v1 Connect API over [`factory_core::Factory`].
//!
//! [`router`] is what an embedder mounts, over a [`Factory`] built with
//! whichever extensions the composition root chooses
//! ([`factory_core::FactoryBuilder`]). [`cli::run`] is the standalone
//! binary's body, for a composition root that wants the same flags; the
//! reference binary runs it with the built-in extension alone.
//!
//! The standalone binary serves the router on
//! a port, and the desktop app can drive the same router in-process (as
//! luminescent-lakehouse's Neon hosts do with `Router::oneshot`) or put it
//! behind its `service_fetch` proxy. Clients speak Connect, JSON or binary,
//! at `/factory.v1.FactoryService/<Method>`.
//!
//! The run-driving RPCs (`StartPlanning`, `StartExecution`, `StartRun`,
//! `CancelRun`, `RetryRun`, `ReportArtifact`) and the attention inbox return
//! `Unimplemented` until the reconciler lands; see
//! docs/factory/execution.md. The registry and run reads, and placement
//! resolution, work. `StartExecution` already refuses a plan it can't place
//! or a revision that isn't the approved one.
//!
//! There's no authorization yet (Cedar comes later). The actor comes from the
//! `x-factory-actor` header, defaulting to `operator`, and the path from
//! `x-factory-via`, defaulting to `control-room`.

#[cfg(feature = "bin")]
pub mod cli;
mod convert;

use std::sync::Arc;

use connectrpc::{
    ConnectError, RequestContext, Response, ServiceRequest, ServiceResult, ServiceStream,
};
use factory_core::{Factory, Provenance};
use factory_proto::connect_gen::factory::v1::{FactoryService, FactoryServiceExt};
use factory_proto::v1;
use factory_proto::workgraph::v1 as wg;
use futures::StreamExt;

pub use convert::error;

/// The path prefix the Connect service routes under.
pub const SERVICE_PATH: &str = "/factory.v1.FactoryService";

/// The axum router serving `FactoryService` for `factory`.
pub fn router(factory: Arc<Factory>) -> axum::Router {
    let connect = Arc::new(FactoryApi { factory }).register(connectrpc::Router::new());
    axum::Router::new().route_service(
        &format!("{SERVICE_PATH}/{{*rest}}"),
        connect.into_axum_service(),
    )
}

/// The `FactoryService` implementation.
pub struct FactoryApi {
    factory: Arc<Factory>,
}

impl FactoryApi {
    pub fn new(factory: Arc<Factory>) -> Self {
        Self { factory }
    }
}

fn provenance(ctx: &RequestContext) -> Provenance {
    let header = |name: &str, default: &str| {
        ctx.header(name)
            .and_then(|v| v.to_str().ok())
            .filter(|v| !v.is_empty())
            .unwrap_or(default)
            .to_owned()
    };
    Provenance::new(
        header("x-factory-actor", "operator"),
        header("x-factory-via", "control-room"),
    )
}

fn required<'a>(field: &str, value: &'a str) -> Result<&'a str, ConnectError> {
    if value.is_empty() {
        Err(ConnectError::invalid_argument(format!(
            "{field} is required"
        )))
    } else {
        Ok(value)
    }
}

fn opt(value: &str) -> Option<&str> {
    (!value.is_empty()).then_some(value)
}

type R<T> = ServiceResult<T>;

/// The error for an RPC the reconciler will serve.
fn not_yet(rpc: &str) -> ConnectError {
    ConnectError::unimplemented(format!(
        "{rpc} needs the reconciler, which isn't built yet (docs/factory/execution.md)"
    ))
}

#[allow(refining_impl_trait)]
impl FactoryService for FactoryApi {
    async fn get_schema(
        &self,
        _ctx: RequestContext,
        _req: ServiceRequest<'_, v1::GetSchemaRequest>,
    ) -> R<v1::GetSchemaResponse> {
        let extensions = self.factory.extensions();
        Response::ok(v1::GetSchemaResponse {
            schema: extensions.schema().proto().clone().into(),
            file_descriptor_set: extensions.descriptors().to_vec(),
            ..Default::default()
        })
    }

    async fn list_graphs(
        &self,
        _ctx: RequestContext,
        _req: ServiceRequest<'_, v1::ListGraphsRequest>,
    ) -> R<v1::ListGraphsResponse> {
        let graphs = self.factory.list_graphs().await.map_err(error)?;
        Response::ok(v1::ListGraphsResponse {
            graphs: graphs.into_iter().map(convert::summary).collect(),
            ..Default::default()
        })
    }

    async fn get_graph(
        &self,
        _ctx: RequestContext,
        req: ServiceRequest<'_, v1::GetGraphRequest>,
    ) -> R<v1::GetGraphResponse> {
        let req = req.to_owned_message();
        let state = self
            .factory
            .graph(required("root", &req.root)?)
            .await
            .map_err(error)?;
        Response::ok(v1::GetGraphResponse {
            graph: state.graph.into(),
            revision: state.revision,
            approved_revision: state.approved.unwrap_or_default(),
            drifted: state.drifted,
            ..Default::default()
        })
    }

    async fn watch_graph(
        &self,
        _ctx: RequestContext,
        req: ServiceRequest<'_, v1::WatchGraphRequest>,
    ) -> R<ServiceStream<v1::WatchGraphResponse>> {
        let req = req.to_owned_message();
        let root = required("root", &req.root)?.to_owned();
        // Subscribe before reading, so no change between the two is lost.
        let rx = self.factory.subscribe();
        let current = self.factory.graph(&root).await.map_err(error)?;
        let first = v1::WatchGraphResponse {
            revision: current.revision,
            sequence: self.factory.sequence(),
            ..Default::default()
        };
        let changes = futures::stream::unfold(rx, move |mut rx| {
            let root = root.clone();
            async move {
                loop {
                    match rx.recv().await {
                        Ok(change) if change.root == root => {
                            let msg = v1::WatchGraphResponse {
                                revision: change.revision,
                                sequence: change.sequence,
                                change: change.run.map(|run| {
                                    v1::watch_graph_response::Change::Run(Box::new(run))
                                }),
                                ..Default::default()
                            };
                            return Some((Ok(msg), rx));
                        }
                        Ok(_) => continue,
                        // A slow watcher only needs to know something
                        // changed; it reloads the graph either way.
                        Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                        Err(tokio::sync::broadcast::error::RecvError::Closed) => return None,
                    }
                }
            }
        });
        let stream = futures::stream::once(async move { Ok(first) }).chain(changes);
        Response::ok(Box::pin(stream) as ServiceStream<v1::WatchGraphResponse>)
    }

    async fn create_node(
        &self,
        ctx: RequestContext,
        req: ServiceRequest<'_, v1::CreateNodeRequest>,
    ) -> R<v1::CreateNodeResponse> {
        let req = req.to_owned_message();
        let spec = req.spec.into_option().unwrap_or_default();
        let node = self
            .factory
            .create_node(
                opt(&req.parent),
                required("kind", &req.kind)?,
                spec,
                req.labels.into_iter().collect(),
                &provenance(&ctx),
            )
            .await
            .map_err(error)?;
        Response::ok(v1::CreateNodeResponse {
            node: node.into(),
            ..Default::default()
        })
    }

    async fn update_node(
        &self,
        ctx: RequestContext,
        req: ServiceRequest<'_, v1::UpdateNodeRequest>,
    ) -> R<v1::UpdateNodeResponse> {
        let req = req.to_owned_message();
        let labels = (!req.labels.is_empty()).then(|| req.labels.into_iter().collect());
        let node = self
            .factory
            .update_node(
                required("ref", &req.r#ref)?,
                req.spec.into_option(),
                labels,
                opt(&req.etag).map(str::to_owned),
                &provenance(&ctx),
            )
            .await
            .map_err(error)?;
        Response::ok(v1::UpdateNodeResponse {
            node: node.into(),
            ..Default::default()
        })
    }

    async fn move_node(
        &self,
        ctx: RequestContext,
        req: ServiceRequest<'_, v1::MoveNodeRequest>,
    ) -> R<v1::MoveNodeResponse> {
        let req = req.to_owned_message();
        let node = self
            .factory
            .move_node(
                required("ref", &req.r#ref)?,
                required("new_parent", &req.new_parent)?,
                &provenance(&ctx),
            )
            .await
            .map_err(error)?;
        Response::ok(v1::MoveNodeResponse {
            node: node.into(),
            ..Default::default()
        })
    }

    async fn delete_node(
        &self,
        ctx: RequestContext,
        req: ServiceRequest<'_, v1::DeleteNodeRequest>,
    ) -> R<v1::DeleteNodeResponse> {
        let req = req.to_owned_message();
        self.factory
            .delete_node(
                required("ref", &req.r#ref)?,
                req.recursive,
                &provenance(&ctx),
            )
            .await
            .map_err(error)?;
        Response::ok(v1::DeleteNodeResponse::default())
    }

    async fn add_edge(
        &self,
        ctx: RequestContext,
        req: ServiceRequest<'_, v1::AddEdgeRequest>,
    ) -> R<v1::AddEdgeResponse> {
        let edge: wg::Edge = req
            .to_owned_message()
            .edge
            .into_option()
            .ok_or_else(|| ConnectError::invalid_argument("edge is required"))?;
        self.factory
            .add_edge(&edge, &provenance(&ctx))
            .await
            .map_err(error)?;
        Response::ok(v1::AddEdgeResponse::default())
    }

    async fn remove_edge(
        &self,
        ctx: RequestContext,
        req: ServiceRequest<'_, v1::RemoveEdgeRequest>,
    ) -> R<v1::RemoveEdgeResponse> {
        let edge: wg::Edge = req
            .to_owned_message()
            .edge
            .into_option()
            .ok_or_else(|| ConnectError::invalid_argument("edge is required"))?;
        self.factory
            .remove_edge(&edge, &provenance(&ctx))
            .await
            .map_err(error)?;
        Response::ok(v1::RemoveEdgeResponse::default())
    }

    async fn set_phase(
        &self,
        ctx: RequestContext,
        req: ServiceRequest<'_, v1::SetPhaseRequest>,
    ) -> R<v1::SetPhaseResponse> {
        let req = req.to_owned_message();
        let phase = req
            .phase
            .as_known()
            .ok_or_else(|| ConnectError::invalid_argument("unknown phase"))?;
        let node = self
            .factory
            .set_phase(required("ref", &req.r#ref)?, phase, &provenance(&ctx))
            .await
            .map_err(error)?;
        Response::ok(v1::SetPhaseResponse {
            node: node.into(),
            ..Default::default()
        })
    }

    async fn annotate(
        &self,
        ctx: RequestContext,
        req: ServiceRequest<'_, v1::AnnotateRequest>,
    ) -> R<v1::AnnotateResponse> {
        let req = req.to_owned_message();
        let annotation = self
            .factory
            .annotate(
                required("node", &req.node)?,
                required("kind", &req.kind)?,
                req.payload.into_option(),
                &provenance(&ctx),
            )
            .await
            .map_err(error)?;
        Response::ok(v1::AnnotateResponse {
            annotation: annotation.into(),
            ..Default::default()
        })
    }

    async fn list_annotations(
        &self,
        _ctx: RequestContext,
        req: ServiceRequest<'_, v1::ListAnnotationsRequest>,
    ) -> R<v1::ListAnnotationsResponse> {
        let req = req.to_owned_message();
        let annotations = self
            .factory
            .annotations(required("node", &req.node)?)
            .await
            .map_err(error)?;
        Response::ok(v1::ListAnnotationsResponse {
            annotations,
            ..Default::default()
        })
    }

    async fn list_runs(
        &self,
        _ctx: RequestContext,
        req: ServiceRequest<'_, v1::ListRunsRequest>,
    ) -> R<v1::ListRunsResponse> {
        let req = req.to_owned_message();
        let runs = self
            .factory
            .runs(required("root", &req.root)?)
            .await
            .map_err(error)?;
        Response::ok(v1::ListRunsResponse {
            runs,
            ..Default::default()
        })
    }

    async fn list_revisions(
        &self,
        _ctx: RequestContext,
        req: ServiceRequest<'_, v1::ListRevisionsRequest>,
    ) -> R<v1::ListRevisionsResponse> {
        let req = req.to_owned_message();
        let root = required("root", &req.root)?;
        let approved = self.factory.graph(root).await.map_err(error)?.approved;
        let revisions = self.factory.revisions(root).await.map_err(error)?;
        Response::ok(v1::ListRevisionsResponse {
            revisions: revisions
                .iter()
                .map(|r| convert::revision(r, approved.as_deref()))
                .collect(),
            ..Default::default()
        })
    }

    async fn get_revision(
        &self,
        _ctx: RequestContext,
        req: ServiceRequest<'_, v1::GetRevisionRequest>,
    ) -> R<v1::GetRevisionResponse> {
        let req = req.to_owned_message();
        let root = required("root", &req.root)?;
        let approved = self.factory.graph(root).await.map_err(error)?.approved;
        let rev = self
            .factory
            .revision(root, opt(&req.root_hash))
            .await
            .map_err(error)?;
        Response::ok(v1::GetRevisionResponse {
            revision: convert::revision(&rev, approved.as_deref()).into(),
            tree: convert::tree(&rev.tree).into(),
            text: rev.text.clone(),
            sections: rev
                .sections
                .iter()
                .map(|s| v1::TextSection {
                    key: s.key.clone(),
                    start_line: s.start_line,
                    end_line: s.end_line,
                    ..Default::default()
                })
                .collect(),
            ..Default::default()
        })
    }

    async fn diff_revisions(
        &self,
        _ctx: RequestContext,
        req: ServiceRequest<'_, v1::DiffRevisionsRequest>,
    ) -> R<v1::DiffRevisionsResponse> {
        let req = req.to_owned_message();
        let entries = self
            .factory
            .diff(
                required("root", &req.root)?,
                opt(&req.base),
                opt(&req.target),
                req.compact,
            )
            .await
            .map_err(error)?;
        Response::ok(v1::DiffRevisionsResponse {
            entries: entries.into_iter().map(convert::diff_entry).collect(),
            ..Default::default()
        })
    }

    async fn approve_revision(
        &self,
        ctx: RequestContext,
        req: ServiceRequest<'_, v1::ApproveRevisionRequest>,
    ) -> R<v1::ApproveRevisionResponse> {
        let req = req.to_owned_message();
        let rev = self
            .factory
            .approve(
                required("root", &req.root)?,
                required("root_hash", &req.root_hash)?,
                &provenance(&ctx),
            )
            .await
            .map_err(error)?;
        Response::ok(v1::ApproveRevisionResponse {
            revision: convert::revision(&rev, Some(&rev.root_hash)).into(),
            ..Default::default()
        })
    }

    async fn list_profiles(
        &self,
        _ctx: RequestContext,
        _req: ServiceRequest<'_, v1::ListProfilesRequest>,
    ) -> R<v1::ListProfilesResponse> {
        Response::ok(v1::ListProfilesResponse {
            profiles: self.factory.registry().profiles().to_vec(),
            ..Default::default()
        })
    }

    async fn list_repos(
        &self,
        _ctx: RequestContext,
        _req: ServiceRequest<'_, v1::ListReposRequest>,
    ) -> R<v1::ListReposResponse> {
        Response::ok(v1::ListReposResponse {
            repos: self.factory.registry().repos().to_vec(),
            ..Default::default()
        })
    }

    async fn list_environments(
        &self,
        _ctx: RequestContext,
        _req: ServiceRequest<'_, v1::ListEnvironmentsRequest>,
    ) -> R<v1::ListEnvironmentsResponse> {
        let registry = self.factory.registry();
        Response::ok(v1::ListEnvironmentsResponse {
            environments: registry.environments().to_vec(),
            default_environment: registry.defaults().environment.clone(),
            ..Default::default()
        })
    }

    async fn resolve_placements(
        &self,
        _ctx: RequestContext,
        req: ServiceRequest<'_, v1::ResolvePlacementsRequest>,
    ) -> R<v1::ResolvePlacementsResponse> {
        let req = req.to_owned_message();
        let choice = req.choice.into_option().unwrap_or_default();
        let placements = self
            .factory
            .placements(required("root", &req.root)?, &choice, None)
            .await
            .map_err(error)?;
        Response::ok(v1::ResolvePlacementsResponse {
            placeable: placements.iter().all(|p| p.problem.is_empty()),
            placements,
            ..Default::default()
        })
    }

    async fn start_execution(
        &self,
        _ctx: RequestContext,
        req: ServiceRequest<'_, v1::StartExecutionRequest>,
    ) -> R<v1::StartExecutionResponse> {
        let req = req.to_owned_message();
        let root = required("root", &req.root)?;
        let state = self.factory.graph(root).await.map_err(error)?;
        match state.approved.as_deref() {
            None => {
                return Err(ConnectError::failed_precondition(
                    "the plan has no approved revision",
                ));
            }
            Some(approved) if approved != required("revision", &req.revision)? => {
                return Err(ConnectError::failed_precondition(format!(
                    "{} isn't the approved revision ({approved})",
                    req.revision
                )));
            }
            Some(_) => {}
        }
        let choice = req.choice.into_option().unwrap_or_default();
        let placements = self
            .factory
            .placements(root, &choice, None)
            .await
            .map_err(error)?;
        let problems: Vec<String> = placements
            .iter()
            .filter(|p| !p.problem.is_empty())
            .map(|p| format!("{} ({}): {}", p.node, p.role, p.problem))
            .collect();
        if !problems.is_empty() {
            return Err(ConnectError::failed_precondition(format!(
                "can't place every session: {}",
                problems.join("; ")
            )));
        }
        Err(not_yet("StartExecution"))
    }

    async fn get_run(
        &self,
        _ctx: RequestContext,
        req: ServiceRequest<'_, v1::GetRunRequest>,
    ) -> R<v1::GetRunResponse> {
        let req = req.to_owned_message();
        let run = self
            .factory
            .run(required("root", &req.root)?, required("run", &req.run)?)
            .await
            .map_err(error)?;
        Response::ok(v1::GetRunResponse {
            run: run.into(),
            ..Default::default()
        })
    }

    async fn start_planning(
        &self,
        _ctx: RequestContext,
        _req: ServiceRequest<'_, v1::StartPlanningRequest>,
    ) -> R<v1::StartPlanningResponse> {
        Err(not_yet("StartPlanning"))
    }

    async fn start_run(
        &self,
        _ctx: RequestContext,
        _req: ServiceRequest<'_, v1::StartRunRequest>,
    ) -> R<v1::StartRunResponse> {
        Err(not_yet("StartRun"))
    }

    async fn cancel_run(
        &self,
        _ctx: RequestContext,
        _req: ServiceRequest<'_, v1::CancelRunRequest>,
    ) -> R<v1::CancelRunResponse> {
        Err(not_yet("CancelRun"))
    }

    async fn retry_run(
        &self,
        _ctx: RequestContext,
        _req: ServiceRequest<'_, v1::RetryRunRequest>,
    ) -> R<v1::RetryRunResponse> {
        Err(not_yet("RetryRun"))
    }

    async fn report_artifact(
        &self,
        _ctx: RequestContext,
        _req: ServiceRequest<'_, v1::ReportArtifactRequest>,
    ) -> R<v1::ReportArtifactResponse> {
        Err(not_yet("ReportArtifact"))
    }

    async fn list_attention(
        &self,
        _ctx: RequestContext,
        _req: ServiceRequest<'_, v1::ListAttentionRequest>,
    ) -> R<v1::ListAttentionResponse> {
        Err(not_yet("ListAttention"))
    }

    async fn resolve_attention(
        &self,
        _ctx: RequestContext,
        _req: ServiceRequest<'_, v1::ResolveAttentionRequest>,
    ) -> R<v1::ResolveAttentionResponse> {
        Err(not_yet("ResolveAttention"))
    }
}
