//! The review summary model: lane-neutral input rows folded into the review-round
//! profile, the record-internal measures, and the exact set of counted inputs.
//!
//! This is the one place the population and measure definitions are written.
//! Both read lanes fill [`ReviewSummaryInputs`] with plain string ids and hand it
//! to [`compute_review_summary`]; neither lane re-implements a rule. The function
//! is pure: no store, no clock, no trust set, and no floating point. Every figure
//! is a count with its denominator, and a measure with nothing to count is
//! `Unavailable`, never zero.

use std::collections::{BTreeMap, BTreeSet};

use crate::session::event::ReviewAssessment;

/// Reason a measure is unavailable: the population holds no counted Change.
pub(crate) const NO_COUNTED_CHANGES: &str = "noCountedChanges";
/// Reason first-capture acceptance is unavailable: no counted Change's first
/// capture carries a live verdict.
pub(crate) const NO_ASSESSED_FIRST_CAPTURES: &str = "noAssessedFirstCaptures";

/// The event a row was read from: its id and the hash of its payload.
#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub(crate) struct CountedEventRef {
    pub(crate) event_id: String,
    pub(crate) payload_hash: String,
}

/// One membership claim placing a Revision in a Change.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct MembershipClaimInput {
    pub(crate) source: CountedEventRef,
    pub(crate) claim_id: String,
    pub(crate) change_id: String,
    pub(crate) revision_id: String,
    /// The claim event is migration backfill named by the store's activation
    /// manifest.
    pub(crate) backfill: bool,
}

/// One withdrawal of a membership claim.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct MembershipWithdrawalInput {
    pub(crate) source: CountedEventRef,
    pub(crate) claim_id: String,
}

/// One capture proposing a Revision.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct CaptureInput {
    pub(crate) source: CountedEventRef,
    pub(crate) revision_id: String,
    /// Only compared against assessment actors; never leaves the model.
    pub(crate) actor: String,
    /// The capture instant as epoch milliseconds, when the event's timestamp
    /// parses.
    pub(crate) captured_at_millis: Option<i64>,
}

/// One recorded assessment.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct AssessmentInput {
    pub(crate) source: CountedEventRef,
    pub(crate) assessment_id: String,
    /// The Revision enclosing the assessment's target, whatever its scope.
    pub(crate) revision_id: Option<String>,
    /// Only compared against the capture actor; never leaves the model.
    pub(crate) actor: String,
    pub(crate) verdict: ReviewAssessment,
    pub(crate) replaces: Vec<String>,
}

/// One commit association recorded on a Revision.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct CommitAssociationInput {
    pub(crate) source: CountedEventRef,
    pub(crate) revision_id: String,
}

/// Everything the summary reads, in lane-neutral rows.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct ReviewSummaryInputs {
    pub(crate) memberships: Vec<MembershipClaimInput>,
    pub(crate) withdrawals: Vec<MembershipWithdrawalInput>,
    pub(crate) captures: Vec<CaptureInput>,
    pub(crate) assessments: Vec<AssessmentInput>,
    pub(crate) commit_associations: Vec<CommitAssociationInput>,
    /// The store's activation manifest hash, when the store has one.
    pub(crate) manifest_hash: Option<String>,
}

/// A measure that is either computed from counted facts or unavailable with the
/// reasons it could not be computed.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Measure<T> {
    Computed(T),
    Unavailable { reasons: Vec<&'static str> },
}

impl<T> Measure<T> {
    fn unavailable(reason: &'static str) -> Self {
        Self::Unavailable {
            reasons: vec![reason],
        }
    }
}

/// A count observed both over current membership and over every membership
/// ever claimed.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct CurrentAndHistorical {
    pub current: u64,
    pub historical: u64,
}

/// The earliest and latest parsed capture instants among counted captures.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ObservedWindow {
    pub from_millis: i64,
    pub to_millis: i64,
}

/// Changes excluded because every membership claim they carry is migration
/// backfill named by the store's activation manifest.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct BackfillExclusion {
    /// The activation manifest the exclusion was read from; `None` when the
    /// store has no activation manifest, in which case nothing is excluded.
    pub manifest_hash: Option<String>,
    pub changes: u64,
    pub memberships: u64,
}

/// Who and what the summary counted.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Population {
    /// Changes with at least one membership claim.
    pub changes_seen: u64,
    /// Review Changes with at least one current membership.
    pub changes_counted: u64,
    /// Membership claims of counted Changes.
    pub memberships: CurrentAndHistorical,
    /// Distinct Revisions across the memberships of counted Changes.
    pub captured_revisions: CurrentAndHistorical,
    pub observed_window: Option<ObservedWindow>,
    pub migration_backfill: BackfillExclusion,
    /// Review Changes whose every membership has been withdrawn.
    pub no_current_members: u64,
}

/// How many counted Changes have a given number of current-member Revisions.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RoundCount {
    pub rounds: u64,
    pub changes: u64,
}

