//! Capability-bound, bodyless Change semantic generation.
#![cfg_attr(not(test), allow(dead_code))]

use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::canonical_hash::sha256_json_prefixed;
use crate::error::{Result, ShoreError};
use crate::session::derived_access::cursor::TruthCursor;
use crate::session::derived_access::generation::{
    GenerationDescriptor, GenerationLayout, GenerationPublication,
};
use crate::session::derived_access::product_contract::DerivedAccessProfile;
use crate::session::derived_access::semantic::{SemanticFact, SemanticSnapshot};
use crate::session::event::ShoreEvent;
use crate::session::projection::change::{
    ChangeProjectionFact, extract_change_projection_fact, project_changes_from_facts,
};
use crate::session::store::EventStore;
use crate::session::store::backend::JournalChangeStamp;
use crate::session::store::capabilities::{
    JournalInspection, REVIEW_CHANGE_REVISION_COHORT_V1, StoreCapabilityStatus,
    reader_profile_versions_v1,
};
use crate::session::{AuthorityCursorV2, ChangeDocumentProjectionV1, ChangeProjection};

pub(crate) const CHANGE_SEMANTIC_GENERATION_SCHEMA_V2: &str =
    "pointbreak.derived-change-semantic-generation.v2";
pub(crate) const CHANGE_READER_PROFILE_RECEIPT_SCHEMA_V2: &str =
    "pointbreak.derived-change-reader-profile-receipt.v2";
