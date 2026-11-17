// @generated — do not edit by hand.
#![allow(unused_mut, unused_imports)]

use crate::models::omnigent::v1::*;
use axum::{RequestExt, RequestPartsExt};

impl<S: Send + Sync> axum::extract::FromRequest<S> for CreateSessionRequest {
    type Rejection = axum::response::Response;
    async fn from_request(
        req: axum::extract::Request<axum::body::Body>,
        _state: &S,
    ) -> Result<Self, Self::Rejection> {
        let axum::extract::Json(request) = req
            .extract()
            .await
            .map_err(axum::response::IntoResponse::into_response)?;
        Ok(request)
    }
}

impl<S: Send + Sync> axum::extract::FromRequestParts<S> for ListSessionsRequest {
    type Rejection = axum::response::Response;
    async fn from_request_parts(
        parts: &mut axum::http::request::Parts,
        _state: &S,
    ) -> Result<Self, Self::Rejection> {
        #[derive(serde::Deserialize)]
        struct QueryParams {
            #[serde(default)]
            limit: Option<i32>,
            #[serde(default)]
            after: Option<String>,
            #[serde(default)]
            before: Option<String>,
            #[serde(default)]
            agent_id: Option<String>,
            #[serde(default)]
            agent_name: Option<String>,
            #[serde(default)]
            include_archived: Option<bool>,
            #[serde(default)]
            kind: Option<String>,
        }
        let axum_extra::extract::Query(QueryParams {
            limit,
            after,
            before,
            agent_id,
            agent_name,
            include_archived,
            kind,
        }) = parts
            .extract::<axum_extra::extract::Query<QueryParams>>()
            .await
            .map_err(axum::response::IntoResponse::into_response)?;
        Ok(ListSessionsRequest {
            limit,
            after,
            before,
            agent_id,
            agent_name,
            include_archived,
            kind,
            ..Default::default()
        })
    }
}

impl<S: Send + Sync> axum::extract::FromRequestParts<S> for GetSessionRequest {
    type Rejection = axum::response::Response;
    async fn from_request_parts(
        parts: &mut axum::http::request::Parts,
        _state: &S,
    ) -> Result<Self, Self::Rejection> {
        let axum::extract::Path(session_id) = parts
            .extract::<axum::extract::Path<String>>()
            .await
            .map_err(axum::response::IntoResponse::into_response)?;
        #[derive(serde::Deserialize)]
        struct QueryParams {
            #[serde(default)]
            include_items: Option<bool>,
            #[serde(default)]
            include_liveness: Option<bool>,
        }
        let axum_extra::extract::Query(QueryParams {
            include_items,
            include_liveness,
        }) = parts
            .extract::<axum_extra::extract::Query<QueryParams>>()
            .await
            .map_err(axum::response::IntoResponse::into_response)?;
        Ok(GetSessionRequest {
            session_id,
            include_items,
            include_liveness,
            ..Default::default()
        })
    }
}

impl<S: Send + Sync> axum::extract::FromRequest<S> for UpdateSessionRequest {
    type Rejection = axum::response::Response;
    async fn from_request(
        mut req: axum::extract::Request<axum::body::Body>,
        _state: &S,
    ) -> Result<Self, Self::Rejection> {
        let (mut parts, body) = req.into_parts();
        let axum::extract::Path(session_id) = parts
            .extract::<axum::extract::Path<String>>()
            .await
            .map_err(axum::response::IntoResponse::into_response)?;
        let body_req = axum::extract::Request::from_parts(parts, body);
        let axum::extract::Json::<UpdateSessionRequest>(body) = body_req
            .extract()
            .await
            .map_err(axum::response::IntoResponse::into_response)?;
        let (title, archived, labels, model_override, reasoning_effort) = (
            body.title,
            body.archived,
            body.labels,
            body.model_override,
            body.reasoning_effort,
        );
        Ok(UpdateSessionRequest {
            session_id,
            title,
            archived,
            labels,
            model_override,
            reasoning_effort,
            ..Default::default()
        })
    }
}

impl<S: Send + Sync> axum::extract::FromRequest<S> for PostEventRequest {
    type Rejection = axum::response::Response;
    async fn from_request(
        mut req: axum::extract::Request<axum::body::Body>,
        _state: &S,
    ) -> Result<Self, Self::Rejection> {
        let (mut parts, body) = req.into_parts();
        let axum::extract::Path(session_id) = parts
            .extract::<axum::extract::Path<String>>()
            .await
            .map_err(axum::response::IntoResponse::into_response)?;
        let body_req = axum::extract::Request::from_parts(parts, body);
        let axum::extract::Json::<PostEventRequest>(body) = body_req
            .extract()
            .await
            .map_err(axum::response::IntoResponse::into_response)?;
        let (event_type, data) = (body.event_type, body.data);
        Ok(PostEventRequest {
            session_id,
            event_type,
            data,
            ..Default::default()
        })
    }
}

impl<S: Send + Sync> axum::extract::FromRequestParts<S> for ListPullRequestsRequest {
    type Rejection = axum::response::Response;
    async fn from_request_parts(
        parts: &mut axum::http::request::Parts,
        _state: &S,
    ) -> Result<Self, Self::Rejection> {
        let axum::extract::Path(session_id) = parts
            .extract::<axum::extract::Path<String>>()
            .await
            .map_err(axum::response::IntoResponse::into_response)?;
        #[derive(serde::Deserialize)]
        struct QueryParams {
            #[serde(default)]
            limit: Option<i32>,
            #[serde(default)]
            after: Option<String>,
        }
        let axum_extra::extract::Query(QueryParams { limit, after }) = parts
            .extract::<axum_extra::extract::Query<QueryParams>>()
            .await
            .map_err(axum::response::IntoResponse::into_response)?;
        Ok(ListPullRequestsRequest {
            session_id,
            limit,
            after,
            ..Default::default()
        })
    }
}

impl<S: Send + Sync> axum::extract::FromRequest<S> for ResolveElicitationRequest {
    type Rejection = axum::response::Response;
    async fn from_request(
        mut req: axum::extract::Request<axum::body::Body>,
        _state: &S,
    ) -> Result<Self, Self::Rejection> {
        let (mut parts, body) = req.into_parts();
        let axum::extract::Path((session_id, elicitation_id)) = parts
            .extract::<axum::extract::Path<(String, String)>>()
            .await
            .map_err(axum::response::IntoResponse::into_response)?;
        let body_req = axum::extract::Request::from_parts(parts, body);
        let axum::extract::Json::<ResolveElicitationRequest>(body) = body_req
            .extract()
            .await
            .map_err(axum::response::IntoResponse::into_response)?;
        let (action, content) = (body.action, body.content);
        Ok(ResolveElicitationRequest {
            session_id,
            elicitation_id,
            action,
            content,
            ..Default::default()
        })
    }
}
