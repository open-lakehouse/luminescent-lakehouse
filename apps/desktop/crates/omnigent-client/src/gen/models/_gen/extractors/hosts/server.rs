// @generated — do not edit by hand.
#![allow(unused_mut, unused_imports)]

use crate::models::omnigent::v1::*;
use axum::{RequestExt, RequestPartsExt};

impl<S: Send + Sync> axum::extract::FromRequestParts<S> for ListHostsRequest {
    type Rejection = axum::response::Response;
    async fn from_request_parts(
        parts: &mut axum::http::request::Parts,
        _state: &S,
    ) -> Result<Self, Self::Rejection> {
        Ok(ListHostsRequest {
            ..Default::default()
        })
    }
}
