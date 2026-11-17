// @generated — do not edit by hand.
#![allow(unused_imports)]

use super::super::encode_path_value;
use crate::codegen::error::Result;
use crate::codegen::transport::HttpClient;
use crate::models::omnigent::v1::*;
use url::Url;

/// HTTP client for service operations
#[derive(Clone)]
pub struct SessionClient {
    pub(crate) client: HttpClient,
    pub(crate) base_url: Url,
}

impl SessionClient {
    /// Create a new client instance
    pub fn new(client: HttpClient, mut base_url: Url) -> Self {
        if !base_url.path().ends_with('/') {
            base_url.set_path(&format!("{}/", base_url.path()));
        }
        Self { client, base_url }
    }

    /// Create a session bound to an installed agent (the JSON form; the multipart form uploads
    /// an agent bundle and is not covered).
    pub async fn create_session(&self, request: &CreateSessionRequest) -> Result<Session> {
        let url = self.base_url.join("v1/sessions")?;
        let response = self.client.post(url).json(request).send().await?;
        if !response.status().is_success() {
            return Err(crate::codegen::error::parse_error_response(response).await);
        }
        let result = response.bytes().await?;
        let options = ::buffa::json::JsonParseOptions::new().ignore_unknown_enum_values(true);
        Ok(::buffa::json::with_json_parse_options(&options, || {
            serde_json::from_slice(&result)
        })?)
    }

    pub async fn list_sessions(&self, request: &ListSessionsRequest) -> Result<SessionList> {
        let mut url = self.base_url.join("v1/sessions")?;
        if let Some(ref value) = request.limit {
            url.query_pairs_mut()
                .append_pair("limit", &value.to_string());
        }
        if let Some(ref value) = request.after {
            url.query_pairs_mut()
                .append_pair("after", &value.to_string());
        }
        if let Some(ref value) = request.before {
            url.query_pairs_mut()
                .append_pair("before", &value.to_string());
        }
        if let Some(ref value) = request.agent_id {
            url.query_pairs_mut()
                .append_pair("agent_id", &value.to_string());
        }
        if let Some(ref value) = request.agent_name {
            url.query_pairs_mut()
                .append_pair("agent_name", &value.to_string());
        }
        if let Some(ref value) = request.include_archived {
            url.query_pairs_mut()
                .append_pair("include_archived", &value.to_string());
        }
        if let Some(ref value) = request.kind {
            url.query_pairs_mut()
                .append_pair("kind", &value.to_string());
        }
        let response = self.client.get(url).send().await?;
        if !response.status().is_success() {
            return Err(crate::codegen::error::parse_error_response(response).await);
        }
        let result = response.bytes().await?;
        let options = ::buffa::json::JsonParseOptions::new().ignore_unknown_enum_values(true);
        Ok(::buffa::json::with_json_parse_options(&options, || {
            serde_json::from_slice(&result)
        })?)
    }

    /// A session's snapshot; with `include_items`, its history. The stream has no replay, so a
    /// reconnecting client resyncs from this.
    pub async fn get_session(&self, request: &GetSessionRequest) -> Result<Session> {
        let formatted_path = format!(
            "v1/sessions/{}",
            encode_path_value(&request.session_id, false)
        );
        let mut url = self.base_url.join(&formatted_path)?;
        if let Some(ref value) = request.include_items {
            url.query_pairs_mut()
                .append_pair("include_items", &value.to_string());
        }
        if let Some(ref value) = request.include_liveness {
            url.query_pairs_mut()
                .append_pair("include_liveness", &value.to_string());
        }
        let response = self.client.get(url).send().await?;
        if !response.status().is_success() {
            return Err(crate::codegen::error::parse_error_response(response).await);
        }
        let result = response.bytes().await?;
        let options = ::buffa::json::JsonParseOptions::new().ignore_unknown_enum_values(true);
        Ok(::buffa::json::with_json_parse_options(&options, || {
            serde_json::from_slice(&result)
        })?)
    }

