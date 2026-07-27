//! Minimal freshness and new-event-count state.
#![cfg_attr(not(test), allow(dead_code))]

use crate::session::derived_access::cursor::TruthCursor;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum DerivedAccessFreshness {
    Current {
        as_of: TruthCursor,
    },
    CatchUpRequired {
        applied: TruthCursor,
        observed: TruthCursor,
    },
    EpochMismatch {
        applied: TruthCursor,
        observed: TruthCursor,
    },
}

impl DerivedAccessFreshness {
    pub(crate) fn between(
        applied: TruthCursor,
        observed: TruthCursor,
    ) -> Result<Self, FreshnessModelError> {
        if applied.epoch != observed.epoch {
            return Ok(Self::EpochMismatch { applied, observed });
        }
        if applied.sequence > observed.sequence {
            return Err(FreshnessModelError::AppliedAhead { applied, observed });
        }
        if applied == observed {
            Ok(Self::Current { as_of: observed })
        } else {
            Ok(Self::CatchUpRequired { applied, observed })
        }
    }

    pub(crate) fn new_event_count(self) -> Option<u64> {
        match self {
            Self::Current { .. } => Some(0),
            Self::CatchUpRequired { applied, observed } => {
                Some(observed.sequence - applied.sequence)
            }
            Self::EpochMismatch { .. } => None,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub(crate) enum FreshnessModelError {
    #[error("derived cursor {applied:?} is ahead of observed truth {observed:?}")]
    AppliedAhead {
        applied: TruthCursor,
        observed: TruthCursor,
    },
}
