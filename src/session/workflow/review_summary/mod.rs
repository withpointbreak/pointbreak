//! A store-level summary of recorded review cycles: the review-round profile,
//! record-internal measures over the review-Change population, and a receipt of
//! the exact facts counted. Read-side only: it records nothing, builds nothing,
//! and gates nothing.

mod authoritative;
mod check;
mod model;
mod receipt;

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

#[cfg(test)]
pub(crate) use authoritative::review_summary_inputs_from_events;
pub use check::{
    ReceiptCheckOptions, ReceiptCheckResult, ReceiptEntryDifference, ReceiptEntryOutcome,
    check_counted_input_receipt,
};
pub use model::{
    ActorRelation, BackfillExclusion, Count, CurrentAndHistorical, FirstCaptureResults, Measure,
    ObservedWindow, Population, ReviewSummary, RoundCount, RoundProfile,
};
pub(crate) use model::{
    AssessmentInput, CaptureInput, CommitAssociationInput, CountedEventRef, MembershipClaimInput,
    MembershipWithdrawalInput, ReviewSummaryInputs, compute_review_summary,
};
#[cfg(test)]
use receipt::counted_input_digest;
use receipt::counted_input_receipt;
pub use receipt::{
    COUNTED_INPUT_DIGEST_ALGORITHM, CountedInputEntry, CountedInputKindCounts, CountedInputReceipt,
    VerificationTally,
};

use crate::error::{Result, ShoreError};
use crate::session::EventStore;
use crate::session::derived_access::history::DerivedHistoryAccess;
use crate::session::derived_access::review_summary::{
    CountedEnvelopes, DerivedReviewSummaryRead, DerivedReviewSummaryRoute,
};
use crate::session::projection::freshness::event_set_hash_for_events;
use crate::session::projection::skipped_to_diagnostics;
use crate::session::signing::TrustSet;
use crate::session::state::ProjectionDiagnostic;
use crate::session::store::capabilities::{BackfillCohort, backfill_cohort};
use crate::session::store::resolution::resolve_read_store;

/// Where to read the summary from, and the trust set its receipt verifies
/// signatures against.
#[derive(Clone, Debug)]
pub struct ReviewSummaryOptions {
    repo: PathBuf,
    trust_set: TrustSet,
}

impl ReviewSummaryOptions {
    /// Read `repo`'s store with an empty trust set.
    pub fn new(repo: impl AsRef<Path>) -> Self {
        Self {
            repo: repo.as_ref().to_path_buf(),
            trust_set: TrustSet::default(),
        }
    }

    /// Verify counted-input signatures against `trust_set`.
    pub fn with_trust_set(mut self, trust_set: TrustSet) -> Self {
        self.trust_set = trust_set;
        self
    }
}

/// The summary, its counted-input receipt, and the identity of the event set it
/// was read from.
#[derive(Clone, Debug)]
pub struct ReviewSummaryResult {
    pub summary: ReviewSummary,
    pub receipt: CountedInputReceipt,
    pub event_set_hash: String,
    pub event_count: usize,
    pub diagnostics: Vec<ProjectionDiagnostic>,
}

/// Read the summary from the repo's store. Undecodable event envelopes surface
/// as diagnostics, as every lenient read does; an envelope that decodes but
/// whose summarized payload does not fails the read.
pub fn review_summary(options: ReviewSummaryOptions) -> Result<ReviewSummaryResult> {
    let read_store = resolve_read_store(&options.repo)?;
    let store = EventStore::from_backend(read_store.backend());
    let (events, skipped) = store.list_events_lenient()?;
    let cohort = backfill_cohort(read_store.backend().journal().as_ref())?;
    let inputs = authoritative::review_summary_inputs_from_events(&events, &cohort)?;
    let summary = compute_review_summary(&inputs);
    let receipt = counted_input_receipt(summary.counted_inputs(), &events, &options.trust_set)?;
    Ok(ReviewSummaryResult {
        summary,
        receipt,
        event_set_hash: event_set_hash_for_events(&events)?,
        event_count: events.len(),
        diagnostics: skipped_to_diagnostics(skipped),
    })
}

/// Which read answered a routed summary.
#[derive(Clone, Debug)]
pub enum RoutedReviewSummary {
    /// Read from the store's recorded facts. `fallback_hint` is the one recovery
    /// hint to show when derived access is active but could not answer.
    Authoritative {
        result: ReviewSummaryResult,
        fallback_hint: Option<&'static str>,
    },
    /// Read from a current derived generation, named by `projection_stamp`.
    /// The receipt is still computed from the counted events' recorded bytes.
    Derived {
        result: ReviewSummaryResult,
        projection_stamp: String,
    },
}

