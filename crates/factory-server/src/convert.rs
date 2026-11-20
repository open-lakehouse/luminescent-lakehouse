//! factory-core's types to and from the factory.v1 wire types.

use connectrpc::ConnectError;
use factory_core::Error;

pub use factory_core::wire::{diff_entry, revision, summary};

pub fn error(e: Error) -> ConnectError {
    match e {
        Error::NotFound(m) => ConnectError::not_found(m),
        Error::AlreadyExists(m) => ConnectError::already_exists(m),
        Error::InvalidArgument(m) => ConnectError::invalid_argument(m),
        Error::FailedPrecondition(m) => ConnectError::failed_precondition(m),
        Error::Conflict(m) => ConnectError::aborted(m),
        Error::Internal(m) => {
            tracing::error!(%m, "internal error");
            ConnectError::internal(m)
        }
    }
}