const CHANGE_SEMANTIC_RESOURCE: &str = "change-semantic.json";

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ChangeFactAvailabilityV1 {
    pub(crate) family: String,
    pub(crate) count: u64,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ChangeResourceAvailabilityV1 {
    pub(crate) content_hash: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ReaderDocumentVersionV1 {
    pub(crate) schema: String,
    pub(crate) version: u32,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ChangeReaderProfileReceiptV2 {
    pub(crate) schema: String,
    pub(crate) version: u32,
    pub(crate) minimum_reader_profile: String,
    pub(crate) authority_cursor: AuthorityCursorV2,
    pub(crate) document_versions: Vec<ReaderDocumentVersionV1>,
    pub(crate) fact_availability: Vec<ChangeFactAvailabilityV1>,
    pub(crate) resource_availability: Vec<ChangeResourceAvailabilityV1>,
    pub(crate) projection_sha256: String,
    pub(crate) document_projection_sha256: String,
    pub(crate) semantic_receipt: String,
    pub(crate) receipt_sha256: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
/// V2 binds the claim-provenance document projection in addition to the
/// effective semantic projection. V1 readers therefore fail closed instead of
/// serving Change documents that would require reconstructing actor support.
pub(crate) struct ChangeSemanticGenerationV2 {
    pub(crate) schema: String,
    pub(crate) version: u32,
    pub(crate) facts: Vec<ChangeProjectionFact>,
    pub(crate) projection: ChangeProjection,
    pub(crate) document_projection: ChangeDocumentProjectionV1,
    pub(crate) reader_profile: ChangeReaderProfileReceiptV2,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ReaderReceiptPreimage<'a> {
    schema: &'a str,
    version: u32,
    minimum_reader_profile: &'a str,
    authority_cursor: &'a AuthorityCursorV2,
    document_versions: &'a [ReaderDocumentVersionV1],
    fact_availability: &'a [ChangeFactAvailabilityV1],
    resource_availability: &'a [ChangeResourceAvailabilityV1],
    projection_sha256: &'a str,
    document_projection_sha256: &'a str,
    semantic_receipt: &'a str,
}

pub(crate) fn build_change_semantic_generation(
    inspection: &JournalInspection,
) -> Result<ChangeSemanticGenerationV2> {
    match inspection.status {
        StoreCapabilityStatus::MigrationRequired => {
            return Err(ShoreError::Message(
                "migration_required; Change semantic generation requires completed adoption"
                    .to_owned(),
            ));
        }
        StoreCapabilityStatus::MigrationInProgress { .. } => {
            return Err(ShoreError::Message(
                "migration_in_progress; refusing a partial Change semantic generation".to_owned(),
            ));
        }
        StoreCapabilityStatus::Ready { .. } => {}
    }
    if inspection.minimum_reader_profile.as_deref() != Some(REVIEW_CHANGE_REVISION_COHORT_V1) {
        return Err(ShoreError::Message(
            "completed Change store has a mismatched minimum reader profile".to_owned(),
        ));
    }

    let mut events = Vec::with_capacity(inspection.event_entries.len());
    for entry in &inspection.event_entries {
        events.push(EventStore::decode_qualification_entry(
            entry.key_digest.clone(),
            entry.bytes.clone(),
        )?);
    }
    let facts = events
        .iter()
        .map(extract_change_projection_fact)
        .collect::<Result<Vec<_>>>()?
        .into_iter()
        .flatten()
        .collect::<Vec<_>>();
    let projection = crate::session::project_changes(&events)?;
    let document_projection = crate::session::project_change_documents(&events)?;
    if project_changes_from_facts(&facts)? != projection {
        return Err(ShoreError::Message(
            "bodyless Change facts diverge from strict replay".to_owned(),
        ));
    }

    // The existing semantic receipt remains the cross-family strict-replay
    // witness. The Change reader receipt additionally binds the capability
    // cursor and the frozen public document versions.
    let cursor = TruthCursor::new(1, inspection.cursor.event_count.max(1));
    let semantic_snapshot = SemanticSnapshot::from_events(cursor, &events)
        .map_err(|error| ShoreError::Message(error.to_string()))?;
    let rebuilt_facts = events
        .iter()
        .enumerate()
        .map(|(index, event)| {
            SemanticFact::from_event(
                TruthCursor::new(
                    1,
                    u64::try_from(index.saturating_add(1)).unwrap_or(u64::MAX),
                ),
                event,
                crate::canonical_hash::sha256_bytes_hex(&inspection.event_entries[index].bytes),
            )
        })
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(|error| ShoreError::Message(error.to_string()))?;
    if SemanticSnapshot::audit_from_facts(cursor, &rebuilt_facts)
        .map_err(|error| ShoreError::Message(error.to_string()))?
        .changes
        != projection
    {
        return Err(ShoreError::Message(
            "rebuilt derived Change facts diverge from strict replay".to_owned(),
        ));
    }

    let fact_availability = fact_availability(&events);
    let resource_availability = resource_availability(&events)?;
    let document_versions = reader_profile_versions_v1()
        .iter()
        .map(|reservation| ReaderDocumentVersionV1 {
            schema: reservation.schema.to_owned(),
            version: reservation.version,
        })
        .collect();
    let projection_sha256 = sha256_json_prefixed(&serde_json::to_value(&projection)?)?;
    let mut reader_profile = ChangeReaderProfileReceiptV2 {
        schema: CHANGE_READER_PROFILE_RECEIPT_SCHEMA_V2.to_owned(),
        version: 2,
        minimum_reader_profile: REVIEW_CHANGE_REVISION_COHORT_V1.to_owned(),
        authority_cursor: inspection.cursor.clone(),
        document_versions,
        fact_availability,
        resource_availability,
        projection_sha256,
        document_projection_sha256: sha256_json_prefixed(&serde_json::to_value(
            &document_projection,
        )?)?,
        semantic_receipt: semantic_snapshot.semantic_receipt,
        receipt_sha256: String::new(),
    };
    reader_profile.receipt_sha256 = reader_receipt_sha256(&reader_profile)?;

    Ok(ChangeSemanticGenerationV2 {
        schema: CHANGE_SEMANTIC_GENERATION_SCHEMA_V2.to_owned(),
        version: 2,
        facts,
        projection,
        document_projection,
        reader_profile,
    })
}

impl ChangeSemanticGenerationV2 {
    pub(crate) fn validate(&self) -> Result<()> {
        if self.schema != CHANGE_SEMANTIC_GENERATION_SCHEMA_V2 || self.version != 2 {
            return Err(ShoreError::Message(
                "incompatible Change semantic generation schema".to_owned(),
            ));
        }
        if project_changes_from_facts(&self.facts)? != self.projection {
            return Err(ShoreError::Message(
                "Change semantic generation projection mismatch".to_owned(),
            ));
        }
        if self.reader_profile.minimum_reader_profile != REVIEW_CHANGE_REVISION_COHORT_V1
            || self.reader_profile.schema != CHANGE_READER_PROFILE_RECEIPT_SCHEMA_V2
            || self.reader_profile.version != 2
            || self.reader_profile.document_versions
                != reader_profile_versions_v1()
                    .iter()
                    .map(|reservation| ReaderDocumentVersionV1 {
                        schema: reservation.schema.to_owned(),
                        version: reservation.version,
                    })
                    .collect::<Vec<_>>()
            || self.reader_profile.projection_sha256
                != sha256_json_prefixed(&serde_json::to_value(&self.projection)?)?
            || self.reader_profile.document_projection_sha256
                != sha256_json_prefixed(&serde_json::to_value(&self.document_projection)?)?
            || self.document_projection.projection_stamp
                != crate::session::change_document_projection_stamp(
                    &self.projection,
                    &self.document_projection,
                )?
            || self.reader_profile.receipt_sha256 != reader_receipt_sha256(&self.reader_profile)?
            || self
                .reader_profile
                .fact_availability
                .windows(2)
                .any(|pair| pair[0].family >= pair[1].family)
            || self
                .reader_profile
                .resource_availability
                .windows(2)
                .any(|pair| pair[0].content_hash >= pair[1].content_hash)
        {
            return Err(ShoreError::Message(
                "Change reader-profile receipt mismatch".to_owned(),
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ChangeGenerationFailurePointV1 {
    None,
    AfterStaging,
    AfterPromotion,
}

#[cfg(any(test, feature = "bench"))]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ChangeSemanticRouteV1 {
    Current,
    LooseFallback,
    ExplicitOff,
}

#[cfg(any(test, feature = "bench"))]
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ChangeSemanticReadV1 {
    pub(crate) route: ChangeSemanticRouteV1,
    pub(crate) projection: ChangeProjection,
    pub(crate) document_projection: ChangeDocumentProjectionV1,
}

fn publish_change_semantic_generation_with_failure(
    store_root: &Path,
    inspection: &JournalInspection,
    authority_now: &AuthorityCursorV2,
    enabled: bool,
    failure: ChangeGenerationFailurePointV1,
) -> Result<String> {
    if !enabled {
        return Err(ShoreError::Message(
            "derived Change semantics are explicitly off".to_owned(),
        ));
    }
    let generation = build_change_semantic_generation(inspection)?;
    generation.validate()?;
    let layout = GenerationLayout::new(store_root).map_err(generation_error)?;
    let _lease = layout.try_rebuild_lease().map_err(generation_error)?;
    layout.ensure_scaffold().map_err(generation_error)?;
    layout.discard_all_staging().map_err(generation_error)?;
    let (sequence, generation_id) = layout.next_generation().map_err(generation_error)?;
    let staging = layout.staging(&generation_id);
    std::fs::create_dir_all(&staging).map_err(|error| {
        ShoreError::Message(format!("create Change generation staging: {error}"))
    })?;
    let bytes = crate::canonical_hash::canonical_json_bytes(&serde_json::to_value(&generation)?)?;
    layout
        .write_resource(&staging, CHANGE_SEMANTIC_RESOURCE, &bytes)
        .map_err(generation_error)?;
    if failure == ChangeGenerationFailurePointV1::AfterStaging {
        return Err(ShoreError::Message(
            "interrupted after Change generation staging".to_owned(),
        ));
    }
    if inspection.cursor != *authority_now {
        layout
            .discard_staging(&generation_id)
            .map_err(generation_error)?;
        return Err(ShoreError::Message(
            "Change authority moved while the derived generation was staging".to_owned(),
        ));
    }
    let store_id = crate::session::store::resolution::opaque_path_identity("store", store_root)?;
    let descriptor = GenerationDescriptor::new(
        &generation_id,
        store_id,
        DerivedAccessProfile::SqliteWalBodylessV1,
        sequence,
        inspection.cursor.event_count,
        JournalChangeStamp::Absent,
        &generation.reader_profile.receipt_sha256,
    );
    let descriptor_sha256 = layout
        .write_descriptor(&staging, &descriptor)
        .map_err(generation_error)?;
    layout
        .promote_staging(&generation_id)
        .map_err(generation_error)?;
    if failure == ChangeGenerationFailurePointV1::AfterPromotion {
        return Err(ShoreError::Message(
            "interrupted after Change generation promotion".to_owned(),
        ));
    }
    layout
        .publish(&GenerationPublication::new(
            sequence,
            &generation_id,
            descriptor_sha256,
        ))
        .map_err(generation_error)?;
    layout
        .retire_prior_publications(sequence)
        .map_err(generation_error)?;
    let _ = layout.reclaim_inactive_generations(&generation_id);
    Ok(generation_id)
}

#[cfg(any(test, feature = "bench"))]
pub(crate) fn read_change_semantics_for_qualification(
    store_root: &Path,
    inspection: &JournalInspection,
    enabled: bool,
) -> Result<ChangeSemanticReadV1> {
    // Capability routing wins before any sidecar read. An old generation can
    // never make L0 or M1 appear semantically usable.
    match inspection.status {
        StoreCapabilityStatus::MigrationRequired => {
            return Err(ShoreError::Message("migration_required".to_owned()));
        }
        StoreCapabilityStatus::MigrationInProgress { .. } => {
            return Err(ShoreError::Message("migration_in_progress".to_owned()));
        }
        StoreCapabilityStatus::Ready { .. } => {}
    }
    let (strict, strict_documents) = strict_projections(inspection)?;
    if !enabled {
        return Ok(ChangeSemanticReadV1 {
            route: ChangeSemanticRouteV1::ExplicitOff,
            projection: strict,
            document_projection: strict_documents,
        });
    }
    let derived = (|| {
        let layout = GenerationLayout::new(store_root).map_err(generation_error)?;
        let publication = layout
            .current_publication()
            .map_err(generation_error)?
            .ok_or_else(|| {
                ShoreError::Message("Change semantic generation is absent".to_owned())
            })?;
        let descriptor = layout.descriptor(&publication).map_err(generation_error)?;
        let path = layout
            .generation(&publication.generation_id)
            .join(CHANGE_SEMANTIC_RESOURCE);
        let bytes = std::fs::read(&path).map_err(|error| {
            ShoreError::Message(format!("read Change semantic generation: {error}"))
        })?;
        let generation: ChangeSemanticGenerationV2 = serde_json::from_slice(&bytes)?;
        generation.validate()?;
        if generation.reader_profile.authority_cursor != inspection.cursor
            || descriptor.semantic_receipt != generation.reader_profile.receipt_sha256
            || generation.projection != strict
            || generation.document_projection != strict_documents
        {
            return Err(ShoreError::Message(
                "Change semantic generation is stale or divergent".to_owned(),
            ));
        }
        Ok((generation.projection, generation.document_projection))
    })();
    Ok(match derived {
        Ok((projection, document_projection)) => ChangeSemanticReadV1 {
            route: ChangeSemanticRouteV1::Current,
            projection,
            document_projection,
        },
        Err(_) => ChangeSemanticReadV1 {
            route: ChangeSemanticRouteV1::LooseFallback,
            projection: strict,
            document_projection: strict_documents,
        },
    })
}

#[cfg(any(test, feature = "bench"))]
fn strict_projections(
    inspection: &JournalInspection,
) -> Result<(ChangeProjection, ChangeDocumentProjectionV1)> {
    let events = inspection
        .event_entries
        .iter()
        .map(|entry| {
            EventStore::decode_qualification_entry(entry.key_digest.clone(), entry.bytes.clone())
        })
        .collect::<Result<Vec<_>>>()?;
    Ok((
        crate::session::project_changes(&events)?,
        crate::session::project_change_documents(&events)?,
    ))
}

fn generation_error(error: impl std::fmt::Display) -> ShoreError {
    ShoreError::Message(format!(
        "Change semantic generation lifecycle failed: {error}"
    ))
}

fn reader_receipt_sha256(receipt: &ChangeReaderProfileReceiptV2) -> Result<String> {
    sha256_json_prefixed(&serde_json::to_value(ReaderReceiptPreimage {
        schema: &receipt.schema,
        version: receipt.version,
        minimum_reader_profile: &receipt.minimum_reader_profile,
        authority_cursor: &receipt.authority_cursor,
        document_versions: &receipt.document_versions,
        fact_availability: &receipt.fact_availability,
        resource_availability: &receipt.resource_availability,
        projection_sha256: &receipt.projection_sha256,
        document_projection_sha256: &receipt.document_projection_sha256,
        semantic_receipt: &receipt.semantic_receipt,
    })?)
}

fn fact_availability(events: &[ShoreEvent]) -> Vec<ChangeFactAvailabilityV1> {
    let mut counts = BTreeMap::<String, u64>::new();
    for event in events {
        *counts
            .entry(event.event_type.as_str().to_owned())
            .or_default() += 1;
    }
    counts
        .into_iter()
        .map(|(family, count)| ChangeFactAvailabilityV1 { family, count })
        .collect()
}

fn resource_availability(events: &[ShoreEvent]) -> Result<Vec<ChangeResourceAvailabilityV1>> {
    Ok(
        crate::session::workflow::selected_support_content_hashes(events)?
            .into_iter()
            .map(|content_hash| ChangeResourceAvailabilityV1 { content_hash })
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{EngagementId, JournalId, ObjectId, RevisionId};
    use crate::session::event::{
        EventTarget, Revision, ShoreEvent, WorkObjectProposal, WorkObjectProposedPayload, Writer,
    };
    use crate::session::store::backend::StoreBackend;
    use crate::session::store::capabilities::{
        CapabilityFixtureState, inspect_journal_records, write_capability_fixture_for_test,
    };

    #[test]
    fn only_completed_capability_state_can_build_change_semantics() {
        let l0_backend = StoreBackend::memory();
        let l0 = inspect_journal_records(l0_backend.journal().as_ref()).unwrap();
        assert!(
            build_change_semantic_generation(&l0)
                .unwrap_err()
                .to_string()
                .contains("migration_required")
        );

        let m1_backend = StoreBackend::memory();
        write_capability_fixture_for_test(
            m1_backend.journal().as_ref(),
            CapabilityFixtureState::M1,
        )
        .unwrap();
        let m1 = inspect_journal_records(m1_backend.journal().as_ref()).unwrap();
        assert_eq!(m1.cursor.event_count, 0);
        assert!(
            build_change_semantic_generation(&m1)
                .unwrap_err()
                .to_string()
                .contains("migration_in_progress")
        );

        let l2_backend = StoreBackend::memory();
        write_capability_fixture_for_test(
            l2_backend.journal().as_ref(),
            CapabilityFixtureState::L2,
        )
        .unwrap();
        let l2 = inspect_journal_records(l2_backend.journal().as_ref()).unwrap();
        let generation = build_change_semantic_generation(&l2).unwrap();
        generation.validate().unwrap();
        assert_eq!(
            generation.reader_profile.document_versions,
            reader_profile_versions_v1()
                .iter()
                .map(|reservation| ReaderDocumentVersionV1 {
                    schema: reservation.schema.to_owned(),
                    version: reservation.version,
                })
                .collect::<Vec<_>>()
        );
        assert!(!generation.reader_profile.receipt_sha256.is_empty());
        assert!(
            !serde_json::to_string(&generation)
                .unwrap()
                .contains("legacy_derived")
        );
    }

    #[test]
    fn old_or_corrupt_change_generation_never_validates() {
        let (_backend, l2) = l2_inspection();
        let generation = build_change_semantic_generation(&l2).unwrap();
        generation.validate().unwrap();

        let mut old = generation.clone();
        old.schema = "pointbreak.derived-access-generation.v2".to_owned();
        assert!(
            old.validate()
                .unwrap_err()
                .to_string()
                .contains("incompatible Change semantic generation schema")
        );

        let mut unsupported = generation;
        unsupported.version = 3;
        assert!(unsupported.validate().is_err());
        assert!(serde_json::from_slice::<ChangeSemanticGenerationV2>(b"not-json").is_err());
    }

    fn l2_inspection() -> (StoreBackend, JournalInspection) {
        let backend = StoreBackend::memory();
        write_capability_fixture_for_test(backend.journal().as_ref(), CapabilityFixtureState::L2)
            .unwrap();
        let inspection = inspect_journal_records(backend.journal().as_ref()).unwrap();
        (backend, inspection)
    }

    #[test]
    fn immutable_change_generation_publishes_only_at_one_l2_authority() {
        let root = tempfile::tempdir().unwrap();
        let (_backend, l2) = l2_inspection();
        let generation_id = publish_change_semantic_generation_with_failure(
            root.path(),
            &l2,
            &l2.cursor,
            true,
            ChangeGenerationFailurePointV1::None,
        )
        .unwrap();
        let read = read_change_semantics_for_qualification(root.path(), &l2, true).unwrap();
        assert_eq!(read.route, ChangeSemanticRouteV1::Current);
        let (strict, strict_documents) = strict_projections(&l2).unwrap();
        assert_eq!(read.projection, strict);
        assert_eq!(read.document_projection, strict_documents);
        assert!(
            GenerationLayout::new(root.path())
                .unwrap()
                .generation(&generation_id)
                .join(CHANGE_SEMANTIC_RESOURCE)
                .is_file()
        );

        let mut moved = l2.cursor.clone();
        moved.journal_record_count += 1;
        moved.journal_record_set_hash = format!("sha256:{}", "f".repeat(64));
        assert!(
            publish_change_semantic_generation_with_failure(
                tempfile::tempdir().unwrap().path(),
                &l2,
                &moved,
                true,
                ChangeGenerationFailurePointV1::None,
            )
            .unwrap_err()
            .to_string()
            .contains("authority moved")
        );
    }

    #[test]
    fn interrupted_staging_and_promotion_retry_to_one_current_generation() {
        let (_backend, l2) = l2_inspection();
        for failure in [
            ChangeGenerationFailurePointV1::AfterStaging,
            ChangeGenerationFailurePointV1::AfterPromotion,
        ] {
            let root = tempfile::tempdir().unwrap();
            assert!(
                publish_change_semantic_generation_with_failure(
                    root.path(),
                    &l2,
                    &l2.cursor,
                    true,
                    failure,
                )
                .is_err()
            );
            assert!(
                GenerationLayout::new(root.path())
                    .unwrap()
                    .current_publication()
                    .unwrap()
                    .is_none()
            );
            publish_change_semantic_generation_with_failure(
                root.path(),
                &l2,
                &l2.cursor,
                true,
                ChangeGenerationFailurePointV1::None,
            )
            .unwrap();
            assert_eq!(
                read_change_semantics_for_qualification(root.path(), &l2, true)
                    .unwrap()
                    .route,
                ChangeSemanticRouteV1::Current
            );
        }
    }

    #[test]
    fn off_missing_corrupt_and_pre_l2_routes_never_serve_derived_authority() {
        let root = tempfile::tempdir().unwrap();
        let (_backend, l2) = l2_inspection();
        let (strict, strict_documents) = strict_projections(&l2).unwrap();
        let off = read_change_semantics_for_qualification(root.path(), &l2, false).unwrap();
        assert_eq!(off.route, ChangeSemanticRouteV1::ExplicitOff);
        assert_eq!(off.projection, strict);
        assert_eq!(off.document_projection, strict_documents);
        assert!(!root.path().join("derived").exists());

        let missing = read_change_semantics_for_qualification(root.path(), &l2, true).unwrap();
        assert_eq!(missing.route, ChangeSemanticRouteV1::LooseFallback);
        publish_change_semantic_generation_with_failure(
            root.path(),
            &l2,
            &l2.cursor,
            true,
            ChangeGenerationFailurePointV1::None,
        )
        .unwrap();
        let layout = GenerationLayout::new(root.path()).unwrap();
        let publication = layout.current_publication().unwrap().unwrap();
        std::fs::write(
            layout
                .generation(&publication.generation_id)
                .join(CHANGE_SEMANTIC_RESOURCE),
            b"corrupt",
        )
        .unwrap();
        let corrupt = read_change_semantics_for_qualification(root.path(), &l2, true).unwrap();
        assert_eq!(corrupt.route, ChangeSemanticRouteV1::LooseFallback);
        assert_eq!(corrupt.projection, strict);
        assert_eq!(corrupt.document_projection, strict_documents);

        let l0_backend = StoreBackend::memory();
        let l0 = inspect_journal_records(l0_backend.journal().as_ref()).unwrap();
        assert!(
            read_change_semantics_for_qualification(root.path(), &l0, true)
                .unwrap_err()
                .to_string()
                .contains("migration_required")
        );
        let m1_backend = StoreBackend::memory();
        write_capability_fixture_for_test(
            m1_backend.journal().as_ref(),
            CapabilityFixtureState::M1,
        )
        .unwrap();
        let m1 = inspect_journal_records(m1_backend.journal().as_ref()).unwrap();
        assert!(
            read_change_semantics_for_qualification(root.path(), &m1, true)
                .unwrap_err()
                .to_string()
                .contains("migration_in_progress")
        );
    }

    #[test]
    fn legacy_revision_refs_do_not_abort_off_or_loose_fallback_routes() {
        let backend = StoreBackend::memory();
        write_capability_fixture_for_test(backend.journal().as_ref(), CapabilityFixtureState::L2)
            .unwrap();
        let payload = WorkObjectProposedPayload {
            engagement_id: EngagementId::new("engagement:sha256:legacy"),
            work_object: WorkObjectProposal::Revision {
                revision: Revision {
                    id: RevisionId::new("review-unit:sha256:legacy"),
                    object_id: ObjectId::new("obj:sha256:legacy"),
                    git_provenance: None,
                },
                summary: None,
                object_artifact_content_hash: "legacy-artifact-hash".to_owned(),
                supersedes: Vec::new(),
            },
        };
        let event = ShoreEvent::new(
            crate::session::event::EventType::WorkObjectProposed,
            "revision:legacy",
            EventTarget::for_journal(JournalId::new("journal:test")),
            Writer::shore_local("test"),
            payload,
            "2026-08-05T00:00:00Z",
        )
        .unwrap();
        backend
            .journal()
            .insert_raw(&event.idempotency_key, &serde_json::to_vec(&event).unwrap())
            .unwrap();
        let inspection = inspect_journal_records(backend.journal().as_ref()).unwrap();
        let root = tempfile::tempdir().unwrap();

        let off = read_change_semantics_for_qualification(root.path(), &inspection, false).unwrap();
        assert_eq!(off.route, ChangeSemanticRouteV1::ExplicitOff);
        assert_eq!(off.document_projection.unavailable_revision_refs.len(), 1);
        let fallback =
            read_change_semantics_for_qualification(root.path(), &inspection, true).unwrap();
        assert_eq!(fallback.route, ChangeSemanticRouteV1::LooseFallback);
        assert_eq!(
            fallback.document_projection.unavailable_revision_refs.len(),
            1
        );
    }
}
