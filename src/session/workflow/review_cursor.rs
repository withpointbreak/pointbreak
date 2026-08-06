//! Pure, derived Review cursor and safe transition policy.
//!
//! A cursor binds one Change context, one exact captured Revision, the complete
//! selected current set, and a source-state proof. It is an optimistic
//! precondition, not journal authority, a lock, or a stored `current` pointer.

use base64::Engine as _;
use serde::{Deserialize, Serialize};

use crate::canonical_hash::{canonical_json_bytes, sha256_json_prefixed};
use crate::model::{ChangeId, DiffFile, ReviewEndpoint, RevisionId, RevisionRefV1, RevisionSource};
use crate::session::{ChangeDocumentProjectionV1, ChangeTopologyV1, ChangeView};

pub const REVIEW_CURSOR_SCHEMA_V1: &str = "pointbreak.review-cursor.v1";

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReviewSourcePathStateV1 {
    pub path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content_hash: Option<String>,
    pub mode: String,
    pub tracked: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorktreeSourceStateV1 {
    pub capture_mode: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base: Option<String>,
    pub path_scope: Vec<String>,
    pub paths: Vec<ReviewSourcePathStateV1>,
}

impl WorktreeSourceStateV1 {
    fn normalize(mut self) -> crate::error::Result<Self> {
        self.path_scope.sort();
        self.path_scope.dedup();
        self.paths.sort_by(|left, right| left.path.cmp(&right.path));
        if self
            .paths
            .windows(2)
            .any(|pair| pair[0].path == pair[1].path)
        {
            return Err(crate::error::ShoreError::Message(
                "source state contains a duplicate path".to_owned(),
            ));
        }
        Ok(self)
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ReviewSourceFingerprintV1(String);

impl ReviewSourceFingerprintV1 {
    pub fn as_str(&self) -> &str {
        &self.0
    }

    fn for_material(material: &impl Serialize) -> crate::error::Result<Self> {
        Ok(Self(sha256_json_prefixed(&serde_json::to_value(
            material,
        )?)?))
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum ReviewSourceBindingV1 {
    Captured,
    WorktreeMatchV1 {
        capture_mode: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        base: Option<String>,
        path_scope: Vec<String>,
        source_fingerprint: ReviewSourceFingerprintV1,
    },
    CommitMatchV1 {
        commit_oid: String,
        tree_oid: String,
        comparison_base: String,
        path_scope: Vec<String>,
        proof_state: CommitProofStateV1,
        #[serde(skip_serializing_if = "Option::is_none")]
        proof_ref: Option<String>,
        source_fingerprint: ReviewSourceFingerprintV1,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CommitProofStateV1 {
    Unknown,
    Exact,
    Equivalent,
    Extension,
    Unavailable,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ReviewSourceRequestV1 {
    Captured,
    Worktree,
    Commit(String),
}

/// Canonical Git state used by a commit-bound cursor.
///
/// The source fingerprint is derived from this complete typed state so every
/// axis that can change a commit comparison participates in compare-at-write
/// validation.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CommitSourceStateV1 {
    pub commit_oid: String,
    pub tree_oid: String,
    pub comparison_base: String,
    pub path_scope: Vec<String>,
    pub proof_state: CommitProofStateV1,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub proof_ref: Option<String>,
}

impl CommitSourceStateV1 {
    fn normalize(mut self) -> Self {
        self.path_scope.sort();
        self.path_scope.dedup();
        self
    }
}

impl ReviewSourceBindingV1 {
    /// Canonically bind the complete captured scope, including tracked and
    /// untracked paths, content, file modes, base, and capture mode.
    pub fn worktree(state: WorktreeSourceStateV1) -> crate::error::Result<Self> {
        let state = state.normalize()?;
        Ok(Self::WorktreeMatchV1 {
            capture_mode: state.capture_mode.clone(),
            base: state.base.clone(),
            path_scope: state.path_scope.clone(),
            source_fingerprint: ReviewSourceFingerprintV1::for_material(&state)?,
        })
    }

    /// Bind a commit comparison from one complete typed source state.
    pub fn commit(state: CommitSourceStateV1) -> crate::error::Result<Self> {
        let state = state.normalize();
        Ok(Self::CommitMatchV1 {
            commit_oid: state.commit_oid.clone(),
            tree_oid: state.tree_oid.clone(),
            comparison_base: state.comparison_base.clone(),
            path_scope: state.path_scope.clone(),
            proof_state: state.proof_state,
            proof_ref: state.proof_ref.clone(),
            source_fingerprint: ReviewSourceFingerprintV1::for_material(&state)?,
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReviewCursorV1 {
    pub schema: String,
    pub change_id: ChangeId,
    pub revision: RevisionRefV1,
    pub change_graph_token: String,
    pub selected_current_revisions: Vec<RevisionRefV1>,
    pub source_binding: ReviewSourceBindingV1,
    /// Successful selection proves this set empty. The field is retained in
    /// v1 so decoded tokens explain the fail-closed invariant; validation
    /// re-derives it rather than trusting token bytes.
    pub blocking_diagnostics: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReviewCursorSelectionV1 {
    pub cursor: ReviewCursorV1,
    pub token: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReviewCursorRefusalV1 {
    pub code: String,
    pub message: String,
    pub exact_candidates: Vec<RevisionRefV1>,
    pub diagnostics: Vec<String>,
}

impl std::fmt::Display for ReviewCursorRefusalV1 {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for ReviewCursorRefusalV1 {}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ReviewCursorTokenWireV1 {
    cursor: ReviewCursorV1,
    self_hash: String,
}

/// Select one exact Revision without silently resolving a fork or successor.
pub fn select_review_cursor(
    change: &ChangeView,
    projection: &ChangeDocumentProjectionV1,
    explicit_revision: Option<&RevisionId>,
    allow_historical: bool,
    source_binding: ReviewSourceBindingV1,
) -> std::result::Result<ReviewCursorSelectionV1, ReviewCursorRefusalV1> {
    let current = exact_refs(&change.current_revisions, projection)?;
    if matches!(
        change.topology,
        ChangeTopologyV1::ReplacementDivergent | ChangeTopologyV1::CycleConflicted
    ) || !change.diagnostics.is_empty()
    {
        return Err(refusal(
            "change_state_unresolved",
            "the Change graph is incomplete or conflicted",
            current,
            change.diagnostics.clone(),
        ));
    }

    let revision = match explicit_revision {
        None if current.len() == 1 => current[0].clone(),
        None => {
            return Err(refusal(
                "explicit_revision_required",
                "the Change has more than one current Revision",
                current,
                Vec::new(),
            ));
        }
        Some(revision_id) => {
            if !change.members.contains(revision_id) {
                return Err(refusal(
                    "revision_not_a_change_member",
                    "the exact Revision is not an active member of this Change",
                    current,
                    Vec::new(),
                ));
            }
            let reference = one_exact_ref(revision_id, projection)?;
            if !change.current_revisions.contains(revision_id)
                && (!allow_historical || source_binding != ReviewSourceBindingV1::Captured)
            {
                return Err(refusal(
                    "historical_revision_not_authorable",
                    "a historical Revision requires explicit captured-resource selection",
                    current,
                    Vec::new(),
                ));
            }
            reference
        }
    };
    let cursor = ReviewCursorV1 {
        schema: REVIEW_CURSOR_SCHEMA_V1.to_owned(),
        change_id: change.change_id.clone(),
        revision,
        change_graph_token: change_graph_token(change).map_err(internal_refusal)?,
        selected_current_revisions: current,
        source_binding,
        blocking_diagnostics: Vec::new(),
    };
    let token = cursor.encode_token().map_err(internal_refusal)?;
    Ok(ReviewCursorSelectionV1 { cursor, token })
}

impl ReviewCursorV1 {
    pub fn encode_token(&self) -> crate::error::Result<String> {
        let self_hash = sha256_json_prefixed(&serde_json::to_value(self)?)?;
        let bytes = canonical_json_bytes(&serde_json::to_value(ReviewCursorTokenWireV1 {
            cursor: self.clone(),
            self_hash,
        })?)?;
        Ok(base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes))
    }

    pub fn decode_token(token: &str) -> crate::error::Result<Self> {
        let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .decode(token)
            .map_err(|error| {
                crate::error::ShoreError::Message(format!("invalid Review cursor token: {error}"))
            })?;
        let wire: ReviewCursorTokenWireV1 = serde_json::from_slice(&bytes)?;
        if wire.cursor.schema != REVIEW_CURSOR_SCHEMA_V1
            || wire.self_hash != sha256_json_prefixed(&serde_json::to_value(&wire.cursor)?)?
        {
            return Err(crate::error::ShoreError::Message(
                "Review cursor token failed its self-hash or schema check".to_owned(),
            ));
        }
        Ok(wire.cursor)
    }
}

/// Recheck every optimistic precondition immediately before an append.
pub fn validate_review_cursor_for_write(
    token: &str,
    current_change: &ChangeView,
    current_projection: &ChangeDocumentProjectionV1,
    current_source_binding: &ReviewSourceBindingV1,
) -> std::result::Result<ReviewCursorV1, ReviewCursorRefusalV1> {
    let cursor = ReviewCursorV1::decode_token(token).map_err(|error| {
        refusal(
            "invalid_review_cursor",
            &error.to_string(),
            Vec::new(),
            Vec::new(),
        )
    })?;
    if cursor.change_id != current_change.change_id
        || cursor.change_graph_token
            != change_graph_token(current_change).map_err(internal_refusal)?
    {
        return Err(refusal(
            "change_graph_stale",
            "Change membership, replacement state, current set, or blocking diagnostics changed",
            cursor.selected_current_revisions,
            current_change.diagnostics.clone(),
        ));
    }
    if &cursor.source_binding != current_source_binding {
        return Err(refusal(
            "source_binding_mismatch",
            "the code, documentation, tests, generated files, modes, untracked set, or scope changed",
            cursor.selected_current_revisions,
            Vec::new(),
        ));
    }
    let selected = select_review_cursor(
        current_change,
        current_projection,
        Some(&cursor.revision.revision_id),
        false,
        current_source_binding.clone(),
    )?;
    if cursor.revision != selected.cursor.revision
        || cursor.selected_current_revisions != selected.cursor.selected_current_revisions
        || cursor.blocking_diagnostics != selected.cursor.blocking_diagnostics
    {
        return Err(refusal(
            "review_cursor_selection_stale",
            "the exact Revision, current candidate set, or blocking state changed",
            selected.cursor.selected_current_revisions,
            current_change.diagnostics.clone(),
        ));
    }
    Ok(cursor)
}

/// Recheck graph and exact-Revision authority for a transition whose purpose
/// is to leave the cursor's old mutable source state. Capture advancement and
/// proof-first landing use this path: the former expects the worktree to have
/// changed, while the latter compares the captured bytes with a commit.
pub(crate) fn validate_review_cursor_for_transition(
    token: &str,
    current_change: &ChangeView,
    current_projection: &ChangeDocumentProjectionV1,
) -> std::result::Result<ReviewCursorV1, ReviewCursorRefusalV1> {
    let cursor = ReviewCursorV1::decode_token(token).map_err(|error| {
        refusal(
            "invalid_review_cursor",
            &error.to_string(),
            Vec::new(),
            Vec::new(),
        )
    })?;
    if cursor.change_id != current_change.change_id
        || cursor.change_graph_token
            != change_graph_token(current_change).map_err(internal_refusal)?
    {
        return Err(refusal(
            "change_graph_stale",
            "Change membership, replacement state, current set, or blocking diagnostics changed",
            cursor.selected_current_revisions,
            current_change.diagnostics.clone(),
        ));
    }
    let selected = select_review_cursor(
        current_change,
        current_projection,
        Some(&cursor.revision.revision_id),
        false,
        cursor.source_binding.clone(),
    )?;
    if cursor.revision != selected.cursor.revision
        || cursor.selected_current_revisions != selected.cursor.selected_current_revisions
        || cursor.blocking_diagnostics != selected.cursor.blocking_diagnostics
    {
        return Err(refusal(
            "review_cursor_selection_stale",
            "the exact Revision, current candidate set, or blocking state changed",
            selected.cursor.selected_current_revisions,
            current_change.diagnostics.clone(),
        ));
    }
    Ok(cursor)
}

/// Resolve one safe writer cursor against the complete capable reader just
/// before a Revision-targeting append. The returned id is exact and never
/// follows a replacement edge. Mutable-source cursors require a caller that can
/// recompute their source binding; the current CLI selects captured-resource
/// cursors and therefore cannot accidentally claim a live checkout match.
pub(crate) fn exact_revision_from_review_cursor(
    repo: &std::path::Path,
    token: &str,
) -> crate::error::Result<RevisionId> {
    let cursor = ReviewCursorV1::decode_token(token)?;
    let state = crate::session::change_reader_state_for_repo(repo)?;
    let ready = state
        .ready()
        .ok_or_else(|| crate::error::ShoreError::WorkflowInputInvalid {
            reason: "complete Change authority is unavailable for the Review cursor".to_owned(),
        })?;
    let change = ready
        .projection
        .changes
        .get(&cursor.change_id)
        .ok_or_else(|| crate::error::ShoreError::WorkflowInputInvalid {
            reason: "the Review cursor Change is unavailable".to_owned(),
        })?;
    validate_review_cursor_for_transition(token, change, &ready.document_projection).map_err(
        |refusal| crate::error::ShoreError::WorkflowInputInvalid {
            reason: refusal.to_string(),
        },
    )?;
    let current_source_binding = current_review_source_binding(repo, &cursor)?;
    let validated = validate_review_cursor_for_write(
        token,
        change,
        &ready.document_projection,
        &current_source_binding,
    )
    .map_err(|refusal| crate::error::ShoreError::WorkflowInputInvalid {
        reason: refusal.to_string(),
    })?;
    Ok(validated.revision.revision_id)
}

/// Resolve a cursor for capture advancement or proof-first landing. These
/// operations deliberately cross away from the old worktree binding, so they
/// validate exact graph/artifact authority without pretending the old source
/// is still current.
pub(crate) fn exact_revision_from_transition_cursor(
    repo: &std::path::Path,
    token: &str,
) -> crate::error::Result<RevisionId> {
    let cursor = ReviewCursorV1::decode_token(token)?;
    let state = crate::session::change_reader_state_for_repo(repo)?;
    let ready = state
        .ready()
        .ok_or_else(|| crate::error::ShoreError::WorkflowInputInvalid {
            reason: "complete Change authority is unavailable for the Review cursor".to_owned(),
        })?;
    let change = ready
        .projection
        .changes
        .get(&cursor.change_id)
        .ok_or_else(|| crate::error::ShoreError::WorkflowInputInvalid {
            reason: "the Review cursor Change is unavailable".to_owned(),
        })?;
    let validated =
        validate_review_cursor_for_transition(token, change, &ready.document_projection).map_err(
            |refusal| crate::error::ShoreError::WorkflowInputInvalid {
                reason: refusal.to_string(),
            },
        )?;
    Ok(validated.revision.revision_id)
}

pub fn review_source_binding(
    repo: &std::path::Path,
    revision: &RevisionRefV1,
    request: ReviewSourceRequestV1,
) -> crate::error::Result<ReviewSourceBindingV1> {
    match request {
        ReviewSourceRequestV1::Captured => Ok(ReviewSourceBindingV1::Captured),
        ReviewSourceRequestV1::Worktree => worktree_source_binding(repo, revision),
        ReviewSourceRequestV1::Commit(revision_spec) => {
            commit_source_binding(repo, revision, &revision_spec)
        }
    }
}

fn current_review_source_binding(
    repo: &std::path::Path,
    cursor: &ReviewCursorV1,
) -> crate::error::Result<ReviewSourceBindingV1> {
    match &cursor.source_binding {
        ReviewSourceBindingV1::Captured => Ok(ReviewSourceBindingV1::Captured),
        ReviewSourceBindingV1::WorktreeMatchV1 { .. } => {
            worktree_source_binding(repo, &cursor.revision)
        }
        ReviewSourceBindingV1::CommitMatchV1 { commit_oid, .. } => {
            commit_source_binding(repo, &cursor.revision, commit_oid)
        }
    }
}

fn worktree_source_binding(
    repo: &std::path::Path,
    revision: &RevisionRefV1,
) -> crate::error::Result<ReviewSourceBindingV1> {
    let shown = exact_revision_source(repo, revision)?;
    let provenance = shown.revision.git_provenance.as_ref().ok_or_else(|| {
        crate::error::ShoreError::WorkflowInputInvalid {
            reason: "the exact Revision has no Git source to compare with the worktree".to_owned(),
        }
    })?;
    let (files, fingerprint) =
        super::capture::prepare_mutable_source_for_provenance(repo, provenance)?;
    if fingerprint.revision_id != revision.revision_id || files != shown.snapshot.files {
        return Err(crate::error::ShoreError::WorkflowInputInvalid {
            reason: "review_cursor_source_changed: the live worktree no longer matches the exact Revision capture mode and scope".to_owned(),
        });
    }
    ReviewSourceBindingV1::worktree(WorktreeSourceStateV1 {
        capture_mode: capture_mode_label(&fingerprint.source)?,
        base: endpoint_identity(&fingerprint.base),
        path_scope: source_path_scope(&fingerprint.source).to_vec(),
        paths: source_path_states(&files)?,
    })
}

fn commit_source_binding(
    repo: &std::path::Path,
    revision: &RevisionRefV1,
    commit_spec: &str,
) -> crate::error::Result<ReviewSourceBindingV1> {
    let shown = exact_revision_source(repo, revision)?;
    let provenance = shown.revision.git_provenance.as_ref().ok_or_else(|| {
        crate::error::ShoreError::WorkflowInputInvalid {
            reason: "the exact Revision has no Git source to compare with a commit".to_owned(),
        }
    })?;
    let commit_oid = crate::git::git_rev_parse_commit_oid(repo, commit_spec)?;
    let tree_oid = crate::git::git_commit_tree_oid(repo, &commit_oid)?;
    let comparison_base = endpoint_identity(&provenance.base).ok_or_else(|| {
        crate::error::ShoreError::WorkflowInputInvalid {
            reason: "the exact Revision has no stable Git base for commit comparison".to_owned(),
        }
    })?;
    let path_scope = source_path_scope(&provenance.source).to_vec();
    let candidate = crate::git::capture_commit_range_diff_files(
        repo,
        &comparison_base,
        &commit_oid,
        &path_scope,
    )?;
    if crate::session::evidence::canonical_candidate_diff_entries(&candidate, &shown.snapshot.files)
        != crate::session::evidence::canonical_diff_entries(&shown.snapshot.files)
    {
        return Err(crate::error::ShoreError::WorkflowInputInvalid {
            reason: "review_cursor_source_changed: the selected commit does not materialize the exact Revision capture mode and scope".to_owned(),
        });
    }
    let exact = matches!(
        &provenance.target,
        ReviewEndpoint::GitCommit {
            commit_oid: captured,
            ..
        } if captured == &commit_oid
    );
    ReviewSourceBindingV1::commit(CommitSourceStateV1 {
        commit_oid,
        tree_oid,
        comparison_base,
        path_scope,
        proof_state: if exact {
            CommitProofStateV1::Exact
        } else {
            CommitProofStateV1::Equivalent
        },
        proof_ref: None,
    })
}

fn exact_revision_source(
    repo: &std::path::Path,
    revision: &RevisionRefV1,
) -> crate::error::Result<crate::session::RevisionShowResult> {
    let shown = crate::session::show_revision_for_change_reader(
        crate::session::RevisionShowOptions::new(repo)
            .with_revision_id(revision.revision_id.clone())
            .with_exact(true),
    )?;
    if shown.revision.object_artifact_content_hash != revision.object_artifact_content_hash {
        return Err(crate::error::ShoreError::WorkflowInputInvalid {
            reason: "review_cursor_artifact_mismatch: the exact Revision artifact binding changed"
                .to_owned(),
        });
    }
    Ok(shown)
}

fn capture_mode_label(source: &RevisionSource) -> crate::error::Result<String> {
    let value = serde_json::to_value(source)?;
    let kind = value
        .get("kind")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("unknown");
    let mode = value
        .get("mode")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("unknown");
    let include_untracked = value
        .get("includeUntracked")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false);
    Ok(format!(
        "{kind}:{mode}:include_untracked={include_untracked}"
    ))
}

fn source_path_scope(source: &RevisionSource) -> &[String] {
    match source {
        RevisionSource::GitWorktree { pathspecs, .. }
        | RevisionSource::GitCommitRange { pathspecs, .. }
        | RevisionSource::GitRootCommit { pathspecs, .. }
        | RevisionSource::GitStaged { pathspecs, .. }
        | RevisionSource::GitUnstaged { pathspecs, .. } => pathspecs,
    }
}

fn endpoint_identity(endpoint: &ReviewEndpoint) -> Option<String> {
    match endpoint {
        ReviewEndpoint::GitCommit { commit_oid, .. } => Some(commit_oid.clone()),
        ReviewEndpoint::GitTree { tree_oid } | ReviewEndpoint::GitIndex { tree_oid } => {
            Some(tree_oid.clone())
        }
        ReviewEndpoint::GitWorkingTree { .. } => None,
    }
}

fn source_path_states(files: &[DiffFile]) -> crate::error::Result<Vec<ReviewSourcePathStateV1>> {
    files
        .iter()
        .map(|file| {
            let path = file
                .new_path
                .as_ref()
                .or(file.old_path.as_ref())
                .ok_or_else(|| crate::error::ShoreError::WorkflowInputInvalid {
                    reason: "captured source entry has no path".to_owned(),
                })?;
            Ok(ReviewSourcePathStateV1 {
                path: path.clone(),
                content_hash: Some(sha256_json_prefixed(&serde_json::to_value(file)?)?),
                mode: format!(
                    "{}->{}",
                    file.old_mode.as_deref().unwrap_or("none"),
                    file.new_mode.as_deref().unwrap_or("none")
                ),
                tracked: !file.synthetic,
            })
        })
        .collect()
}

/// Digest only authoritative Change graph state. Review facts, timestamps,
/// labels, and duplicate carriers cannot make this token stale.
pub fn change_graph_token(change: &ChangeView) -> crate::error::Result<String> {
    sha256_json_prefixed(&serde_json::json!({
        "changeId": change.change_id,
        "members": change.members,
        "effectiveSupersedes": change.supersedes,
        "currentRevisions": change.current_revisions,
        "blockingDiagnostics": change.diagnostics,
    }))
}

fn exact_refs(
    revisions: &std::collections::BTreeSet<RevisionId>,
    projection: &ChangeDocumentProjectionV1,
) -> std::result::Result<Vec<RevisionRefV1>, ReviewCursorRefusalV1> {
    revisions
        .iter()
        .map(|revision_id| one_exact_ref(revision_id, projection))
        .collect()
}

fn one_exact_ref(
    revision_id: &RevisionId,
    projection: &ChangeDocumentProjectionV1,
) -> std::result::Result<RevisionRefV1, ReviewCursorRefusalV1> {
    match projection.revision_refs.get(revision_id) {
        Some(references) if references.len() == 1 => Ok(references[0].clone()),
        Some(references) => Err(refusal(
            "revision_artifact_conflicted",
            "the Revision has more than one object artifact hash",
            references.clone(),
            Vec::new(),
        )),
        None => Err(refusal(
            "revision_artifact_missing",
            "the Revision object artifact is unavailable",
            Vec::new(),
            Vec::new(),
        )),
    }
}

fn refusal(
    code: &str,
    message: &str,
    exact_candidates: Vec<RevisionRefV1>,
    diagnostics: Vec<String>,
) -> ReviewCursorRefusalV1 {
    ReviewCursorRefusalV1 {
        code: code.to_owned(),
        message: message.to_owned(),
        exact_candidates,
        diagnostics,
    }
}

fn internal_refusal(error: crate::error::ShoreError) -> ReviewCursorRefusalV1 {
    refusal(
        "review_cursor_internal_error",
        &error.to_string(),
        Vec::new(),
        Vec::new(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::{ChangeLifecycleV1, ChangeTopologyV1};

    fn reference(name: &str, byte: char) -> RevisionRefV1 {
        RevisionRefV1::new(
            RevisionId::new(format!("rev:sha256:{name}")),
            format!("sha256:{}", byte.to_string().repeat(64)),
        )
        .unwrap()
    }

    fn projection(references: &[RevisionRefV1]) -> ChangeDocumentProjectionV1 {
        ChangeDocumentProjectionV1 {
            revision_refs: references
                .iter()
                .map(|reference| (reference.revision_id.clone(), vec![reference.clone()]))
                .collect(),
            unavailable_revision_refs: Default::default(),
            membership_claims: Vec::new(),
            relation_claims: Vec::new(),
            diagnostics: Vec::new(),
            projection_stamp: "sha256:projection".to_owned(),
        }
    }

    fn view(references: &[RevisionRefV1]) -> ChangeView {
        ChangeView {
            change_id: ChangeId::new("change:sha256:cursor"),
            members: references
                .iter()
                .map(|reference| reference.revision_id.clone())
                .collect(),
            current_revisions: references
                .iter()
                .map(|reference| reference.revision_id.clone())
                .collect(),
            supersedes: Default::default(),
            topology: if references.len() > 1 {
                ChangeTopologyV1::ParallelCurrent
            } else {
                ChangeTopologyV1::Initial
            },
            lifecycle: ChangeLifecycleV1::InProgress,
            qualified_current_revisions: Default::default(),
            operative_obligations: Default::default(),
            diagnostics: Vec::new(),
        }
    }

    fn source_state() -> WorktreeSourceStateV1 {
        WorktreeSourceStateV1 {
            capture_mode: "worktree".to_owned(),
            base: Some("base-a".to_owned()),
            path_scope: vec!["src".to_owned(), "docs".to_owned()],
            paths: vec![
                ReviewSourcePathStateV1 {
                    path: "docs/guide.md".to_owned(),
                    content_hash: Some("sha256:docs".to_owned()),
                    mode: "100644".to_owned(),
                    tracked: true,
                },
                ReviewSourcePathStateV1 {
                    path: "src/lib.rs".to_owned(),
                    content_hash: Some("sha256:code".to_owned()),
                    mode: "100644".to_owned(),
                    tracked: true,
                },
            ],
        }
    }

    #[test]
    fn a_change_only_selector_refuses_parallel_current_and_lists_exact_candidates() {
        let revisions = [reference("a", 'a'), reference("b", 'b')];
        let error = select_review_cursor(
            &view(&revisions),
            &projection(&revisions),
            None,
            false,
            ReviewSourceBindingV1::Captured,
        )
        .unwrap_err();
        assert_eq!(error.code, "explicit_revision_required");
        assert_eq!(error.exact_candidates, revisions);

        let selected = select_review_cursor(
            &view(&revisions),
            &projection(&revisions),
            Some(&revisions[1].revision_id),
            false,
            ReviewSourceBindingV1::Captured,
        )
        .unwrap();
        assert_eq!(selected.cursor.revision, revisions[1]);
        assert_eq!(selected.cursor.selected_current_revisions, revisions);
    }

    #[test]
    fn cursor_token_is_self_hashed_and_graph_or_source_changes_refuse_before_write() {
        let revisions = [reference("a", 'a')];
        let source = ReviewSourceBindingV1::worktree(source_state()).unwrap();
        let change = view(&revisions);
        let selected = select_review_cursor(
            &change,
            &projection(&revisions),
            None,
            false,
            source.clone(),
        )
        .unwrap();
        assert_eq!(
            ReviewCursorV1::decode_token(&selected.token).unwrap(),
            selected.cursor
        );

        let mut changed_state = source_state();
        changed_state.paths[0].content_hash = Some("sha256:changed".to_owned());
        let changed_source = ReviewSourceBindingV1::worktree(changed_state).unwrap();
        assert_eq!(
            validate_review_cursor_for_write(
                &selected.token,
                &change,
                &projection(&revisions),
                &changed_source,
            )
            .unwrap_err()
            .code,
            "source_binding_mismatch"
        );

        let mut changed_graph = change;
        changed_graph
            .diagnostics
            .push("change_incomplete".to_owned());
        assert_eq!(
            validate_review_cursor_for_write(
                &selected.token,
                &changed_graph,
                &projection(&revisions),
                &source,
            )
            .unwrap_err()
            .code,
            "change_graph_stale"
        );
    }

    #[test]
    fn worktree_binding_covers_capture_mode_base_scope_and_every_source_class() {
        let revisions = [reference("binding", 'c')];
        let change = view(&revisions);
        let projection = projection(&revisions);
        let source = ReviewSourceBindingV1::worktree(source_state()).unwrap();
        let selected =
            select_review_cursor(&change, &projection, None, false, source.clone()).unwrap();

        let mut states = Vec::new();
        let mut changed = source_state();
        changed.capture_mode = "staged".to_owned();
        states.push(changed);
        let mut changed = source_state();
        changed.base = Some("base-b".to_owned());
        states.push(changed);
        let mut changed = source_state();
        changed.path_scope = vec!["src".to_owned()];
        states.push(changed);
        for (path, changed_class) in [
            ("tests/flow.rs", "tests"),
            ("generated/schema.rs", "generated"),
        ] {
            let mut changed = source_state();
            changed.paths.push(ReviewSourcePathStateV1 {
                path: path.to_owned(),
                content_hash: Some(format!("sha256:changed-{changed_class}")),
                mode: "100644".to_owned(),
                tracked: true,
            });
            states.push(changed);
        }
        for index in 0..2 {
            let mut changed = source_state();
            changed.paths[index].content_hash = Some("sha256:changed-content".to_owned());
            states.push(changed);
        }
        let mut changed = source_state();
        changed.paths[0].mode = "100755".to_owned();
        states.push(changed);
        let mut changed = source_state();
        changed.paths.push(ReviewSourcePathStateV1 {
            path: "notes.tmp".to_owned(),
            content_hash: Some("sha256:untracked".to_owned()),
            mode: "100644".to_owned(),
            tracked: false,
        });
        states.push(changed);
        let mutations = states
            .into_iter()
            .map(|state| ReviewSourceBindingV1::worktree(state).unwrap());
        for mutation in mutations {
            assert_eq!(
                validate_review_cursor_for_write(&selected.token, &change, &projection, &mutation,)
                    .unwrap_err()
                    .code,
                "source_binding_mismatch"
            );
        }
    }

    #[test]
    fn commit_binding_covers_every_typed_comparison_axis() {
        let state = CommitSourceStateV1 {
            commit_oid: "1".repeat(40),
            tree_oid: "2".repeat(40),
            comparison_base: "3".repeat(40),
            path_scope: vec!["src".to_owned(), "docs".to_owned()],
            proof_state: CommitProofStateV1::Exact,
            proof_ref: Some("proof:sha256:one".to_owned()),
        };
        let original = ReviewSourceBindingV1::commit(state.clone()).unwrap();
        let mut mutations = Vec::new();
        let mut changed = state.clone();
        changed.commit_oid = "4".repeat(40);
        mutations.push(changed);
        let mut changed = state.clone();
        changed.tree_oid = "5".repeat(40);
        mutations.push(changed);
        let mut changed = state.clone();
        changed.comparison_base = "6".repeat(40);
        mutations.push(changed);
        let mut changed = state.clone();
        changed.path_scope = vec!["src".to_owned()];
        mutations.push(changed);
        let mut changed = state.clone();
        changed.proof_state = CommitProofStateV1::Equivalent;
        mutations.push(changed);
        let mut changed = state;
        changed.proof_ref = Some("proof:sha256:changed".to_owned());
        mutations.push(changed);

        for mutation in mutations {
            assert_ne!(ReviewSourceBindingV1::commit(mutation).unwrap(), original);
        }
    }

    #[test]
    fn token_tampering_and_replacement_divergence_fail_closed() {
        let revisions = [reference("left", 'd'), reference("right", 'e')];
        let mut change = view(&revisions);
        change.topology = ChangeTopologyV1::ReplacementDivergent;
        assert_eq!(
            select_review_cursor(
                &change,
                &projection(&revisions),
                Some(&revisions[0].revision_id),
                false,
                ReviewSourceBindingV1::Captured,
            )
            .unwrap_err()
            .code,
            "change_state_unresolved"
        );

        let singleton = [reference("single", 'f')];
        let selected = select_review_cursor(
            &view(&singleton),
            &projection(&singleton),
            None,
            false,
            ReviewSourceBindingV1::Captured,
        )
        .unwrap();
        let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .decode(&selected.token)
            .unwrap();
        let mut wire: ReviewCursorTokenWireV1 = serde_json::from_slice(&bytes).unwrap();
        wire.cursor.blocking_diagnostics.push("tampered".to_owned());
        let tampered = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .encode(canonical_json_bytes(&serde_json::to_value(wire).unwrap()).unwrap());
        assert!(ReviewCursorV1::decode_token(&tampered).is_err());

        let selected = select_review_cursor(
            &view(&singleton),
            &projection(&singleton),
            None,
            false,
            ReviewSourceBindingV1::Captured,
        )
        .unwrap();
        let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .decode(&selected.token)
            .unwrap();
        let mut wire: ReviewCursorTokenWireV1 = serde_json::from_slice(&bytes).unwrap();
        wire.cursor.revision = reference("attacker", '9');
        wire.self_hash =
            sha256_json_prefixed(&serde_json::to_value(&wire.cursor).unwrap()).unwrap();
        let coherently_rehashed = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .encode(canonical_json_bytes(&serde_json::to_value(wire).unwrap()).unwrap());
        assert_eq!(
            validate_review_cursor_for_write(
                &coherently_rehashed,
                &view(&singleton),
                &projection(&singleton),
                &ReviewSourceBindingV1::Captured,
            )
            .unwrap_err()
            .code,
            "revision_not_a_change_member"
        );
    }
}
