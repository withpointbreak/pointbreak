//! Product contract and candidate-independent reference semantics for derived access.
// Dead-code analysis here is only meaningful with the evidence harness compiled in:
// in a `--test` build the tests are the roots, and the paths this module keeps for
// qualification are exercised by `bench_support::derived_access`, which lives behind
// the `bench` feature. `just lint` and `just check-types` run with all features, so
// the analysis still happens where it can tell the truth.
#![cfg_attr(not(all(test, feature = "bench")), allow(dead_code))]

pub(crate) mod attention;
pub(crate) mod changes;
#[cfg(any(test, feature = "bench"))]
pub(crate) mod checkpoint;
pub(crate) mod cursor;
pub(crate) mod generation;
pub(crate) mod history;
mod interaction;
pub(crate) mod layout;
pub(crate) mod lifecycle;
pub(crate) mod locator;
#[cfg(any(test, feature = "bench"))]
pub(crate) mod oracle;
pub(crate) mod product_contract;
pub(crate) mod revisions;
pub(crate) mod runtime;
pub(crate) mod semantic;
pub(crate) mod service;
pub(crate) mod sqlite;
mod support;
pub(crate) mod threads;
pub(crate) mod timeline;
pub(crate) mod verification;
pub(crate) mod writer;
pub(crate) use super::store::backend::{QualificationJournalCursor, QualificationLocalJournal};

