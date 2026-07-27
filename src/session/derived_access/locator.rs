//! Candidate-independent bodyless locator semantics.
#![cfg_attr(not(test), allow(dead_code))]

use crate::canonical_hash::sha256_bytes_hex;
use crate::session::derived_access::cursor::TruthCursor;
use crate::session::event::ShoreEvent;
use crate::session::{format_rfc3339_utc_millis, parse_event_instant};

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct LocatorRow {
    pub(crate) cursor: TruthCursor,
    pub(crate) logical_reread_key: String,
    pub(crate) event_id: String,
    pub(crate) normalized_occurred_at: String,
    pub(crate) replay_key: String,
    pub(crate) event_type: String,
    pub(crate) journal_id: String,
    pub(crate) subject_id: Option<String>,
    pub(crate) track_id: Option<String>,
    pub(crate) payload_hash: String,
    pub(crate) validation_witness: String,
}

impl LocatorRow {
    pub(crate) fn from_event(
        cursor: TruthCursor,
        event: &ShoreEvent,
        validation_witness: impl Into<String>,
    ) -> Result<Self, LocatorModelError> {
        let validation_witness = validation_witness.into();
        if cursor.epoch == 0 || cursor.sequence == 0 {
            return Err(LocatorModelError::InvalidCursor(cursor));
        }
        if validation_witness.len() != 64
            || !validation_witness
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit())
        {
            return Err(LocatorModelError::InvalidWitness);
        }
        Ok(Self {
            cursor,
            logical_reread_key: event.idempotency_key.clone(),
            event_id: event.event_id.as_str().to_owned(),
            normalized_occurred_at: normalize_occurred_at(&event.occurred_at)?,
            replay_key: sha256_bytes_hex(event.idempotency_key.as_bytes()),
            event_type: event.event_type.as_str().to_owned(),
            journal_id: event.target.journal_id.as_str().to_owned(),
            subject_id: event.target.subject_id.clone(),
            track_id: event
                .target
                .track_id
                .as_ref()
                .map(|track| track.as_str().to_owned()),
            payload_hash: event.payload_hash.clone(),
            validation_witness,
        })
    }

    pub(crate) fn display_key(&self) -> DisplayKey {
        DisplayKey {
            normalized_occurred_at: self.normalized_occurred_at.clone(),
            event_id: self.event_id.clone(),
        }
    }
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) struct DisplayKey {
    pub(crate) normalized_occurred_at: String,
    pub(crate) event_id: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum WindowContinuation {
    After {
        anchor: DisplayKey,
        as_of: TruthCursor,
    },
    Before {
        anchor: DisplayKey,
        as_of: TruthCursor,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum WindowPosition {
    Head,
    Continue(WindowContinuation),
    Tail,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ChronologicalWindowRequest {
    position: WindowPosition,
    limit: usize,
}

impl ChronologicalWindowRequest {
    pub(crate) fn head(limit: usize) -> Self {
        Self::new(WindowPosition::Head, limit)
    }

    pub(crate) fn after(anchor: DisplayKey, limit: usize, as_of: TruthCursor) -> Self {
        Self::continue_from(WindowContinuation::After { anchor, as_of }, limit)
    }

    pub(crate) fn before(anchor: DisplayKey, limit: usize, as_of: TruthCursor) -> Self {
        Self::continue_from(WindowContinuation::Before { anchor, as_of }, limit)
    }

    pub(crate) fn tail(limit: usize) -> Self {
        Self::new(WindowPosition::Tail, limit)
    }

    pub(crate) fn continue_from(continuation: WindowContinuation, limit: usize) -> Self {
        Self::new(WindowPosition::Continue(continuation), limit)
    }

    pub(crate) fn position(&self) -> &WindowPosition {
        &self.position
    }

    pub(crate) fn limit(&self) -> usize {
        self.limit
    }

    pub(crate) fn requested_as_of(&self) -> Option<TruthCursor> {
        match self.position {
            WindowPosition::Head | WindowPosition::Tail => None,
            WindowPosition::Continue(WindowContinuation::After { as_of, .. })
            | WindowPosition::Continue(WindowContinuation::Before { as_of, .. }) => Some(as_of),
        }
    }

    fn new(position: WindowPosition, limit: usize) -> Self {
        Self { position, limit }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct LocatorWindow {
    pub(crate) as_of: TruthCursor,
    pub(crate) rows: Vec<LocatorRow>,
    pub(crate) continuation: Option<WindowContinuation>,
    pub(crate) has_more: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct LocatorCheckpoint {
    pub(crate) applied: TruthCursor,
    pub(crate) observed: TruthCursor,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct HydratedWindow {
    pub(crate) as_of: TruthCursor,
    pub(crate) events: Vec<ShoreEvent>,
    pub(crate) continuation: Option<WindowContinuation>,
    pub(crate) has_more: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum LocatorRead<T> {
    Ready(T),
    CatchUpRequired {
        applied: TruthCursor,
        observed: TruthCursor,
    },
}

pub(crate) fn normalize_occurred_at(value: &str) -> Result<String, LocatorModelError> {
    parse_event_instant(value)
        .map(format_rfc3339_utc_millis)
        .ok_or_else(|| LocatorModelError::InvalidOccurredAt(value.to_owned()))
}

#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub(crate) enum LocatorModelError {
    #[error("locator cursor must be nonzero: {0:?}")]
    InvalidCursor(TruthCursor),
    #[error("locator validation witness must be 64 hexadecimal characters")]
    InvalidWitness,
    #[error("locator cannot normalize occurredAt value {0}")]
    InvalidOccurredAt(String),
    #[error("chronological window limit must be greater than zero")]
    ZeroWindowLimit,
    #[error("requested as_of cursor {requested:?} is ahead of observed truth {observed:?}")]
    AsOfAhead {
        requested: TruthCursor,
        observed: TruthCursor,
    },
    #[error("requested as_of cursor {requested:?} has a different epoch than {observed:?}")]
    AsOfEpochMismatch {
        requested: TruthCursor,
        observed: TruthCursor,
    },
}
