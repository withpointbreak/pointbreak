use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use serde::Serialize;

use super::cursor::{HistoryCursor, HistoryWindow};
use crate::model::{RevisionId, TrackId};
use crate::session::event::EventType;
use crate::session::{
    ActorAttributesMap, DelegationMap, EventVerificationPolicy, RefFilterMode, RemovalPolicy,
    TrustSet,
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReviewHistoryOptions {
    pub(super) repo: PathBuf,
    pub(super) revision_id: Option<RevisionId>,
    pub(super) track: Option<String>,
    pub(super) event_types: Vec<EventType>,
    pub(super) ref_filter: Option<(String, RefFilterMode)>,
    pub(super) include_body: bool,
    pub(super) verification_policy: Option<EventVerificationPolicy>,
    pub(super) trust_set: TrustSet,
    pub(super) removal_policy: RemovalPolicy,
    pub(super) actor_attributes: Option<ActorAttributesMap>,
    pub(super) delegation_map: Option<DelegationMap>,
    pub(super) read_for_display: bool,
    pub(super) limit: Option<usize>,
    pub(super) cursor: Option<HistoryCursor>,
}

impl ReviewHistoryOptions {
    pub fn new(repo: impl AsRef<Path>) -> Self {
        Self {
            repo: repo.as_ref().to_path_buf(),
            revision_id: None,
            track: None,
            event_types: Vec::new(),
            ref_filter: None,
            include_body: false,
            verification_policy: None,
            trust_set: TrustSet::default(),
            removal_policy: RemovalPolicy::default(),
            actor_attributes: None,
            delegation_map: None,
            read_for_display: false,
            limit: None,
            cursor: None,
        }
    }

    /// Filter history to events of units associated with `name`. The name is
    /// normalized to its full ref before matching the stored `ref_name`.
    pub fn with_ref_filter(mut self, name: impl Into<String>, mode: RefFilterMode) -> Self {
        self.ref_filter = Some((name.into(), mode));
        self
    }

    pub fn with_revision_id(mut self, revision_id: RevisionId) -> Self {
        self.revision_id = Some(revision_id);
        self
    }

    pub fn with_track(mut self, track: impl Into<String>) -> Self {
        self.track = Some(track.into());
        self
    }

    pub fn with_event_type(mut self, event_type: EventType) -> Self {
        self.event_types.push(event_type);
        self
    }

    pub fn with_include_body(mut self, include_body: bool) -> Self {
        self.include_body = include_body;
        self
    }

    pub fn with_verification_policy(mut self, policy: EventVerificationPolicy) -> Self {
        self.verification_policy = Some(policy);
        self
    }

    pub fn with_trust_set(mut self, trust_set: TrustSet) -> Self {
        self.trust_set = trust_set;
        self
    }

    /// Supply the render-time removal policy for body-content states. A
    /// non-operative removal claim renders the bytes; an operative one renders
    /// the explained removed state. Render-only: it never gates the compact
    /// erasure sweep.
    pub fn with_removal_policy(mut self, removal_policy: RemovalPolicy) -> Self {
        self.removal_policy = removal_policy;
        self
    }

    /// Supply the reader's actor-attributes map. Sibling enrichment for endorsement
    /// readbacks (the endorser's attested kind/roles) — never a classifier input.
    pub fn with_actor_attributes(mut self, actor_attributes: Option<ActorAttributesMap>) -> Self {
        self.actor_attributes = actor_attributes;
        self
    }

    /// Supply the reader-side delegation map. Beside `with_trust_set`: config the
    /// reader provides, never store content. With it set, agent-scheme writers'
    /// entries carry a resolved principal object; without it they degrade to the
    /// mirror posture (`status: none`).
    pub fn with_delegation_map(mut self, delegation_map: DelegationMap) -> Self {
        self.delegation_map = Some(delegation_map);
        self
    }

    /// Read for a human-facing surface: skip a retired/unsupported event and
    /// surface it as a diagnostic instead of hard-failing the read. Off by
    /// default, so the relay and other strict callers keep the typed error.
    pub fn with_read_for_display(mut self, value: bool) -> Self {
        self.read_for_display = value;
        self
    }

    /// Bound the rendered output to at most `limit` entries (the first page when
    /// no cursor is set). Absent by default — the full unbounded result.
    pub fn with_limit(mut self, limit: usize) -> Self {
        self.limit = Some(limit);
        self
    }

    /// Continue from a previous page's opaque cursor: take entries strictly after
    /// it. Absent by default.
    pub fn with_cursor(mut self, cursor: HistoryCursor) -> Self {
        self.cursor = Some(cursor);
        self
    }

    /// Resolve the requested limit/cursor into the window the projection applies.
    pub(super) fn window(&self) -> HistoryWindow {
        HistoryWindow {
            limit: self.limit,
            after: self.cursor.clone(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReviewHistoryFilters {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub revision_id: Option<RevisionId>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub track_id: Option<TrackId>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub event_types: Vec<EventType>,
    pub include_body: bool,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(super) struct ResolvedHistoryFilters {
    pub(super) revision_id: Option<RevisionId>,
    pub(super) track_id: Option<TrackId>,
    pub(super) event_types: Vec<EventType>,
    /// When a `--ref` filter resolves, the review-unit ids that match it. An
    /// event passes only if its target unit is in this set.
    pub(super) ref_matched_units: Option<BTreeSet<RevisionId>>,
    pub(super) include_body: bool,
    pub(super) verification_policy: Option<EventVerificationPolicy>,
    pub(super) trust_set: TrustSet,
    pub(super) removal_policy: RemovalPolicy,
    pub(super) actor_attributes: Option<ActorAttributesMap>,
    pub(super) delegation_map: Option<DelegationMap>,
}

impl From<ResolvedHistoryFilters> for ReviewHistoryFilters {
    fn from(filters: ResolvedHistoryFilters) -> Self {
        Self {
            revision_id: filters.revision_id,
            track_id: filters.track_id,
            event_types: filters.event_types,
            include_body: filters.include_body,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::EventId;

    #[test]
    fn options_carry_limit_and_cursor() {
        let cursor = HistoryCursor {
            occurred_at: "unix-ms:1".into(),
            event_id: EventId::new("evt:sha256:a"),
        };
        let opts = ReviewHistoryOptions::new("/repo")
            .with_limit(10)
            .with_cursor(cursor.clone());
        let window = opts.window();
        assert_eq!(window.limit, Some(10));
        assert_eq!(window.after, Some(cursor));
    }

    #[test]
    fn options_default_to_no_window() {
        let opts = ReviewHistoryOptions::new("/repo");
        assert_eq!(opts.window(), HistoryWindow::default());
    }
}