    pub async fn update_session(&self, request: &UpdateSessionRequest) -> Result<Session> {
        let formatted_path = format!(
            "v1/sessions/{}",
            encode_path_value(&request.session_id, false)
        );
        let url = self.base_url.join(&formatted_path)?;
        let mut body = request.clone();
        body.session_id = Default::default();
        let response = self.client.patch(url).json(&body).send().await?;
        if !response.status().is_success() {
            return Err(crate::codegen::error::parse_error_response(response).await);
        }
        let result = response.bytes().await?;
        let options = ::buffa::json::JsonParseOptions::new().ignore_unknown_enum_values(true);
        Ok(::buffa::json::with_json_parse_options(&options, || {
            serde_json::from_slice(&result)
        })?)
    }

    /// Post an event: a user `message`, an `interrupt`, a client tool's `function_call_output`.
    /// Not in the server's OpenAPI document (`include_in_schema=False`, routes_events.py).
    pub async fn post_event(&self, request: &PostEventRequest) -> Result<EventAck> {
        let formatted_path = format!(
            "v1/sessions/{}/events",
            encode_path_value(&request.session_id, false)
        );
        let url = self.base_url.join(&formatted_path)?;
        let mut body = request.clone();
        body.session_id = Default::default();
        let response = self.client.post(url).json(&body).send().await?;
        if !response.status().is_success() {
            return Err(crate::codegen::error::parse_error_response(response).await);
        }
        let result = response.bytes().await?;
        let options = ::buffa::json::JsonParseOptions::new().ignore_unknown_enum_values(true);
        Ok(::buffa::json::with_json_parse_options(&options, || {
            serde_json::from_slice(&result)
        })?)
    }

    /// The pull requests the server linked to the session: the ones its agent created or
    /// worked on, attached by hand, or inferred from its branch. Not on the snapshot.
    pub async fn list_pull_requests(
        &self,
        request: &ListPullRequestsRequest,
    ) -> Result<PullRequestList> {
        let formatted_path = format!(
            "v1/sessions/{}/pull-requests",
            encode_path_value(&request.session_id, false)
        );
        let mut url = self.base_url.join(&formatted_path)?;
        if let Some(ref value) = request.limit {
            url.query_pairs_mut()
                .append_pair("limit", &value.to_string());
        }
        if let Some(ref value) = request.after {
            url.query_pairs_mut()
                .append_pair("after", &value.to_string());
        }
        let response = self.client.get(url).send().await?;
        if !response.status().is_success() {
            return Err(crate::codegen::error::parse_error_response(response).await);
        }
        let result = response.bytes().await?;
        let options = ::buffa::json::JsonParseOptions::new().ignore_unknown_enum_values(true);
        Ok(::buffa::json::with_json_parse_options(&options, || {
            serde_json::from_slice(&result)
        })?)
    }

    /// Answer a `response.elicitation_request`. Not in the server's OpenAPI document either
    /// (routes_elicitations.py).
    pub async fn resolve_elicitation(
        &self,
        request: &ResolveElicitationRequest,
    ) -> Result<EventAck> {
        let formatted_path = format!(
            "v1/sessions/{}/elicitations/{}/resolve",
            encode_path_value(&request.session_id, false),
            encode_path_value(&request.elicitation_id, false)
        );
        let url = self.base_url.join(&formatted_path)?;
        let mut body = request.clone();
        body.session_id = Default::default();
        body.elicitation_id = Default::default();
        let response = self.client.post(url).json(&body).send().await?;
        if !response.status().is_success() {
            return Err(crate::codegen::error::parse_error_response(response).await);
        }
        let result = response.bytes().await?;
        let options = ::buffa::json::JsonParseOptions::new().ignore_unknown_enum_values(true);
        Ok(::buffa::json::with_json_parse_options(&options, || {
            serde_json::from_slice(&result)
        })?)
    }
}
