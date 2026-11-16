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

/// Builder for agents
pub struct ListAgentsBuilder {
    client: AgentClient,
    request: ListAgentsRequest,
}

impl ListAgentsBuilder {
    /// Create a new builder instance.
    /// Obtain via the corresponding method on `AgentClient`.
    pub(crate) fn new(client: AgentClient) -> Self {
        let request = ListAgentsRequest {
            ..Default::default()
        };
        Self { client, request }
    }

    /// Set limit
    pub fn with_limit(mut self, limit: impl Into<Option<i32>>) -> Self {
        self.request.limit = limit.into();
        self
    }

    /// Set after
    pub fn with_after(mut self, after: impl Into<Option<String>>) -> Self {
        self.request.after = after.into();
        self
    }

    /// `user`: the caller's own agents (`omnigent agent add`), which the default (the
    /// server's agents) never lists.
    pub fn with_scope(mut self, scope: impl Into<Option<String>>) -> Self {
        self.request.scope = scope.into();
        self
    }
}

impl IntoFuture for ListAgentsBuilder {
    type Output = Result<AgentList>;
    type IntoFuture = BoxFut<'static, Self::Output>;
    fn into_future(self) -> Self::IntoFuture {
        let client = self.client;
        let request = self.request;
        Box::pin(async move { client.list_agents(&request).await })
    }
}
