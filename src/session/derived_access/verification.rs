//! Product-safe strict replay verification used before generation publication.
#![cfg_attr(not(test), allow(dead_code))]

use std::collections::BTreeMap;

use super::cursor::TruthCursor;
use super::semantic::{SemanticModelError, SemanticSnapshot};
use crate::canonical_hash::sha256_bytes_hex;
use crate::error::{Result as ShoreResult, ShoreError};
use crate::model::RevisionRefV1;
use crate::session::event::{EventType, ShoreEvent, WorkObjectProposal, WorkObjectProposedPayload};

pub(crate) fn strict_bodyless_materialized_snapshot_at(
    as_of: TruthCursor,
    mut events: Vec<ShoreEvent>,
) -> Result<SemanticSnapshot, SemanticModelError> {
    events.sort_by(|left, right| {
        replay_key_for(&left.idempotency_key)
            .cmp(&replay_key_for(&right.idempotency_key))
            .then_with(|| left.idempotency_key.cmp(&right.idempotency_key))
    });
    SemanticSnapshot::materialized_oracle_from_events(as_of, &events)
}

/// Strictly derive the first truth sequence where each exact Revision's
/// authoritative proposal summary disagrees with its earliest carrier.
pub(crate) fn strict_proposal_summary_conflicts(
    events: &[ShoreEvent],
) -> ShoreResult<BTreeMap<RevisionRefV1, u64>> {
    let mut events = events.iter().collect::<Vec<_>>();
    events.sort_by(|left, right| {
        replay_key_for(&left.idempotency_key)
            .cmp(&replay_key_for(&right.idempotency_key))
            .then_with(|| left.idempotency_key.cmp(&right.idempotency_key))
    });
    let mut baselines = BTreeMap::<RevisionRefV1, Option<String>>::new();
    let mut conflicts = BTreeMap::new();
    for (index, event) in events.into_iter().enumerate() {
        if event.event_type != EventType::WorkObjectProposed {
            continue;
        }
        let payload: WorkObjectProposedPayload = serde_json::from_value(event.payload.clone())?;
        let WorkObjectProposal::Revision {
            revision,
            summary,
            object_artifact_content_hash,
            ..
        } = payload.work_object
        else {
            continue;
        };
        let Ok(exact) = RevisionRefV1::new(revision.id, object_artifact_content_hash) else {
            continue;
        };
        match baselines.get(&exact) {
            None => {
                baselines.insert(exact, summary);
            }
            Some(baseline) if baseline != &summary => {
                let sequence = u64::try_from(index + 1)
                    .map_err(|_| ShoreError::Message("proposal sequence overflow".to_owned()))?;
                conflicts.entry(exact).or_insert(sequence);
            }
            Some(_) => {}
        }
    }
    Ok(conflicts)
}

fn replay_key_for(logical_reread_key: &str) -> String {
    sha256_bytes_hex(logical_reread_key.as_bytes())
}
