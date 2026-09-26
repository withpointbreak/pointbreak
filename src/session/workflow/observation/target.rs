use std::collections::BTreeSet;
use std::path::Path;

use crate::error::{Result, ShoreError};
use crate::git::git_head_ref;
use crate::model::{JournalId, ObjectId, ReviewEndpoint, ReviewTargetRef, RevisionId, Side};
use crate::session::event::{
    EventType, Revision, RevisionRefAssociatedPayload, ShoreEvent, WorkObjectProposal,
    WorkObjectProposedPayload,
};
use crate::session::object_artifact::read_bound_object_artifact_for_write_validation;
use crate::session::projection::change::{ChangeProjection, project_changes};
use crate::session::projection::commit_range::revision_of;
use crate::session::projection::supersession::SupersessionView;
use crate::session::store::fingerprint::normalized_worktree_root;
use crate::session::workflow::attention::change_aware_supersession;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ResolvedRevision {
    pub journal_id: JournalId,
    pub revision_id: RevisionId,
    pub object_id: ObjectId,
    pub object_artifact_content_hash: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum RevisionSelection<'a> {
    Current,
    /// Resolve a revision id directly (no head resolution).
    Exact(&'a RevisionId),
    /// The `--revision` head seed: if the id is itself a current head it resolves
    /// exactly; otherwise it resolves the current head of the id's thread,
    /// force-disambiguating when that thread has competing heads.
    Head(&'a RevisionId),
}

impl<'a> RevisionSelection<'a> {
    /// Build a selection from the mutually exclusive revision write options.
    /// `--revision` remains a head seed; `--exact-revision` addresses the named
    /// captured revision without following supersession.
    pub(crate) fn from_revision_options(
        head_seed: Option<&'a RevisionId>,
        exact_revision: Option<&'a RevisionId>,
    ) -> Result<Self> {
        match (head_seed, exact_revision) {
            (Some(_), Some(_)) => Err(ShoreError::WorkflowInputInvalid {
                reason: "--revision and --exact-revision are mutually exclusive".to_owned(),
            }),
            (None, Some(exact_revision)) => Ok(Self::Exact(exact_revision)),
            (head_seed, None) => Ok(Self::from_revision_seed(head_seed)),
        }
    }

    /// Build the selection from an optional `--revision` seed: a present seed is a
    /// head seed (`Head`), an absent one defaults to the current capture.
    pub(crate) fn from_revision_seed(seed: Option<&'a RevisionId>) -> Self {
        match seed {
            Some(seed) => Self::Head(seed),
            None => Self::Current,
        }
    }
}

/// The caller's current git context for scoping [`RevisionSelection::Current`].
/// `worktree_root` is the canonical root from [`normalized_worktree_root`];
/// `head_ref` is the full ref of HEAD (`refs/heads/...`), `None` on detached HEAD.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct CurrentRevisionContext {
    pub worktree_root: String,
    pub head_ref: Option<String>,
}

impl CurrentRevisionContext {
    /// Resolve the context from a repo path: the canonical worktree root plus
    /// HEAD's full ref. Used by the read/observation workflows before selection.
    pub(crate) fn for_repo(repo: &Path) -> Result<Self> {
        Ok(Self {
            worktree_root: normalized_worktree_root(repo)?,
            head_ref: git_head_ref(repo)?,
        })
    }
}

/// How widely `Current` searches the (shared) store. The default scopes to the
/// caller's current worktree; the widening variants are the fixed vocabulary the
/// widening read selectors construct as they are wired onto the read surfaces.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
#[allow(dead_code)] // widening variants are constructed by the read selectors that surface them
pub(crate) enum RevisionScope {
    /// Default: only captures belonging to the caller's current worktree context.
    #[default]
    CurrentWorktree,
    /// Scope to a named worktree root: the named root rides in the context's
    /// `worktree_root` and is matched the same way as the current worktree.
    Worktree,
    /// Widen to the whole store (resolve against every captured unit).
    All,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ObservationTargetSelector {
    pub file_path: Option<String>,
    pub side: Side,
    pub start_line: Option<u32>,
    pub end_line: Option<u32>,
}

impl ObservationTargetSelector {
    pub fn revision() -> Self {
        Self {
            file_path: None,
            side: Side::New,
            start_line: None,
            end_line: None,
        }
    }

    pub fn file(path: impl Into<String>) -> Self {
        Self {
            file_path: Some(path.into()),
            side: Side::New,
            start_line: None,
            end_line: None,
        }
    }

    pub fn range(
        path: impl Into<String>,
        side: Side,
        start_line: u32,
        end_line: Option<u32>,
    ) -> Self {
        Self {
            file_path: Some(path.into()),
            side,
            start_line: Some(start_line),
            end_line,
        }
    }
}

pub(crate) fn resolve_revision(
    events: &[ShoreEvent],
    selection: RevisionSelection<'_>,
    context: &CurrentRevisionContext,
    scope: RevisionScope,
) -> Result<ResolvedRevision> {
    if let RevisionSelection::Head(seed) = selection {
        let resolved_id = resolve_head_seed(events, seed)?;
        return resolve_revision(
            events,
            RevisionSelection::Exact(&resolved_id),
            context,
            scope,
        );
    }

    // `Current` under a worktree scope (current or named) only considers captures
    // belonging to that worktree context; `All` considers the whole store. `Exact`
    // (handled inside the loop) is always context-independent.
    let scoped = matches!(
        scope,
        RevisionScope::CurrentWorktree | RevisionScope::Worktree
    );

    let mut captured = Vec::new();
    for event in events
        .iter()
        .filter(|event| event.event_type == EventType::WorkObjectProposed)
    {
        let Some((revision, object_artifact_content_hash)) = revision_from_capture_event(event)?
        else {
            continue;
        };
        let resolved = ResolvedRevision {
            journal_id: event.target.journal_id.clone(),
            revision_id: revision.id.clone(),
            object_id: revision.object_id.clone(),
            object_artifact_content_hash,
        };
        if matches!(selection, RevisionSelection::Exact(requested) if requested == &resolved.revision_id)
        {
            return Ok(resolved);
        }
        if matches!(selection, RevisionSelection::Current)
            && scoped
            && !capture_matches_current_worktree(events, event, &revision, context)?
        {
            continue;
        }
        captured.push(resolved);
    }

    if let RevisionSelection::Exact(requested) = selection {
        return Err(ShoreError::Message(format!(
            "unknown revision: {}",
            requested.as_str()
        )));
    }

    match captured.as_slice() {
        [] => Err(ShoreError::Message("no captured revision".to_owned())),
        [resolved] => Ok(resolved.clone()),
        _ => Err(ShoreError::Message(
            "multiple captured revisions; pass --revision <id> (`revision show` takes it \
             positionally: `revision show <revision>`); list candidates with \
             `pointbreak revision list`"
                .to_owned(),
        )),
    }
}

/// [`resolve_revision`] for a writer verb (`observation add`, `validation add`,
/// `assessment add`, `input-request open`).
///
/// A `--revision` head seed follows only proposal-borne supersession, which
/// Change capture never writes (ADR-0042), so on a store that holds Change
/// claims a Revision replaced through a Change relation claim still reads as a
/// current head and the write would land on replaced bytes. Before the legacy
/// head resolution runs, the seed is checked against the Change-aware
/// replacement view: a seed that some usable Change has replaced is refused
/// with a typed error naming the replacing current Revision(s) and the exact
/// selectors (`--review-cursor`, `--exact-revision`). Nothing else changes:
/// `Exact` and `Current` selections, and a store without Change claims, resolve
/// exactly as [`resolve_revision`] does.
///
/// `events` must be the writer-visible, store-wide event slice: the rule reads
/// every Change that holds the seed, so a projection narrowed to one Change
/// cannot decide it.
///
/// The refusal is decided on that snapshot, which the command loaded before it
/// appends; it is not repeated under the store's authority lock at durable
/// append time. A replacement relation that another writer appends after the
/// snapshot was taken does not refuse this write, so the guarantee is "refused
/// on the pre-append snapshot", not "never appended". One authority-lock
/// boundary around read, validation and append is tracked in #837.
pub(crate) fn resolve_revision_for_write(
    events: &[ShoreEvent],
    selection: RevisionSelection<'_>,
    context: &CurrentRevisionContext,
    scope: RevisionScope,
) -> Result<ResolvedRevision> {
    if let RevisionSelection::Head(seed) = selection {
        let legacy = SupersessionView::from_events(events)?;
        let changes = project_changes(events)?;
        if let Some(replacing) = change_replacing_revisions(seed, &legacy, &changes) {
            return Err(replaced_revision_error(seed, &replacing));
        }
    }
    resolve_revision(events, selection, context, scope)
}

/// The current Revisions that replace `seed` through effective Change relation
/// claims, or `None` when no usable Change has replaced it. A store without
/// Change claims never replaces anything here (its proposal-borne edges keep
/// their legacy head-following), and a Revision that is still current in any
/// Change that holds it is not replaced (the [`change_aware_supersession`]
/// authority rule).
///
/// The result is the set of heads reachable from `seed` along admitted
/// replacement edges, so a crossed pair of histories (whose union loops) still
/// names the Revision both end at, and a divergent or twice-replaced seed names
/// every successor head. It falls back to the direct successors only if no head
/// is reachable, which the per-Change current set makes unreachable in
/// practice.
pub(crate) fn change_replacing_revisions(
    seed: &RevisionId,
    legacy: &SupersessionView,
    changes: &ChangeProjection,
) -> Option<BTreeSet<RevisionId>> {
    if changes.changes.is_empty() {
        return None;
    }
    let replacement = change_aware_supersession(legacy, changes).replacement;
    if !replacement.superseded.contains(seed) {
        return None;
    }
    let mut visited: BTreeSet<RevisionId> = BTreeSet::new();
    let mut frontier: Vec<RevisionId> = vec![seed.clone()];
    while let Some(revision) = frontier.pop() {
        for successor in replacement.stale_by_superseding_revision(&revision) {
            if visited.insert(successor.clone()) {
                frontier.push(successor);
            }
        }
    }
    let heads: BTreeSet<RevisionId> = visited
        .iter()
        .filter(|revision| replacement.heads.contains(*revision))
        .cloned()
        .collect();
    Some(if heads.is_empty() {
        replacement.stale_by_superseding_revision(seed)
    } else {
        heads
    })
}

fn replaced_revision_error(seed: &RevisionId, replacing: &BTreeSet<RevisionId>) -> ShoreError {
    let listed = replacing
        .iter()
        .map(RevisionId::as_str)
        .collect::<Vec<_>>()
        .join(", ");
    let current = if replacing.len() == 1 {
        format!("the current Revision is {listed}")
    } else {
        format!("the current Revisions are {listed}")
    };
    ShoreError::WorkflowInputInvalid {
        reason: format!(
            "revision_replaced_by_change: revision {seed} was replaced through a Change relation \
             claim and --revision does not follow Change replacement; {current}. Write to the \
             current Revision with --review-cursor <token> (from `pointbreak change select` or the \
             replacing capture), or address this exact Revision with --exact-revision {seed}",
            seed = seed.as_str(),
        ),
    }
}

/// Resolve a `--revision` head seed to a concrete revision id over the
/// supersession graph (fork-tolerant): if the seed is itself a current head it
/// resolves exactly (the escape from a fork — never loops); otherwise it resolves
/// the single current head of the seed's thread, force-disambiguating when that
/// thread has competing heads. Thread-scoped — unrelated threads' heads never
/// leak in.
fn resolve_head_seed(events: &[ShoreEvent], seed: &RevisionId) -> Result<RevisionId> {
    let view = SupersessionView::from_events(events)?;
    if view.heads.contains(seed) {
        return Ok(seed.clone());
    }
    let heads = view.heads_for(seed);
    match heads.len() {
        0 => Err(ShoreError::Message(format!(
            "unknown revision: {}",
            seed.as_str()
        ))),
        1 => Ok(heads.into_iter().next().expect("one head")),
        _ => {
            let listed = heads
                .iter()
                .map(RevisionId::as_str)
                .collect::<Vec<_>>()
                .join(", ");
            Err(ShoreError::Message(format!(
                "revision {} has competing heads; pass one as --revision: {listed}",
                seed.as_str()
            )))
        }
    }
}

/// Whether a capture belongs to the caller's current worktree context. A capture
/// matches when **any** of: a positive worktree-identity match (its `GitWorkingTree`
/// target equals the current worktree root, or a capture-time `RevisionRefAssociated`
/// names the current branch); or (fail-open) it carries no locally-meaningful
/// worktree signal.
///
/// The fail-open net keeps two kinds of capture resolvable via bare `Current`: a
/// lone commit-range capture (no worktree path and no ref association) and an
/// ingested capture (its worktree path is the origin's, meaningless locally).
/// Scoping must never silently hide a capture it cannot classify. A
/// locally-authored worktree capture for a different root is a positive non-match
/// and does not fall through to fail-open.
fn capture_matches_current_worktree(
    events: &[ShoreEvent],
    capture_event: &ShoreEvent,
    revision: &Revision,
    context: &CurrentRevisionContext,
) -> Result<bool> {
    if capture_has_worktree_identity_match(events, revision, context)? {
        return Ok(true);
    }

    let target_is_git_commit = matches!(
        revision.git_provenance.as_ref().map(|p| &p.target),
        Some(ReviewEndpoint::GitCommit { .. })
    );
    let signal_less_range =
        target_is_git_commit && !capture_has_any_ref_association(events, &revision.id)?;
    let is_ingested = capture_event.ingest.is_some();
    Ok(signal_less_range || is_ingested)
}

/// Decode the captured revision from a generative move, or `None` if the move is
/// not a review-domain revision proposal.
fn revision_from_capture_event(event: &ShoreEvent) -> Result<Option<(Revision, String)>> {
    let payload: WorkObjectProposedPayload = serde_json::from_value(event.payload.clone())?;
    Ok(match payload.work_object {
        WorkObjectProposal::Revision {
            revision,
            object_artifact_content_hash,
            ..
        } => Some((revision, object_artifact_content_hash)),
        WorkObjectProposal::TaskAttempt { .. } => None,
    })
}

/// The positive worktree-identity half of [`capture_matches_current_worktree`]:
/// a capture matches iff its `GitWorkingTree` target equals the context's worktree
/// root, or it carries a capture-time `RevisionRefAssociated` for the context's
/// branch. This is the strict identity match — without the fail-open net — that an
/// explicit worktree read selector scopes by.
pub(crate) fn capture_has_worktree_identity_match(
    events: &[ShoreEvent],
    revision: &Revision,
    context: &CurrentRevisionContext,
) -> Result<bool> {
    // Ref associations may later enrich liveness, but they do not manufacture
    // capture-time Git identity for a provenance-free revision.
    if revision.git_provenance.is_none() {
        return Ok(false);
    }

    if let Some(ReviewEndpoint::GitWorkingTree { worktree_root }) =
        revision.git_provenance.as_ref().map(|p| &p.target)
        && worktree_root == &context.worktree_root
    {
        return Ok(true);
    }

    if let Some(head_ref) = context.head_ref.as_deref()
        && capture_has_ref_association(events, &revision.id, head_ref)?
    {
        return Ok(true);
    }

    Ok(false)
}

/// The review-unit ids whose `WorkObjectProposed` event has a positive
/// worktree-identity match against `context`. Shared by the explicit worktree read
/// selector on the list surfaces (the strict identity match, no fail-open).
pub(crate) fn revision_ids_in_worktree(
    events: &[ShoreEvent],
    context: &CurrentRevisionContext,
) -> Result<std::collections::BTreeSet<RevisionId>> {
    let mut ids = std::collections::BTreeSet::new();
    for event in events
        .iter()
        .filter(|event| event.event_type == EventType::WorkObjectProposed)
    {
        let Some((revision, _)) = revision_from_capture_event(event)? else {
            continue;
        };
        if capture_has_worktree_identity_match(events, &revision, context)? {
            ids.insert(revision.id);
        }
    }
    Ok(ids)
}

/// Whether the event set carries a `RevisionRefAssociated` for `revision_id`
/// naming `head_ref` (the full ref, matching `git_head_ref`'s spelling).
fn capture_has_ref_association(
    events: &[ShoreEvent],
    revision_id: &RevisionId,
    head_ref: &str,
) -> Result<bool> {
    for event in events
        .iter()
        .filter(|event| event.event_type == EventType::RevisionRefAssociated)
    {
        let payload: RevisionRefAssociatedPayload = serde_json::from_value(event.payload.clone())?;
        if revision_of(&payload.target).as_ref() == Some(revision_id)
            && payload.ref_name == head_ref
        {
            return Ok(true);
        }
    }
    Ok(false)
}

/// Whether the event set carries any `RevisionRefAssociated` for
/// `revision_id`, regardless of which ref it names.
fn capture_has_any_ref_association(
    events: &[ShoreEvent],
    revision_id: &RevisionId,
) -> Result<bool> {
    for event in events
        .iter()
        .filter(|event| event.event_type == EventType::RevisionRefAssociated)
    {
        let payload: RevisionRefAssociatedPayload = serde_json::from_value(event.payload.clone())?;
        if revision_of(&payload.target).as_ref() == Some(revision_id) {
            return Ok(true);
        }
    }
    Ok(false)
}

pub(crate) fn resolve_observation_target(
    repo: &Path,
    resolved: &ResolvedRevision,
    selector: &ObservationTargetSelector,
) -> Result<ReviewTargetRef> {
    let Some(file_path) = selector.file_path.as_deref() else {
        if selector.start_line.is_some() || selector.end_line.is_some() {
            return Err(ShoreError::WorkflowInputInvalid {
                reason: "file is required when selecting observation lines".to_owned(),
            });
        }
        return Ok(ReviewTargetRef::Revision {
            revision_id: resolved.revision_id.clone(),
        });
    };

    let artifact = read_bound_object_artifact_for_write_validation(
        repo,
        &resolved.object_id,
        &resolved.object_artifact_content_hash,
    )?;
    if !artifact.snapshot.files.iter().any(|file| {
        file.new_path.as_deref() == Some(file_path) || file.old_path.as_deref() == Some(file_path)
    }) {
        return Err(ShoreError::Message(format!(
            "file target is not present in captured snapshot: {file_path}"
        )));
    }

    match selector.start_line {
        Some(start_line) => {
            if start_line == 0 {
                return Err(ShoreError::WorkflowInputInvalid {
                    reason: "start line must be greater than zero".to_owned(),
                });
            }
            let end_line = selector.end_line.unwrap_or(start_line);
            if end_line < start_line {
                return Err(ShoreError::WorkflowInputInvalid {
                    reason: "end line must be greater than or equal to start line".to_owned(),
                });
            }
            Ok(ReviewTargetRef::Range {
                revision_id: resolved.revision_id.clone(),
                file_path: file_path.to_owned(),
                side: selector.side,
                start_line,
                end_line,
            })
        }
        None => {
            if selector.end_line.is_some() {
                return Err(ShoreError::WorkflowInputInvalid {
                    reason: "start line is required when end line is supplied".to_owned(),
                });
            }
            Ok(ReviewTargetRef::File {
                revision_id: resolved.revision_id.clone(),
                file_path: file_path.to_owned(),
            })
        }
    }
}

#[cfg(test)]
mod scope_tests {
    use std::process::Command;

    use super::*;
    use crate::model::{
        CommitRangeCaptureMode, EngagementId, JournalId, ObjectId, ReviewEndpoint, RevisionId,
        RevisionSource, WorktreeCaptureMode,
    };
    use crate::session::event::{EventTarget, GitProvenance, IngestProvenance, IngestVia, Writer};

    fn capture_event(suffix: &str, source: RevisionSource, target: ReviewEndpoint) -> ShoreEvent {
        let revision_id = RevisionId::new(format!("review-unit:sha256:{suffix}"));
        let object_id = ObjectId::new(format!("snap:{suffix}"));
        ShoreEvent::new(
            EventType::WorkObjectProposed,
            format!("work_object_proposed:{}", revision_id.as_str()),
            EventTarget::for_revision(JournalId::new("journal:default"), revision_id.clone(), None)
                .unwrap(),
            Writer::shore_local("test"),
            WorkObjectProposedPayload {
                engagement_id: EngagementId::new(format!(
                    "engagement:sha256:{}",
                    crate::canonical_hash::sha256_bytes_hex(revision_id.as_str().as_bytes())
                )),
                work_object: WorkObjectProposal::Revision {
                    revision: Revision {
                        id: revision_id,
                        object_id,
                        git_provenance: Some(GitProvenance {
                            source,
                            base: ReviewEndpoint::GitCommit {
                                commit_oid: format!("base-oid:{suffix}"),
                                tree_oid: format!("base-tree:{suffix}"),
                            },
                            target,
                        }),
                    },
                    summary: None,
                    object_artifact_content_hash: "sha256:artifact".to_owned(),
                    supersedes: vec![],
                },
            },
            "2026-05-12T00:00:00Z",
        )
        .unwrap()
    }

    /// A worktree capture: `GitWorkingTree` target carrying the worktree root.
    fn worktree_capture(suffix: &str, worktree_root: &str) -> ShoreEvent {
        capture_event(
            suffix,
            RevisionSource::GitWorktree {
                mode: WorktreeCaptureMode::CombinedHeadToWorkingTree,
                include_untracked: true,
                pathspecs: Vec::new(),
            },
            ReviewEndpoint::GitWorkingTree {
                worktree_root: worktree_root.to_owned(),
            },
        )
    }

    /// A commit-range capture: `GitCommit` target, no worktree root.
    fn range_capture(suffix: &str) -> ShoreEvent {
        capture_event(
            suffix,
            RevisionSource::GitCommitRange {
                mode: CommitRangeCaptureMode::BaseTreeToTargetTree,
                pathspecs: Vec::new(),
            },
            ReviewEndpoint::GitCommit {
                commit_oid: format!("oid:{suffix}"),
                tree_oid: format!("tree:{suffix}"),
            },
        )
    }

    fn provenance_free_capture(suffix: &str) -> ShoreEvent {
        let revision_id = RevisionId::new(format!("review-unit:sha256:{suffix}"));
        ShoreEvent::new(
            EventType::WorkObjectProposed,
            format!("work_object_proposed:{}", revision_id.as_str()),
            EventTarget::for_revision(JournalId::new("journal:default"), revision_id.clone(), None)
                .unwrap(),
            Writer::shore_local("test"),
            WorkObjectProposedPayload {
                engagement_id: EngagementId::new(format!("engagement:sha256:{suffix}")),
                work_object: WorkObjectProposal::Revision {
                    revision: Revision {
                        id: revision_id,
                        object_id: ObjectId::new(format!("obj:sha256:{suffix}")),
                        git_provenance: None,
                    },
                    summary: None,
                    object_artifact_content_hash: "sha256:artifact".to_owned(),
                    supersedes: vec![],
                },
            },
            "2026-05-12T00:00:00Z",
        )
        .unwrap()
    }

    /// An ingested copy of a worktree capture: it keeps the origin's worktree root
    /// and carries an `ingest` provenance marker (the dest stamps this on import).
    fn ingested_worktree_capture(suffix: &str, origin_root: &str) -> ShoreEvent {
        let mut event = worktree_capture(suffix, origin_root);
        event.ingest = Some(IngestProvenance {
            via: IngestVia::IngestEvents,
            received_at: "unix-ms:1760000000000".to_owned(),
        });
        event
    }

    /// A capture-time branch ref association for a given unit + full ref.
    fn ref_association(unit_suffix: &str, ref_name: &str) -> ShoreEvent {
        crate::session::workflow::association::build_ref_association_event(
            &JournalId::new("journal:default"),
            &RevisionId::new(format!("review-unit:sha256:{unit_suffix}")),
            ref_name,
            &"0".repeat(40),
            None,
            Writer::shore_local("test"),
            "2026-05-12T00:00:00Z",
        )
        .unwrap()
    }

    fn ctx(worktree_root: &str, head_ref: Option<&str>) -> CurrentRevisionContext {
        CurrentRevisionContext {
            worktree_root: worktree_root.to_owned(),
            head_ref: head_ref.map(str::to_owned),
        }
    }

    #[test]
    fn revision_options_select_exact_head_seed_or_current_and_reject_both() {
        let head_seed = RevisionId::new("rev:sha256:head-seed");
        let exact = RevisionId::new("rev:sha256:exact");

        assert_eq!(
            RevisionSelection::from_revision_options(Some(&head_seed), None).unwrap(),
            RevisionSelection::Head(&head_seed)
        );
        assert_eq!(
            RevisionSelection::from_revision_options(None, Some(&exact)).unwrap(),
            RevisionSelection::Exact(&exact)
        );
        assert_eq!(
            RevisionSelection::from_revision_options(None, None).unwrap(),
            RevisionSelection::Current
        );

        let error = RevisionSelection::from_revision_options(Some(&head_seed), Some(&exact))
            .expect_err("the two revision selectors are mutually exclusive");
        assert_eq!(
            error.to_string(),
            "--revision and --exact-revision are mutually exclusive"
        );
    }

    fn current(
        events: &[ShoreEvent],
        context: &CurrentRevisionContext,
        scope: RevisionScope,
    ) -> Result<ResolvedRevision> {
        resolve_revision(events, RevisionSelection::Current, context, scope)
    }

    #[test]
    fn current_resolves_this_worktrees_capture_with_sibling_captures_present() {
        let events = [
            worktree_capture("a", "/wt/a"),
            worktree_capture("b", "/wt/b"),
        ];

        let resolved_a = current(&events, &ctx("/wt/a", None), RevisionScope::CurrentWorktree)
            .expect("worktree a resolves its own capture");
        assert_eq!(resolved_a.revision_id.as_str(), "review-unit:sha256:a");

        let resolved_b = current(&events, &ctx("/wt/b", None), RevisionScope::CurrentWorktree)
            .expect("worktree b resolves its own capture");
        assert_eq!(resolved_b.revision_id.as_str(), "review-unit:sha256:b");
    }

    #[test]
    fn current_two_own_worktree_captures_is_selection_error() {
        let events = [
            worktree_capture("a", "/wt/a"),
            worktree_capture("a2", "/wt/a"),
        ];

        let error =
            current(&events, &ctx("/wt/a", None), RevisionScope::CurrentWorktree).unwrap_err();

        assert!(
            error
                .to_string()
                .contains("multiple captured revisions; pass --revision")
        );
    }

    #[test]
    fn current_matches_capture_by_capture_time_ref_when_target_path_differs() {
        // A range capture (no worktree path) that carries a capture-time ref for
        // the current branch, with a context whose worktree root differs.
        let events = [
            range_capture("r"),
            ref_association("r", "refs/heads/feat/x"),
        ];

        let resolved = current(
            &events,
            &ctx("/some/other/root", Some("refs/heads/feat/x")),
            RevisionScope::CurrentWorktree,
        )
        .expect("ref association scopes the range capture into the current branch context");
        assert_eq!(resolved.revision_id.as_str(), "review-unit:sha256:r");
    }

    #[test]
    fn worktree_identity_excludes_provenance_free_capture_with_matching_ref() {
        let events = [
            provenance_free_capture("nongit"),
            ref_association("nongit", "refs/heads/feat/x"),
        ];

        let in_scope =
            revision_ids_in_worktree(&events, &ctx("/wt/a", Some("refs/heads/feat/x"))).unwrap();

        assert!(in_scope.is_empty());
    }

    #[test]
    fn current_no_match_in_worktree_is_no_captured_revision_error() {
        // Both captures carry a worktree-path signal that does not match the
        // context, so the fail-open clause does not apply to either.
        let events = [
            worktree_capture("b", "/wt/b"),
            worktree_capture("c", "/wt/c"),
        ];

        let error =
            current(&events, &ctx("/wt/a", None), RevisionScope::CurrentWorktree).unwrap_err();

        assert!(error.to_string().contains("no captured revision"));
    }

    #[test]
    fn exact_resolves_regardless_of_worktree_context() {
        // `Exact` widens by construction: it resolves a sibling worktree's unit
        // even from a non-matching context. (LineageHead threads the same context
        // through its recursion; the lineage suite covers that path.)
        let events = [
            worktree_capture("a", "/wt/a"),
            worktree_capture("b", "/wt/b"),
        ];
        let requested = RevisionId::new("review-unit:sha256:b");

        let resolved = resolve_revision(
            &events,
            RevisionSelection::Exact(&requested),
            &ctx("/wt/a", None),
            RevisionScope::CurrentWorktree,
        )
        .expect("exact selection is context-independent");
        assert_eq!(resolved.revision_id.as_str(), "review-unit:sha256:b");
    }

    #[test]
    fn widen_to_all_resolves_single_cross_worktree_capture_but_still_errors_on_two() {
        let one = [worktree_capture("b", "/wt/b")];
        let resolved = current(&one, &ctx("/wt/a", None), RevisionScope::All)
            .expect("widening to the whole store resolves a single capture");
        assert_eq!(resolved.revision_id.as_str(), "review-unit:sha256:b");

        let two = [
            worktree_capture("b", "/wt/b"),
            worktree_capture("c", "/wt/c"),
        ];
        let error = current(&two, &ctx("/wt/a", None), RevisionScope::All).unwrap_err();
        assert!(error.to_string().contains("multiple captured revisions"));
    }

    #[test]
    fn normalized_worktree_root_is_pub_crate() {
        // Compile-level proof that the function is reachable from a sibling module.
        let dir = tempfile::tempdir().unwrap();
        let run = |args: &[&str]| {
            assert!(
                Command::new("git")
                    .args(args)
                    .current_dir(dir.path())
                    .status()
                    .unwrap()
                    .success()
            );
        };
        run(&["init"]);

        let root = crate::session::store::fingerprint::normalized_worktree_root(dir.path())
            .expect("normalized worktree root resolves for an initialized repo");
        assert!(!root.is_empty());
    }

    #[test]
    fn lone_signal_less_range_capture_resolves_via_fail_open() {
        // A range capture with neither a worktree path nor any ref association.
        // It must keep resolving via bare `Current` (the no-regression guarantee).
        let events = [range_capture("r")];

        let resolved = current(
            &events,
            &ctx("/wt/a", Some("refs/heads/main")),
            RevisionScope::CurrentWorktree,
        )
        .expect("a signal-less range capture fails open to the current worktree");
        assert_eq!(resolved.revision_id.as_str(), "review-unit:sha256:r");
    }

    #[test]
    fn signal_less_range_capture_plus_worktree_capture_is_ambiguous() {
        // A matching worktree capture and a signal-less range capture both land in
        // scope (worktree-path match + fail-open), so bare `Current` stays
        // ambiguous rather than silently picking one.
        let events = [worktree_capture("w", "/wt/a"), range_capture("r")];

        let error =
            current(&events, &ctx("/wt/a", None), RevisionScope::CurrentWorktree).unwrap_err();
        assert!(error.to_string().contains("multiple captured revisions"));
    }

    #[test]
    fn ingested_capture_resolves_via_fail_open_in_dest_worktree() {
        // The capture's only worktree path is the origin's; in the destination
        // worktree it resolves via the ingest fail-open clause, not hidden.
        let events = [ingested_worktree_capture("i", "/origin/wt")];

        let resolved = current(
            &events,
            &ctx("/dest/wt", None),
            RevisionScope::CurrentWorktree,
        )
        .expect("an ingested capture fails open into the destination worktree");
        assert_eq!(resolved.revision_id.as_str(), "review-unit:sha256:i");
    }

    #[test]
    fn ingested_capture_plus_own_worktree_capture_is_ambiguous() {
        // A local capture matches by worktree path; an ingested foreign capture
        // fails open. Both in scope → the explicit-selection error, not a silent
        // pick.
        let events = [
            worktree_capture("local", "/dest/wt"),
            ingested_worktree_capture("foreign", "/origin/wt"),
        ];

        let error = current(
            &events,
            &ctx("/dest/wt", None),
            RevisionScope::CurrentWorktree,
        )
        .unwrap_err();
        assert!(error.to_string().contains("multiple captured revisions"));
    }

    #[test]
    fn worktree_ids_select_identity_matches_without_fail_open() {
        // A matching worktree capture, a sibling-root worktree capture, and a
        // signal-less range capture. The explicit worktree selector is the strict
        // identity match — only the matching worktree path is selected; the range
        // capture's fail-open does not widen it.
        let events = [
            worktree_capture("here", "/wt/a"),
            worktree_capture("there", "/wt/b"),
            range_capture("floaty"),
        ];

        let ids = revision_ids_in_worktree(&events, &ctx("/wt/a", None)).unwrap();

        assert!(ids.contains(&RevisionId::new("review-unit:sha256:here")));
        assert!(!ids.contains(&RevisionId::new("review-unit:sha256:there")));
        assert!(!ids.contains(&RevisionId::new("review-unit:sha256:floaty")));
    }

    mod change_replacement {
        use super::*;
        use crate::model::ChangeId;
        use crate::session::projection::change::{ChangeLifecycleV1, ChangeTopologyV1, ChangeView};

        fn rev(suffix: &str) -> RevisionId {
            RevisionId::new(format!("rev:sha256:{suffix}"))
        }

        fn roots(suffixes: &[&str]) -> SupersessionView {
            SupersessionView::from_edges(suffixes.iter().map(|suffix| (rev(suffix), Vec::new())))
        }

        fn set(suffixes: &[&str]) -> BTreeSet<RevisionId> {
            suffixes.iter().map(|suffix| rev(suffix)).collect()
        }

        /// A usable Change: `members` plus `(successor, predecessor)` edges, with
        /// the current set derived as "members no edge replaces".
        fn change(id: &str, members: &[&str], edges: &[(&str, &str)]) -> ChangeView {
            change_with_topology(id, members, edges, ChangeTopologyV1::Replacement)
        }

        fn change_with_topology(
            id: &str,
            members: &[&str],
            edges: &[(&str, &str)],
            topology: ChangeTopologyV1,
        ) -> ChangeView {
            let members: BTreeSet<RevisionId> = members.iter().map(|suffix| rev(suffix)).collect();
            let supersedes: BTreeSet<(RevisionId, RevisionId)> = edges
                .iter()
                .map(|(successor, predecessor)| (rev(successor), rev(predecessor)))
                .collect();
            let replaced: BTreeSet<&RevisionId> = supersedes
                .iter()
                .map(|(_, predecessor)| predecessor)
                .collect();
            let current_revisions = members
                .iter()
                .filter(|member| !replaced.contains(member))
                .cloned()
                .collect();
            ChangeView {
                change_id: ChangeId::new(format!("change:sha256:{id}")),
                members,
                current_revisions,
                supersedes,
                topology,
                lifecycle: ChangeLifecycleV1::InProgress,
                qualified_current_revisions: BTreeSet::new(),
                operative_obligations: BTreeSet::new(),
                diagnostics: Vec::new(),
            }
        }

        fn projection(views: Vec<ChangeView>) -> ChangeProjection {
            ChangeProjection {
                changes: views
                    .into_iter()
                    .map(|view| (view.change_id.clone(), view))
                    .collect(),
                links: Vec::new(),
            }
        }

        #[test]
        fn a_change_replaced_seed_names_its_current_successor() {
            let changes = projection(vec![change("x", &["a", "b"], &[("b", "a")])]);
            assert_eq!(
                change_replacing_revisions(&rev("a"), &roots(&["a", "b"]), &changes),
                Some(set(&["b"]))
            );
            assert_eq!(
                change_replacing_revisions(&rev("b"), &roots(&["a", "b"]), &changes),
                None
            );
        }

        #[test]
        fn a_multi_round_change_names_the_head_not_the_intermediate() {
            let changes = projection(vec![change(
                "x",
                &["a", "b", "c"],
                &[("b", "a"), ("c", "b")],
            )]);
            assert_eq!(
                change_replacing_revisions(&rev("a"), &roots(&["a", "b", "c"]), &changes),
                Some(set(&["c"]))
            );
        }

        #[test]
        fn a_store_without_change_claims_never_refuses() {
            // The proposal-borne edge keeps its legacy head-following.
            let legacy =
                SupersessionView::from_edges([(rev("a"), vec![]), (rev("b"), vec![rev("a")])]);
            assert_eq!(
                change_replacing_revisions(&rev("a"), &legacy, &ChangeProjection::default()),
                None
            );
        }

        #[test]
        fn a_proposal_borne_edge_carries_no_authority_once_a_change_claim_exists() {
            let legacy = SupersessionView::from_edges([
                (rev("a"), vec![]),
                (rev("b"), vec![rev("a")]),
                (rev("z"), vec![]),
            ]);
            let changes = projection(vec![change("x", &["z"], &[])]);
            assert_eq!(
                change_replacing_revisions(&rev("a"), &legacy, &changes),
                None
            );
        }

        #[test]
        fn a_seed_still_current_in_another_change_is_not_refused() {
            let changes = projection(vec![
                change("x", &["a"], &[]),
                change("y", &["a", "b"], &[("b", "a")]),
            ]);
            assert_eq!(
                change_replacing_revisions(&rev("a"), &roots(&["a", "b"]), &changes),
                None
            );
        }

        #[test]
        fn an_unusable_change_replaces_nothing() {
            for topology in [
                ChangeTopologyV1::Incomplete,
                ChangeTopologyV1::CycleConflicted,
            ] {
                let changes = projection(vec![change_with_topology(
                    "x",
                    &["a", "b"],
                    &[("b", "a")],
                    topology,
                )]);
                assert_eq!(
                    change_replacing_revisions(&rev("a"), &roots(&["a", "b"]), &changes),
                    None,
                    "{topology:?}"
                );
            }
        }

        #[test]
        fn two_changes_replacing_a_shared_seed_name_every_successor() {
            let changes = projection(vec![
                change("x", &["a", "b"], &[("b", "a")]),
                change("y", &["a", "c"], &[("c", "a")]),
            ]);
            assert_eq!(
                change_replacing_revisions(&rev("a"), &roots(&["a", "b", "c"]), &changes),
                Some(set(&["b", "c"]))
            );
        }

        #[test]
        fn crossed_histories_name_the_revision_both_end_at() {
            // Each Change is acyclic; only their union loops a <-> b.
            let changes = projection(vec![
                change("x", &["a", "b", "c"], &[("b", "a"), ("c", "b")]),
                change("y", &["a", "b", "c"], &[("a", "b"), ("c", "a")]),
            ]);
            let legacy = roots(&["a", "b", "c"]);
            assert_eq!(
                change_replacing_revisions(&rev("a"), &legacy, &changes),
                Some(set(&["c"]))
            );
            assert_eq!(
                change_replacing_revisions(&rev("b"), &legacy, &changes),
                Some(set(&["c"]))
            );
            assert_eq!(
                change_replacing_revisions(&rev("c"), &legacy, &changes),
                None
            );
        }

        #[test]
        fn a_sibling_head_that_does_not_replace_the_seed_is_not_named() {
            // b consolidates a and q; r replaces q on its own. r shares a's
            // thread but never replaced a, so only b is named.
            let changes = projection(vec![change(
                "x",
                &["a", "q", "b", "r"],
                &[("b", "a"), ("b", "q"), ("r", "q")],
            )]);
            assert_eq!(
                change_replacing_revisions(&rev("a"), &roots(&["a", "q", "b", "r"]), &changes),
                Some(set(&["b"]))
            );
        }

        #[test]
        fn the_refusal_is_typed_and_names_the_selectors() {
            let error = replaced_revision_error(&rev("a"), &set(&["b", "c"]));
            let message = error.to_string();
            assert!(
                message.starts_with("revision_replaced_by_change: "),
                "{message}"
            );
            assert!(message.contains("rev:sha256:a"), "{message}");
            assert!(
                message.contains("the current Revisions are rev:sha256:b, rev:sha256:c"),
                "{message}"
            );
            assert!(message.contains("--review-cursor"), "{message}");
            assert!(
                message.contains("--exact-revision rev:sha256:a"),
                "{message}"
            );
            let single = replaced_revision_error(&rev("a"), &set(&["b"])).to_string();
            assert!(
                single.contains("the current Revision is rev:sha256:b"),
                "{single}"
            );
        }
    }
}
