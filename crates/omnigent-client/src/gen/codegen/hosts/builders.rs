// @generated — do not edit by hand.
#![allow(unused_mut)]
#![allow(unused_imports)]

#[cfg(not(target_arch = "wasm32"))]
type BoxFut<'a, T> = ::futures::future::BoxFuture<'a, T>;
#[cfg(target_arch = "wasm32")]
type BoxFut<'a, T> = ::futures::future::LocalBoxFuture<'a, T>;
use super::client::*;
use crate::codegen::error::Result;
use crate::models::omnigent::v1::*;
use std::future::IntoFuture;

/// Builder for hosts
pub struct ListHostsBuilder {
    client: HostClient,
    request: ListHostsRequest,
}

impl ListHostsBuilder {
    /// Create a new builder instance.
    /// Obtain via the corresponding method on `HostClient`.
    pub(crate) fn new(client: HostClient) -> Self {
        let request = ListHostsRequest {
            ..Default::default()
        };
        Self { client, request }
    }
}

impl IntoFuture for ListHostsBuilder {
    type Output = Result<HostList>;
    type IntoFuture = BoxFut<'static, Self::Output>;
    fn into_future(self) -> Self::IntoFuture {
        let client = self.client;
        let request = self.request;
        Box::pin(async move { client.list_hosts(&request).await })
    }
}
