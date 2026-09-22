//! The review summary's input rows, read from the active derived generation.
//!
//! Five reads over the bodyless index fill the same lane-neutral rows the
//! authoritative lane folds from events; the population and measure rules stay
//! in the pure model, so no read here filters by them. Only the counted events
//! are hydrated, from their authoritative carriers and at the checkpoint the rows
//! were read at, so the receipt binds recorded bytes rather than index columns.

use std::sync::Arc;

use rusqlite::{Connection, params};

use super::cursor::TruthCursor;
use super::history::{
    CurrentRead, DerivedHistoryAccess, DerivedHistoryStatus, catching_up_status, hydrate_events,
    legacy_terminal, projection_stamp,
};
use super::lifecycle::CurrentGeneration;
use super::locator::LocatorRead;
use super::semantic::{decode_enum, decode_string_list};
use super::sqlite::LegacyReadContext;
#[cfg(any(test, feature = "longitudinal-counting"))]
use crate::bench_support::longitudinal::{
    LongitudinalDerivedAccessPhaseV1 as Phase, enter_derived_access_phase_v1,
};
use crate::error::Result as ShoreResult;
use crate::session::event::{ReviewAssessment, ShoreEvent};
use crate::session::store::capabilities::{BackfillCohort, backfill_cohort};
use crate::session::workflow::review_summary::{
    AssessmentInput, CaptureInput, CommitAssociationInput, CountedEventRef, MembershipClaimInput,
    MembershipWithdrawalInput, ReviewSummaryInputs,
};

pub(crate) enum DerivedReviewSummaryRoute {
    Ready(DerivedReviewSummaryRead),
    Off,
    Unavailable(DerivedHistoryStatus),
}

/// Input rows read at one checkpoint, and the generation that can hydrate the
/// counted events at that same checkpoint.
pub(crate) struct DerivedReviewSummaryRead {
    inputs: ReviewSummaryInputs,
    projection_stamp: String,
    event_count: usize,
    current: Arc<CurrentGeneration>,
    as_of: TruthCursor,
}

/// Counted envelopes at the read's checkpoint, or word that the generation
/// moved past it; a moved read is never completed from mixed checkpoints.
pub(crate) enum CountedEnvelopes {
    Ready(Vec<ShoreEvent>),
    Stale,
}

impl DerivedReviewSummaryRead {
    pub(crate) fn inputs(&self) -> &ReviewSummaryInputs {
        &self.inputs
    }

    pub(crate) fn projection_stamp(&self) -> &str {
        &self.projection_stamp
    }

    pub(crate) fn event_count(&self) -> usize {
        self.event_count
    }

    /// The same read pinned one sequence behind the checkpoint it was taken at,
    /// as a read is once its generation has moved on.
    #[cfg(test)]
    fn pinned_behind_for_test(mut self) -> Self {
        self.as_of = TruthCursor::new(self.as_of.epoch, self.as_of.sequence - 1);
        self
    }

    /// Envelopes for exactly these event ids, in request order, validated from
    /// their authoritative carriers as of the checkpoint the inputs were read at.
    pub(crate) fn hydrate_counted(&self, event_ids: &[String]) -> Result<CountedEnvelopes, String> {
        #[cfg(any(test, feature = "longitudinal-counting"))]
        let _hydration_phase =
            enter_derived_access_phase_v1(Phase::ReviewSummaryCountedCarrierHydrationValidation);
        let service = self.current.service();
        legacy_terminal(
            service,
            self.as_of,
            hydrate_events(service, event_ids, self.as_of).map(CountedEnvelopes::Ready),
            || CountedEnvelopes::Stale,
        )
    }
}

impl DerivedHistoryAccess {
    /// The backfill cohort of the store this access serves; `None` when derived
    /// access is off.
    pub(crate) fn review_summary_cohort(&self) -> ShoreResult<Option<BackfillCohort>> {
        self.active_context()
            .map(|(_, backend)| backfill_cohort(backend.journal().as_ref()))
            .transpose()
    }

