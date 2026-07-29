//! Derived attention state over the review record (ADR-0019: attention and
//! notification without an executive controller). Read-side only: no write
//! workflow may consult this module, and its output never authorizes or blocks a
//! write. The projection surfaces *what is outstanding* — open asks, ambiguous
//! decisions, competing heads, stale decisions, failed checks, outstanding
//! follow-ups — as peer items; it never tie-breaks and never carries a
//! per-revision lifecycle stage. "Attention guides, never gates" (ADR-0019 D4).

mod items;

use std::path::{Path, PathBuf};

pub use items::{
    AttentionAssessmentRecord, AttentionDetail, AttentionFreshness, AttentionFreshnessState,
    AttentionItem, AttentionProjection, AttentionTier,
};
pub(crate) use items::{attention_from_events, scope_attention_items};

use crate::error::Result;
use crate::model::RevisionId;
use crate::session::EventStore;
use crate::session::projection::freshness::event_set_hash_for_events;
use crate::session::projection::skipped_to_diagnostics;
use crate::session::state::ProjectionDiagnostic;
use crate::session::store::resolution::resolve_read_store;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AttentionListOptions {
    repo: PathBuf,
    revision: Option<RevisionId>,
}

impl AttentionListOptions {
    pub fn new(repo: impl AsRef<Path>) -> Self {
        Self {
            repo: repo.as_ref().to_path_buf(),
            revision: None,
        }
    }

    pub fn with_revision(mut self, revision: RevisionId) -> Self {
        self.revision = Some(revision);
        self
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AttentionListResult {
    pub event_set_hash: String,
    pub event_count: usize,
    pub revision: Option<RevisionId>,
    pub items: Vec<AttentionItem>,
    pub diagnostics: Vec<ProjectionDiagnostic>,
}

/// Resolve the repo's store, replay the event log leniently (an undecodable event
/// surfaces as a diagnostic rather than aborting the read, as `revision list`
/// does), and derive the attention projection over it. Pull-only: the envelope
/// carries the event-set hash and count so callers poll like every other surface.
pub fn list_attention(options: AttentionListOptions) -> Result<AttentionListResult> {
    let read_store = resolve_read_store(&options.repo)?;
    let store = EventStore::from_backend(read_store.backend());
    let (events, skipped) = store.list_events_lenient()?;
    let event_set_hash = event_set_hash_for_events(&events)?;
    let mut projection = attention_from_events(&events, options.revision.as_ref())?;
    projection
        .diagnostics
        .extend(skipped_to_diagnostics(skipped));

    Ok(AttentionListResult {
        event_set_hash,
        event_count: events.len(),
        revision: options.revision,
        items: projection.items,
        diagnostics: projection.diagnostics,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn list_attention_over_empty_store_returns_well_formed_envelope() {
        let repo = tempfile::tempdir().expect("tempdir");
        std::process::Command::new("git")
            .args(["init"])
            .current_dir(repo.path())
            .output()
            .expect("git init");

        let result = list_attention(AttentionListOptions::new(repo.path())).expect("list");

        assert_eq!(result.event_count, 0);
        assert!(result.items.is_empty());
        assert!(result.diagnostics.is_empty());
        assert!(!result.event_set_hash.is_empty());
    }
}
