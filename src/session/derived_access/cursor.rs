#[cfg(any(test, feature = "bench"))]
use std::collections::{BTreeMap, HashMap, HashSet};

#[cfg(any(test, feature = "bench"))]
use crate::session::store::backend::QualificationJournalCursor;

/// Private append position. It is deliberately distinct from every event,
/// timestamp, replay, display, and public pagination identity.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub(crate) struct TruthCursor {
    pub(crate) epoch: u64,
    pub(crate) sequence: u64,
}

impl TruthCursor {
    pub(crate) const fn new(epoch: u64, sequence: u64) -> Self {
        Self { epoch, sequence }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct TruthHead {
    pub(crate) store_id: String,
    pub(crate) cursor: TruthCursor,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct CursorIntent {
    pub(crate) proposed_cursor: TruthCursor,
    pub(crate) logical_reread_key: String,
    pub(crate) validation_witness: String,
    pub(crate) attempt_token: String,
}

impl CursorIntent {
    pub(crate) fn new(
        proposed_cursor: TruthCursor,
        logical_reread_key: impl Into<String>,
        validation_witness: impl Into<String>,
        attempt_token: impl Into<String>,
    ) -> Self {
        Self {
            proposed_cursor,
            logical_reread_key: logical_reread_key.into(),
            validation_witness: validation_witness.into(),
            attempt_token: attempt_token.into(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct CursorReceipt {
    pub(crate) cursor: TruthCursor,
    pub(crate) logical_reread_key: String,
    pub(crate) validation_witness: String,
    pub(crate) attempt_token: String,
}

impl CursorReceipt {
    #[cfg(any(test, feature = "bench"))]
    fn from_intent(intent: CursorIntent) -> Self {
        Self {
            cursor: intent.proposed_cursor,
            logical_reread_key: intent.logical_reread_key,
            validation_witness: intent.validation_witness,
            attempt_token: intent.attempt_token,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
#[cfg(any(test, feature = "bench"))]
pub(crate) enum CarrierState {
    Unambiguous { validation_witness: String },
    Ambiguous { validation_witness: String },
}

#[cfg(any(test, feature = "bench"))]
impl CarrierState {
    pub(crate) fn unambiguous(validation_witness: impl Into<String>) -> Self {
        Self::Unambiguous {
            validation_witness: validation_witness.into(),
        }
    }

    pub(crate) fn ambiguous(validation_witness: impl Into<String>) -> Self {
        Self::Ambiguous {
            validation_witness: validation_witness.into(),
        }
    }

    fn validation_witness(&self) -> &str {
        match self {
            Self::Unambiguous { validation_witness } | Self::Ambiguous { validation_witness } => {
                validation_witness
            }
        }
    }

    fn is_unambiguous(&self) -> bool {
        matches!(self, Self::Unambiguous { .. })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum AppendResolution {
    Created(TruthCursor),
    Existing(TruthCursor),
    Conflict(TruthCursor),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum RecoveryResolution {
    NoIntent,
    LiveWriterBusy,
    RetiredAbsent,
    Existing(TruthCursor),
    Conflict(TruthCursor),
    Published(TruthCursor),
    AdvancedHead(TruthCursor),
    RetiredFinalized(TruthCursor),
    EpochInvalidated(TruthCursor),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[cfg(any(test, feature = "bench"))]
pub(crate) enum IntentRecoveryAuthority {
    WriterMayBeLive,
    ExclusiveAfterWriterExit,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct CursorDelta {
    pub(crate) after: TruthCursor,
    pub(crate) observed_head: TruthCursor,
    pub(crate) receipts: Vec<CursorReceipt>,
    pub(crate) complete: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
#[cfg(any(test, feature = "bench"))]
pub(crate) struct EpochRebuildReceipt {
    pub(crate) previous_epoch: u64,
    pub(crate) new_head: TruthCursor,
    pub(crate) full_validation_witness: String,
}

#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
#[cfg(any(test, feature = "bench"))]
pub(crate) enum CursorModelError {
    #[error("cursor ledger is quarantined: {0}")]
    Quarantined(String),
    #[error("cursor intent is already active")]
    IntentAlreadyActive,
    #[error("cursor intent is absent")]
    IntentAbsent,
    #[error("attempt token is empty")]
    EmptyAttemptToken,
    #[error("attempt token was already used: {0}")]
    DuplicateAttemptToken(String),
    #[error("logical reread key is empty")]
    EmptyLogicalRereadKey,
    #[error("validation witness is empty")]
    EmptyValidationWitness,
    #[error("wrong cursor epoch: expected {expected}, observed {observed}")]
    WrongEpoch { expected: u64, observed: u64 },
    #[error("cursor {cursor:?} is ahead of head {head:?}")]
    CursorAhead {
        cursor: TruthCursor,
        head: TruthCursor,
    },
    #[error("cursor sequence gap: expected {expected}, observed {observed}")]
    SequenceGap { expected: u64, observed: u64 },
    #[error("duplicate cursor sequence {0}")]
    DuplicateSequence(u64),
    #[error("duplicate logical reread key {0}")]
    DuplicateLogicalKey(String),
    #[error("cursor receipt witness is corrupt for {0}")]
    CorruptReceiptWitness(String),
    #[error("carrier is absent for {0}")]
    CarrierAbsent(String),
    #[error("carrier is ambiguous for {0}")]
    AmbiguousCarrier(String),
    #[error("carrier witness mismatch for {0}")]
    WitnessMismatch(String),
    #[error("delta limit must be greater than zero")]
    ZeroDeltaLimit,
    #[error("new epoch {new_epoch} must be greater than previous epoch {previous_epoch}")]
    EpochDidNotAdvance { previous_epoch: u64, new_epoch: u64 },
    #[error("epoch rebuild requires a full-validation witness")]
    MissingFullValidationWitness,
}

/// Pure reference protocol. The carrier map stands in for authoritative loose
/// truth, while intent/head/receipts stand in for private rebuildable metadata.
/// No method repairs or rewrites a carrier.
#[derive(Clone, Debug)]
#[cfg(any(test, feature = "bench"))]
pub(crate) struct ReferenceCursorLedger {
    store_id: String,
    epoch: u64,
    head_sequence: u64,
    intent: Option<CursorIntent>,
    receipts_by_sequence: BTreeMap<u64, CursorReceipt>,
    sequence_by_logical_key: HashMap<String, u64>,
    used_attempt_tokens: HashSet<String>,
    carriers: HashMap<String, CarrierState>,
    quarantine_reason: Option<String>,
}

#[cfg(any(test, feature = "bench"))]
impl ReferenceCursorLedger {
    pub(crate) fn new(store_id: impl Into<String>, epoch: u64) -> Self {
        Self {
            store_id: store_id.into(),
            epoch,
            head_sequence: 0,
            intent: None,
            receipts_by_sequence: BTreeMap::new(),
            sequence_by_logical_key: HashMap::new(),
            used_attempt_tokens: HashSet::new(),
            carriers: HashMap::new(),
            quarantine_reason: None,
        }
    }

    pub(crate) fn head(&self) -> TruthHead {
        TruthHead {
            store_id: self.store_id.clone(),
            cursor: TruthCursor::new(self.epoch, self.head_sequence),
        }
    }

    fn next_cursor(&self) -> TruthCursor {
        TruthCursor::new(self.epoch, self.head_sequence + 1)
    }

    pub(crate) fn active_intent(&self) -> Option<&CursorIntent> {
        self.intent.as_ref()
    }

    pub(crate) fn quarantine_reason(&self) -> Option<&str> {
        self.quarantine_reason.as_deref()
    }

    pub(crate) fn append(
        &mut self,
        logical_reread_key: &str,
        validation_witness: &str,
        attempt_token: &str,
    ) -> Result<AppendResolution, CursorModelError> {
        self.ensure_available()?;
        validate_identity_fields(logical_reread_key, validation_witness, attempt_token)?;

        if let Some(receipt) = self.receipt_for_key(logical_reread_key).cloned() {
            self.set_intent(CursorIntent::new(
                self.next_cursor(),
                logical_reread_key,
                validation_witness,
                attempt_token,
            ))?;
            self.intent = None;
            return Ok(classify_append_receipt(&receipt, validation_witness));
        }

        self.set_intent(CursorIntent::new(
            self.next_cursor(),
            logical_reread_key,
            validation_witness,
            attempt_token,
        ))?;

        match self.carriers.get(logical_reread_key) {
            None => {
                self.carriers.insert(
                    logical_reread_key.to_owned(),
                    CarrierState::unambiguous(validation_witness),
                );
            }
            Some(existing)
                if existing.is_unambiguous()
                    && existing.validation_witness() == validation_witness => {}
            Some(_) => {
                self.quarantine(format!(
                    "unreceipted pre-existing carrier cannot be assigned: {logical_reread_key}"
                ));
                return Err(CursorModelError::WitnessMismatch(
                    logical_reread_key.to_owned(),
                ));
            }
        }

        let receipt = self.finalize_active_intent()?;
        Ok(AppendResolution::Created(receipt.cursor))
    }

    pub(crate) fn set_abandoned_intent(
        &mut self,
        intent: CursorIntent,
    ) -> Result<(), CursorModelError> {
        self.ensure_available()?;
        if intent.proposed_cursor == self.head().cursor {
            return self.restore_finalized_intent(intent);
        }
        self.set_intent(intent)
    }

    fn restore_finalized_intent(&mut self, intent: CursorIntent) -> Result<(), CursorModelError> {
        if self.intent.is_some() {
            return Err(CursorModelError::IntentAlreadyActive);
        }
        validate_identity_fields(
            &intent.logical_reread_key,
            &intent.validation_witness,
            &intent.attempt_token,
        )?;
        let Some(receipt) = self.receipts_by_sequence.get(&self.head_sequence) else {
            return Err(CursorModelError::SequenceGap {
                expected: self.head_sequence,
                observed: intent.proposed_cursor.sequence,
            });
        };
        if receipt.cursor != intent.proposed_cursor
            || receipt.logical_reread_key != intent.logical_reread_key
            || receipt.validation_witness != intent.validation_witness
            || receipt.attempt_token != intent.attempt_token
        {
            return Err(CursorModelError::WitnessMismatch(intent.logical_reread_key));
        }
        self.intent = Some(intent);
        Ok(())
    }

    fn set_intent(&mut self, intent: CursorIntent) -> Result<(), CursorModelError> {
        if self.intent.is_some() {
            return Err(CursorModelError::IntentAlreadyActive);
        }
        validate_identity_fields(
            &intent.logical_reread_key,
            &intent.validation_witness,
            &intent.attempt_token,
        )?;
        let expected = self.next_cursor();
        if intent.proposed_cursor.epoch != self.epoch {
            return Err(CursorModelError::WrongEpoch {
                expected: self.epoch,
                observed: intent.proposed_cursor.epoch,
            });
        }
        if intent.proposed_cursor.sequence != expected.sequence {
            return Err(CursorModelError::SequenceGap {
                expected: expected.sequence,
                observed: intent.proposed_cursor.sequence,
            });
        }
        if !self
            .used_attempt_tokens
            .insert(intent.attempt_token.clone())
        {
            return Err(CursorModelError::DuplicateAttemptToken(
                intent.attempt_token,
            ));
        }
        self.intent = Some(intent);
        Ok(())
    }

    pub(crate) fn set_carrier_for_test(
        &mut self,
        logical_reread_key: impl Into<String>,
        state: CarrierState,
    ) {
        self.carriers.insert(logical_reread_key.into(), state);
    }

    pub(crate) fn recover_abandoned_intent(
        &mut self,
    ) -> Result<RecoveryResolution, CursorModelError> {
        self.recover_intent(IntentRecoveryAuthority::ExclusiveAfterWriterExit)
    }

    pub(crate) fn recover_intent(
        &mut self,
        authority: IntentRecoveryAuthority,
    ) -> Result<RecoveryResolution, CursorModelError> {
        self.ensure_available()?;
        let Some(intent) = self.intent.clone() else {
            let next_sequence = self.head_sequence + 1;
            if let Some(receipt) = self.receipts_by_sequence.get(&next_sequence) {
                self.head_sequence = next_sequence;
                return Ok(RecoveryResolution::AdvancedHead(receipt.cursor));
            }
            if let Some((&observed, _)) = self
                .receipts_by_sequence
                .range((next_sequence + 1)..)
                .next()
            {
                self.quarantine(format!(
                    "receipt sequence {observed} skips expected {next_sequence}"
                ));
                return Err(CursorModelError::SequenceGap {
                    expected: next_sequence,
                    observed,
                });
            }
            return Ok(RecoveryResolution::NoIntent);
        };
        if authority == IntentRecoveryAuthority::WriterMayBeLive {
            return Ok(RecoveryResolution::LiveWriterBusy);
        }

        if let Some(receipt) = self.receipt_for_key(&intent.logical_reread_key).cloned() {
            self.intent = None;
            let advanced_head = self.head_sequence < receipt.cursor.sequence;
            if advanced_head {
                let expected = self.head_sequence + 1;
                if receipt.cursor.sequence != expected {
                    self.quarantine(format!(
                        "receipt sequence {} skips expected {expected}",
                        receipt.cursor.sequence
                    ));
                    return Err(CursorModelError::SequenceGap {
                        expected,
                        observed: receipt.cursor.sequence,
                    });
                }
                self.head_sequence = receipt.cursor.sequence;
            }
            if receipt.validation_witness == intent.validation_witness
                && receipt.cursor == intent.proposed_cursor
                && receipt.cursor.sequence == self.head_sequence
            {
                return Ok(RecoveryResolution::RetiredFinalized(receipt.cursor));
            }
            return Ok(classify_recovered_receipt(
                &receipt,
                &intent.validation_witness,
                advanced_head,
            ));
        }

        let Some(carrier) = self.carriers.get(&intent.logical_reread_key) else {
            self.intent = None;
            return Ok(RecoveryResolution::RetiredAbsent);
        };
        if !carrier.is_unambiguous() {
            let key = intent.logical_reread_key.clone();
            self.quarantine(format!("ambiguous unreceipted carrier: {key}"));
            return Err(CursorModelError::AmbiguousCarrier(key));
        }
        if carrier.validation_witness() != intent.validation_witness {
            let key = intent.logical_reread_key.clone();
            self.quarantine(format!("unreceipted carrier witness mismatch: {key}"));
            return Err(CursorModelError::WitnessMismatch(key));
        }

        if self
            .receipts_by_sequence
            .contains_key(&intent.proposed_cursor.sequence)
        {
            self.quarantine(format!(
                "duplicate sequence {}",
                intent.proposed_cursor.sequence
            ));
            return Err(CursorModelError::DuplicateSequence(
                intent.proposed_cursor.sequence,
            ));
        }

        let receipt = self.finalize_active_intent()?;
        Ok(RecoveryResolution::Published(receipt.cursor))
    }

    pub(crate) fn discover_legacy_write(
        &mut self,
        logical_reread_key: &str,
    ) -> Result<RecoveryResolution, CursorModelError> {
        self.ensure_available()?;
        if logical_reread_key.is_empty() {
            return Err(CursorModelError::EmptyLogicalRereadKey);
        }
        let head = self.head().cursor;
        self.quarantine(format!(
            "out-of-band carrier discovered by explicit audit: {logical_reread_key}"
        ));
        Ok(RecoveryResolution::EpochInvalidated(head))
    }

    fn finalize_active_intent(&mut self) -> Result<CursorReceipt, CursorModelError> {
        let intent = self.intent.clone().ok_or(CursorModelError::IntentAbsent)?;
        let expected = self.head_sequence + 1;
        if intent.proposed_cursor.sequence != expected {
            self.quarantine(format!(
                "intent sequence {} does not follow head {}",
                intent.proposed_cursor.sequence, self.head_sequence
            ));
            return Err(CursorModelError::SequenceGap {
                expected,
                observed: intent.proposed_cursor.sequence,
            });
        }

        let carrier = self
            .carriers
            .get(&intent.logical_reread_key)
            .ok_or_else(|| CursorModelError::CarrierAbsent(intent.logical_reread_key.clone()))?;
        if !carrier.is_unambiguous() {
            self.quarantine(format!(
                "ambiguous carrier for {}",
                intent.logical_reread_key
            ));
            return Err(CursorModelError::AmbiguousCarrier(
                intent.logical_reread_key,
            ));
        }
        if carrier.validation_witness() != intent.validation_witness {
            self.quarantine(format!(
                "carrier witness mismatch for {}",
                intent.logical_reread_key
            ));
            return Err(CursorModelError::WitnessMismatch(intent.logical_reread_key));
        }

        let receipt = CursorReceipt::from_intent(intent);
        self.insert_receipt(receipt.clone())?;
        self.head_sequence = receipt.cursor.sequence;
        self.intent = None;
        Ok(receipt)
    }

    fn insert_receipt(&mut self, receipt: CursorReceipt) -> Result<(), CursorModelError> {
        if receipt.validation_witness.is_empty() {
            return Err(CursorModelError::CorruptReceiptWitness(
                receipt.logical_reread_key,
            ));
        }
        if self
            .receipts_by_sequence
            .contains_key(&receipt.cursor.sequence)
        {
            return Err(CursorModelError::DuplicateSequence(receipt.cursor.sequence));
        }
        if self
            .sequence_by_logical_key
            .contains_key(&receipt.logical_reread_key)
        {
            return Err(CursorModelError::DuplicateLogicalKey(
                receipt.logical_reread_key,
            ));
        }
        self.sequence_by_logical_key
            .insert(receipt.logical_reread_key.clone(), receipt.cursor.sequence);
        self.receipts_by_sequence
            .insert(receipt.cursor.sequence, receipt);
        Ok(())
    }

    fn receipt_for_key(&self, logical_reread_key: &str) -> Option<&CursorReceipt> {
        self.sequence_by_logical_key
            .get(logical_reread_key)
            .and_then(|sequence| self.receipts_by_sequence.get(sequence))
    }

    pub(crate) fn events_after(
        &self,
        after: TruthCursor,
        limit: usize,
    ) -> Result<CursorDelta, CursorModelError> {
        self.ensure_available()?;
        if limit == 0 {
            return Err(CursorModelError::ZeroDeltaLimit);
        }
        if after.epoch != self.epoch {
            return Err(CursorModelError::WrongEpoch {
                expected: self.epoch,
                observed: after.epoch,
            });
        }
        let head = TruthCursor::new(self.epoch, self.head_sequence);
        if after.sequence > self.head_sequence {
            return Err(CursorModelError::CursorAhead {
                cursor: after,
                head,
            });
        }

        let mut receipts = Vec::new();
        let final_sequence = self.head_sequence.min(after.sequence + limit as u64);
        for expected in (after.sequence + 1)..=final_sequence {
            let receipt =
                self.receipts_by_sequence
                    .get(&expected)
                    .ok_or(CursorModelError::SequenceGap {
                        expected,
                        observed: self
                            .receipts_by_sequence
                            .range(expected..)
                            .next()
                            .map_or(self.head_sequence + 1, |(sequence, _)| *sequence),
                    })?;
            if receipt.cursor.epoch != self.epoch {
                return Err(CursorModelError::WrongEpoch {
                    expected: self.epoch,
                    observed: receipt.cursor.epoch,
                });
            }
            receipts.push(receipt.clone());
        }

        Ok(CursorDelta {
            after,
            observed_head: head,
            complete: final_sequence == self.head_sequence,
            receipts,
        })
    }

    pub(crate) fn validate_integrity(&self) -> Result<(), CursorModelError> {
        self.ensure_available()?;
        for expected in 1..=self.head_sequence {
            let receipt =
                self.receipts_by_sequence
                    .get(&expected)
                    .ok_or(CursorModelError::SequenceGap {
                        expected,
                        observed: self
                            .receipts_by_sequence
                            .range(expected..)
                            .next()
                            .map_or(self.head_sequence + 1, |(sequence, _)| *sequence),
                    })?;
            if receipt.cursor.epoch != self.epoch {
                return Err(CursorModelError::WrongEpoch {
                    expected: self.epoch,
                    observed: receipt.cursor.epoch,
                });
            }
            if receipt.validation_witness.is_empty() {
                return Err(CursorModelError::CorruptReceiptWitness(
                    receipt.logical_reread_key.clone(),
                ));
            }
            if self
                .sequence_by_logical_key
                .get(&receipt.logical_reread_key)
                != Some(&expected)
            {
                return Err(CursorModelError::DuplicateLogicalKey(
                    receipt.logical_reread_key.clone(),
                ));
            }
        }
        Ok(())
    }

    pub(crate) fn quarantine(&mut self, reason: impl Into<String>) {
        self.quarantine_reason = Some(reason.into());
    }

    #[cfg(test)]
    pub(crate) fn set_head_sequence_for_test(&mut self, sequence: u64) {
        self.head_sequence = sequence;
    }

    #[cfg(test)]
    pub(crate) fn receipt_mut_for_test(&mut self, sequence: u64) -> Option<&mut CursorReceipt> {
        self.receipts_by_sequence.get_mut(&sequence)
    }

    #[cfg(test)]
    pub(crate) fn remove_receipt_for_test(&mut self, sequence: u64) {
        self.receipts_by_sequence.remove(&sequence);
    }

    pub(crate) fn rebuild_epoch(
        &mut self,
        new_epoch: u64,
        ordered_validated_carriers: impl IntoIterator<Item = (String, String)>,
        full_validation_witness: impl Into<String>,
    ) -> Result<EpochRebuildReceipt, CursorModelError> {
        if new_epoch <= self.epoch {
            return Err(CursorModelError::EpochDidNotAdvance {
                previous_epoch: self.epoch,
                new_epoch,
            });
        }
        let full_validation_witness = full_validation_witness.into();
        if full_validation_witness.is_empty() {
            return Err(CursorModelError::MissingFullValidationWitness);
        }

        let previous_epoch = self.epoch;
        let mut rebuilt = Self::new(self.store_id.clone(), new_epoch);
        for (offset, (logical_key, witness)) in ordered_validated_carriers.into_iter().enumerate() {
            rebuilt.append(
                &logical_key,
                &witness,
                &format!("full-rebuild-{new_epoch}-{}", offset + 1),
            )?;
        }
        *self = rebuilt;
        Ok(EpochRebuildReceipt {
            previous_epoch,
            new_head: self.head().cursor,
            full_validation_witness,
        })
    }

    fn ensure_available(&self) -> Result<(), CursorModelError> {
        if let Some(reason) = &self.quarantine_reason {
            return Err(CursorModelError::Quarantined(reason.clone()));
        }
        Ok(())
    }
}

#[cfg(any(test, feature = "bench"))]
fn validate_identity_fields(
    logical_reread_key: &str,
    validation_witness: &str,
    attempt_token: &str,
) -> Result<(), CursorModelError> {
    if logical_reread_key.is_empty() {
        return Err(CursorModelError::EmptyLogicalRereadKey);
    }
    if validation_witness.is_empty() {
        return Err(CursorModelError::EmptyValidationWitness);
    }
    if attempt_token.is_empty() {
        return Err(CursorModelError::EmptyAttemptToken);
    }
    Ok(())
}

#[cfg(any(test, feature = "bench"))]
fn classify_append_receipt(receipt: &CursorReceipt, validation_witness: &str) -> AppendResolution {
    if receipt.validation_witness == validation_witness {
        AppendResolution::Existing(receipt.cursor)
    } else {
        AppendResolution::Conflict(receipt.cursor)
    }
}

#[cfg(any(test, feature = "bench"))]
fn classify_recovered_receipt(
    receipt: &CursorReceipt,
    validation_witness: &str,
    advanced_head: bool,
) -> RecoveryResolution {
    if receipt.validation_witness != validation_witness {
        RecoveryResolution::Conflict(receipt.cursor)
    } else if advanced_head {
        RecoveryResolution::AdvancedHead(receipt.cursor)
    } else {
        RecoveryResolution::Existing(receipt.cursor)
    }
}

#[cfg(any(test, feature = "bench"))]
impl QualificationJournalCursor for ReferenceCursorLedger {
    type Error = CursorModelError;

    fn qualification_truth_head(&self) -> Result<TruthHead, CursorModelError> {
        self.ensure_available()?;
        Ok(self.head())
    }

    fn qualification_events_after(
        &self,
        after: TruthCursor,
        limit: usize,
    ) -> Result<CursorDelta, CursorModelError> {
        self.events_after(after, limit)
    }
}
