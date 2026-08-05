mod auxiliary_documents;
mod relation_proof;

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceAvailabilityV1 {
    Available,
    Removed,
    Missing,
}

pub use auxiliary_documents::*;
pub use relation_proof::*;