    /// Read every summary input row from the current generation at one
    /// checkpoint. Anything but a current generation that stays at that
    /// checkpoint is `Unavailable`; this read never builds or catches up.
    pub(crate) fn review_summary_inputs(
        &self,
        cohort: &BackfillCohort,
    ) -> Result<DerivedReviewSummaryRoute, String> {
        let Some((store_identity, _)) = self.active_context() else {
            return Ok(DerivedReviewSummaryRoute::Off);
        };
        let current = match self.current()? {
            CurrentRead::Ready(current) => current,
            CurrentRead::Unavailable(status) => {
                return Ok(DerivedReviewSummaryRoute::Unavailable(status));
            }
        };
        #[cfg(any(test, feature = "longitudinal-counting"))]
        let selection_phase = enter_derived_access_phase_v1(Phase::ReviewSummarySqlSelection);
        let service = current.service();
        let context = match service
            .legacy_read_context()
            .map_err(|error| error.to_string())?
        {
            LocatorRead::Ready(context) => context,
            LocatorRead::CatchUpRequired { .. } => {
                return Ok(DerivedReviewSummaryRoute::Unavailable(catching_up_status()));
            }
        };
        let LegacyReadContext {
            connection,
            state,
            as_of,
        } = context;
        let outcome = read_inputs(&connection, as_of, cohort).and_then(|inputs| {
            Ok(DerivedReviewSummaryRoute::Ready(DerivedReviewSummaryRead {
                inputs,
                projection_stamp: projection_stamp(store_identity, as_of)?,
                event_count: state.event_count,
                current: Arc::clone(&current),
                as_of,
            }))
        });
        #[cfg(any(test, feature = "longitudinal-counting"))]
        drop(selection_phase);
        legacy_terminal(service, as_of, outcome, || {
            DerivedReviewSummaryRoute::Unavailable(catching_up_status())
        })
    }
}

/// Every row is bounded by the checkpoint: the fact tables are append-only by
/// sequence, so rows past it belong to a later read.
fn read_inputs(
    connection: &Connection,
    as_of: TruthCursor,
    cohort: &BackfillCohort,
) -> Result<ReviewSummaryInputs, String> {
    let as_of = i64::try_from(as_of.sequence)
        .map_err(|_| "derived checkpoint does not fit SQLite INTEGER".to_owned())?;
    Ok(ReviewSummaryInputs {
        memberships: membership_claims(connection, as_of, cohort)?,
        withdrawals: membership_withdrawals(connection, as_of)?,
        captures: captures(connection, as_of)?,
        assessments: assessments(connection, as_of)?,
        commit_associations: commit_associations(connection, as_of)?,
        manifest_hash: cohort.manifest_hash().map(str::to_owned),
    })
}

