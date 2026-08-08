//! Store capability and reader-negotiation documents.

use serde::{Deserialize, Serialize};

pub use super::inspect::INSPECT_READER_PROFILE_SCHEMA;
#[cfg(test)]
use crate::session::AUTHORITY_CURSOR_SCHEMA_V2;
use crate::session::{AuthorityCursorV2, StoreCapabilityInspection, StoreCapabilityStatus};
pub const READER_UPGRADE_REQUIRED_SCHEMA: &str = "pointbreak.reader-upgrade-required";
pub const STORE_MIGRATION_REQUIRED_SCHEMA: &str = "pointbreak.store-migration-required";
pub const STORE_MIGRATION_IN_PROGRESS_SCHEMA: &str = "pointbreak.store-migration-in-progress";

/// Authority state for the reader's target store cohort.
///
/// This is a recurring migration boundary, not a one-time legacy-store flag:
/// a root may be ready for one cohort and require migration for a later cohort.
/// It gates semantic Change reads independently of projection freshness and of
/// exact Revision or fact-body availability.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReaderProfileAvailabilityV1 {
    MigrationRequired,
    MigrationInProgress,
    Ready,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReaderProfileDocumentV1 {
    pub schema: String,
    pub version: u32,
    /// Whether the target cohort has durable, complete authority in this root.
    /// This does not summarize captured-resource or externalized-body bytes.
    pub availability: ReaderProfileAvailabilityV1,
    pub authority_cursor: AuthorityCursorV2,
    #[serde(skip_serializing_if = "Option::is_none")]
    /// Minimum compatible semantic reader for the activated target cohort.
    /// Product versions and resource-body availability do not participate.
    pub minimum_reader_profile: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub activation_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub manifest_hash: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub completion_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub commit_graph_stamp: Option<String>,
    pub documents: std::collections::BTreeMap<String, u32>,
}