/// The review-round distribution over counted Changes, ascending by rounds.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RoundProfile {
    pub of: u64,
    pub distribution: Vec<RoundCount>,
}

/// One result per counted Change, read from its first-captured Revision.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct FirstCaptureResults {
    pub of: u64,
    pub accepted: u64,
    pub needs_changes: u64,
    pub other_verdict: u64,
    pub unassessed: u64,
}

/// A count over its denominator.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Count {
    pub count: u64,
    pub of: u64,
}

/// Per captured Revision: whether some assessment was recorded under a
/// different actor id than the capture's. Actor ids are asserted, not verified.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ActorRelation {
    pub of: u64,
    pub actor_distinct: u64,
    pub actor_same: u64,
    pub undetermined: u64,
}

/// The kind of fact a counted input is.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub(crate) enum CountedInputKind {
    MembershipClaim,
    MembershipWithdrawal,
    Capture,
    Assessment,
    CommitAssociation,
}

/// One event the measures consumed.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct CountedInput {
    pub(crate) source: CountedEventRef,
    pub(crate) kind: CountedInputKind,
}

/// The computed summary over the review-Change population.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReviewSummary {
    pub population: Population,
    pub profile: Measure<RoundProfile>,
    pub first_capture: Measure<FirstCaptureResults>,
    /// Captured Revisions with at least one assessment, replaced or not.
    pub assessed_capture: Measure<Count>,
    /// Captured Revisions with at least one commit association, withdrawn or not.
    pub commit_associated_capture: Measure<Count>,
    /// Accepted first captures over first captures carrying a live verdict.
    pub first_capture_acceptance: Measure<Count>,
    pub actor_relation: Measure<ActorRelation>,
    counted: Vec<CountedInput>,
}

impl ReviewSummary {
    /// The events the measures consumed, sorted by event id, one entry per event.
    pub(crate) fn counted_inputs(&self) -> &[CountedInput] {
        &self.counted
    }
}

