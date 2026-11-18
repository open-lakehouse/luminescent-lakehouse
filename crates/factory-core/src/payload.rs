//! Packing the core payload kinds into `google.protobuf.Any`.

use buffa::{Message, MessageName};
use buffa_types::google::protobuf::Any;
use factory_proto::workgraph::v1 as wg;

/// Pack a payload message under its own type URL.
pub fn pack<M: Message + MessageName>(msg: &M) -> Any {
    Any::pack(msg, M::TYPE_URL)
}

/// A node spec with a packed payload.
pub fn spec<M: Message + MessageName>(title: &str, body: &str, payload: &M) -> wg::NodeSpec {
    wg::NodeSpec {
        title: title.into(),
        body: body.into(),
        payload: pack(payload).into(),
        ..Default::default()
    }
}

pub fn task(acceptance: &str, touched_paths: &[&str]) -> wg::Task {
    wg::Task {
        acceptance: acceptance.into(),
        touched_paths: touched_paths.iter().map(|s| (*s).to_owned()).collect(),
        ..Default::default()
    }
}
