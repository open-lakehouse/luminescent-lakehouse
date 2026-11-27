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
//! `CancelRun`, `RetryRun`, `ReportArtifact`) and the attention inbox only
//! write records: an execution, a queued run, a canceled run, a resolution.
//! The [`reconciler`] acts on them, so the router works without one (nothing
//! is dispatched until a reconciler runs over the same factory). See
//! docs/factory/execution.md.
//!
//! The router itself has no authorization (Cedar comes later). The actor
//! comes from the `x-factory-actor` header, defaulting to `operator`, and the
//! path from `x-factory-via`, defaulting to `control-room`. A host that
//! authenticates its callers sets the actor itself: the standalone binary's
//! Apps mode ([`apps::require_caller`]) replaces it with the caller the
//! Databricks Apps proxy forwarded.

#[cfg(feature = "bin")]
pub mod apps;
#[cfg(feature = "bin")]
pub mod cli;
mod convert;
#[cfg(feature = "bin")]
pub mod git_credential;
pub mod reconciler;
pub mod runs;

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

#[allow(refining_impl_trait)]
impl FactoryService for FactoryApi {
    async fn get_schema(
        &self,
        _ctx: RequestContext,
        _req: ServiceRequest<'_, v1::GetSchemaRequest>,
    ) -> R<v1::GetSchemaResponse> {
        Response::ok(factory_core::wire::schema_response(
            self.factory.extensions(),
        ))
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
        Response::ok(factory_core::wire::graph_response(state))
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
                                change: change
                                    .run
                                    .map(|run| v1::watch_graph_response::Change::Run(Box::new(run)))
                                    .or(change.attention.map(|item| {
                                        v1::watch_graph_response::Change::Attention(Box::new(item))
                                    })),
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
        Response::ok(factory_core::wire::revision_response(
            &rev,
            approved.as_deref(),
        ))
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
        ctx: RequestContext,
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
        runs::start_execution(
            &self.factory,
            root,
            &req.revision,
            &choice,
            &provenance(&ctx),
        )
        .await
        .map_err(error)?;
        Response::ok(v1::StartExecutionResponse {
            placements,
            ..Default::default()
        })
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
        ctx: RequestContext,
        req: ServiceRequest<'_, v1::StartPlanningRequest>,
    ) -> R<v1::StartPlanningResponse> {
        let req = req.to_owned_message();
        let goal = required("goal", &req.goal)?;
        let registry = self.factory.registry();
        let repos = req
            .repos
            .iter()
            .map(|slug| {
                registry.repo(slug).cloned().ok_or_else(|| {
                    ConnectError::invalid_argument(format!("repo {slug} isn't in the registry"))
                })
            })
            .collect::<Result<Vec<_>, _>>()?;
        let title = opt(&req.title).map(str::to_owned).unwrap_or_else(|| {
            goal.lines()
                .next()
                .unwrap_or(goal)
                .trim_start_matches('#')
                .trim()
                .to_owned()
        });
        let plan = wg::Plan {
            goal: goal.into(),
            repos,
            planner_profile: req.planner_profile.clone(),
            ..Default::default()
        };
        let p = provenance(&ctx);
        let root = self
            .factory
            .create_node(
                None,
                factory_core::schema::PLAN,
                factory_core::payload::spec(&title, goal, &plan),
                Default::default(),
                &p,
            )
            .await
            .map_err(error)?;
        let executor = self.executor_name(&root.kind)?;
        if let Some(env) = opt(&req.environment) {
            self.check_placement(&root.r#ref, &root.r#ref, env).await?;
        }
        let run = runs::queue_run(
            &self.factory,
            &root.r#ref,
            &root.r#ref,
            &executor,
            "",
            &req.environment,
            &p,
        )
        .await
        .map_err(error)?;
        Response::ok(v1::StartPlanningResponse {
            root: root.into(),
            run: run.into(),
            ..Default::default()
        })
    }

    async fn start_run(
        &self,
        ctx: RequestContext,
        req: ServiceRequest<'_, v1::StartRunRequest>,
    ) -> R<v1::StartRunResponse> {
        let req = req.to_owned_message();
        let node_ref = required("node", &req.node)?;
        let node = self.factory.node(node_ref).await.map_err(error)?;
        let root = self.factory.root_of(node_ref).await.map_err(error)?;
        let executor = self.executor_name(&node.kind)?;
        if let Some(profile) = opt(&req.profile)
            && self.factory.registry().profile(profile).is_none()
        {
            return Err(ConnectError::invalid_argument(format!(
                "profile {profile} isn't in the registry"
            )));
        }
        if let Some(env) = opt(&req.environment) {
            self.check_placement(&root, node_ref, env).await?;
        }
        let run = runs::queue_run(
            &self.factory,
            &root,
            node_ref,
            &executor,
            &req.profile,
            &req.environment,
            &provenance(&ctx),
        )
        .await
        .map_err(error)?;
        Response::ok(v1::StartRunResponse {
            run: run.into(),
            ..Default::default()
        })
    }

    async fn cancel_run(
        &self,
        ctx: RequestContext,
        req: ServiceRequest<'_, v1::CancelRunRequest>,
    ) -> R<v1::CancelRunResponse> {
        let req = req.to_owned_message();
        let (_, mut run) = self
            .factory
            .find_run(required("run", &req.run)?)
            .await
            .map_err(error)?;
        if !runs::is_live(&run) {
            return Err(ConnectError::failed_precondition(format!(
                "{} isn't live",
                run.r#ref
            )));
        }
        // The steps stay active until the reconciler has interrupted their
        // sessions.
        run.state = wg::RunState::RUN_STATE_CANCELED.into();
        run.error = opt(&req.reason).unwrap_or("canceled").to_owned();
        run.lease_holder.clear();
        run.updated_at = buffa_types::google::protobuf::Timestamp::now().into();
        self.factory
            .put_run(&run, &provenance(&ctx))
            .await
            .map_err(error)?;
        Response::ok(v1::CancelRunResponse {
            run: run.into(),
            ..Default::default()
        })
    }

    async fn retry_run(
        &self,
        ctx: RequestContext,
        req: ServiceRequest<'_, v1::RetryRunRequest>,
    ) -> R<v1::RetryRunResponse> {
        let req = req.to_owned_message();
        let (root, run) = self
            .factory
            .find_run(required("run", &req.run)?)
            .await
            .map_err(error)?;
        let next = runs::retry(&self.factory, &root, &run, "retried", &provenance(&ctx))
            .await
            .map_err(error)?;
        Response::ok(v1::RetryRunResponse {
            run: next.into(),
            ..Default::default()
        })
    }

    async fn report_artifact(
        &self,
        ctx: RequestContext,
        req: ServiceRequest<'_, v1::ReportArtifactRequest>,
    ) -> R<v1::ReportArtifactResponse> {
        let req = req.to_owned_message();
        let run = self
            .factory
            .report_artifacts(
                required("run", &req.run)?,
                &req.artifacts,
                &provenance(&ctx),
            )
            .await
            .map_err(error)?;
        Response::ok(v1::ReportArtifactResponse {
            run: run.into(),
            ..Default::default()
        })
    }

    async fn list_attention(
        &self,
        _ctx: RequestContext,
        req: ServiceRequest<'_, v1::ListAttentionRequest>,
    ) -> R<v1::ListAttentionResponse> {
        let req = req.to_owned_message();
        let items = self
            .factory
            .attention(opt(&req.root), req.include_resolved)
            .await
            .map_err(error)?;
        Response::ok(v1::ListAttentionResponse {
            items,
            ..Default::default()
        })
    }

    async fn resolve_attention(
        &self,
        ctx: RequestContext,
        req: ServiceRequest<'_, v1::ResolveAttentionRequest>,
    ) -> R<v1::ResolveAttentionResponse> {
        let req = req.to_owned_message();
        let resolution = match req.resolution.as_known() {
            Some(v1::Resolution::RESOLUTION_ACCEPT) => "accept",
            Some(v1::Resolution::RESOLUTION_DECLINE) => "decline",
            Some(v1::Resolution::RESOLUTION_RETRY) => "retry",
            Some(v1::Resolution::RESOLUTION_DISMISS) => "dismiss",
            _ => return Err(ConnectError::invalid_argument("resolution is required")),
        };
        let item = self
            .factory
            .resolve_attention(
                required("ref", &req.r#ref)?,
                resolution,
                &req.comment,
                &provenance(&ctx),
            )
            .await
            .map_err(error)?;
        Response::ok(v1::ResolveAttentionResponse {
            item: item.into(),
            ..Default::default()
        })
    }
}

impl FactoryApi {
    fn executor_name(&self, kind: &str) -> Result<String, ConnectError> {
        self.factory
            .extensions()
            .executor(kind)
            .map(|e| e.name().to_owned())
            .ok_or_else(|| {
                ConnectError::failed_precondition(format!(
                    "no executor runs {}",
                    factory_core::schema::short_kind(kind)
                ))
            })
    }

    /// Refuse an environment override that can't place the node's sessions.
    async fn check_placement(&self, root: &str, node: &str, env: &str) -> Result<(), ConnectError> {
        let choice = runs::execution(&self.factory, root)
            .await
            .map_err(error)?
            .map(|e| e.choice())
            .unwrap_or_default();
        let placements = self
            .factory
            .placements(root, &choice, Some((node, env)))
            .await
            .map_err(error)?;
        let problems: Vec<String> = placements
            .iter()
            .filter(|p| p.node == node && !p.problem.is_empty())
            .map(|p| format!("{}: {}", p.role, p.problem))
            .collect();
        if problems.is_empty() {
            Ok(())
        } else {
            Err(ConnectError::failed_precondition(problems.join("; ")))
        }
    }
}