/// Fold lane-neutral input rows into the summary and its counted inputs.
///
/// Population: a Change is *seen* when it has a membership claim, and is a
/// *review* Change when at least one of its claims is not migration backfill.
/// A review Change is *counted* when at least one of its claims is current (no
/// withdrawal names it); the rest are reported as having no current members.
/// Every membership of a counted Change counts, backfill-written or not.
pub(crate) fn compute_review_summary(inputs: &ReviewSummaryInputs) -> ReviewSummary {
    let withdrawn: BTreeSet<&str> = inputs
        .withdrawals
        .iter()
        .map(|withdrawal| withdrawal.claim_id.as_str())
        .collect();

    let mut claims_by_change: BTreeMap<&str, Vec<&MembershipClaimInput>> = BTreeMap::new();
    for claim in &inputs.memberships {
        claims_by_change
            .entry(claim.change_id.as_str())
            .or_default()
            .push(claim);
    }

    let mut population = Population {
        changes_seen: len_u64(claims_by_change.len()),
        migration_backfill: BackfillExclusion {
            manifest_hash: inputs.manifest_hash.clone(),
            ..BackfillExclusion::default()
        },
        ..Population::default()
    };
    let mut counted_claims: Vec<&MembershipClaimInput> = Vec::new();
    // Per counted Change, its distinct current-member Revisions.
    let mut counted_changes: Vec<BTreeSet<&str>> = Vec::new();
    let mut captured: BTreeSet<&str> = BTreeSet::new();
    let mut captured_historical: BTreeSet<&str> = BTreeSet::new();
    for claims in claims_by_change.values() {
        if claims.iter().all(|claim| claim.backfill) {
            population.migration_backfill.changes += 1;
            population.migration_backfill.memberships += len_u64(claims.len());
            continue;
        }
        let current: BTreeSet<&str> = claims
            .iter()
            .filter(|claim| !withdrawn.contains(claim.claim_id.as_str()))
            .map(|claim| claim.revision_id.as_str())
            .collect();
        if current.is_empty() {
            population.no_current_members += 1;
            continue;
        }
        population.memberships.current += len_u64(
            claims
                .iter()
                .filter(|claim| !withdrawn.contains(claim.claim_id.as_str()))
                .count(),
        );
        population.memberships.historical += len_u64(claims.len());
        captured.extend(current.iter().copied());
        captured_historical.extend(claims.iter().map(|claim| claim.revision_id.as_str()));
        counted_claims.extend(claims.iter().copied());
        counted_changes.push(current);
    }
    population.changes_counted = len_u64(counted_changes.len());
    population.captured_revisions = CurrentAndHistorical {
        current: len_u64(captured.len()),
        historical: len_u64(captured_historical.len()),
    };

    // A Revision's capture instant and actor come from its earliest capture
    // event: smallest instant (missing sorts as zero), then smallest event id.
    let mut representative: BTreeMap<&str, &CaptureInput> = BTreeMap::new();
    for capture in &inputs.captures {
        let slot = representative
            .entry(capture.revision_id.as_str())
            .or_insert(capture);
        if capture_order(capture) < capture_order(slot) {
            *slot = capture;
        }
    }
    let counted_captures: Vec<&CaptureInput> = inputs
        .captures
        .iter()
        .filter(|capture| captured.contains(capture.revision_id.as_str()))
        .collect();
    population.observed_window = observed_window(&counted_captures);

    let replaced: BTreeSet<&str> = inputs
        .assessments
        .iter()
        .flat_map(|assessment| assessment.replaces.iter().map(String::as_str))
        .collect();
    let mut assessments_by_revision: BTreeMap<&str, Vec<&AssessmentInput>> = BTreeMap::new();
    for assessment in &inputs.assessments {
        if let Some(revision_id) = assessment.revision_id.as_deref() {
            assessments_by_revision
                .entry(revision_id)
                .or_default()
                .push(assessment);
        }
    }
    let associated: BTreeSet<&str> = inputs
        .commit_associations
        .iter()
        .map(|association| association.revision_id.as_str())
        .collect();

    let counted = counted_inputs(
        inputs,
        &counted_claims,
        &counted_captures,
        &captured,
        &assessments_by_revision,
    );

    if counted_changes.is_empty() {
        return ReviewSummary {
            population,
            profile: Measure::unavailable(NO_COUNTED_CHANGES),
            first_capture: Measure::unavailable(NO_COUNTED_CHANGES),
            assessed_capture: Measure::unavailable(NO_COUNTED_CHANGES),
            commit_associated_capture: Measure::unavailable(NO_COUNTED_CHANGES),
            first_capture_acceptance: Measure::unavailable(NO_COUNTED_CHANGES),
            actor_relation: Measure::unavailable(NO_COUNTED_CHANGES),
            counted,
        };
    }

    let mut rounds: BTreeMap<u64, u64> = BTreeMap::new();
    for revisions in &counted_changes {
        *rounds.entry(len_u64(revisions.len())).or_default() += 1;
    }
    let profile = RoundProfile {
        of: population.changes_counted,
        distribution: rounds
            .into_iter()
            .map(|(rounds, changes)| RoundCount { rounds, changes })
            .collect(),
    };

    let mut first_capture = FirstCaptureResults {
        of: population.changes_counted,
        ..FirstCaptureResults::default()
    };
    for revisions in &counted_changes {
        let first = revisions
            .iter()
            .min_by_key(|revision| {
                let instant = representative
                    .get(**revision)
                    .and_then(|capture| capture.captured_at_millis)
                    .unwrap_or(0);
                (instant, **revision)
            })
            .expect("a counted Change has a current member");
        let live: Vec<ReviewAssessment> = assessments_by_revision
            .get(first)
            .into_iter()
            .flatten()
            .filter(|assessment| !replaced.contains(assessment.assessment_id.as_str()))
            .map(|assessment| assessment.verdict)
            .collect();
        if live.is_empty() {
            first_capture.unassessed += 1;
        } else if live.contains(&ReviewAssessment::NeedsChanges) {
            first_capture.needs_changes += 1;
        } else if live.iter().any(|verdict| {
            matches!(
                verdict,
                ReviewAssessment::Accepted | ReviewAssessment::AcceptedWithFollowUp
            )
        }) {
            first_capture.accepted += 1;
        } else {
            first_capture.other_verdict += 1;
        }
    }
    let assessed_first_captures =
        first_capture.accepted + first_capture.needs_changes + first_capture.other_verdict;
    let first_capture_acceptance = if assessed_first_captures == 0 {
        Measure::unavailable(NO_ASSESSED_FIRST_CAPTURES)
    } else {
        Measure::Computed(Count {
            count: first_capture.accepted,
            of: assessed_first_captures,
        })
    };

    let of = population.captured_revisions.current;
    let assessed_capture = Count {
        count: len_u64(
            captured
                .iter()
                .filter(|revision| assessments_by_revision.contains_key(**revision))
                .count(),
        ),
        of,
    };
    let commit_associated_capture = Count {
        count: len_u64(
            captured
                .iter()
                .filter(|revision| associated.contains(**revision))
                .count(),
        ),
        of,
    };

    let mut actor_relation = ActorRelation {
        of,
        ..ActorRelation::default()
    };
    for revision in &captured {
        match (
            representative.get(revision),
            assessments_by_revision.get(revision),
        ) {
            (Some(capture), Some(assessments)) => {
                if assessments
                    .iter()
                    .any(|assessment| assessment.actor != capture.actor)
                {
                    actor_relation.actor_distinct += 1;
                } else {
                    actor_relation.actor_same += 1;
                }
            }
            _ => actor_relation.undetermined += 1,
        }
    }

    ReviewSummary {
        population,
        profile: Measure::Computed(profile),
        first_capture: Measure::Computed(first_capture),
        assessed_capture: Measure::Computed(assessed_capture),
        commit_associated_capture: Measure::Computed(commit_associated_capture),
        first_capture_acceptance,
        actor_relation: Measure::Computed(actor_relation),
        counted,
    }
}

