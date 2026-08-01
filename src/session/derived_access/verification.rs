//! Product-safe strict replay verification used before generation publication.
#![cfg_attr(not(test), allow(dead_code))]

use super::cursor::TruthCursor;
use super::semantic::{SemanticModelError, SemanticSnapshot};
use crate::canonical_hash::sha256_bytes_hex;
use crate::session::event::ShoreEvent;

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

fn replay_key_for(logical_reread_key: &str) -> String {
    sha256_bytes_hex(logical_reread_key.as_bytes())
}
