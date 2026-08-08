//! Exact captured-Revision resource documents.
//!
//! A captured resource is authoritative only for one [`RevisionRefV1`]. Git
//! comparisons and Revision interdiffs have separate identities and must never
//! be substituted when these bytes are absent or fail their bound hash.

use serde::{Deserialize, Serialize};

use super::inspect::{ReviewSnapshotDocument, review_snapshot_document_from_snapshot};
use crate::canonical_hash::{sha256_json_hex, sha256_json_prefixed};
use crate::error::Result;
use crate::model::{DiffSnapshot, ObjectId, RevisionRefV1, TrackId};
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
    /// Inspector generation identity. Cold CLI documents omit this additive
    /// field; the Inspector binds it to the same Change facade generation as
    /// the contextual selection before any captured bytes are painted.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub projection_stamp: Option<String>,
    pub resource: RevisionResourceRefV1,
    pub projection: RevisionResourceProjectionV1,
    pub availability: ContentAvailabilityV1,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub captured_document_hash: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub captured_document: Option<ReviewSnapshotDocument>,
    pub diagnostics: Vec<String>,
    pub cache_key: String,
}

impl RevisionResourceDocumentV1 {
    /// Wrap an already-projected captured document after re-establishing its
    /// exact object-artifact binding. This function never rebuilds captured
    /// rows from Git. Invalid supplied material degrades to a typed, bodyless
    /// mismatch rather than crossing the exact-resource boundary.
    pub fn available(
        resource: RevisionResourceRefV1,
        projection: RevisionResourceProjectionV1,
        snapshot: &DiffSnapshot,
    ) -> Result<Self> {
        let captured_document = review_snapshot_document_from_snapshot(
            resource.revision.object_artifact_content_hash.clone(),
            snapshot,
        );
        if captured_document
            .validate_binding(
                &resource.revision.object_artifact_content_hash,
                &resource.object_id,
            )
            .is_err()
        {
            return Self::unavailable(resource, projection, ContentAvailabilityV1::Mismatch);
        }
        let availability = ContentAvailabilityV1::Available;
        let captured_document_hash = Some(captured_document_hash(&captured_document)?);
        let cache_key = revision_resource_cache_key(
            &resource,
            &projection,
            availability,
            captured_document_hash.as_deref(),
        )?;
        let document = Self {
            schema: REVISION_RESOURCE_SCHEMA.to_owned(),
            version: 1,
            projection_stamp: None,
            resource,
            projection,
            availability,
            captured_document_hash,
            captured_document: Some(captured_document),
            diagnostics: Vec::new(),
            cache_key,
        };
        document.validate_integrity()?;
        Ok(document)
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
        let cache_key = revision_resource_cache_key(&resource, &projection, availability, None)?;
        let document = Self {
            schema: REVISION_RESOURCE_SCHEMA.to_owned(),
            version: 1,
            projection_stamp: None,
            resource,
            projection,
            availability,
            captured_document_hash: None,
            captured_document: None,
            diagnostics: vec![format!("captured_resource_{availability:?}").to_lowercase()],
            cache_key,
        };
        document.validate_integrity()?;
        Ok(document)
    }

    /// Revalidate a transported or deserialized exact-resource document before
    /// a Change facade is allowed to stamp it into contextual presentation.
    /// Availability, body/hash presence, immutable snapshot identity, and the
    /// self-derived cache key must all agree.
    pub fn validate_integrity(&self) -> Result<()> {
        if self.schema != REVISION_RESOURCE_SCHEMA || self.version != 1 {
            return Err(crate::error::ShoreError::Message(
                "captured resource document schema or version is unsupported".to_owned(),
            ));
        }
        match self.availability {
            ContentAvailabilityV1::Available => {
                let captured_document = self.captured_document.as_ref().ok_or_else(|| {
                    crate::error::ShoreError::Message(
                        "available captured resource is missing its document".to_owned(),
                    )
                })?;
                let expected_document_hash =
                    self.captured_document_hash.as_deref().ok_or_else(|| {
                        crate::error::ShoreError::Message(
                            "available captured resource is missing its document hash".to_owned(),
                        )
                    })?;
                captured_document.validate_binding(
                    &self.resource.revision.object_artifact_content_hash,
                    &self.resource.object_id,
                )?;
                if captured_document_hash(captured_document)? != expected_document_hash {
                    return Err(crate::error::ShoreError::Message(
                        "captured resource document hash does not match its body".to_owned(),
                    ));
                }
            }
            _ => {
                if self.captured_document.is_some() || self.captured_document_hash.is_some() {
                    return Err(crate::error::ShoreError::Message(
                        "unavailable captured resources must remain bodyless".to_owned(),
                    ));
                }
            }
        }
        let expected_diagnostics = if self.availability == ContentAvailabilityV1::Available {
            Vec::new()
        } else {
            vec![format!("captured_resource_{:?}", self.availability).to_lowercase()]
        };
        if self.diagnostics != expected_diagnostics {
            return Err(crate::error::ShoreError::Message(
                "captured resource diagnostics do not match availability".to_owned(),
            ));
        }
        let expected_cache_key = revision_resource_cache_key(
            &self.resource,
            &self.projection,
            self.availability,
            self.captured_document_hash.as_deref(),
        )?;
        if self.cache_key != expected_cache_key {
            return Err(crate::error::ShoreError::Message(
                "captured resource cache identity does not match its exact document".to_owned(),
            ));
        }
        Ok(())
    }