/// The events the measures consumed for the counted population: every
/// membership claim of a counted Change and every withdrawal naming one; every
/// capture of a captured Revision; every assessment on a captured Revision and
/// every assessment replacing one of those; every commit association on a
/// captured Revision. Sorted by event id, one entry per event.
fn counted_inputs(
    inputs: &ReviewSummaryInputs,
    counted_claims: &[&MembershipClaimInput],
    counted_captures: &[&CaptureInput],
    captured: &BTreeSet<&str>,
    assessments_by_revision: &BTreeMap<&str, Vec<&AssessmentInput>>,
) -> Vec<CountedInput> {
    let mut counted: BTreeMap<String, CountedInput> = BTreeMap::new();
    let mut add = |source: &CountedEventRef, kind: CountedInputKind| {
        counted
            .entry(source.event_id.clone())
            .or_insert_with(|| CountedInput {
                source: source.clone(),
                kind,
            });
    };

    let claim_ids: BTreeSet<&str> = counted_claims
        .iter()
        .map(|claim| claim.claim_id.as_str())
        .collect();
    for claim in counted_claims {
        add(&claim.source, CountedInputKind::MembershipClaim);
    }
    for withdrawal in &inputs.withdrawals {
        if claim_ids.contains(withdrawal.claim_id.as_str()) {
            add(&withdrawal.source, CountedInputKind::MembershipWithdrawal);
        }
    }
    for capture in counted_captures {
        add(&capture.source, CountedInputKind::Capture);
    }
    let mut assessment_ids: BTreeSet<&str> = BTreeSet::new();
    for revision in captured {
        for assessment in assessments_by_revision.get(revision).into_iter().flatten() {
            assessment_ids.insert(assessment.assessment_id.as_str());
            add(&assessment.source, CountedInputKind::Assessment);
        }
    }
    for assessment in &inputs.assessments {
        if assessment
            .replaces
            .iter()
            .any(|replaced| assessment_ids.contains(replaced.as_str()))
        {
            add(&assessment.source, CountedInputKind::Assessment);
        }
    }
    for association in &inputs.commit_associations {
        if captured.contains(association.revision_id.as_str()) {
            add(&association.source, CountedInputKind::CommitAssociation);
        }
    }
    counted.into_values().collect()
}

fn capture_order(capture: &CaptureInput) -> (i64, &str) {
    (
        capture.captured_at_millis.unwrap_or(0),
        capture.source.event_id.as_str(),
    )
}

fn observed_window(captures: &[&CaptureInput]) -> Option<ObservedWindow> {
    let mut instants = captures
        .iter()
        .filter_map(|capture| capture.captured_at_millis);
    let first = instants.next()?;
    let (from_millis, to_millis) = instants.fold((first, first), |(low, high), instant| {
        (low.min(instant), high.max(instant))
    });
    Some(ObservedWindow {
        from_millis,
        to_millis,
    })
}

