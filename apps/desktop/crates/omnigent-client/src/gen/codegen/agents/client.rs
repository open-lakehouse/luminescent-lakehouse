// @generated — do not edit by hand.
#![allow(unused_imports)]

use crate::codegen::error::Result;
use crate::codegen::transport::HttpClient;
use crate::models::omnigent::v1::*;
use url::Url;

/// HTTP client for service operations
#[derive(Clone)]
pub struct AgentClient {
    pub(crate) client: HttpClient,
    pub(crate) base_url: Url,
}

impl AgentClient {
    /// Create a new client instance
    pub fn new(client: HttpClient, mut base_url: Url) -> Self {
        if !base_url.path().ends_with('/') {
            base_url.set_path(&format!("{}/", base_url.path()));
        }
        Self { client, base_url }
    }

    pub async fn list_agents(&self, request: &ListAgentsRequest) -> Result<AgentList> {
        let mut url = self.base_url.join("v1/agents")?;
        if let Some(ref value) = request.limit {
            url.query_pairs_mut()
                .append_pair("limit", &value.to_string());
        }
        if let Some(ref value) = request.after {
            url.query_pairs_mut()
                .append_pair("after", &value.to_string());
        }
        if let Some(ref value) = request.scope {
            url.query_pairs_mut()
                .append_pair("scope", &value.to_string());
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
}
