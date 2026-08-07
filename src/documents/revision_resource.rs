//! Exact captured-Revision resource documents.
//!
//! A captured resource is authoritative only for one [`RevisionRefV1`]. Git
//! comparisons and Revision interdiffs have separate identities and must never
//! be substituted when these bytes are absent or fail their bound hash.

use serde::{Deserialize, Serialize};

use crate::canonical_hash::sha256_json_prefixed;
use crate::error::Result;
use crate::model::{ObjectId, RevisionRefV1, TrackId};
pub use crate::session::ContentAvailabilityV1;

pub const REVISION_RESOURCE_SCHEMA: &str = "pointbreak.review-revision-resource";

pub type RevisionResourceAvailabilityV1 = ContentAvailabilityV1;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RevisionResourceRefV1 {
    pub revision: RevisionRefV1,
    pub object_id: ObjectId,
}

/// Projection axes that may change the captured-resource representation
/// without changing the underlying immutable Revision or object.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RevisionResourceProjectionV1 {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub track_id: Option<TrackId>,
    pub include_body: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RevisionResourceDocumentV1 {
    pub schema: String,
    pub version: u32,
    pub resource: RevisionResourceRefV1,
    pub projection: RevisionResourceProjectionV1,
    pub availability: ContentAvailabilityV1,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub captured_document_hash: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub captured_document: Option<serde_json::Value>,
    pub diagnostics: Vec<String>,
    pub cache_key: String,
}

impl RevisionResourceDocumentV1 {
    /// Wrap an already-projected captured document after checking the artifact
    /// hash that was read. This function never rebuilds captured rows from Git.
    pub fn available(
        resource: RevisionResourceRefV1,
        projection: RevisionResourceProjectionV1,
        actual_artifact_hash: &str,
        captured_document: serde_json::Value,
    ) -> Result<Self> {
        let matches = resource.revision.object_artifact_content_hash == actual_artifact_hash;
        let availability = if matches {
            ContentAvailabilityV1::Available
        } else {
            ContentAvailabilityV1::Mismatch
        };
        let captured_document_hash = matches
            .then(|| sha256_json_prefixed(&captured_document))
            .transpose()?;
        let cache_key = exact_cache_key(
            REVISION_RESOURCE_SCHEMA,
            1,
            &serde_json::json!({
                "resource": resource,
                "projection": projection,
                "availability": availability,
                "capturedDocumentHash": captured_document_hash,
            }),
        )?;
        Ok(Self {
            schema: REVISION_RESOURCE_SCHEMA.to_owned(),
            version: 1,
            resource,
            projection,
            availability,
            captured_document_hash,
            captured_document: matches.then_some(captured_document),
            diagnostics: (!matches)
                .then_some("captured_artifact_hash_mismatch".to_owned())
                .into_iter()
                .collect(),
            cache_key,
        })
    }

    pub fn unavailable(
        resource: RevisionResourceRefV1,
        projection: RevisionResourceProjectionV1,
        availability: ContentAvailabilityV1,
    ) -> Result<Self> {
        if availability == ContentAvailabilityV1::Available {
            return Err(crate::error::ShoreError::Message(
                "available captured resources require the captured document and verified hash"
                    .to_owned(),
            ));
        }
        let cache_key = exact_cache_key(
            REVISION_RESOURCE_SCHEMA,
            1,
            &serde_json::json!({
                "resource": resource,
                "projection": projection,
                "availability": availability,
            }),
        )?;
        Ok(Self {
            schema: REVISION_RESOURCE_SCHEMA.to_owned(),
            version: 1,
            resource,
            projection,
            availability,
            captured_document_hash: None,
            captured_document: None,
            diagnostics: vec![format!("captured_resource_{availability:?}").to_lowercase()],
            cache_key,
        })
    }
}

pub(crate) fn exact_cache_key<T: Serialize>(
    schema: &str,
    version: u32,
    identity: &T,
) -> Result<String> {
    sha256_json_prefixed(&serde_json::json!({
        "schema": schema,
        "version": version,
        "identity": identity,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::RevisionId;

    fn reference(byte: char) -> RevisionRefV1 {
        RevisionRefV1::new(
            RevisionId::new("rev:sha256:resource"),
            format!("sha256:{}", byte.to_string().repeat(64)),
        )
        .unwrap()
    }

    fn projection() -> RevisionResourceProjectionV1 {
        RevisionResourceProjectionV1 {
            track_id: None,
            include_body: true,
        }
    }

    #[test]
    fn hash_mismatch_is_typed_and_never_returns_captured_bytes() {
        let resource = RevisionResourceRefV1 {
            revision: reference('a'),
            object_id: ObjectId::new("obj:sha256:resource"),
        };
        let document = RevisionResourceDocumentV1::available(
            resource,
            projection(),
            &format!("sha256:{}", "b".repeat(64)),
            serde_json::json!({"rows": ["must-not-leak"]}),
        )
        .unwrap();
        assert_eq!(
            document.availability,
            RevisionResourceAvailabilityV1::Mismatch
        );
        assert!(document.captured_document.is_none());
    }

    #[test]
    fn every_unavailable_state_is_typed_and_bodyless() {
        for availability in [
            RevisionResourceAvailabilityV1::Removed,
            RevisionResourceAvailabilityV1::Missing,
            RevisionResourceAvailabilityV1::Mismatch,
            RevisionResourceAvailabilityV1::NonTextual,
        ] {
            let document = RevisionResourceDocumentV1::unavailable(
                RevisionResourceRefV1 {
                    revision: reference('a'),
                    object_id: ObjectId::new("obj:sha256:resource"),
                },
                projection(),
                availability,
            )
            .unwrap();
            assert_eq!(document.availability, availability);
            assert!(document.captured_document.is_none());
        }
    }

    #[test]
    fn cache_key_binds_projection_availability_and_exact_document_bytes() {
        let resource = RevisionResourceRefV1 {
            revision: reference('a'),
            object_id: ObjectId::new("obj:sha256:resource"),
        };
        let available = RevisionResourceDocumentV1::available(
            resource.clone(),
            projection(),
            &resource.revision.object_artifact_content_hash,
            serde_json::json!({"rows": ["one"]}),
        )
        .unwrap();
        let mut filtered = projection();
        filtered.track_id = Some(crate::model::TrackId::new("agent:author"));
        let filtered = RevisionResourceDocumentV1::available(
            resource.clone(),
            filtered,
            &resource.revision.object_artifact_content_hash,
            serde_json::json!({"rows": ["one"]}),
        )
        .unwrap();
        let changed_body = RevisionResourceDocumentV1::available(
            resource.clone(),
            projection(),
            &resource.revision.object_artifact_content_hash,
            serde_json::json!({"rows": ["two"]}),
        )
        .unwrap();
        let missing = RevisionResourceDocumentV1::unavailable(
            resource,
            projection(),
            ContentAvailabilityV1::Missing,
        )
        .unwrap();

        assert_ne!(available.cache_key, filtered.cache_key);
        assert_ne!(available.cache_key, changed_body.cache_key);
        assert_ne!(available.cache_key, missing.cache_key);
    }
}