fn len_u64(len: usize) -> u64 {
    u64::try_from(len).unwrap_or(u64::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Builds inputs with generated event ids so each case states only the facts
    /// it turns on.
    #[derive(Default)]
    struct Fixture {
        inputs: ReviewSummaryInputs,
        next: usize,
    }

    fn fixture() -> Fixture {
        Fixture::default()
    }

    impl Fixture {
        fn source(&mut self, prefix: &str) -> CountedEventRef {
            self.next += 1;
            CountedEventRef {
                event_id: format!("evt:{prefix}{:03}", self.next),
                payload_hash: format!("sha256:{prefix}{:03}", self.next),
            }
        }

        /// A membership claim; returns its claim id.
        fn member(&mut self, change: &str, revision: &str) -> String {
            self.claim(change, revision, false)
        }

        fn backfill_member(&mut self, change: &str, revision: &str) -> String {
            self.claim(change, revision, true)
        }

        fn claim(&mut self, change: &str, revision: &str, backfill: bool) -> String {
            let source = self.source("m");
            let claim_id = format!("claim:{}", source.event_id);
            self.inputs.memberships.push(MembershipClaimInput {
                source,
                claim_id: claim_id.clone(),
                change_id: change.to_owned(),
                revision_id: revision.to_owned(),
                backfill,
            });
            claim_id
        }

        fn withdraw(&mut self, claim_id: &str) -> String {
            let source = self.source("w");
            let event_id = source.event_id.clone();
            self.inputs.withdrawals.push(MembershipWithdrawalInput {
                source,
                claim_id: claim_id.to_owned(),
            });
            event_id
        }

        fn capture(&mut self, revision: &str, actor: &str, at: Option<i64>) -> String {
            let source = self.source("c");
            let event_id = source.event_id.clone();
            self.inputs.captures.push(CaptureInput {
                source,
                revision_id: revision.to_owned(),
                actor: actor.to_owned(),
                captured_at_millis: at,
            });
            event_id
        }

        /// An assessment; returns its assessment id (the event id is derivable
        /// through [`Fixture::event_of`]).
        fn assess(
            &mut self,
            revision: &str,
            actor: &str,
            verdict: ReviewAssessment,
            replaces: &[&str],
        ) -> String {
            let source = self.source("a");
            let assessment_id = format!("assessment:{}", source.event_id);
            self.inputs.assessments.push(AssessmentInput {
                source,
                assessment_id: assessment_id.clone(),
                revision_id: Some(revision.to_owned()),
                actor: actor.to_owned(),
                verdict,
                replaces: replaces.iter().map(|id| (*id).to_owned()).collect(),
            });
            assessment_id
        }

        fn associate(&mut self, revision: &str) -> String {
            let source = self.source("x");
            let event_id = source.event_id.clone();
            self.inputs
                .commit_associations
                .push(CommitAssociationInput {
                    source,
                    revision_id: revision.to_owned(),
                });
            event_id
        }

        fn event_of(&self, id: &str) -> String {
            id.split_once(':')
                .map(|(_, event)| event.to_owned())
                .expect("fixture ids embed their event id")
        }

        fn summary(&self) -> ReviewSummary {
            compute_review_summary(&self.inputs)
        }
    }

    fn counted_ids(summary: &ReviewSummary) -> Vec<(String, CountedInputKind)> {
        summary
            .counted_inputs()
            .iter()
            .map(|input| (input.source.event_id.clone(), input.kind))
            .collect()
    }

    fn sorted(mut ids: Vec<(String, CountedInputKind)>) -> Vec<(String, CountedInputKind)> {
        ids.sort();
        ids
    }

    fn computed<T: Clone + std::fmt::Debug>(measure: &Measure<T>) -> T {
        match measure {
            Measure::Computed(value) => value.clone(),
            Measure::Unavailable { reasons } => panic!("expected computed, got {reasons:?}"),
        }
    }

    fn distribution(summary: &ReviewSummary) -> Vec<(u64, u64)> {
        computed(&summary.profile)
            .distribution
            .iter()
            .map(|row| (row.rounds, row.changes))
            .collect()
    }

    use CountedInputKind::{
        Assessment, Capture, CommitAssociation, MembershipClaim, MembershipWithdrawal,
    };
    use ReviewAssessment::{Accepted, AcceptedWithFollowUp, NeedsChanges, NeedsClarification};

    #[test]
    fn backfill_only_change_is_excluded_and_sized() {
        let mut f = fixture();
        f.inputs.manifest_hash = Some("sha256:manifest".to_owned());
        f.backfill_member("c-old", "r-old-1");
        f.backfill_member("c-old", "r-old-2");
        f.member("c-new", "r-new");
        f.capture("r-new", "actor:a", Some(10));

        let summary = f.summary();

        assert_eq!(summary.population.changes_seen, 2);
        assert_eq!(summary.population.changes_counted, 1);
        assert_eq!(
            summary.population.migration_backfill,
            BackfillExclusion {
                manifest_hash: Some("sha256:manifest".to_owned()),
                changes: 1,
                memberships: 2,
            }
        );
        assert_eq!(distribution(&summary), vec![(1, 1)]);
    }

    #[test]
    fn mixed_change_is_a_review_change_and_counts_its_backfill_membership() {
        let mut f = fixture();
        let old_claim = f.backfill_member("c1", "r1");
        let new_claim = f.member("c1", "r2");
        let capture_1 = f.capture("r1", "actor:a", Some(10));
        let capture_2 = f.capture("r2", "actor:a", Some(20));

        let summary = f.summary();

        assert_eq!(summary.population.changes_counted, 1);
        assert_eq!(summary.population.migration_backfill.changes, 0);
        assert_eq!(summary.population.migration_backfill.memberships, 0);
        assert_eq!(distribution(&summary), vec![(2, 1)]);
        assert_eq!(
            counted_ids(&summary),
            sorted(vec![
                (f.event_of(&old_claim), MembershipClaim),
                (f.event_of(&new_claim), MembershipClaim),
                (capture_1, Capture),
                (capture_2, Capture),
            ])
        );
    }

    #[test]
    fn withdrawn_membership_is_historical_but_not_current() {
        let mut f = fixture();
        f.member("c1", "r1");
        let withdrawn = f.member("c1", "r2");
        f.withdraw(&withdrawn);

        let summary = f.summary();

        assert_eq!(
            summary.population.memberships,
            CurrentAndHistorical {
                current: 1,
                historical: 2
            }
        );
        assert_eq!(
            summary.population.captured_revisions,
            CurrentAndHistorical {
                current: 1,
                historical: 2
            }
        );
        assert_eq!(distribution(&summary), vec![(1, 1)]);
    }

    #[test]
    fn review_change_with_only_withdrawn_membership_is_excluded_as_no_current_members() {
        let mut f = fixture();
        f.member("c-live", "r1");
        let claim = f.member("c-gone", "r2");
        let withdrawal = f.withdraw(&claim);

        let summary = f.summary();

        assert_eq!(summary.population.changes_seen, 2);
        assert_eq!(summary.population.changes_counted, 1);
        assert_eq!(summary.population.no_current_members, 1);
        assert_eq!(summary.population.migration_backfill.changes, 0);
        assert!(
            !counted_ids(&summary)
                .iter()
                .any(|(id, _)| *id == withdrawal || *id == f.event_of(&claim)),
            "an excluded Change contributes no counted input"
        );
    }

    #[test]
    fn revision_in_two_counted_changes_counts_once_as_captured_and_once_per_change() {
        let mut f = fixture();
        f.member("c1", "r-shared");
        f.member("c2", "r-shared");
        f.member("c2", "r-other");
        let capture = f.capture("r-shared", "actor:a", Some(10));

        let summary = f.summary();

        assert_eq!(summary.population.captured_revisions.current, 2);
        assert_eq!(summary.population.memberships.current, 3);
        assert_eq!(distribution(&summary), vec![(1, 1), (2, 1)]);
        assert_eq!(
            counted_ids(&summary)
                .iter()
                .filter(|(id, _)| *id == capture)
                .count(),
            1
        );
    }

    #[test]
    fn first_capture_orders_by_instant_then_revision_id_with_missing_as_zero() {
        // Input order puts the later capture first; the instant decides.
        let mut by_instant = fixture();
        by_instant.member("c1", "r-a");
        by_instant.member("c1", "r-b");
        by_instant.capture("r-a", "actor:a", Some(200));
        by_instant.capture("r-b", "actor:a", Some(100));
        by_instant.assess("r-b", "actor:r", Accepted, &[]);
        by_instant.assess("r-a", "actor:r", NeedsChanges, &[]);
        assert_eq!(
            computed(&by_instant.summary().first_capture).accepted,
            1,
            "r-b was captured first"
        );

        // Equal instants break on Revision id ascending.
        let mut tie = fixture();
        tie.member("c1", "r-b");
        tie.member("c1", "r-a");
        tie.capture("r-b", "actor:a", Some(100));
        tie.capture("r-a", "actor:a", Some(100));
        tie.assess("r-a", "actor:r", NeedsChanges, &[]);
        tie.assess("r-b", "actor:r", Accepted, &[]);
        assert_eq!(computed(&tie.summary().first_capture).needs_changes, 1);

        // A missing instant sorts as zero, ahead of any parsed instant.
        let mut missing = fixture();
        missing.member("c1", "r-a");
        missing.member("c1", "r-z");
        missing.capture("r-a", "actor:a", Some(5));
        missing.capture("r-z", "actor:a", None);
        missing.assess("r-z", "actor:r", Accepted, &[]);
        assert_eq!(computed(&missing.summary().first_capture).accepted, 1);
    }

    #[test]
    fn earliest_capture_event_decides_a_revisions_instant_and_actor() {
        let mut f = fixture();
        f.member("c1", "r1");
        f.member("c1", "r2");
        // r1 carries two capture events; the earlier one (by instant) decides
        // both its instant and its actor.
        let late = f.capture("r1", "actor:reviewer", Some(300));
        let early = f.capture("r1", "actor:author", Some(50));
        let other = f.capture("r2", "actor:author", Some(100));
        f.assess("r1", "actor:reviewer", NeedsChanges, &[]);
        f.assess("r2", "actor:reviewer", Accepted, &[]);

        let summary = f.summary();

        // r1 at 50 precedes r2 at 100, so the first capture needs changes.
        assert_eq!(computed(&summary.first_capture).needs_changes, 1);
        // The representative actor is the author, so r1 is actor-distinct.
        assert_eq!(computed(&summary.actor_relation).actor_distinct, 2);
        let captures: Vec<_> = counted_ids(&summary)
            .into_iter()
            .filter(|(_, kind)| *kind == Capture)
            .map(|(id, _)| id)
            .collect();
        let mut expected = vec![late, early, other];
        expected.sort();
        assert_eq!(captures, expected, "every capture event is a counted input");

        // Equal instants break on the smaller event id.
        let mut tie = fixture();
        tie.member("c1", "r1");
        tie.capture("r1", "actor:first", Some(10));
        tie.capture("r1", "actor:second", Some(10));
        tie.assess("r1", "actor:first", Accepted, &[]);
        assert_eq!(computed(&tie.summary().actor_relation).actor_same, 1);
    }

    #[test]
    fn first_capture_result_precedence() {
        fn result_of(build: impl FnOnce(&mut Fixture)) -> FirstCaptureResults {
            let mut f = fixture();
            f.member("c1", "r1");
            f.capture("r1", "actor:a", Some(1));
            build(&mut f);
            computed(&f.summary().first_capture)
        }

        let both = result_of(|f| {
            f.assess("r1", "actor:r", Accepted, &[]);
            f.assess("r1", "actor:q", NeedsChanges, &[]);
        });
        assert_eq!((both.needs_changes, both.accepted), (1, 0));

        let replaced_needs_changes = result_of(|f| {
            let first = f.assess("r1", "actor:r", NeedsChanges, &[]);
            f.assess("r1", "actor:r", Accepted, &[&first]);
        });
        assert_eq!(
            (
                replaced_needs_changes.accepted,
                replaced_needs_changes.needs_changes
            ),
            (1, 0)
        );

        let follow_up = result_of(|f| {
            f.assess("r1", "actor:r", AcceptedWithFollowUp, &[]);
        });
        assert_eq!(follow_up.accepted, 1);

        let clarification = result_of(|f| {
            f.assess("r1", "actor:r", NeedsClarification, &[]);
        });
        assert_eq!(clarification.other_verdict, 1);

        let only_replaced = result_of(|f| {
            let first = f.assess("r1", "actor:r", Accepted, &[]);
            // The replacing verdict lands on another Revision, so r1 has no live
            // verdict of its own.
            f.assess("r-elsewhere", "actor:r", NeedsChanges, &[&first]);
        });
        assert_eq!(only_replaced.unassessed, 1);
        assert_eq!(only_replaced.of, 1);
    }

    #[test]
    fn replacing_assessment_on_another_revision_retires_the_verdict_and_is_counted() {
        let mut f = fixture();
        f.member("c1", "r1");
        let claim_capture = f.capture("r1", "actor:a", Some(1));
        let replaced = f.assess("r1", "actor:r", NeedsChanges, &[]);
        let replacing = f.assess("r-outside", "actor:r", Accepted, &[&replaced]);
        f.assess("r-outside", "actor:r", Accepted, &[]);

        let summary = f.summary();

        assert_eq!(computed(&summary.first_capture).unassessed, 1);
        let assessments: Vec<_> = counted_ids(&summary)
            .into_iter()
            .filter(|(_, kind)| *kind == Assessment)
            .map(|(id, _)| id)
            .collect();
        let mut expected = vec![f.event_of(&replaced), f.event_of(&replacing)];
        expected.sort();
        assert_eq!(
            assessments, expected,
            "the replacing assessment is counted; an unrelated one is not"
        );
        assert!(
            counted_ids(&summary)
                .iter()
                .any(|(id, kind)| *id == claim_capture && *kind == Capture)
        );
    }

    #[test]
    fn assessed_capture_share_counts_a_revision_whose_only_assessment_is_replaced() {
        let mut f = fixture();
        f.member("c1", "r1");
        f.member("c1", "r2");
        let replaced = f.assess("r1", "actor:r", NeedsChanges, &[]);
        f.assess("r-outside", "actor:r", Accepted, &[&replaced]);

        let summary = f.summary();

        assert_eq!(
            computed(&summary.assessed_capture),
            Count { count: 1, of: 2 }
        );
    }

    #[test]
    fn actor_relation_is_three_valued() {
        let mut f = fixture();
        f.member("c1", "r-none");
        f.member("c1", "r-nocapture");
        f.member("c1", "r-distinct");
        f.member("c1", "r-same");
        f.capture("r-none", "actor:author", Some(1));
        f.capture("r-distinct", "actor:author", Some(2));
        f.capture("r-same", "actor:author", Some(3));
        f.assess("r-nocapture", "actor:reviewer", Accepted, &[]);
        f.assess("r-distinct", "actor:author", NeedsChanges, &[]);
        f.assess("r-distinct", "actor:reviewer", Accepted, &[]);
        f.assess("r-same", "actor:author", Accepted, &[]);
        f.assess("r-same", "actor:author", Accepted, &[]);

        assert_eq!(
            computed(&f.summary().actor_relation),
            ActorRelation {
                of: 4,
                actor_distinct: 1,
                actor_same: 1,
                undetermined: 2,
            }
        );
    }

    #[test]
    fn empty_population_makes_every_measure_unavailable() {
        let unavailable = |summary: &ReviewSummary| {
            let expected = || vec![NO_COUNTED_CHANGES];
            assert_eq!(
                summary.profile,
                Measure::Unavailable {
                    reasons: expected()
                }
            );
            assert_eq!(
                summary.first_capture,
                Measure::Unavailable {
                    reasons: expected()
                }
            );
            assert_eq!(
                summary.assessed_capture,
                Measure::Unavailable {
                    reasons: expected()
                }
            );
            assert_eq!(
                summary.commit_associated_capture,
                Measure::Unavailable {
                    reasons: expected()
                }
            );
            assert_eq!(
                summary.first_capture_acceptance,
                Measure::Unavailable {
                    reasons: expected()
                }
            );
            assert_eq!(
                summary.actor_relation,
                Measure::Unavailable {
                    reasons: expected()
                }
            );
            assert!(summary.counted_inputs().is_empty());
            assert_eq!(summary.population.changes_counted, 0);
            assert_eq!(
                summary.population.memberships,
                CurrentAndHistorical::default()
            );
            assert_eq!(summary.population.observed_window, None);
        };

        let empty = compute_review_summary(&ReviewSummaryInputs::default());
        unavailable(&empty);
        assert_eq!(empty.population, Population::default());

        let mut backfill_only = fixture();
        backfill_only.inputs.manifest_hash = Some("sha256:manifest".to_owned());
        backfill_only.backfill_member("c-old", "r-old");
        backfill_only.capture("r-old", "actor:a", Some(1));
        backfill_only.assess("r-old", "actor:r", Accepted, &[]);
        let summary = backfill_only.summary();
        unavailable(&summary);
        assert_eq!(summary.population.changes_seen, 1);
        assert_eq!(summary.population.migration_backfill.changes, 1);
    }

    #[test]
    fn first_capture_acceptance_is_over_assessed_first_captures() {
        let mut f = fixture();
        for (change, verdict) in [
            ("c1", Some(Accepted)),
            ("c2", Some(NeedsChanges)),
            ("c3", Some(NeedsClarification)),
            ("c4", None),
        ] {
            let revision = format!("r-{change}");
            f.member(change, &revision);
            f.capture(&revision, "actor:a", Some(1));
            if let Some(verdict) = verdict {
                f.assess(&revision, "actor:r", verdict, &[]);
            }
        }
        let summary = f.summary();
        assert_eq!(
            computed(&summary.first_capture_acceptance),
            Count { count: 1, of: 3 }
        );
        assert_eq!(
            computed(&summary.first_capture),
            FirstCaptureResults {
                of: 4,
                accepted: 1,
                needs_changes: 1,
                other_verdict: 1,
                unassessed: 1,
            }
        );

        let mut unassessed = fixture();
        unassessed.member("c1", "r1");
        unassessed.capture("r1", "actor:a", Some(1));
        assert_eq!(
            unassessed.summary().first_capture_acceptance,
            Measure::Unavailable {
                reasons: vec![NO_ASSESSED_FIRST_CAPTURES]
            }
        );
    }

    #[test]
    fn commit_associated_capture_and_counted_set_are_exact() {
        let mut f = fixture();
        let claim = f.member("c1", "r1");
        f.member("c1", "r2");
        let withdrawn = f.member("c1", "r-withdrawn");
        let withdrawal = f.withdraw(&withdrawn);
        let association = f.associate("r1");
        f.associate("r-withdrawn");
        f.associate("r-outside");

        let summary = f.summary();

        assert_eq!(
            computed(&summary.commit_associated_capture),
            Count { count: 1, of: 2 }
        );
        let ids = counted_ids(&summary);
        assert!(ids.contains(&(association, CommitAssociation)));
        assert!(ids.contains(&(withdrawal, MembershipWithdrawal)));
        assert!(ids.contains(&(f.event_of(&claim), MembershipClaim)));
        assert!(ids.contains(&(f.event_of(&withdrawn), MembershipClaim)));
        assert_eq!(
            ids.iter()
                .filter(|(_, kind)| *kind == CommitAssociation)
                .count(),
            1,
            "associations on a withdrawn or outside Revision are not counted"
        );
    }

    #[test]
    fn observed_window_spans_counted_captures_with_a_parsed_instant() {
        let mut f = fixture();
        f.member("c1", "r1");
        f.member("c1", "r2");
        f.member("c1", "r3");
        f.capture("r1", "actor:a", Some(40));
        f.capture("r2", "actor:a", Some(10));
        f.capture("r3", "actor:a", None);
        f.capture("r-outside", "actor:a", Some(1));

        assert_eq!(
            f.summary().population.observed_window,
            Some(ObservedWindow {
                from_millis: 10,
                to_millis: 40
            })
        );
    }

    #[test]
    fn summary_does_not_depend_on_input_order() {
        let mut f = fixture();
        f.inputs.manifest_hash = Some("sha256:manifest".to_owned());
        f.backfill_member("c-old", "r-old");
        let old = f.backfill_member("c1", "r1");
        f.member("c1", "r2");
        f.member("c2", "r3");
        let gone = f.member("c2", "r4");
        f.withdraw(&gone);
        f.capture("r1", "actor:a", Some(30));
        f.capture("r2", "actor:a", Some(20));
        f.capture("r2", "actor:b", Some(20));
        f.capture("r3", "actor:a", None);
        let first = f.assess("r2", "actor:r", NeedsChanges, &[]);
        f.assess("r2", "actor:r", Accepted, &[&first]);
        f.assess("r3", "actor:a", NeedsClarification, &[]);
        f.associate("r1");
        let _ = old;

        let forward = f.summary();
        let mut reversed = f.inputs.clone();
        reversed.memberships.reverse();
        reversed.withdrawals.reverse();
        reversed.captures.reverse();
        reversed.assessments.reverse();
        reversed.commit_associations.reverse();
        let mut rotated = f.inputs.clone();
        rotated.memberships.rotate_left(2);
        rotated.captures.rotate_left(1);
        rotated.assessments.rotate_left(1);

        assert_eq!(compute_review_summary(&reversed), forward);
        assert_eq!(compute_review_summary(&rotated), forward);
        let ids: Vec<_> = forward
            .counted_inputs()
            .iter()
            .map(|input| input.source.event_id.clone())
            .collect();
        let mut sorted_ids = ids.clone();
        sorted_ids.sort();
        sorted_ids.dedup();
        assert_eq!(ids, sorted_ids, "counted inputs are sorted and unique");
    }
}
