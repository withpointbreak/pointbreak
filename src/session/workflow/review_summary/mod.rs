//! A store-level summary of recorded review cycles: the review-round profile,
//! record-internal measures over the review-Change population, and a receipt of
//! the exact facts counted. Read-side only: it records nothing, builds nothing,
//! and gates nothing.

mod authoritative;
mod check;
mod model;
mod receipt;

use std::path::{Path, PathBuf};

pub use check::{
    ReceiptCheckOptions, ReceiptCheckResult, ReceiptEntryDifference, ReceiptEntryOutcome,
    check_counted_input_receipt,
};
pub(crate) use model::compute_review_summary;
pub use model::{
    ActorRelation, BackfillExclusion, Count, CurrentAndHistorical, FirstCaptureResults, Measure,
    ObservedWindow, Population, ReviewSummary, RoundCount, RoundProfile,
};
#[cfg(test)]
pub(crate) use model::{
    AssessmentInput, CaptureInput, CommitAssociationInput, CountedEventRef, MembershipClaimInput,
    ReviewSummaryInputs,
};
#[cfg(test)]
use receipt::counted_input_digest;
use receipt::counted_input_receipt;
pub use receipt::{
    COUNTED_INPUT_DIGEST_ALGORITHM, CountedInputEntry, CountedInputKindCounts, CountedInputReceipt,
    VerificationTally,
};

use crate::error::Result;
use crate::session::EventStore;
use crate::session::projection::freshness::event_set_hash_for_events;
use crate::session::projection::skipped_to_diagnostics;
use crate::session::signing::TrustSet;
use crate::session::state::ProjectionDiagnostic;
use crate::session::store::capabilities::backfill_cohort;
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
}
