//! [`TokenSource`]: where a bearer token comes from, for a client whose
//! token can change while it runs (one renewed before it expires).
//!
//! [`StaticToken`] is a fixed one. With feature `databricks`,
//! [`DatabricksOAuth`](crate::databricks::DatabricksOAuth) is a service
//! principal's OAuth token, renewed before it expires.

use async_trait::async_trait;

use crate::Result;

/// A bearer token, current whenever it's asked for.
#[async_trait]
pub trait TokenSource: Send + Sync + 'static {
    /// The token to send now. A source that renews returns its cached token
    /// while it's current, and a new one once it's due.
    async fn token(&self) -> Result<String>;
}

/// The same token every time.
#[derive(Clone)]
pub struct StaticToken(String);

impl StaticToken {
    pub fn new(token: impl Into<String>) -> Self {
        Self(token.into())
    }
}

impl std::fmt::Debug for StaticToken {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("StaticToken(..)")
    }
}

#[async_trait]
impl TokenSource for StaticToken {
    async fn token(&self) -> Result<String> {
        Ok(self.0.clone())
    }
}