impl From<&StoreCapabilityInspection> for ReaderProfileDocumentV1 {
    fn from(inspection: &StoreCapabilityInspection) -> Self {
        let (availability, activation_id, manifest_hash, completion_id) = match &inspection.status {
            StoreCapabilityStatus::MigrationRequired => (
                ReaderProfileAvailabilityV1::MigrationRequired,
                None,
                None,
                None,
            ),
            StoreCapabilityStatus::MigrationInProgress {
                activation_id,
                manifest_hash,
            } => (
                ReaderProfileAvailabilityV1::MigrationInProgress,
                Some(activation_id.clone()),
                Some(manifest_hash.clone()),
                None,
            ),
            StoreCapabilityStatus::Ready {
                activation_id,
                manifest_hash,
                completion_id,
            } => (
                ReaderProfileAvailabilityV1::Ready,
                Some(activation_id.clone()),
                Some(manifest_hash.clone()),
                Some(completion_id.clone()),
            ),
        };
        Self {
            schema: INSPECT_READER_PROFILE_SCHEMA.to_owned(),
            version: 1,
            availability,
            authority_cursor: inspection.cursor.clone(),
            minimum_reader_profile: inspection.minimum_reader_profile.clone(),
            activation_id,
            manifest_hash,
            completion_id,
            commit_graph_stamp: None,
            documents: super::change_revision_document_registry()
                .iter()
                .map(|(schema, version)| ((*schema).to_owned(), *version))
                .collect(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReaderUpgradeRequiredDocumentV1 {
    pub schema: String,
    pub version: u32,
    pub required_reader_profile: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub available_reader_profile: Option<String>,
    pub code: String,
}

impl ReaderUpgradeRequiredDocumentV1 {
    pub fn new(required: impl Into<String>, available: Option<String>) -> Self {
        Self {
            schema: READER_UPGRADE_REQUIRED_SCHEMA.to_owned(),
            version: 1,
            required_reader_profile: required.into(),
            available_reader_profile: available,
            code: "reader_upgrade_required".to_owned(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "state",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum ChangeQueryUnavailableDocumentV1 {
    MigrationRequired {
        schema: String,
        version: u32,
        authority_cursor: AuthorityCursorV2,
    },
    MigrationInProgress {
        schema: String,
        version: u32,
        authority_cursor: AuthorityCursorV2,
        activation_id: String,
        manifest_hash: String,
    },
}

impl ChangeQueryUnavailableDocumentV1 {
    /// Return the store-level stop document that must precede any Change query.
    /// `None` means the store has completed the activated cohort.
    pub fn for_inspection(inspection: &StoreCapabilityInspection) -> Option<Self> {
        match &inspection.status {
            StoreCapabilityStatus::MigrationRequired => Some(Self::MigrationRequired {
                schema: STORE_MIGRATION_REQUIRED_SCHEMA.to_owned(),
                version: 1,
                authority_cursor: inspection.cursor.clone(),
            }),
            StoreCapabilityStatus::MigrationInProgress {
                activation_id,
                manifest_hash,
            } => Some(Self::MigrationInProgress {
                schema: STORE_MIGRATION_IN_PROGRESS_SCHEMA.to_owned(),
                version: 1,
                authority_cursor: inspection.cursor.clone(),
                activation_id: activation_id.clone(),
                manifest_hash: manifest_hash.clone(),
            }),
            StoreCapabilityStatus::Ready { .. } => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn inspect(status: StoreCapabilityStatus) -> StoreCapabilityInspection {
        StoreCapabilityInspection {
            minimum_reader_profile: (!matches!(status, StoreCapabilityStatus::MigrationRequired))
                .then_some("review_change_revision_v1".to_owned()),
            status,
            cursor: AuthorityCursorV2 {
                schema: AUTHORITY_CURSOR_SCHEMA_V2.to_owned(),
                journal_record_count: 0,
                event_count: 0,
                journal_record_set_hash: format!("sha256:{}", "0".repeat(64)),
                event_set_hash: format!("sha256:{}", "0".repeat(64)),
                capability_set_hash: format!("sha256:{}", "0".repeat(64)),
            },
        }
    }

    #[test]
    fn change_queries_stop_at_store_level_for_l0_and_m1() {
        let l0 = inspect(StoreCapabilityStatus::MigrationRequired);
        assert!(matches!(
            ChangeQueryUnavailableDocumentV1::for_inspection(&l0),
            Some(ChangeQueryUnavailableDocumentV1::MigrationRequired { .. })
        ));

        let m1 = inspect(StoreCapabilityStatus::MigrationInProgress {
            activation_id: "activation:sha256:test".to_owned(),
            manifest_hash: format!("sha256:{}", "1".repeat(64)),
        });
        assert!(matches!(
            ChangeQueryUnavailableDocumentV1::for_inspection(&m1),
            Some(ChangeQueryUnavailableDocumentV1::MigrationInProgress { .. })
        ));

        let l2 = inspect(StoreCapabilityStatus::Ready {
            activation_id: "activation:sha256:test".to_owned(),
            manifest_hash: format!("sha256:{}", "1".repeat(64)),
            completion_id: "completion:sha256:test".to_owned(),
        });
        assert!(ChangeQueryUnavailableDocumentV1::for_inspection(&l2).is_none());
        assert_eq!(
            ReaderProfileDocumentV1::from(&l2).availability,
            ReaderProfileAvailabilityV1::Ready
        );
    }

    #[test]
    fn incapable_readers_receive_a_typed_upgrade_document() {
        let document = ReaderUpgradeRequiredDocumentV1::new(
            "review_change_revision_v1",
            Some("legacy_revision_v2".to_owned()),
        );
        assert_eq!(document.schema, READER_UPGRADE_REQUIRED_SCHEMA);
        assert_eq!(document.code, "reader_upgrade_required");
        assert!(document.required_reader_profile.contains("change_revision"));
    }
}