fn rows<T>(
    connection: &Connection,
    what: &str,
    sql: &str,
    as_of: i64,
    mut map: impl FnMut(&rusqlite::Row<'_>) -> Result<T, String>,
) -> Result<Vec<T>, String> {
    let sql_error = |error: rusqlite::Error| format!("read summary {what}: {error}");
    let mut statement = connection.prepare(sql).map_err(sql_error)?;
    let mut query = statement.query(params![as_of]).map_err(sql_error)?;
    let mut rows = Vec::new();
    while let Some(row) = query.next().map_err(sql_error)? {
        rows.push(map(row)?);
    }
    Ok(rows)
}

fn column<T: rusqlite::types::FromSql>(row: &rusqlite::Row<'_>, index: usize) -> Result<T, String> {
    row.get(index)
        .map_err(|error| format!("read summary column {index}: {error}"))
}

fn source(row: &rusqlite::Row<'_>) -> Result<CountedEventRef, String> {
    Ok(CountedEventRef {
        event_id: column(row, 0)?,
        payload_hash: column(row, 1)?,
    })
}

fn membership_claims(
    connection: &Connection,
    as_of: i64,
    cohort: &BackfillCohort,
) -> Result<Vec<MembershipClaimInput>, String> {
    rows(
        connection,
        "membership claims",
        "SELECT locator.event_id, locator.payload_hash,
                claim.claim_id, claim.change_id, claim.revision_id
         FROM product_history_membership_claim AS claim
         JOIN locator_event_text AS locator ON locator.sequence = claim.sequence
         WHERE claim.sequence <= ?1
         ORDER BY claim.sequence",
        as_of,
        |row| {
            let source = source(row)?;
            let backfill = cohort.contains(&source.event_id);
            Ok(MembershipClaimInput {
                source,
                claim_id: column(row, 2)?,
                change_id: column(row, 3)?,
                revision_id: column(row, 4)?,
                backfill,
            })
        },
    )
}

fn membership_withdrawals(
    connection: &Connection,
    as_of: i64,
) -> Result<Vec<MembershipWithdrawalInput>, String> {
    rows(
        connection,
        "membership withdrawals",
        "SELECT locator.event_id, locator.payload_hash, withdrawal.claim_id
         FROM product_history_membership_withdrawal AS withdrawal
         JOIN locator_event_text AS locator ON locator.sequence = withdrawal.sequence
         WHERE withdrawal.sequence <= ?1
         ORDER BY withdrawal.sequence",
        as_of,
        |row| {
            Ok(MembershipWithdrawalInput {
                source: source(row)?,
                claim_id: column(row, 2)?,
            })
        },
    )
}

/// The capture instant is the index's parse of the event's own timestamp, the
/// same parse the authoritative lane applies, so legacy and RFC 3339 instants
/// order identically in both lanes.
fn captures(connection: &Connection, as_of: i64) -> Result<Vec<CaptureInput>, String> {
    rows(
        connection,
        "captures",
        "SELECT locator.event_id, locator.payload_hash,
                revision.revision_id, fact.actor_id, revision.captured_at_millis
         FROM product_revision AS revision
         JOIN semantic_event_fact_text AS fact ON fact.sequence = revision.sequence
         JOIN locator_event_text AS locator ON locator.sequence = revision.sequence
         WHERE revision.sequence <= ?1
         ORDER BY revision.sequence",
        as_of,
        |row| {
            Ok(CaptureInput {
                source: source(row)?,
                revision_id: column(row, 2)?,
                actor: column(row, 3)?,
                captured_at_millis: Some(column(row, 4)?),
            })
        },
    )
}

/// The Revision is the one the index records for the assessment's target,
/// whatever its scope: the enclosing Revision, as in the authoritative lane.
fn assessments(connection: &Connection, as_of: i64) -> Result<Vec<AssessmentInput>, String> {
    rows(
        connection,
        "assessments",
        "SELECT locator.event_id, locator.payload_hash,
                fact.semantic_id, fact.revision_id, fact.actor_id,
                assessment.assessment, assessment.replaces_json
         FROM semantic_assessment_fact AS assessment
         JOIN semantic_event_fact_text AS fact ON fact.sequence = assessment.sequence
         JOIN locator_event_text AS locator ON locator.sequence = assessment.sequence
         WHERE assessment.sequence <= ?1
         ORDER BY assessment.sequence",
        as_of,
        |row| {
            let source = source(row)?;
            let assessment_id: Option<String> = column(row, 2)?;
            let assessment_id = assessment_id.ok_or_else(|| {
                format!(
                    "summary assessment {} has no assessment id",
                    source.event_id
                )
            })?;
            let verdict: String = column(row, 5)?;
            let replaces: String = column(row, 6)?;
            Ok(AssessmentInput {
                assessment_id,
                revision_id: column(row, 3)?,
                actor: column(row, 4)?,
                verdict: decode_enum::<ReviewAssessment>(&verdict)
                    .map_err(|error| format!("summary assessment verdict: {error}"))?,
                replaces: decode_string_list(&replaces)
                    .map_err(|error| format!("summary replaced assessments: {error}"))?,
                source,
            })
        },
    )
}

fn commit_associations(
    connection: &Connection,
    as_of: i64,
) -> Result<Vec<CommitAssociationInput>, String> {
    rows(
        connection,
        "commit associations",
        "SELECT locator.event_id, locator.payload_hash, fact.revision_id
         FROM semantic_commit_association_fact AS association
         JOIN semantic_event_fact_text AS fact ON fact.sequence = association.sequence
         JOIN locator_event_text AS locator ON locator.sequence = association.sequence
         WHERE association.sequence <= ?1 AND fact.revision_id IS NOT NULL
         ORDER BY association.sequence",
        as_of,
        |row| {
            Ok(CommitAssociationInput {
                source: source(row)?,
                revision_id: column(row, 2)?,
            })
        },
    )
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use tempfile::TempDir;

    use super::*;
    use crate::bench_support::longitudinal::{
        LongitudinalCountingScopeV1, LongitudinalDerivedAccessPhaseV1 as Phase,
    };
    use crate::model::{
        ActorId, AssessmentId, ChangeId, ChangeIdentityDescriptorV1, CommitAssociationId,
        EngagementId, JournalId, ObjectId, ObservationId, ReviewEndpoint, ReviewTargetRef,
        RevisionId, Side, TrackId,
    };
    use crate::session::derived_access::history::{DerivedHistoryAccess, DerivedHistoryMode};
    use crate::session::derived_access::lifecycle::{DerivedAccessLifecycle, LifecycleControl};
    use crate::session::derived_access::product_contract::DerivedAccessProfile;
    use crate::session::event::{
        ChangeMembershipAssertedPayload, EventPayload, EventTarget, ReviewAssessment,
        ReviewAssessmentRecordedPayload, Revision, RevisionCommitAssociatedPayload, ShoreEvent,
        WorkObjectProposal, WorkObjectProposedPayload, Writer, WriterProducer,
        build_change_declared, build_membership_asserted, build_membership_withdrawn,
    };
    use crate::session::store::backend::StoreBackend;
    use crate::session::store::capabilities::{
        BackfillCohort, CapabilityFixtureState, write_capability_fixture_for_test,
    };
    use crate::session::workflow::review_summary::{
        ReviewSummaryInputs, compute_review_summary, review_summary_inputs_from_events,
    };
    use crate::session::{EventStore, EventWriteOutcome};

    const JOURNAL: &str = "journal:default";
    const AUTHOR: &str = "actor:author";
    const REVIEWER: &str = "actor:reviewer";
    const TRACK: &str = "human:reviewer";
    /// 2025-10-09T08:53:20.000Z in the legacy `unix-ms:` form: earlier than the
    /// first RFC 3339 capture, yet later when compared as a string.
    const LEGACY_INSTANT: &str = "unix-ms:1760000000000";

    fn writer(actor: &str) -> Writer {
        Writer {
            actor_id: ActorId::new(actor.to_owned()),
            producer: WriterProducer {
                name: "pointbreak".to_owned(),
                version: "test".to_owned(),
            },
        }
    }

    fn rev(suffix: &str) -> RevisionId {
        RevisionId::new(format!("rev:sha256:{}", suffix.repeat(64 / suffix.len())))
    }

    fn journal_event<P: EventPayload>(key: &str, actor: &str, payload: P, at: &str) -> ShoreEvent {
        ShoreEvent::new(
            payload.event_type(),
            key,
            EventTarget::for_journal(JournalId::new(JOURNAL)),
            writer(actor),
            payload,
            at,
        )
        .expect("build journal event")
    }

    fn revision_event<P: EventPayload>(
        key: &str,
        actor: &str,
        revision: &RevisionId,
        payload: P,
        at: &str,
    ) -> ShoreEvent {
        ShoreEvent::new(
            payload.event_type(),
            key,
            EventTarget::for_revision(
                JournalId::new(JOURNAL),
                revision.clone(),
                Some(TrackId::new(TRACK)),
            )
            .expect("revision target"),
            writer(actor),
            payload,
            at,
        )
        .expect("build revision event")
    }

    fn capture(revision: &RevisionId, at: &str) -> ShoreEvent {
        revision_event(
            &format!("summary:capture:{}", revision.as_str()),
            AUTHOR,
            revision,
            WorkObjectProposedPayload {
                engagement_id: EngagementId::new(format!("engagement:sha256:{}", "e".repeat(64))),
                work_object: WorkObjectProposal::Revision {
                    revision: Revision {
                        id: revision.clone(),
                        object_id: ObjectId::new(format!("obj:sha256:{}", "0".repeat(64))),
                        git_provenance: None,
                    },
                    summary: None,
                    object_artifact_content_hash: format!("sha256:{}", "1".repeat(64)),
                    supersedes: Vec::new(),
                },
            },
            at,
        )
    }

    fn assessment(
        key: &str,
        actor: &str,
        revision: &RevisionId,
        target: ReviewTargetRef,
        verdict: ReviewAssessment,
        replaces: &[&str],
    ) -> ShoreEvent {
        revision_event(
            &format!("summary:assessment:{key}"),
            actor,
            revision,
            ReviewAssessmentRecordedPayload {
                assessment_id: AssessmentId::new(format!("assess:sha256:{key}")),
                target,
                assessment: verdict,
                summary: None,
                summary_content_type: Default::default(),
                summary_artifact_path: None,
                summary_byte_size: None,
                summary_content_hash: None,
                replaces_assessment_ids: replaces
                    .iter()
                    .map(|id| AssessmentId::new(format!("assess:sha256:{id}")))
                    .collect(),
                related_observation_ids: Vec::new(),
                related_input_request_ids: Vec::new(),
            },
            "2026-06-03T00:00:00Z",
        )
    }

    fn association(key: &str, revision: &RevisionId) -> ShoreEvent {
        revision_event(
            &format!("summary:association:{key}"),
            REVIEWER,
            revision,
            RevisionCommitAssociatedPayload {
                commit_association_id: CommitAssociationId::new(format!(
                    "commit-association:sha256:{key}"
                )),
                target: ReviewTargetRef::Revision {
                    revision_id: revision.clone(),
                },
                commit: ReviewEndpoint::GitCommit {
                    commit_oid: "c".repeat(40),
                    tree_oid: "d".repeat(40),
                },
            },
            "2026-06-04T00:00:00Z",
        )
    }

    /// A store holding one of each event the summary reads, plus an event it
    /// ignores, with a current derived generation built over it.
    struct SummaryStore {
        _temp: TempDir,
        store: EventStore,
        access: DerivedHistoryAccess,
        events: Vec<ShoreEvent>,
        cohort: BackfillCohort,
        backfill_claim: ShoreEvent,
        legacy: RevisionId,
    }

    impl SummaryStore {
        fn new(build: bool) -> Self {
            let temp = TempDir::new().expect("disposable store");
            let backend = StoreBackend::Local(temp.path().to_path_buf());
            write_capability_fixture_for_test(
                backend.journal().as_ref(),
                CapabilityFixtureState::EmptyL2,
            )
            .expect("activate disposable store");
            let store = EventStore::from_backend(&backend);

            let declared =
                build_change_declared(ChangeIdentityDescriptorV1::opaque_nonce([7; 32]), [8; 32])
                    .expect("declare Change");
            let change: ChangeId = declared.change_id.clone();
            let (first, second, legacy) = (rev("a1"), rev("b2"), rev("c3"));
            let claim = |key: &str, revision: &RevisionId, nonce: u8| {
                journal_event(
                    key,
                    AUTHOR,
                    build_membership_asserted(&change, revision, [nonce; 32]).unwrap(),
                    "2026-06-01T00:00:00Z",
                )
            };
            let backfill_claim = claim("summary:claim:first", &first, 1);
            let second_claim = claim("summary:claim:second", &second, 2);
            let legacy_claim = claim("summary:claim:legacy", &legacy, 3);
            let second_claim_id = serde_json::from_value::<ChangeMembershipAssertedPayload>(
                second_claim.payload.clone(),
            )
            .unwrap()
            .membership_claim_id;
            let file = |revision: &RevisionId| ReviewTargetRef::File {
                revision_id: revision.clone(),
                file_path: "src/lib.rs".to_owned(),
            };

            let events = vec![
                journal_event(
                    "summary:declare",
                    AUTHOR,
                    declared.clone(),
                    "2026-06-01T00:00:00Z",
                ),
                capture(&first, "2025-10-09T08:53:20.001Z"),
                capture(&second, "2026-06-02T00:00:00Z"),
                capture(&legacy, LEGACY_INSTANT),
                backfill_claim.clone(),
                second_claim,
                legacy_claim,
                journal_event(
                    "summary:withdraw:second",
                    AUTHOR,
                    build_membership_withdrawn(&second_claim_id, [4; 32]).unwrap(),
                    "2026-06-02T00:00:01Z",
                ),
                assessment(
                    "whole",
                    REVIEWER,
                    &first,
                    ReviewTargetRef::Revision {
                        revision_id: first.clone(),
                    },
                    ReviewAssessment::NeedsChanges,
                    &[],
                ),
                assessment(
                    "replacing",
                    REVIEWER,
                    &first,
                    ReviewTargetRef::Revision {
                        revision_id: first.clone(),
                    },
                    ReviewAssessment::Accepted,
                    &["whole"],
                ),
                assessment(
                    "file",
                    AUTHOR,
                    &legacy,
                    file(&legacy),
                    ReviewAssessment::Accepted,
                    &[],
                ),
                assessment(
                    "range",
                    REVIEWER,
                    &legacy,
                    ReviewTargetRef::Range {
                        revision_id: legacy.clone(),
                        file_path: "src/lib.rs".to_owned(),
                        side: Side::New,
                        start_line: 1,
                        end_line: 2,
                    },
                    ReviewAssessment::NeedsClarification,
                    &[],
                ),
                assessment(
                    "observation",
                    REVIEWER,
                    &second,
                    ReviewTargetRef::Observation {
                        revision_id: second.clone(),
                        observation_id: ObservationId::new(format!(
                            "obs:sha256:{}",
                            "f".repeat(64)
                        )),
                    },
                    ReviewAssessment::AcceptedWithFollowUp,
                    &[],
                ),
                association("first", &first),
            ];
            for event in &events {
                assert_eq!(
                    store
                        .record_event_once(event)
                        .expect("record fixture event"),
                    EventWriteOutcome::Created
                );
            }
            let lifecycle = DerivedAccessLifecycle::new(
                DerivedAccessProfile::SqliteWalBodylessV1,
                temp.path(),
                "store:summary",
            )
            .expect("open lifecycle");
            if build {
                lifecycle
                    .rebuild(|_| LifecycleControl::Continue)
                    .expect("publish current generation");
            }
            let access = DerivedHistoryAccess::from_mode(DerivedHistoryMode::Active {
                lifecycle,
                current: Mutex::new(None),
                store_identity: "store:summary".to_owned(),
                backend,
            });
            let cohort = BackfillCohort::from_event_ids_for_test(
                [backfill_claim.event_id.as_str().to_owned()],
                Some("sha256:manifest".to_owned()),
            );
            Self {
                _temp: temp,
                store,
                access,
                events,
                cohort,
                backfill_claim,
                legacy,
            }
        }

        fn ready(&self) -> DerivedReviewSummaryRead {
            match self
                .access
                .review_summary_inputs(&self.cohort)
                .expect("derived summary read")
            {
                DerivedReviewSummaryRoute::Ready(read) => read,
                DerivedReviewSummaryRoute::Off => panic!("active access routed off"),
                DerivedReviewSummaryRoute::Unavailable(status) => {
                    panic!("current generation unavailable: {status:?}")
                }
            }
        }

        fn authoritative_inputs(&self) -> ReviewSummaryInputs {
            let events = self.store.list_events().expect("list fixture events");
            review_summary_inputs_from_events(&events, &self.cohort).expect("fold fixture events")
        }
    }

    fn sorted(mut inputs: ReviewSummaryInputs) -> ReviewSummaryInputs {
        inputs.memberships.sort_by(|a, b| a.source.cmp(&b.source));
        inputs.withdrawals.sort_by(|a, b| a.source.cmp(&b.source));
        inputs.captures.sort_by(|a, b| a.source.cmp(&b.source));
        inputs.assessments.sort_by(|a, b| a.source.cmp(&b.source));
        inputs
            .commit_associations
            .sort_by(|a, b| a.source.cmp(&b.source));
        inputs
    }

    #[test]
    fn derived_inputs_equal_the_authoritative_fold_over_the_same_events() {
        let fixture = SummaryStore::new(true);
        let read = fixture.ready();
        let authoritative = sorted(fixture.authoritative_inputs());

        assert_eq!(sorted(read.inputs().clone()), authoritative);
        assert_eq!(authoritative.memberships.len(), 3);
        assert_eq!(authoritative.withdrawals.len(), 1);
        assert_eq!(authoritative.captures.len(), 3);
        assert_eq!(authoritative.assessments.len(), 5);
        assert_eq!(authoritative.commit_associations.len(), 1);
        let backfill = read
            .inputs()
            .memberships
            .iter()
            .find(|row| row.source.event_id == fixture.backfill_claim.event_id.as_str())
            .expect("backfill claim row");
        assert!(backfill.backfill);
        assert_eq!(
            read.inputs().manifest_hash.as_deref(),
            Some("sha256:manifest")
        );
        assert_eq!(read.event_count(), fixture.events.len());
        assert!(!read.projection_stamp().is_empty());
    }

    #[test]
    fn scoped_assessments_carry_their_enclosing_revision_in_the_derived_lane() {
        let fixture = SummaryStore::new(true);
        let read = fixture.ready();
        let revision_of = |key: &str| {
            read.inputs()
                .assessments
                .iter()
                .find(|row| row.assessment_id == format!("assess:sha256:{key}"))
                .and_then(|row| row.revision_id.clone())
                .expect("scoped assessment row")
        };

        assert_eq!(revision_of("file"), fixture.legacy.as_str());
        assert_eq!(revision_of("range"), fixture.legacy.as_str());
        assert_eq!(revision_of("observation"), rev("b2").as_str());
    }

    #[test]
    fn a_legacy_instant_capture_orders_identically_in_both_lanes() {
        let fixture = SummaryStore::new(true);
        let read = fixture.ready();
        let legacy = read
            .inputs()
            .captures
            .iter()
            .find(|row| row.revision_id == fixture.legacy.as_str())
            .expect("legacy capture row");

        assert_eq!(legacy.captured_at_millis, Some(1_760_000_000_000));
        assert_eq!(
            compute_review_summary(read.inputs()),
            compute_review_summary(&fixture.authoritative_inputs())
        );
    }

    #[test]
    fn hydrate_counted_returns_exactly_the_requested_envelopes_in_order() {
        let fixture = SummaryStore::new(true);
        let read = fixture.ready();
        let requested = [&fixture.events[5], &fixture.events[1], &fixture.events[12]]
            .map(|event| event.event_id.as_str().to_owned());

        let CountedEnvelopes::Ready(events) =
            read.hydrate_counted(&requested).expect("hydrate counted")
        else {
            panic!("an unmoved snapshot hydrates");
        };

        assert_eq!(
            events
                .iter()
                .map(|event| event.event_id.as_str().to_owned())
                .collect::<Vec<_>>(),
            requested
        );
        assert_eq!(events[0], fixture.events[5]);
    }

    #[test]
    fn a_snapshot_that_moves_before_hydration_is_stale_not_partial() {
        let fixture = SummaryStore::new(true);
        // A governed append does not reliably advance a generation this read
        // holds open (on Windows it does not), so the read is pinned behind the
        // generation's checkpoint instead of racing a writer.
        let read = fixture.ready().pinned_behind_for_test();

        let outcome = read
            .hydrate_counted(&[fixture.events[1].event_id.as_str().to_owned()])
            .expect("hydration reports movement, not an error");

        assert!(matches!(outcome, CountedEnvelopes::Stale));
    }

    #[test]
    fn off_access_routes_off() {
        let access = DerivedHistoryAccess::from_mode(DerivedHistoryMode::Off);

        assert!(matches!(
            access.review_summary_inputs(&BackfillCohort::empty()),
            Ok(DerivedReviewSummaryRoute::Off)
        ));
    }

    #[test]
    fn a_generation_that_is_not_current_is_unavailable() {
        let fixture = SummaryStore::new(false);

        assert!(matches!(
            fixture.access.review_summary_inputs(&fixture.cohort),
            Ok(DerivedReviewSummaryRoute::Unavailable(_))
        ));
    }

    #[test]
    fn the_read_attributes_selection_and_counted_hydration_to_their_phases() {
        let fixture = SummaryStore::new(true);
        fixture.access.current().expect("warm current generation");

        let scope = LongitudinalCountingScopeV1::new("5".repeat(64)).unwrap();
        let guard = scope.enter();
        let read = fixture.ready();
        let ids = compute_review_summary(read.inputs())
            .counted_inputs()
            .iter()
            .map(|input| input.source.event_id.clone())
            .collect::<Vec<_>>();
        read.hydrate_counted(&ids).expect("hydrate counted");
        drop(guard);

        let phases = scope
            .snapshot()
            .derived_access_phases
            .iter()
            .map(|sample| sample.phase)
            .collect::<Vec<_>>();
        assert!(
            phases.contains(&Phase::ReviewSummarySqlSelection),
            "{phases:?}"
        );
        assert!(
            phases.contains(&Phase::ReviewSummaryCountedCarrierHydrationValidation),
            "{phases:?}"
        );
    }
}
