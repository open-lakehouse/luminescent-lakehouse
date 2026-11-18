/// What a work-graph operation can fail with. Each variant maps onto one
/// Connect error code in factory-server.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("not found: {0}")]
    NotFound(String),
    #[error("already exists: {0}")]
    AlreadyExists(String),
    /// The request breaks the schema: an unknown kind, a payload that
    /// doesn't match its kind, an edge between kinds that may not be joined.
    #[error("invalid: {0}")]
    InvalidArgument(String),
    /// The request is well-formed but the graph's state refuses it: a second
    /// parent, a cycle, deleting a node with children.
    #[error("precondition failed: {0}")]
    FailedPrecondition(String),
    /// An optimistic update lost: the etag is stale.
    #[error("conflict: {0}")]
    Conflict(String),
    #[error("internal: {0}")]
    Internal(String),
}

pub type Result<T, E = Error> = std::result::Result<T, E>;

impl Error {
    pub fn invalid(msg: impl Into<String>) -> Self {
        Self::InvalidArgument(msg.into())
    }

    pub fn precondition(msg: impl Into<String>) -> Self {
        Self::FailedPrecondition(msg.into())
    }

    pub fn internal(msg: impl std::fmt::Display) -> Self {
        Self::Internal(msg.to_string())
    }
}

impl From<olai_store::Error> for Error {
    fn from(e: olai_store::Error) -> Self {
        use olai_store::Error as E;
        match e {
            E::NotFound => Self::NotFound("object".into()),
            E::AlreadyExists => Self::AlreadyExists("object".into()),
            E::Conflict => Self::Conflict("the node changed since it was read".into()),
            E::InvalidArgument(m) => Self::InvalidArgument(m),
            other => Self::Internal(other.to_string()),
        }
    }
}

impl From<serde_json::Error> for Error {
    fn from(e: serde_json::Error) -> Self {
        Self::Internal(format!("serialization: {e}"))
    }
}

impl From<plan_tree::TreeError> for Error {
    fn from(e: plan_tree::TreeError) -> Self {
        Self::Internal(format!("merkle tree: {e}"))
    }
}
