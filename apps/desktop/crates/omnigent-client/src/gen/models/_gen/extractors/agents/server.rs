// @generated — do not edit by hand.
#![allow(unused_mut, unused_imports)]

use crate::models::omnigent::v1::*;
use axum::{RequestExt, RequestPartsExt};

impl<S: Send + Sync> axum::extract::FromRequestParts<S> for ListAgentsRequest {
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
            scope: Option<String>,
        }
        let axum_extra::extract::Query(QueryParams {
            limit,
            after,
            scope,
        }) = parts
            .extract::<axum_extra::extract::Query<QueryParams>>()
            .await
            .map_err(axum::response::IntoResponse::into_response)?;
        Ok(ListAgentsRequest {
            limit,
            after,
            scope,
            ..Default::default()
        })
    }
}