/// Read the summary from a current derived generation when there is one, and
/// otherwise from the store's recorded facts. No derived state — absent,
/// building, catching up, or moved during the read — is built, caught up, or
/// treated as a failure here; it answers from the facts instead.
pub fn review_summary_routed(options: ReviewSummaryOptions) -> Result<RoutedReviewSummary> {
    let access = DerivedHistoryAccess::resolve(&options.repo).map_err(ShoreError::Message)?;
    if let Some(cohort) = access.review_summary_cohort()?
        && let DerivedReviewSummaryRoute::Ready(read) = access
            .review_summary_inputs(&cohort)
            .map_err(ShoreError::Message)?
        && let Some(derived) = derived_review_summary(&read, &cohort, &options.trust_set)?
    {
        return Ok(derived);
    }
    let fallback_hint = if access.is_active() {
        access.claim_authoritative_fallback_hint()
    } else {
        None
    };
    Ok(RoutedReviewSummary::Authoritative {
        result: review_summary(options)?,
        fallback_hint,
    })
}

/// Fold the derived rows and hydrate exactly the counted events for the
/// receipt; `None` when the generation moved before hydration finished. A
/// counted row that disagrees with its recorded event fails the read rather
/// than falling back.
fn derived_review_summary(
    read: &DerivedReviewSummaryRead,
    cohort: &BackfillCohort,
    trust_set: &TrustSet,
) -> Result<Option<RoutedReviewSummary>> {
    let summary = compute_review_summary(read.inputs());
    let event_ids = summary
        .counted_inputs()
        .iter()
        .map(|input| input.source.event_id.clone())
        .collect::<Vec<_>>();
    let events = match read
        .hydrate_counted(&event_ids)
        .map_err(ShoreError::Message)?
    {
        CountedEnvelopes::Ready(events) => events,
        CountedEnvelopes::Stale => return Ok(None),
    };
    let receipt = counted_input_receipt(summary.counted_inputs(), &events, trust_set)?;
    verify_counted_rows(
        read.inputs(),
        &authoritative::review_summary_inputs_from_events(&events, cohort)?,
        summary.counted_inputs(),
    )?;
    Ok(Some(RoutedReviewSummary::Derived {
        result: ReviewSummaryResult {
            summary,
            receipt,
            event_set_hash: String::new(),
            event_count: read.event_count(),
            // Records the fact-set read would skip cannot be present here: an
            // activated store holding one fails that read instead.
            diagnostics: Vec::new(),
        },
        projection_stamp: read.projection_stamp().to_owned(),
    }))
}

/// Every counted row read from the index must equal the row its recorded event
/// folds to under the fact-set lane's rules. Without this, a damaged index row
/// could move a figure while the receipt, which binds only the recorded events,
/// stayed the same.
fn verify_counted_rows(
    indexed: &ReviewSummaryInputs,
    recorded: &ReviewSummaryInputs,
    counted: &[model::CountedInput],
) -> Result<()> {
    let counted = counted
        .iter()
        .map(|input| input.source.event_id.as_str())
        .collect::<BTreeSet<_>>();
    let indexed = counted_rows_by_event(indexed, &counted);
    let recorded = counted_rows_by_event(recorded, &counted);
    let Some(event_id) = counted
        .iter()
        .find(|event_id| indexed.get(*event_id) != recorded.get(*event_id))
    else {
        return Ok(());
    };
    Err(ShoreError::InvalidEvent {
        message: format!(
            "review summary: the derived index row for counted event {event_id} disagrees with \
             its recorded event; `pointbreak store derived rebuild` replaces the index"
        ),
    })
}