    /// Bind an Inspector response to the exact Change-reader generation that
    /// authorized this resource. The stamp is transport coherence, not part of
    /// the immutable captured-resource cache identity.
    pub fn with_projection_stamp(mut self, projection_stamp: String) -> Self {
        self.projection_stamp = Some(projection_stamp);
        self
    }
}

fn captured_document_hash(document: &ReviewSnapshotDocument) -> Result<String> {
    Ok(format!("sha256:{}", sha256_json_hex(document)?))
}

fn revision_resource_cache_key(
    resource: &RevisionResourceRefV1,
    projection: &RevisionResourceProjectionV1,
    availability: ContentAvailabilityV1,
    captured_document_hash: Option<&str>,
) -> Result<String> {
    let mut identity = serde_json::json!({
        "resource": resource,
        "projection": projection,
        "availability": availability,
    });
    if let Some(captured_document_hash) = captured_document_hash {
        identity["capturedDocumentHash"] =
            serde_json::Value::String(captured_document_hash.to_owned());
    }
    exact_cache_key(REVISION_RESOURCE_SCHEMA, 1, &identity)
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
    use crate::model::{DiffSnapshot, ReviewId, RevisionId};
    use crate::session::{ObjectArtifact, build_object_artifact_v2};

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

    fn artifact(suffix: &str) -> ObjectArtifact {
        build_object_artifact_v2(DiffSnapshot::new(
            ReviewId::new(format!("review:{suffix}")),
            ObjectId::new(format!("obj:sha256:{suffix}")),
            Vec::new(),
        ))
        .unwrap()
    }

    fn resource(artifact: &ObjectArtifact) -> RevisionResourceRefV1 {
        RevisionResourceRefV1 {
            revision: RevisionRefV1::new(
                RevisionId::new(format!(
                    "rev:sha256:{}",
                    artifact.snapshot.review_id.as_str()
                )),
                artifact.content_hash.clone(),
            )
            .unwrap(),
            object_id: artifact.snapshot.object_id.clone(),
        }
    }

    #[test]
    fn bound_identity_mismatch_is_typed_and_never_returns_captured_bytes() {
        let artifact = artifact("actual");
        let mut resource = resource(&artifact);
        resource.object_id = ObjectId::new("obj:sha256:different");
        let document =
            RevisionResourceDocumentV1::available(resource, projection(), &artifact.snapshot)
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
        let first_artifact = artifact("one");
        let first_resource = resource(&first_artifact);
        let available = RevisionResourceDocumentV1::available(
            first_resource.clone(),
            projection(),
            &first_artifact.snapshot,
        )
        .unwrap();
        let mut filtered = projection();
        filtered.track_id = Some(crate::model::TrackId::new("agent:author"));
        let filtered = RevisionResourceDocumentV1::available(
            first_resource.clone(),
            filtered,
            &first_artifact.snapshot,
        )
        .unwrap();
        let other_artifact = artifact("two");
        let other_resource = resource(&other_artifact);
        let changed_body = RevisionResourceDocumentV1::available(
            other_resource,
            projection(),
            &other_artifact.snapshot,
        )
        .unwrap();
        let missing = RevisionResourceDocumentV1::unavailable(
            first_resource,
            projection(),
            ContentAvailabilityV1::Missing,
        )
        .unwrap();

        assert_ne!(available.cache_key, filtered.cache_key);
        assert_ne!(available.cache_key, changed_body.cache_key);
        assert_ne!(available.cache_key, missing.cache_key);
    }

    #[test]
    fn deserialized_captured_body_cannot_bypass_exact_integrity() {
        let artifact = artifact("transport");
        let document = RevisionResourceDocumentV1::available(
            resource(&artifact),
            projection(),
            &artifact.snapshot,
        )
        .unwrap();
        let mut value = serde_json::to_value(document).unwrap();
        value["capturedDocument"]["snapshot"]["review_id"] =
            serde_json::Value::String("review:forged".to_owned());
        let forged: RevisionResourceDocumentV1 = serde_json::from_value(value).unwrap();

        assert!(forged.validate_integrity().is_err());
    }

    #[test]
    fn inspector_projection_stamp_is_additive_and_not_resource_identity() {
        let resource = RevisionResourceRefV1 {
            revision: reference('a'),
            object_id: ObjectId::new("obj:sha256:resource"),
        };
        let document = RevisionResourceDocumentV1::unavailable(
            resource,
            projection(),
            ContentAvailabilityV1::Missing,
        )
        .unwrap();
        let cache_key = document.cache_key.clone();
        let stamped = document.with_projection_stamp("sha256:generation".to_owned());

        assert_eq!(
            stamped.projection_stamp.as_deref(),
            Some("sha256:generation")
        );
        assert_eq!(stamped.cache_key, cache_key);
        assert_eq!(
            serde_json::to_value(stamped).unwrap()["projectionStamp"],
            "sha256:generation"
        );
    }
}
