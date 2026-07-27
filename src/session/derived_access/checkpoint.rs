use std::collections::BTreeMap;

use super::cursor::{CursorReceipt, EpochRebuildReceipt, TruthCursor};

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct CheckpointAuthority {
    pub(crate) store_id: String,
    pub(crate) profile_id: String,
    pub(crate) schema_version: u32,
    pub(crate) semantic_versions: BTreeMap<String, u32>,
}

impl CheckpointAuthority {
    pub(crate) fn new(
        store_id: impl Into<String>,
        profile_id: impl Into<String>,
        schema_version: u32,
        semantic_versions: BTreeMap<String, u32>,
    ) -> Self {
        Self {
            store_id: store_id.into(),
            profile_id: profile_id.into(),
            schema_version,
            semantic_versions,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct DerivedCheckpoint {
    pub(crate) authority: CheckpointAuthority,
    pub(crate) applied_cursor: TruthCursor,
    pub(crate) observed_cursor: TruthCursor,
    pub(crate) family_coverage: BTreeMap<String, TruthCursor>,
    pub(crate) rebuild_receipt: Option<EpochRebuildReceipt>,
}

impl DerivedCheckpoint {
    pub(crate) fn empty(authority: CheckpointAuthority, cursor: TruthCursor) -> Self {
        Self {
            authority,
            applied_cursor: cursor,
            observed_cursor: cursor,
            family_coverage: BTreeMap::new(),
            rebuild_receipt: None,
        }
    }

    pub(crate) fn validate_authority(
        &self,
        expected: &CheckpointAuthority,
    ) -> Result<(), CheckpointModelError> {
        if self.authority.store_id != expected.store_id {
            return Err(CheckpointModelError::WrongStore {
                expected: expected.store_id.clone(),
                observed: self.authority.store_id.clone(),
            });
        }
        if self.authority.profile_id != expected.profile_id {
            return Err(CheckpointModelError::WrongProfile {
                expected: expected.profile_id.clone(),
                observed: self.authority.profile_id.clone(),
            });
        }
        if self.authority.schema_version != expected.schema_version {
            return Err(CheckpointModelError::WrongSchema {
                expected: expected.schema_version,
                observed: self.authority.schema_version,
            });
        }
        if self.authority.semantic_versions != expected.semantic_versions {
            return Err(CheckpointModelError::WrongSemanticVersions);
        }
        Ok(())
    }

    pub(crate) fn freshness(
        &self,
        truth_head: TruthCursor,
    ) -> Result<CheckpointFreshness, CheckpointModelError> {
        if self.applied_cursor.epoch != truth_head.epoch {
            return Ok(CheckpointFreshness::EpochMismatch {
                applied: self.applied_cursor,
                truth_head,
            });
        }
        if self.applied_cursor.sequence > truth_head.sequence {
            return Err(CheckpointModelError::CursorAhead {
                applied: self.applied_cursor,
                truth_head,
            });
        }
        if self.observed_cursor.sequence < self.applied_cursor.sequence
            || self.observed_cursor.epoch != self.applied_cursor.epoch
        {
            return Err(CheckpointModelError::ObservedBehindApplied {
                observed: self.observed_cursor,
                applied: self.applied_cursor,
            });
        }
        if self.applied_cursor == truth_head {
            Ok(CheckpointFreshness::Current)
        } else {
            Ok(CheckpointFreshness::CatchUpRequired {
                from: self.applied_cursor,
                to: truth_head,
            })
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum CheckpointFreshness {
    Current,
    CatchUpRequired {
        from: TruthCursor,
        to: TruthCursor,
    },
    EpochMismatch {
        applied: TruthCursor,
        truth_head: TruthCursor,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum DerivedMutation {
    Put {
        family: String,
        key: String,
        value: String,
    },
    Delete {
        family: String,
        key: String,
    },
    Unsupported {
        family: String,
        reason: String,
    },
}

impl DerivedMutation {
    pub(crate) fn put(
        family: impl Into<String>,
        key: impl Into<String>,
        value: impl Into<String>,
    ) -> Self {
        Self::Put {
            family: family.into(),
            key: key.into(),
            value: value.into(),
        }
    }

    pub(crate) fn delete(family: impl Into<String>, key: impl Into<String>) -> Self {
        Self::Delete {
            family: family.into(),
            key: key.into(),
        }
    }

    pub(crate) fn unsupported(family: impl Into<String>, reason: impl Into<String>) -> Self {
        Self::Unsupported {
            family: family.into(),
            reason: reason.into(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub(crate) enum CheckpointModelError {
    #[error("checkpoint belongs to store {observed}, expected {expected}")]
    WrongStore { expected: String, observed: String },
    #[error("checkpoint belongs to profile {observed}, expected {expected}")]
    WrongProfile { expected: String, observed: String },
    #[error("checkpoint schema version is {observed}, expected {expected}")]
    WrongSchema { expected: u32, observed: u32 },
    #[error("checkpoint semantic versions do not match")]
    WrongSemanticVersions,
    #[error("checkpoint cursor {applied:?} is ahead of truth head {truth_head:?}")]
    CursorAhead {
        applied: TruthCursor,
        truth_head: TruthCursor,
    },
    #[error("observed cursor {observed:?} is behind applied cursor {applied:?}")]
    ObservedBehindApplied {
        observed: TruthCursor,
        applied: TruthCursor,
    },
    #[error("wrong transition epoch: applied {applied:?}, observed {observed:?}")]
    WrongEpoch {
        applied: TruthCursor,
        observed: TruthCursor,
    },
    #[error("derived transition has a cursor gap: expected {expected}, observed {observed}")]
    CursorGap { expected: u64, observed: u64 },
    #[error("derived transition ends at {actual:?}, expected {expected:?}")]
    DeltaDoesNotReachHead {
        expected: TruthCursor,
        actual: TruthCursor,
    },
    #[error("derived family {family} has no bounded correct transition: {reason}")]
    UnsupportedFamily { family: String, reason: String },
    #[error("derived transaction interrupted before commit: {0}")]
    Interrupted(String),
    #[error("family coverage {coverage:?} is ahead of applied cursor {applied:?}")]
    CoverageAhead {
        coverage: TruthCursor,
        applied: TruthCursor,
    },
    #[error("derived checkpoint is quarantined: {0}")]
    Quarantined(String),
}

#[derive(Clone, Debug)]
pub(crate) struct ReferenceDerivedState {
    checkpoint: DerivedCheckpoint,
    rows: BTreeMap<(String, String), String>,
    quarantine_reason: Option<String>,
}

impl ReferenceDerivedState {
    pub(crate) fn new(checkpoint: DerivedCheckpoint) -> Self {
        Self {
            checkpoint,
            rows: BTreeMap::new(),
            quarantine_reason: None,
        }
    }

    pub(crate) fn checkpoint(&self) -> &DerivedCheckpoint {
        &self.checkpoint
    }

    pub(crate) fn row(&self, family: &str, key: &str) -> Option<&str> {
        self.rows
            .get(&(family.to_owned(), key.to_owned()))
            .map(String::as_str)
    }

    pub(crate) fn quarantine_reason(&self) -> Option<&str> {
        self.quarantine_reason.as_deref()
    }

    pub(crate) fn validate_authority_or_quarantine(
        &mut self,
        expected: &CheckpointAuthority,
    ) -> Result<(), CheckpointModelError> {
        self.ensure_available()?;
        if let Err(error) = self.checkpoint.validate_authority(expected) {
            self.quarantine_reason = Some(error.to_string());
            return Err(error);
        }
        Ok(())
    }

    /// One-step convenience used by the pure model tests. Multi-receipt
    /// transitions use `apply_delta_atomic`, which proves contiguity explicitly.
    pub(crate) fn apply_atomic(
        &mut self,
        observed_head: TruthCursor,
        mutations: &[DerivedMutation],
        interrupt_before_commit: Option<&str>,
    ) -> Result<(), CheckpointModelError> {
        self.ensure_available()?;
        let expected = self.checkpoint.applied_cursor.sequence + 1;
        if observed_head.epoch != self.checkpoint.applied_cursor.epoch {
            return Err(CheckpointModelError::WrongEpoch {
                applied: self.checkpoint.applied_cursor,
                observed: observed_head,
            });
        }
        if observed_head.sequence != expected {
            return Err(CheckpointModelError::CursorGap {
                expected,
                observed: observed_head.sequence,
            });
        }
        self.apply_transaction(observed_head, mutations, interrupt_before_commit)
    }

    pub(crate) fn apply_delta_atomic(
        &mut self,
        observed_head: TruthCursor,
        receipts: &[CursorReceipt],
        mutations: &[DerivedMutation],
        interrupt_before_commit: Option<&str>,
    ) -> Result<(), CheckpointModelError> {
        self.ensure_available()?;
        if observed_head.epoch != self.checkpoint.applied_cursor.epoch {
            return Err(CheckpointModelError::WrongEpoch {
                applied: self.checkpoint.applied_cursor,
                observed: observed_head,
            });
        }

        let mut actual = self.checkpoint.applied_cursor;
        for (expected, receipt) in (self.checkpoint.applied_cursor.sequence + 1..).zip(receipts) {
            if receipt.cursor.epoch != observed_head.epoch {
                return Err(CheckpointModelError::WrongEpoch {
                    applied: self.checkpoint.applied_cursor,
                    observed: receipt.cursor,
                });
            }
            if receipt.cursor.sequence != expected {
                return Err(CheckpointModelError::CursorGap {
                    expected,
                    observed: receipt.cursor.sequence,
                });
            }
            actual = receipt.cursor;
        }
        if actual != observed_head {
            return Err(CheckpointModelError::DeltaDoesNotReachHead {
                expected: observed_head,
                actual,
            });
        }

        self.apply_transaction(observed_head, mutations, interrupt_before_commit)
    }

    fn apply_transaction(
        &mut self,
        observed_head: TruthCursor,
        mutations: &[DerivedMutation],
        interrupt_before_commit: Option<&str>,
    ) -> Result<(), CheckpointModelError> {
        let mut next_rows = self.rows.clone();
        let mut next_checkpoint = self.checkpoint.clone();
        next_checkpoint.observed_cursor = observed_head;

        for mutation in mutations {
            match mutation {
                DerivedMutation::Put { family, key, value } => {
                    next_rows.insert((family.clone(), key.clone()), value.clone());
                }
                DerivedMutation::Delete { family, key } => {
                    next_rows.remove(&(family.clone(), key.clone()));
                }
                DerivedMutation::Unsupported { family, reason } => {
                    return Err(CheckpointModelError::UnsupportedFamily {
                        family: family.clone(),
                        reason: reason.clone(),
                    });
                }
            }
        }
        next_checkpoint.applied_cursor = observed_head;

        if let Some(reason) = interrupt_before_commit {
            return Err(CheckpointModelError::Interrupted(reason.to_owned()));
        }

        self.rows = next_rows;
        self.checkpoint = next_checkpoint;
        Ok(())
    }

    pub(crate) fn mark_family_covered(
        &mut self,
        family: impl Into<String>,
        coverage: TruthCursor,
    ) -> Result<(), CheckpointModelError> {
        self.ensure_available()?;
        if coverage.epoch != self.checkpoint.applied_cursor.epoch
            || coverage.sequence > self.checkpoint.applied_cursor.sequence
        {
            return Err(CheckpointModelError::CoverageAhead {
                coverage,
                applied: self.checkpoint.applied_cursor,
            });
        }
        self.checkpoint
            .family_coverage
            .insert(family.into(), coverage);
        Ok(())
    }

    pub(crate) fn absence_is_authoritative(
        &self,
        family: &str,
        key: &str,
        observed_head: TruthCursor,
    ) -> bool {
        self.quarantine_reason.is_none()
            && self.row(family, key).is_none()
            && self.checkpoint.applied_cursor == observed_head
            && self.checkpoint.observed_cursor == observed_head
            && self.checkpoint.family_coverage.get(family) == Some(&observed_head)
    }

    pub(crate) fn install_rebuild_receipt(&mut self, receipt: EpochRebuildReceipt) {
        self.checkpoint.rebuild_receipt = Some(receipt);
    }

    fn ensure_available(&self) -> Result<(), CheckpointModelError> {
        if let Some(reason) = &self.quarantine_reason {
            return Err(CheckpointModelError::Quarantined(reason.clone()));
        }
        Ok(())
    }
}
