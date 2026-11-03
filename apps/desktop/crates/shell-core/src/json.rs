//! [`Json`]: a JSON value passed through as is (a `Selection`, a planner DTO), typed
//! `unknown` in the generated TypeScript, where the UI narrows it to the editor's own
//! types. (specta can't export `serde_json::Value` itself: its numbers may be `i64`.)

use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(transparent)]
pub struct Json(
    #[cfg_attr(feature = "specta", specta(type = specta_typescript::Unknown))] pub Value,
);

impl From<Value> for Json {
    fn from(value: Value) -> Self {
        Json(value)
    }
}

impl From<Json> for Value {
    fn from(json: Json) -> Self {
        json.0
    }
}
