use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceRef {
    pub source_system: String,
    pub source_id: String,
}

impl SourceRef {
    pub fn new(source_system: impl Into<String>, source_id: impl Into<String>) -> Self {
        Self {
            source_system: source_system.into(),
            source_id: source_id.into(),
        }
    }
}
