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

/// Builder for session
pub struct CreateSessionBuilder {
    client: SessionClient,
    request: CreateSessionRequest,
}

impl CreateSessionBuilder {
    /// Create a new builder instance.
    /// Obtain via the corresponding method on `SessionClient`.
    pub(crate) fn new(client: SessionClient, agent_id: impl Into<String>) -> Self {
        let request = CreateSessionRequest {
            agent_id: agent_id.into(),
            ..Default::default()
        };
        Self { client, request }
    }

    /// Set title
    pub fn with_title(mut self, title: impl Into<Option<String>>) -> Self {
        self.request.title = title.into();
        self
    }

    /// Set labels
    pub fn with_labels<I, K, V>(mut self, labels: I) -> Self
    where
        I: IntoIterator<Item = (K, V)>,
        K: Into<String>,
        V: Into<String>,
    {
        self.request.labels = labels
            .into_iter()
            .map(|(k, v)| (k.into(), v.into()))
            .collect();
        self
    }

    /// `external` (a connected host, the default) or `managed`.
    pub fn with_host_type(mut self, host_type: impl Into<Option<String>>) -> Self {
        self.request.host_type = host_type.into();
        self
    }

    /// The host to launch the runner on; needs `workspace`.
    pub fn with_host_id(mut self, host_id: impl Into<Option<String>>) -> Self {
        self.request.host_id = host_id.into();
        self
    }

    /// An absolute path on the host, inside the agent's `os_env.cwd`.
    pub fn with_workspace(mut self, workspace: impl Into<Option<String>>) -> Self {
        self.request.workspace = workspace.into();
        self
    }

    /// Set model_override
    pub fn with_model_override(mut self, model_override: impl Into<Option<String>>) -> Self {
        self.request.model_override = model_override.into();
        self
    }

    /// Set reasoning_effort
    pub fn with_reasoning_effort(mut self, reasoning_effort: impl Into<Option<String>>) -> Self {
        self.request.reasoning_effort = reasoning_effort.into();
        self
    }

    /// Events posted before the first turn, as `PostEventRequest` bodies without the id.
    pub fn with_initial_items<I>(mut self, initial_items: I) -> Self
    where
        I: IntoIterator<Item = SessionEventInput>,
    {
        self.request.initial_items = initial_items.into_iter().collect();
        self
    }
}

impl IntoFuture for CreateSessionBuilder {
    type Output = Result<Session>;
    type IntoFuture = BoxFut<'static, Self::Output>;
    fn into_future(self) -> Self::IntoFuture {
        let client = self.client;
        let request = self.request;
        Box::pin(async move { client.create_session(&request).await })
    }
}

/// Builder for sessions
pub struct ListSessionsBuilder {
    client: SessionClient,
    request: ListSessionsRequest,
}