/// One input row of any kind, compared by value.
#[derive(Debug, Eq, PartialEq)]
enum CountedRow<'a> {
    MembershipClaim(&'a MembershipClaimInput),
    MembershipWithdrawal(&'a MembershipWithdrawalInput),
    Capture(&'a CaptureInput),
    Assessment(&'a AssessmentInput),
    CommitAssociation(&'a CommitAssociationInput),
}

fn counted_rows_by_event<'a>(
    inputs: &'a ReviewSummaryInputs,
    counted: &BTreeSet<&str>,
) -> BTreeMap<&'a str, Vec<CountedRow<'a>>> {
    let rows = inputs
        .memberships
        .iter()
        .map(|row| (&row.source, CountedRow::MembershipClaim(row)))
        .chain(
            inputs
                .withdrawals
                .iter()
                .map(|row| (&row.source, CountedRow::MembershipWithdrawal(row))),
        )
        .chain(
            inputs
                .captures
                .iter()
                .map(|row| (&row.source, CountedRow::Capture(row))),
        )
        .chain(
            inputs
                .assessments
                .iter()
                .map(|row| (&row.source, CountedRow::Assessment(row))),
        )
        .chain(
            inputs
                .commit_associations
                .iter()
                .map(|row| (&row.source, CountedRow::CommitAssociation(row))),
        );
    let mut by_event: BTreeMap<&str, Vec<CountedRow<'_>>> = BTreeMap::new();
    for (source, row) in rows {
        if counted.contains(source.event_id.as_str()) {
            by_event
                .entry(source.event_id.as_str())
                .or_default()
                .push(row);
        }
    }
    by_event
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn review_summary_of_an_empty_store_is_all_unavailable() {
        let repo = tempfile::tempdir().expect("tempdir");
        std::process::Command::new("git")
            .args(["init"])
            .current_dir(repo.path())
            .output()
            .expect("git init");

        let result = review_summary(ReviewSummaryOptions::new(repo.path())).expect("summary");

        assert_eq!(result.event_count, 0);
        assert!(result.diagnostics.is_empty());
        assert!(!result.event_set_hash.is_empty());
        assert_eq!(result.summary.population, Population::default());
        assert!(matches!(
            result.summary.profile,
            Measure::Unavailable { ref reasons } if reasons == &["noCountedChanges"]
        ));
        assert!(result.receipt.entries.is_empty());
        assert_eq!(result.receipt.digest, counted_input_digest(&[]));
    }

    fn source(event: &str) -> CountedEventRef {
        CountedEventRef {
            event_id: format!("evt:sha256:{event}"),
            payload_hash: format!("sha256:payload-{event}"),
        }
    }

    /// One counted Change holding one captured, assessed, commit-associated
    /// Revision with a withdrawn second membership, plus an uncounted capture.
    fn recorded_inputs() -> ReviewSummaryInputs {
        ReviewSummaryInputs {
            memberships: vec![
                MembershipClaimInput {
                    source: source("claim"),
                    claim_id: "claim:one".to_owned(),
                    change_id: "change:one".to_owned(),
                    revision_id: "rev:one".to_owned(),
                    backfill: false,
                },
                MembershipClaimInput {
                    source: source("withdrawn-claim"),
                    claim_id: "claim:two".to_owned(),
                    change_id: "change:one".to_owned(),
                    revision_id: "rev:two".to_owned(),
                    backfill: false,
                },
            ],
            withdrawals: vec![MembershipWithdrawalInput {
                source: source("withdrawal"),
                claim_id: "claim:two".to_owned(),
            }],
            captures: vec![
                CaptureInput {
                    source: source("capture"),
                    revision_id: "rev:one".to_owned(),
                    actor: "actor:author".to_owned(),
                    captured_at_millis: Some(1),
                },
                CaptureInput {
                    source: source("uncounted-capture"),
                    revision_id: "rev:elsewhere".to_owned(),
                    actor: "actor:author".to_owned(),
                    captured_at_millis: Some(2),
                },
            ],
            assessments: vec![AssessmentInput {
                source: source("assessment"),
                assessment_id: "assess:one".to_owned(),
                revision_id: Some("rev:one".to_owned()),
                actor: "actor:reviewer".to_owned(),
                verdict: crate::session::event::ReviewAssessment::Accepted,
                replaces: Vec::new(),
            }],
            commit_associations: vec![CommitAssociationInput {
                source: source("association"),
                revision_id: "rev:one".to_owned(),
            }],
            manifest_hash: None,
        }
    }

    #[test]
    fn a_counted_row_must_equal_the_row_its_recorded_event_folds_to() {
        let recorded = recorded_inputs();
        let summary = compute_review_summary(&recorded);
        let counted = summary.counted_inputs();
        assert_eq!(counted.len(), 6, "every row but the uncounted capture");
        assert!(verify_counted_rows(&recorded, &recorded, counted).is_ok());

        type Edit = fn(&mut ReviewSummaryInputs);
        let edits: [(&str, Edit); 7] = [
            ("claim change", |rows| {
                rows.memberships[0].change_id = "change:other".to_owned()
            }),
            ("withdrawn claim", |rows| {
                rows.withdrawals[0].claim_id = "claim:one".to_owned()
            }),
            ("capture actor", |rows| {
                rows.captures[0].actor = "actor:other".to_owned()
            }),
            ("capture instant", |rows| {
                rows.captures[0].captured_at_millis = Some(0)
            }),
            ("verdict", |rows| {
                rows.assessments[0].verdict = crate::session::event::ReviewAssessment::NeedsChanges;
            }),
            ("assessment replaces", |rows| {
                rows.assessments[0].replaces = vec!["assess:earlier".to_owned()];
            }),
            ("association Revision", |rows| {
                rows.commit_associations[0].revision_id = "rev:two".to_owned();
            }),
        ];
        for (label, edit) in edits {
            let mut indexed = recorded.clone();
            edit(&mut indexed);
            let error = verify_counted_rows(&indexed, &recorded, counted)
                .expect_err(label)
                .to_string();
            assert!(
                error.contains("disagrees with its recorded event"),
                "{label}: {error}"
            );
        }

        let mut indexed = recorded.clone();
        indexed.captures[1].actor = "actor:other".to_owned();
        assert!(
            verify_counted_rows(&indexed, &recorded, counted).is_ok(),
            "an uncounted row is outside the check"
        );
    }
}