#[cfg(test)]
mod service_tests;

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::checkpoint::{
        CheckpointAuthority, CheckpointFreshness, CheckpointModelError, DerivedCheckpoint,
        DerivedMutation, ReferenceDerivedState,
    };
    use super::cursor::{
        AppendResolution, CarrierState, CursorIntent, CursorModelError, CursorReceipt,
        EpochRebuildReceipt, IntentRecoveryAuthority, RecoveryResolution, ReferenceCursorLedger,
        TruthCursor,
    };
    use super::oracle::{ReferenceFact, ReferenceOracle, ReferenceOverlays};
    use crate::session::store::backend::QualificationJournalCursor;

    const STORE: &str = "store:sha256:reference";

    #[test]
    fn cursor_sequence_is_gap_free_and_duplicate_stable() {
        let mut ledger = ReferenceCursorLedger::new(STORE, 1);

        let first = ledger
            .append("event:a", "witness:a", "attempt:a")
            .expect("unique append");
        assert_eq!(first, AppendResolution::Created(TruthCursor::new(1, 1)));

        let duplicate = ledger
            .append("event:a", "witness:a", "attempt:duplicate")
            .expect("equal duplicate");
        assert_eq!(
            duplicate,
            AppendResolution::Existing(TruthCursor::new(1, 1))
        );
        assert_eq!(ledger.head().cursor, TruthCursor::new(1, 1));

        let conflict = ledger
            .append("event:a", "witness:other", "attempt:conflict")
            .expect("conflicting duplicate is classified");
        assert_eq!(conflict, AppendResolution::Conflict(TruthCursor::new(1, 1)));
        assert_eq!(ledger.head().cursor, TruthCursor::new(1, 1));

        let second = ledger
            .append("event:b", "witness:b", "attempt:b")
            .expect("second unique append");
        assert_eq!(second, AppendResolution::Created(TruthCursor::new(1, 2)));
        assert!(matches!(
            ledger.append("event:c", "witness:c", "attempt:b"),
            Err(CursorModelError::DuplicateAttemptToken(_))
        ));
        assert_eq!(
            ledger
                .qualification_events_after(TruthCursor::new(1, 0), 8)
                .expect("bounded delta")
                .receipts
                .iter()
                .map(|receipt| receipt.cursor.sequence)
                .collect::<Vec<_>>(),
            vec![1, 2]
        );
        assert_eq!(
            ledger.qualification_truth_head().unwrap().cursor,
            TruthCursor::new(1, 2)
        );
    }

    #[test]
    fn abandoned_intent_recovery_is_targeted_and_deterministic() {
        let mut ledger = ReferenceCursorLedger::new(STORE, 7);

        assert_eq!(
            ledger.recover_abandoned_intent().unwrap(),
            RecoveryResolution::NoIntent
        );

        ledger
            .set_abandoned_intent(CursorIntent::new(
                TruthCursor::new(7, 1),
                "event:absent",
                "witness:absent",
                "attempt:absent",
            ))
            .unwrap();
        assert_eq!(
            ledger.recover_abandoned_intent().unwrap(),
            RecoveryResolution::RetiredAbsent
        );
        assert_eq!(ledger.head().cursor, TruthCursor::new(7, 0));

        ledger
            .set_abandoned_intent(CursorIntent::new(
                TruthCursor::new(7, 1),
                "event:present",
                "witness:present",
                "attempt:present",
            ))
            .unwrap();
        ledger.set_carrier_for_test(
            "event:present",
            CarrierState::unambiguous("witness:present"),
        );
        assert_eq!(
            ledger.recover_abandoned_intent().unwrap(),
            RecoveryResolution::Published(TruthCursor::new(7, 1))
        );

        ledger
            .set_abandoned_intent(CursorIntent::new(
                TruthCursor::new(7, 2),
                "event:present",
                "witness:present",
                "attempt:already-receipted",
            ))
            .unwrap();
        assert_eq!(
            ledger.recover_abandoned_intent().unwrap(),
            RecoveryResolution::Existing(TruthCursor::new(7, 1))
        );
        assert_eq!(ledger.head().cursor, TruthCursor::new(7, 1));
    }

    #[test]
    fn live_writer_intent_is_never_recovered_and_legacy_audit_invalidates_epoch() {
        let mut ledger = ReferenceCursorLedger::new(STORE, 8);
        ledger
            .set_abandoned_intent(CursorIntent::new(
                TruthCursor::new(8, 1),
                "event:live",
                "witness:live",
                "attempt:live",
            ))
            .unwrap();
        ledger.set_carrier_for_test("event:live", CarrierState::unambiguous("witness:live"));

        assert_eq!(
            ledger
                .recover_intent(IntentRecoveryAuthority::WriterMayBeLive)
                .unwrap(),
            RecoveryResolution::LiveWriterBusy
        );
        assert!(ledger.active_intent().is_some());
        assert_eq!(ledger.head().cursor, TruthCursor::new(8, 0));

        assert_eq!(
            ledger.recover_abandoned_intent().unwrap(),
            RecoveryResolution::Published(TruthCursor::new(8, 1))
        );
        assert_eq!(
            ledger.discover_legacy_write("event:legacy").unwrap(),
            RecoveryResolution::EpochInvalidated(TruthCursor::new(8, 1))
        );
        assert!(ledger.quarantine_reason().is_some());
        assert!(matches!(
            ledger.qualification_truth_head(),
            Err(CursorModelError::Quarantined(_))
        ));
    }

    #[test]
    fn checkpoint_transition_is_atomic_and_absence_requires_coverage() {
        let authority = CheckpointAuthority::new(STORE, "profile:v1", 1, BTreeMap::new());
        let checkpoint = DerivedCheckpoint::empty(authority, TruthCursor::new(1, 0));
        let mut state = ReferenceDerivedState::new(checkpoint);

        let failed = state.apply_atomic(
            TruthCursor::new(1, 1),
            &[DerivedMutation::put("revision", "rev:a", "active")],
            Some("injected failure"),
        );
        assert!(failed.is_err());
        assert!(state.row("revision", "rev:a").is_none());
        assert_eq!(state.checkpoint().applied_cursor, TruthCursor::new(1, 0));

        state
            .apply_atomic(
                TruthCursor::new(1, 1),
                &[DerivedMutation::put("revision", "rev:a", "active")],
                None,
            )
            .unwrap();
        assert_eq!(state.row("revision", "rev:a"), Some("active"));
        assert!(!state.absence_is_authoritative("revision", "rev:missing", TruthCursor::new(1, 1)));

        state
            .mark_family_covered("revision", TruthCursor::new(1, 1))
            .unwrap();
        assert!(state.absence_is_authoritative("revision", "rev:missing", TruthCursor::new(1, 1)));
    }

    #[test]
    fn append_replay_and_display_orders_remain_distinct() {
        let mut oracle = ReferenceOracle::new(STORE, 1);
        let later_replay_key = ReferenceFact::new(
            "event:later-replay",
            "z-replay",
            "2026-07-26T00:00:00Z",
            "revision",
            "rev:one",
            "first by display",
        );
        let earlier_replay_key = ReferenceFact::new(
            "event:earlier-replay",
            "a-replay",
            "2026-07-27T00:00:00Z",
            "revision",
            "rev:one",
            "first by replay",
        );

        oracle.append(later_replay_key).unwrap();
        oracle.append(earlier_replay_key).unwrap();
        let receipt = oracle.receipt(&ReferenceOverlays::default()).unwrap();

        assert_eq!(
            receipt.append_order,
            vec!["event:later-replay", "event:earlier-replay"]
        );
        assert_eq!(
            receipt.replay_order,
            vec!["event:earlier-replay", "event:later-replay"]
        );
        assert_eq!(
            receipt.display_order,
            vec!["event:later-replay", "event:earlier-replay"]
        );
        assert_eq!(
            receipt
                .selected_semantics
                .get("revision:rev:one")
                .map(String::as_str),
            Some("first by replay")
        );
    }

    #[test]
    fn overlay_changes_do_not_advance_truth_cursor() {
        let mut oracle = ReferenceOracle::new(STORE, 1);
        oracle
            .append(ReferenceFact::new(
                "event:a",
                "a",
                "2026-07-26T00:00:00Z",
                "revision",
                "rev:a",
                "active",
            ))
            .unwrap();
        let before = oracle.receipt(&ReferenceOverlays::default()).unwrap();
        let after = oracle
            .receipt(&ReferenceOverlays {
                trust_generation: 1,
                git_generation: 2,
                removed_content: ["sha256:gone".to_owned()].into_iter().collect(),
            })
            .unwrap();

        assert_eq!(before.truth_cursor, after.truth_cursor);
        assert_ne!(before.semantic_receipt, after.semantic_receipt);
    }

    #[test]
    fn checkpoint_authority_and_freshness_fail_closed() {
        let mut versions = BTreeMap::new();
        versions.insert("revision".to_owned(), 1);
        let expected = CheckpointAuthority::new(STORE, "profile:v1", 1, versions.clone());
        let checkpoint = DerivedCheckpoint::empty(expected.clone(), TruthCursor::new(3, 2));

        assert_eq!(
            checkpoint.freshness(TruthCursor::new(3, 2)).unwrap(),
            CheckpointFreshness::Current
        );
        assert_eq!(
            checkpoint.freshness(TruthCursor::new(3, 4)).unwrap(),
            CheckpointFreshness::CatchUpRequired {
                from: TruthCursor::new(3, 2),
                to: TruthCursor::new(3, 4),
            }
        );
        assert!(matches!(
            checkpoint.freshness(TruthCursor::new(4, 0)).unwrap(),
            CheckpointFreshness::EpochMismatch { .. }
        ));
        assert!(matches!(
            checkpoint.freshness(TruthCursor::new(3, 1)),
            Err(CheckpointModelError::CursorAhead { .. })
        ));

        for incompatible in [
            CheckpointAuthority::new("store:other", "profile:v1", 1, versions.clone()),
            CheckpointAuthority::new(STORE, "profile:v2", 1, versions.clone()),
            CheckpointAuthority::new(STORE, "profile:v1", 2, versions.clone()),
            CheckpointAuthority::new(STORE, "profile:v1", 1, BTreeMap::new()),
        ] {
            assert!(checkpoint.validate_authority(&incompatible).is_err());
        }

        let mut quarantined = ReferenceDerivedState::new(DerivedCheckpoint::empty(
            expected.clone(),
            TruthCursor::new(3, 2),
        ));
        let wrong_store =
            CheckpointAuthority::new("store:other", "profile:v1", 1, versions.clone());
        assert!(matches!(
            quarantined.validate_authority_or_quarantine(&wrong_store),
            Err(CheckpointModelError::WrongStore { .. })
        ));
        assert!(quarantined.quarantine_reason().is_some());
        assert!(matches!(
            quarantined.apply_atomic(TruthCursor::new(3, 3), &[], None),
            Err(CheckpointModelError::Quarantined(_))
        ));
        assert!(!quarantined.absence_is_authoritative(
            "revision",
            "rev:missing",
            TruthCursor::new(3, 2)
        ));

        let broken = DerivedCheckpoint {
            observed_cursor: TruthCursor::new(3, 1),
            ..checkpoint
        };
        assert!(matches!(
            broken.freshness(TruthCursor::new(3, 2)),
            Err(CheckpointModelError::ObservedBehindApplied { .. })
        ));
    }

    #[test]
    fn multi_receipt_delta_is_contiguous_and_unsupported_family_never_rebuilds() {
        let authority = CheckpointAuthority::new(STORE, "profile:v1", 1, BTreeMap::new());
        let mut state =
            ReferenceDerivedState::new(DerivedCheckpoint::empty(authority, TruthCursor::new(5, 0)));
        let receipts = [
            CursorReceipt {
                cursor: TruthCursor::new(5, 1),
                logical_reread_key: "event:a".to_owned(),
                validation_witness: "witness:a".to_owned(),
                attempt_token: "attempt:a".to_owned(),
            },
            CursorReceipt {
                cursor: TruthCursor::new(5, 2),
                logical_reread_key: "event:b".to_owned(),
                validation_witness: "witness:b".to_owned(),
                attempt_token: "attempt:b".to_owned(),
            },
        ];
        state
            .apply_delta_atomic(
                TruthCursor::new(5, 2),
                &receipts,
                &[
                    DerivedMutation::put("revision", "rev:a", "active"),
                    DerivedMutation::delete("revision", "rev:a"),
                ],
                None,
            )
            .unwrap();
        assert!(state.row("revision", "rev:a").is_none());

        let before = state.checkpoint().clone();
        let unsupported = state.apply_atomic(
            TruthCursor::new(5, 3),
            &[DerivedMutation::unsupported(
                "search",
                "body-derived persistence is excluded",
            )],
            None,
        );
        assert!(matches!(
            unsupported,
            Err(CheckpointModelError::UnsupportedFamily { .. })
        ));
        assert_eq!(state.checkpoint(), &before);

        state.install_rebuild_receipt(EpochRebuildReceipt {
            previous_epoch: 4,
            new_head: TruthCursor::new(5, 2),
            full_validation_witness: "full-replay:sha256:ok".to_owned(),
        });
        assert!(state.checkpoint().rebuild_receipt.is_some());
    }

    #[test]
    fn corrupt_or_ambiguous_cursor_metadata_quarantines_until_full_rebuild() {
        let mut ledger = ReferenceCursorLedger::new(STORE, 9);
        ledger
            .set_abandoned_intent(CursorIntent::new(
                TruthCursor::new(9, 1),
                "event:ambiguous",
                "witness:a",
                "attempt:a",
            ))
            .unwrap();
        assert!(ledger.active_intent().is_some());
        ledger.set_carrier_for_test("event:ambiguous", CarrierState::ambiguous("witness:a"));
        assert!(matches!(
            ledger.recover_abandoned_intent(),
            Err(CursorModelError::AmbiguousCarrier(_))
        ));
        assert!(ledger.quarantine_reason().is_some());
        assert!(matches!(
            ledger.events_after(TruthCursor::new(9, 0), 1),
            Err(CursorModelError::Quarantined(_))
        ));

        let rebuild = ledger
            .rebuild_epoch(
                10,
                [
                    ("event:a".to_owned(), "witness:a".to_owned()),
                    ("event:b".to_owned(), "witness:b".to_owned()),
                ],
                "full-replay:sha256:ok",
            )
            .unwrap();
        assert_eq!(rebuild.previous_epoch, 9);
        assert_eq!(rebuild.new_head, TruthCursor::new(10, 2));
        assert_eq!(ledger.quarantine_reason(), None);
        ledger.validate_integrity().unwrap();

        assert!(matches!(
            ledger.events_after(TruthCursor::new(9, 0), 1),
            Err(CursorModelError::WrongEpoch { .. })
        ));
        assert!(matches!(
            ledger.events_after(TruthCursor::new(10, 3), 1),
            Err(CursorModelError::CursorAhead { .. })
        ));
        assert!(
            !ledger
                .events_after(TruthCursor::new(10, 0), 1)
                .unwrap()
                .complete
        );
        assert!(matches!(
            ledger.events_after(TruthCursor::new(10, 0), 0),
            Err(CursorModelError::ZeroDeltaLimit)
        ));

        let mut mismatch = ReferenceCursorLedger::new(STORE, 9);
        mismatch
            .set_abandoned_intent(CursorIntent::new(
                TruthCursor::new(9, 1),
                "event:mismatch",
                "witness:expected",
                "attempt:mismatch",
            ))
            .unwrap();
        mismatch.set_carrier_for_test(
            "event:mismatch",
            CarrierState::unambiguous("witness:observed"),
        );
        assert!(matches!(
            mismatch.recover_abandoned_intent(),
            Err(CursorModelError::WitnessMismatch(_))
        ));
        assert_eq!(mismatch.head().cursor, TruthCursor::new(9, 0));
        assert!(mismatch.quarantine_reason().is_some());
    }

    #[test]
    fn receipt_with_old_head_advances_and_finalized_intent_retires() {
        let mut ledger = ReferenceCursorLedger::new(STORE, 11);
        ledger.append("event:a", "witness:a", "attempt:a").unwrap();
        ledger.set_head_sequence_for_test(0);
        assert_eq!(
            ledger.recover_abandoned_intent().unwrap(),
            RecoveryResolution::AdvancedHead(TruthCursor::new(11, 1))
        );

        ledger
            .set_abandoned_intent(CursorIntent::new(
                TruthCursor::new(11, 1),
                "event:a",
                "witness:a",
                "attempt:a",
            ))
            .unwrap();
        assert_eq!(
            ledger.recover_abandoned_intent().unwrap(),
            RecoveryResolution::RetiredFinalized(TruthCursor::new(11, 1))
        );
        assert!(ledger.active_intent().is_none());

        ledger
            .set_abandoned_intent(CursorIntent::new(
                TruthCursor::new(11, 2),
                "event:a",
                "witness:a",
                "attempt:retire",
            ))
            .unwrap();
        assert_eq!(
            ledger.recover_abandoned_intent().unwrap(),
            RecoveryResolution::Existing(TruthCursor::new(11, 1))
        );
        assert!(ledger.active_intent().is_none());
    }

    #[test]
    fn receipt_gaps_forks_wrong_epochs_and_corrupt_witnesses_are_rejected() {
        fn populated() -> ReferenceCursorLedger {
            let mut ledger = ReferenceCursorLedger::new(STORE, 12);
            ledger.append("event:a", "witness:a", "attempt:a").unwrap();
            ledger.append("event:b", "witness:b", "attempt:b").unwrap();
            ledger
        }

        let mut gap = populated();
        gap.remove_receipt_for_test(1);
        assert!(matches!(
            gap.validate_integrity(),
            Err(CursorModelError::SequenceGap { .. })
        ));

        let mut fork = populated();
        fork.receipt_mut_for_test(2).unwrap().logical_reread_key = "event:a".to_owned();
        assert!(matches!(
            fork.validate_integrity(),
            Err(CursorModelError::DuplicateLogicalKey(_))
        ));

        let mut wrong_epoch = populated();
        wrong_epoch.receipt_mut_for_test(2).unwrap().cursor.epoch = 13;
        assert!(matches!(
            wrong_epoch.validate_integrity(),
            Err(CursorModelError::WrongEpoch { .. })
        ));

        let mut corrupt = populated();
        corrupt
            .receipt_mut_for_test(2)
            .unwrap()
            .validation_witness
            .clear();
        assert!(matches!(
            corrupt.validate_integrity(),
            Err(CursorModelError::CorruptReceiptWitness(_))
        ));
    }

    #[test]
    fn receipted_conflict_and_duplicate_sequence_fail_without_advancing() {
        let mut conflict = ReferenceCursorLedger::new(STORE, 14);
        conflict
            .append("event:a", "witness:a", "attempt:a")
            .unwrap();
        conflict
            .set_abandoned_intent(CursorIntent::new(
                TruthCursor::new(14, 2),
                "event:a",
                "witness:other",
                "attempt:conflict",
            ))
            .unwrap();
        assert_eq!(
            conflict.recover_abandoned_intent().unwrap(),
            RecoveryResolution::Conflict(TruthCursor::new(14, 1))
        );
        assert_eq!(conflict.head().cursor, TruthCursor::new(14, 1));
        assert!(conflict.active_intent().is_none());

        let mut duplicate_sequence = ReferenceCursorLedger::new(STORE, 14);
        duplicate_sequence
            .append("event:a", "witness:a", "attempt:a")
            .unwrap();
        duplicate_sequence.set_head_sequence_for_test(0);
        duplicate_sequence
            .set_abandoned_intent(CursorIntent::new(
                TruthCursor::new(14, 1),
                "event:b",
                "witness:b",
                "attempt:b",
            ))
            .unwrap();
        duplicate_sequence.set_carrier_for_test("event:b", CarrierState::unambiguous("witness:b"));
        assert!(matches!(
            duplicate_sequence.recover_abandoned_intent(),
            Err(CursorModelError::DuplicateSequence(1))
        ));
        assert_eq!(duplicate_sequence.head().cursor, TruthCursor::new(14, 0));
        assert!(duplicate_sequence.quarantine_reason().is_some());
    }

    #[test]
    fn reference_semantic_family_matrix_is_first_seen_by_replay() {
        let mut oracle = ReferenceOracle::new(STORE, 1);
        for fact in [
            ReferenceFact::new(
                "event:fork-b",
                "b",
                "2026-07-26T00:00:00Z",
                "revision",
                "rev:fork",
                "fork-b",
            ),
            ReferenceFact::new(
                "event:fork-a",
                "a",
                "2026-07-27T00:00:00Z",
                "revision",
                "rev:fork",
                "fork-a",
            ),
            ReferenceFact::new(
                "event:supersession",
                "c",
                "2026-07-27T00:00:01Z",
                "supersession",
                "rev:old->rev:new",
                "active",
            ),
            ReferenceFact::new(
                "event:assessment",
                "d",
                "2026-07-27T00:00:02Z",
                "assessment",
                "assess:one",
                "accepted",
            ),
            ReferenceFact::new(
                "event:request",
                "e",
                "2026-07-27T00:00:03Z",
                "input-request",
                "request:one",
                "open",
            ),
            ReferenceFact::new(
                "event:response",
                "f",
                "2026-07-27T00:00:04Z",
                "input-response",
                "response:one",
                "answered",
            ),
            ReferenceFact::new(
                "event:validation",
                "g",
                "2026-07-27T00:00:05Z",
                "validation",
                "validation:one",
                "passed",
            ),
            ReferenceFact::new(
                "event:association",
                "h",
                "2026-07-27T00:00:06Z",
                "association",
                "commit:one",
                "current",
            ),
        ] {
            oracle.append(fact).unwrap();
        }
        let receipt = oracle.receipt(&ReferenceOverlays::default()).unwrap();
        assert_eq!(
            receipt
                .selected_semantics
                .get("revision:rev:fork")
                .map(String::as_str),
            Some("fork-a")
        );
        for key in [
            "supersession:rev:old->rev:new",
            "assessment:assess:one",
            "input-request:request:one",
            "input-response:response:one",
            "validation:validation:one",
            "association:commit:one",
        ] {
            assert!(receipt.selected_semantics.contains_key(key), "{key}");
        }
    }

    #[test]
    fn canonical_reference_fixture_declares_recovery_and_semantic_vectors() {
        let fixture_bytes =
            include_str!("../../../tests/fixtures/derived-access/reference-v1.json");
        assert_eq!(
            crate::canonical_hash::sha256_bytes_hex(fixture_bytes.as_bytes()),
            "a8391645c3da83f1eea6c3b0eaedfdf361013bf21543bf30e066a670a6668ad3"
        );
        let fixture: serde_json::Value = serde_json::from_str(fixture_bytes).unwrap();
        assert_eq!(
            fixture["schema"],
            "pointbreak.qualification-derived-access-reference.v1"
        );
        #[cfg(feature = "bench")]
        assert_eq!(
            fixture["contractSha256"],
            crate::bench_support::derived_access::QUALIFICATION_DERIVED_ACCESS_CONTRACT_SHA256_V1
        );

        let recovery_states = fixture["recoveryStates"].as_array().unwrap();
        for state in [
            "no_intent",
            "live_writer_intent",
            "abandoned_event_absent",
            "abandoned_receipted_earlier",
            "abandoned_matching_unreceipted",
            "abandoned_mismatch",
            "receipt_head_old",
            "head_current_intent_remains",
            "derived_transaction_interrupted",
            "incompatible_or_corrupt",
            "legacy_write_audit",
        ] {
            assert!(
                recovery_states.iter().any(|candidate| candidate == state),
                "{state}"
            );
        }

        #[derive(serde::Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct FixtureFact {
            logical_reread_key: String,
            replay_key: String,
            occurred_at: String,
            semantic_family: String,
            semantic_id: String,
            value: String,
        }
        let schedule: Vec<FixtureFact> =
            serde_json::from_value(fixture["referenceSchedule"].clone()).unwrap();
        let mut oracle = ReferenceOracle::new(STORE, 1);
        for fact in schedule {
            oracle
                .append(ReferenceFact::new(
                    fact.logical_reread_key,
                    fact.replay_key,
                    fact.occurred_at,
                    fact.semantic_family,
                    fact.semantic_id,
                    fact.value,
                ))
                .unwrap();
        }
        let receipt = oracle.receipt(&ReferenceOverlays::default()).unwrap();
        assert_eq!(receipt.truth_cursor, TruthCursor::new(1, 8));
        assert_eq!(receipt.selected_semantics.len(), 7);
        assert_eq!(
            receipt
                .selected_semantics
                .get("revision:rev:fork")
                .map(String::as_str),
            Some("fork-a")
        );
    }
}

#[cfg(test)]
mod product_contract_tests;