impl ListSessionsBuilder {
    /// Create a new builder instance.
    /// Obtain via the corresponding method on `SessionClient`.
    pub(crate) fn new(client: SessionClient) -> Self {
        let request = ListSessionsRequest {
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

    /// Set before
    pub fn with_before(mut self, before: impl Into<Option<String>>) -> Self {
        self.request.before = before.into();
        self
    }

    /// Set agent_id
    pub fn with_agent_id(mut self, agent_id: impl Into<Option<String>>) -> Self {
        self.request.agent_id = agent_id.into();
        self
    }

    /// Set agent_name
    pub fn with_agent_name(mut self, agent_name: impl Into<Option<String>>) -> Self {
        self.request.agent_name = agent_name.into();
        self
    }

    /// Set include_archived
    pub fn with_include_archived(mut self, include_archived: impl Into<Option<bool>>) -> Self {
        self.request.include_archived = include_archived.into();
        self
    }
}

impl IntoFuture for ListSessionsBuilder {
    type Output = Result<SessionList>;
    type IntoFuture = BoxFut<'static, Self::Output>;
    fn into_future(self) -> Self::IntoFuture {
        let client = self.client;
        let request = self.request;
        Box::pin(async move { client.list_sessions(&request).await })
    }
}

/// Builder for session
pub struct GetSessionBuilder {
    client: SessionClient,
    request: GetSessionRequest,
}

impl GetSessionBuilder {
    /// Create a new builder instance.
    /// Obtain via the corresponding method on `SessionClient`.
    pub(crate) fn new(client: SessionClient, session_id: impl Into<String>) -> Self {
        let request = GetSessionRequest {
            session_id: session_id.into(),
            ..Default::default()
        };
        Self { client, request }
    }

    /// Set include_items
    pub fn with_include_items(mut self, include_items: impl Into<Option<bool>>) -> Self {
        self.request.include_items = include_items.into();
        self
    }

    /// Set include_liveness
    pub fn with_include_liveness(mut self, include_liveness: impl Into<Option<bool>>) -> Self {
        self.request.include_liveness = include_liveness.into();
        self
    }
}

impl IntoFuture for GetSessionBuilder {
    type Output = Result<Session>;
    type IntoFuture = BoxFut<'static, Self::Output>;
    fn into_future(self) -> Self::IntoFuture {
        let client = self.client;
        let request = self.request;
        Box::pin(async move { client.get_session(&request).await })
    }
}

/// Builder for session
pub struct UpdateSessionBuilder {
    client: SessionClient,
    request: UpdateSessionRequest,
}

impl UpdateSessionBuilder {
    /// Create a new builder instance.
    /// Obtain via the corresponding method on `SessionClient`.
    pub(crate) fn new(client: SessionClient, session_id: impl Into<String>) -> Self {
        let request = UpdateSessionRequest {
            session_id: session_id.into(),
            ..Default::default()
        };
        Self { client, request }
    }

    /// Set title
    pub fn with_title(mut self, title: impl Into<Option<String>>) -> Self {
        self.request.title = title.into();
        self
    }

    /// Set archived
    pub fn with_archived(mut self, archived: impl Into<Option<bool>>) -> Self {
        self.request.archived = archived.into();
        self
    }

    /// Set labels
    pub fn with_labels<I, K, V>(mut self, labels: I) -> Self
    where
        I: IntoIterator<Item = (K, V)>,
        K: Into<String>,
        V: Into<String>,
    {
        self.request.labels = labels
            .into_iter()
            .map(|(k, v)| (k.into(), v.into()))
            .collect();
        self
    }

    /// Set model_override
    pub fn with_model_override(mut self, model_override: impl Into<Option<String>>) -> Self {
        self.request.model_override = model_override.into();
        self
    }

    /// Set reasoning_effort
    pub fn with_reasoning_effort(mut self, reasoning_effort: impl Into<Option<String>>) -> Self {
        self.request.reasoning_effort = reasoning_effort.into();
        self
    }
}

impl IntoFuture for UpdateSessionBuilder {
    type Output = Result<Session>;
    type IntoFuture = BoxFut<'static, Self::Output>;
    fn into_future(self) -> Self::IntoFuture {
        let client = self.client;
        let request = self.request;
        Box::pin(async move { client.update_session(&request).await })
    }
}

/// Builder for event
pub struct PostEventBuilder {
    client: SessionClient,
    request: PostEventRequest,
}

impl PostEventBuilder {
    /// Create a new builder instance.
    /// Obtain via the corresponding method on `SessionClient`.
    pub(crate) fn new(
        client: SessionClient,
        session_id: impl Into<String>,
        event_type: impl Into<String>,
    ) -> Self {
        let request = PostEventRequest {
            session_id: session_id.into(),
            event_type: event_type.into(),
            ..Default::default()
        };
        Self { client, request }
    }

    /// Set data
    pub fn with_data(
        mut self,
        data: impl Into<Option<::buffa_types::google::protobuf::Struct>>,
    ) -> Self {
        self.request.data = {
            let data: ::core::option::Option<_> = data.into();
            buffa::MessageField::from(data)
        };
        self
    }
}

impl IntoFuture for PostEventBuilder {
    type Output = Result<EventAck>;
    type IntoFuture = BoxFut<'static, Self::Output>;
    fn into_future(self) -> Self::IntoFuture {
        let client = self.client;
        let request = self.request;
        Box::pin(async move { client.post_event(&request).await })
    }
}

/// Builder for elicitation
pub struct ResolveElicitationBuilder {
    client: SessionClient,
    request: ResolveElicitationRequest,
}

impl ResolveElicitationBuilder {
    /// Create a new builder instance.
    /// Obtain via the corresponding method on `SessionClient`.
    pub(crate) fn new(
        client: SessionClient,
        session_id: impl Into<String>,
        elicitation_id: impl Into<String>,
        action: impl Into<String>,
    ) -> Self {
        let request = ResolveElicitationRequest {
            session_id: session_id.into(),
            elicitation_id: elicitation_id.into(),
            action: action.into(),
            ..Default::default()
        };
        Self { client, request }
    }

    /// The form's values, with `accept`.
    pub fn with_content(
        mut self,
        content: impl Into<Option<::buffa_types::google::protobuf::Struct>>,
    ) -> Self {
        self.request.content = {
            let content: ::core::option::Option<_> = content.into();
            buffa::MessageField::from(content)
        };
        self
    }
}

impl IntoFuture for ResolveElicitationBuilder {
    type Output = Result<EventAck>;
    type IntoFuture = BoxFut<'static, Self::Output>;
    fn into_future(self) -> Self::IntoFuture {
        let client = self.client;
        let request = self.request;
        Box::pin(async move { client.resolve_elicitation(&request).await })
    }
}
