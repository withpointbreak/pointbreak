//! Read-time reachability enrichment for the commit-range lifecycle.
//!
//! This is the **only** place git touches the lifecycle: the pure projection
//! never imports it. Given a unit's current commit associations and a repo, it
//! joins each OID against the live-ref set to derive merged/live/orphaned, plus
//! a landing headline and the divergence verdict. Landing claims are the
//! association-source edges (the capture target is provenance, never a claim);
//! a chain of successive landings collapses to its tip via one
//! `merge-base --independent` call, and `divergent_commit_association` fires
//! only when two or more incomparable claims are each live or merged with
//! distinct trees — competing to be the same landing (ADR-0014, 2026-07-09
//! amendment).

use std::cell::RefCell;
use std::collections::{BTreeSet, HashMap, HashSet};
use std::path::Path;

use serde::Serialize;

use crate::error::Result;
use crate::git::{
    Ancestry, git_default_branch_ref, git_for_each_ref, git_independent_commits, git_is_ancestor,
    git_object_exists, git_ref_state_lines, git_rev_list_reachable, git_rev_parse_commit_oid,
    git_worktree_list,
};
use crate::session::projection::commit_range::DIVERGENT_COMMIT_ASSOCIATION_CODE;
use crate::session::state::ProjectionDiagnostic;
use crate::session::{CommitEdgeSource, RevisionCommitRangeView};

const SHORT_OID_LEN: usize = 12;

/// Why a commit is orphaned: its object is gone, or it exists but no live ref
/// reaches it.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum OrphanReason {
    ObjectMissing,
    Unreachable,
}

/// A commit's relationship to the live commit graph. Internally tagged on
/// `condition` so `Orphaned` can carry its reason in the same object; distinct
/// from `ResolutionStatus::Orphaned` (both serialize an `"orphaned"` token but
/// never share an object).
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case", tag = "condition")]
pub enum CommitGraphCondition {
    Merged,
    Live,
    Orphaned { reason: OrphanReason },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CommitLiveness {
    pub commit_oid: String,
    #[serde(flatten)]
    pub condition: CommitGraphCondition,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub live_branch: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LivenessEnrichment {
    pub per_commit: Vec<CommitLiveness>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub headline: Option<CommitGraphCondition>,
    /// Enrichment-level diagnostics — today only `divergent_commit_association`,
    /// which needs ancestry and therefore cannot come from the git-free fold.
    /// Read surfaces merge these into the same per-unit diagnostics they already
    /// render.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub diagnostics: Vec<ProjectionDiagnostic>,
}

/// Joins `view`'s current commit associations against the repo's live-ref set.
///
/// Broad default (`integration_ref == None`): a commit is merged when it is an
/// ancestor of some live tip other than itself, live when it is itself a tip,
/// else orphaned. Narrow (`Some(r)`): merged only when an ancestor of `r`. A
/// floating unit (no current commits) short-circuits with an empty result. Any
/// git error other than a missing object propagates, so the caller can render
/// "reachability unknown".
pub fn enrich_liveness(
    view: &RevisionCommitRangeView,
    repo: &Path,
    integration_ref: Option<&str>,
) -> Result<LivenessEnrichment> {
    if view.current_commits.is_empty() {
        return Ok(LivenessEnrichment {
            per_commit: Vec::new(),
            headline: None,
            diagnostics: Vec::new(),
        });
    }

    let tips = live_tips(repo)?;
    let integration = match integration_ref {
        Some(reference) => Some(IntegrationRef {
            oid: git_rev_parse_commit_oid(repo, reference)?,
            label: short_ref_label(reference),
        }),
        None => None,
    };
    // Broad mode only: resolve the repo's integration/default branch so a
    // broad-merged commit reads its landing branch rather than an arbitrary
    // ref-sorted tip that merely contains main's history (#445). We keep the
    // resolved ref's **own** label alongside its OID — the live-tip set dedups by
    // OID in ref order, so recovering the label from a tip could return a
    // same-OID alias that sorts earlier. Best-effort — a repo with no detectable
    // default falls back to the ref-order walk.
    let default_branch = match integration_ref {
        Some(_) => None,
        None => git_default_branch_ref(repo)
            .ok()
            .flatten()
            .and_then(|reference| {
                git_rev_parse_commit_oid(repo, &reference)
                    .ok()
                    .map(|oid| (oid, short_ref_label(&reference)))
            }),
    };

    let mut cache = HashMap::new();
    let mut seen = HashSet::new();
    let mut per_commit = Vec::new();
    for commit in &view.current_commits {
        if !seen.insert(commit.commit_oid.clone()) {
            continue;
        }
        let (condition, live_branch) = classify(
            repo,
            &commit.commit_oid,
            &tips,
            integration.as_ref(),
            default_branch
                .as_ref()
                .map(|(oid, label)| (oid.as_str(), label.as_str())),
            &mut cache,
        )?;
        per_commit.push(CommitLiveness {
            commit_oid: commit.commit_oid.clone(),
            condition,
            live_branch,
        });
    }

    let (headline, diagnostics) = landing_analysis(view, &per_commit, &mut |oids| {
        git_independent_commits(repo, oids)
    })?;
    Ok(LivenessEnrichment {
        per_commit,
        headline,
        diagnostics,
    })
}

/// The repository's integration/default branch as a full ref, for a caller that
/// wants a "did this land on the default branch?" merge answer — the `revision
/// show` narrow default. Best-effort: `None` when no default branch is detectable
/// (no `origin/HEAD`, no local `main`/`master`), in which case the caller keeps
/// broad reachability rather than a fabricated ref.
pub fn resolve_default_integration_ref(repo: &Path) -> Option<String> {
    git_default_branch_ref(repo).ok().flatten()
}

/// The effective reachability target for a merged/open/orphaned readout: the
/// caller's explicit integration ref when given, else the repository's detected
/// default branch, else `None` (broad reachability — any live tip). The single
/// policy point for the narrow-by-default merge answer, so every read surface
/// (`revision show`, `revision list`, the association digest) agrees on what
/// "merged" means (#466).
pub fn effective_integration_ref(repo: &Path, explicit: Option<&str>) -> Option<String> {
    match explicit {
        Some(reference) => Some(reference.to_owned()),
        None => resolve_default_integration_ref(repo),
    }
}

/// A change-detection stamp over every git input the commit-graph liveness
/// reads: branch and remote ref tips with their symref targets (which drive
/// default-branch detection, #466), plus linked-worktree HEADs (the only way a
/// detached-worktree commit counts as live). Equal ref states always stamp
/// identically; any ref move, branch create/delete, fetch, or worktree HEAD
/// change produces a different stamp. Two git spawns, no ancestry walks — this
/// detects change for cache keys and freshness polls (#467), it does not
/// classify.
pub fn commit_graph_stamp(repo: &Path) -> Result<String> {
    let mut input = git_ref_state_lines(repo)?;
    input.push_str("\n--worktrees--\n");
    for worktree in git_worktree_list(repo)? {
        let head = worktree.head.as_deref().unwrap_or("-");
        let branch = worktree.branch.as_deref().unwrap_or("-");
        input.push_str(head);
        input.push(' ');
        input.push_str(branch);
        input.push('\n');
    }
    Ok(crate::canonical_hash::sha256_bytes_hex(input.as_bytes()))
}

/// Reachability resolved **once** for an entire revision list, so classifying each
/// revision's commits is in-memory set membership rather than a git ancestry probe
/// per (commit, tip) pair. The live tips and the set of commits reachable from them
/// come from one `git for-each-ref` + one `git worktree list` + one `git rev-list`;
/// an optional integration ref adds one `rev-parse` + one `rev-list`. The only
/// per-commit git calls left are the rare ones the membership cannot answer — a
/// capture commit that is itself a tip needs an ancestor check to split merged from
/// live, and a commit absent from the reachable set needs one object-existence
/// check — and both are cached across the whole list.
///
/// The per-commit **conditions** (and therefore the merge-status headline and the
/// orphan-visibility decision) match [`enrich_liveness`] exactly. The `live_branch`
/// label is best-effort: it is populated for a live tip and for an
/// integration-merged commit, but a broad-merged commit carries `None` (the
/// containing tip is the one thing the O(1) reachability does not name). The
/// list-surface consumers read only the condition, never the label, so this is the
/// right trade; single-view callers that need the label keep using
/// [`enrich_liveness`].
pub(crate) struct LivenessBatch {
    tips: Vec<LiveTip>,
    broad_reachable: HashSet<String>,
    integration: Option<IntegrationRef>,
    integration_reachable: HashSet<String>,
    ancestry: RefCell<HashMap<(String, String), Ancestry>>,
    object_exists: RefCell<HashMap<String, bool>>,
    /// Maximal-claims memo keyed by the sorted claim set: sibling captures that
    /// converge on the same landing claims share one `merge-base --independent`
    /// spawn across the whole list.
    independent: RefCell<HashMap<Vec<String>, Vec<String>>>,
}

impl LivenessBatch {
    /// Resolve the live tips and the reachable set(s) once for the whole list. A
    /// git failure here propagates; the list surface degrades it to "reachability
    /// unknown" for every entry, the same graceful fallback the per-entry path
    /// applies when git is unavailable.
    pub(crate) fn build(repo: &Path, integration_ref: Option<&str>) -> Result<Self> {
        let tips = live_tips(repo)?;
        let tip_oids: Vec<String> = tips.iter().map(|tip| tip.oid.clone()).collect();
        let broad_reachable = git_rev_list_reachable(repo, &tip_oids)?;
        let (integration, integration_reachable) = match integration_ref {
            Some(reference) => {
                let oid = git_rev_parse_commit_oid(repo, reference)?;
                let reachable = git_rev_list_reachable(repo, std::slice::from_ref(&oid))?;
                (
                    Some(IntegrationRef {
                        oid,
                        label: short_ref_label(reference),
                    }),
                    reachable,
                )
            }
            None => (None, HashSet::new()),
        };
        Ok(Self {
            tips,
            broad_reachable,
            integration,
            integration_reachable,
            ancestry: RefCell::new(HashMap::new()),
            object_exists: RefCell::new(HashMap::new()),
            independent: RefCell::new(HashMap::new()),
        })
    }

    /// Broad-default enrichment — every live tip is a merge target — the form the
    /// orphan-visibility filter reads. Mirrors `enrich_liveness(view, repo, None)`.
    pub(crate) fn enrich_broad(
        &self,
        repo: &Path,
        view: &RevisionCommitRangeView,
    ) -> Result<LivenessEnrichment> {
        self.enrich(repo, view, false)
    }

    /// Integration-scoped enrichment — merged only against the integration ref when
    /// one is set, else identical to broad — the form merge-status reads. Mirrors
    /// `enrich_liveness(view, repo, integration_ref)`.
    pub(crate) fn enrich_merge(
        &self,
        repo: &Path,
        view: &RevisionCommitRangeView,
    ) -> Result<LivenessEnrichment> {
        self.enrich(repo, view, true)
    }

    fn enrich(
        &self,
        repo: &Path,
        view: &RevisionCommitRangeView,
        use_integration: bool,
    ) -> Result<LivenessEnrichment> {
        if view.current_commits.is_empty() {
            return Ok(LivenessEnrichment {
                per_commit: Vec::new(),
                headline: None,
                diagnostics: Vec::new(),
            });
        }
        let mut seen = HashSet::new();
        let mut per_commit = Vec::new();
        for commit in &view.current_commits {
            if !seen.insert(commit.commit_oid.clone()) {
                continue;
            }
            let (condition, live_branch) =
                self.classify(repo, &commit.commit_oid, use_integration)?;
            per_commit.push(CommitLiveness {
                commit_oid: commit.commit_oid.clone(),
                condition,
                live_branch,
            });
        }
        let (headline, diagnostics) = landing_analysis(view, &per_commit, &mut |oids| {
            self.independent_of(repo, oids)
        })?;
        Ok(LivenessEnrichment {
            per_commit,
            headline,
            diagnostics,
        })
    }

    fn independent_of(&self, repo: &Path, oids: &[String]) -> Result<Vec<String>> {
        let mut key: Vec<String> = oids.to_vec();
        key.sort();
        if let Some(maximal) = self.independent.borrow().get(&key) {
            return Ok(maximal.clone());
        }
        let maximal = git_independent_commits(repo, oids)?;
        self.independent.borrow_mut().insert(key, maximal.clone());
        Ok(maximal)
    }

    fn classify(
        &self,
        repo: &Path,
        commit_oid: &str,
        use_integration: bool,
    ) -> Result<(CommitGraphCondition, Option<String>)> {
        // Mode-specific MERGED determination. When the commit is merged under this
        // mode, return here; otherwise fall through to the shared live-tip/orphaned
        // check below — the same fall-through `enrich_liveness` applies after its
        // integration check, which keeps a live side-branch tip `Live` rather than
        // orphaning it just because it is not reachable from the integration ref.
        if use_integration && let Some(integration) = &self.integration {
            // `integration_reachable` is `git rev-list <ref>`, which includes the
            // ref's own tip — so a commit sitting at the integration tip is merged
            // into it, matching git's `merge-base --is-ancestor` equality (#447)
            // and the per-entry `enrich_liveness` narrow arm.
            if self.integration_reachable.contains(commit_oid) {
                return Ok((
                    CommitGraphCondition::Merged,
                    Some(integration.label.clone()),
                ));
            }
            // Not merged into the integration ref — shared fall-through below.
        } else if self.broad_reachable.contains(commit_oid)
            && !self.tips.iter().any(|tip| tip.oid == commit_oid)
        {
            // Broad default: a reachable commit that is not itself a tip is a proper
            // ancestor of some tip, hence merged.
            return Ok((CommitGraphCondition::Merged, None));
        }

        // Shared fall-through. A commit that is itself a live tip is `Live` — except,
        // in broad mode, a tip that is also an ancestor of another tip is `Merged`
        // (the merged-before-live order). Run that disambiguation unless we are in
        // *active* narrow mode (an integration ref is set): merge-status passes
        // `use_integration` even with no ref, and that case is the broad default, so
        // it must still disambiguate. In active narrow mode the merged check already
        // ran, so a tip here is simply `Live`.
        let narrow = use_integration && self.integration.is_some();
        if let Some(tip) = self.tips.iter().find(|tip| tip.oid == commit_oid) {
            if !narrow {
                for other in &self.tips {
                    if other.oid == commit_oid {
                        continue;
                    }
                    if self.ancestry_of(repo, commit_oid, &other.oid)? == Ancestry::Ancestor {
                        return Ok((CommitGraphCondition::Merged, other.label.clone()));
                    }
                }
            }
            return Ok((CommitGraphCondition::Live, tip.label.clone()));
        }

        self.orphaned(repo, commit_oid)
    }

    /// A commit absent from the reachable set: orphaned, by a missing object or a
    /// live-but-unreachable one. The object-existence probe is cached across the
    /// list (the same swept commit re-binds across sibling captures).
    fn orphaned(
        &self,
        repo: &Path,
        commit_oid: &str,
    ) -> Result<(CommitGraphCondition, Option<String>)> {
        let reason = if self.object_exists_of(repo, commit_oid)? {
            OrphanReason::Unreachable
        } else {
            OrphanReason::ObjectMissing
        };
        Ok((CommitGraphCondition::Orphaned { reason }, None))
    }

    fn ancestry_of(
        &self,
        repo: &Path,
        ancestor_oid: &str,
        descendant_oid: &str,
    ) -> Result<Ancestry> {
        let key = (ancestor_oid.to_owned(), descendant_oid.to_owned());
        if let Some(ancestry) = self.ancestry.borrow().get(&key) {
            return Ok(*ancestry);
        }
        let ancestry = git_is_ancestor(repo, ancestor_oid, descendant_oid)?;
        self.ancestry.borrow_mut().insert(key, ancestry);
        Ok(ancestry)
    }

    fn object_exists_of(&self, repo: &Path, commit_oid: &str) -> Result<bool> {
        if let Some(exists) = self.object_exists.borrow().get(commit_oid) {
            return Ok(*exists);
        }
        let exists = git_object_exists(repo, commit_oid)?;
        self.object_exists
            .borrow_mut()
            .insert(commit_oid.to_owned(), exists);
        Ok(exists)
    }
}

struct LiveTip {
    oid: String,
    label: Option<String>,
}

struct IntegrationRef {
    oid: String,
    label: String,
}

/// The live-tip set: branch and remote-tracking tips (matched by prefix so
/// nested names like `feat/x` are included), plus linked-worktree HEADs — the
/// only way a detached-worktree commit counts as live. Deduped by OID.
fn live_tips(repo: &Path) -> Result<Vec<LiveTip>> {
    let mut tips = Vec::new();
    let mut seen = HashSet::new();

    for entry in git_for_each_ref(repo, &["refs/heads/", "refs/remotes/"])? {
        if seen.insert(entry.oid.clone()) {
            tips.push(LiveTip {
                label: Some(short_ref_label(&entry.name)),
                oid: entry.oid,
            });
        }
    }

    for worktree in git_worktree_list(repo)? {
        let Some(head) = worktree.head else {
            continue;
        };
        let label = match &worktree.branch {
            Some(branch) => Some(short_ref_label(branch)),
            None => Some(detached_label(&head)),
        };
        if seen.insert(head.clone()) {
            tips.push(LiveTip { oid: head, label });
        }
    }

    Ok(tips)
}

fn classify(
    repo: &Path,
    commit_oid: &str,
    tips: &[LiveTip],
    integration: Option<&IntegrationRef>,
    default_branch: Option<(&str, &str)>,
    cache: &mut HashMap<(String, String), Ancestry>,
) -> Result<(CommitGraphCondition, Option<String>)> {
    if !git_object_exists(repo, commit_oid)? {
        return Ok((
            CommitGraphCondition::Orphaned {
                reason: OrphanReason::ObjectMissing,
            },
            None,
        ));
    }

    if let Some(integration) = integration {
        // Narrow: merged into the integration ref when the commit is an ancestor
        // of it — and equality counts, matching git's own `merge-base
        // --is-ancestor` (a commit sitting at the integration tip has landed on
        // it, #447). Everything else falls through to the live-tip check below, so
        // a live side-branch tip stays `Live` rather than orphaned.
        if integration.oid == commit_oid
            || ancestry(repo, commit_oid, &integration.oid, cache)? == Ancestry::Ancestor
        {
            return Ok((
                CommitGraphCondition::Merged,
                Some(integration.label.clone()),
            ));
        }
    } else if let Some(label) = broad_merged_label(repo, commit_oid, tips, default_branch, cache)? {
        return Ok((CommitGraphCondition::Merged, label));
    }

    if let Some(tip) = tips.iter().find(|tip| tip.oid == commit_oid) {
        return Ok((CommitGraphCondition::Live, tip.label.clone()));
    }

    Ok((
        CommitGraphCondition::Orphaned {
            reason: OrphanReason::Unreachable,
        },
        None,
    ))
}

/// The label for a broad-merged commit (`Some(label)`), or `None` when the commit
/// is not broad-merged. A commit is broad-merged when it is a proper ancestor of
/// some live tip other than itself — the condition, unchanged. The returned label
/// prefers the integration/default branch (`(oid, label)`) when that branch also
/// reaches the commit (equality counts), so a freshly landed commit reads its
/// landing branch and not an arbitrary ref-sorted feature branch that merely
/// contains main's history (#445). The default branch's **own** label is used —
/// resolved from its ref name, not recovered from the deduped live-tip set, which
/// could carry a same-OID alias that sorts earlier. Otherwise the label is the
/// first containing tip in ref order — a truthful witness that *some* live branch
/// reaches it.
fn broad_merged_label(
    repo: &Path,
    commit_oid: &str,
    tips: &[LiveTip],
    default_branch: Option<(&str, &str)>,
    cache: &mut HashMap<(String, String), Ancestry>,
) -> Result<Option<Option<String>>> {
    let mut fallback_label: Option<Option<String>> = None;
    for tip in tips {
        if tip.oid == commit_oid {
            continue;
        }
        if ancestry(repo, commit_oid, &tip.oid, cache)? == Ancestry::Ancestor {
            fallback_label = Some(tip.label.clone());
            break;
        }
    }
    let Some(fallback_label) = fallback_label else {
        return Ok(None);
    };

    // Merged. Prefer the default branch's own label when it too reaches the commit
    // — the ancestry probe is memoized in `cache`, so overlap with the walk above
    // is free.
    if let Some((default_oid, default_label)) = default_branch
        && (default_oid == commit_oid
            || ancestry(repo, commit_oid, default_oid, cache)? == Ancestry::Ancestor)
    {
        return Ok(Some(Some(default_label.to_owned())));
    }

    Ok(Some(fallback_label))
}

fn ancestry(
    repo: &Path,
    commit_oid: &str,
    tip_oid: &str,
    cache: &mut HashMap<(String, String), Ancestry>,
) -> Result<Ancestry> {
    let key = (commit_oid.to_owned(), tip_oid.to_owned());
    if let Some(ancestry) = cache.get(&key) {
        return Ok(*ancestry);
    }
    let ancestry = git_is_ancestor(repo, commit_oid, tip_oid)?;
    cache.insert(key, ancestry);
    Ok(ancestry)
}

/// The landing headline plus the divergence verdict (ADR-0014, 2026-07-09
/// amendment). Landing claims are the association-source edges — the capture
/// target is provenance and never competes. A chain of successive landings
/// collapses to its tip through `independent` (one `merge-base --independent`
/// per claim set); orphaned claims never compete; and only two or more
/// incomparable live-or-merged claims with **distinct trees** are divergent —
/// then the headline is withheld and `divergent_commit_association` fires.
/// Same-tree survivors are a content-equivalent rewrite (the fold's
/// `rewritten_commit_association`): the content landed, so the headline reads
/// `Merged` when any of them is merged, else `Live`. Fold-level diagnostics
/// never withhold the headline — an unrelated `retraction_target_missing` says
/// nothing about the landing.
fn landing_analysis(
    view: &RevisionCommitRangeView,
    per_commit: &[CommitLiveness],
    independent: &mut dyn FnMut(&[String]) -> Result<Vec<String>>,
) -> Result<(Option<CommitGraphCondition>, Vec<ProjectionDiagnostic>)> {
    let condition_of = |oid: &str| {
        per_commit
            .iter()
            .find(|commit| commit.commit_oid == oid)
            .map(|commit| &commit.condition)
    };

    let mut seen = HashSet::new();
    let claims: Vec<_> = view
        .current_commits
        .iter()
        .filter(|commit| commit.source == CommitEdgeSource::Association)
        .filter(|commit| seen.insert(commit.commit_oid.as_str()))
        .collect();

    // No landing claims: the headline is the capture target's own condition
    // (agreed across the per-OID set, which is a single seeded commit here).
    if claims.is_empty() {
        return Ok((
            agreed_condition(per_commit.iter().map(|c| &c.condition)),
            Vec::new(),
        ));
    }

    // Only live-or-merged claims can compete, so they alone enter the
    // independence probe: an orphaned claim never competes regardless of
    // topology (an abandoned descendant of the real landing must not shadow
    // it), and a gc'd claim would make `merge-base` refuse the whole call.
    let alive: Vec<String> = claims
        .iter()
        .filter(|claim| {
            matches!(
                condition_of(&claim.commit_oid),
                Some(CommitGraphCondition::Live | CommitGraphCondition::Merged)
            )
        })
        .map(|claim| claim.commit_oid.clone())
        .collect();
    let maximal: HashSet<String> = independent(&alive)?.into_iter().collect();

    let competing: Vec<_> = claims
        .iter()
        .filter(|claim| maximal.contains(&claim.commit_oid))
        .collect();

    match competing.len() {
        // Every claim is orphaned: the headline reads orphaned regardless of how
        // the reasons mix (the amendment's contract). `ObjectMissing` survives
        // only when every claim's object is gone; otherwise `Unreachable` is the
        // truthful summary — a missing object is also unreachable from live
        // refs, and the per-OID matrix keeps the per-claim reasons.
        0 => {
            let reason = if claims.iter().all(|claim| {
                matches!(
                    condition_of(&claim.commit_oid),
                    Some(CommitGraphCondition::Orphaned {
                        reason: OrphanReason::ObjectMissing
                    })
                )
            }) {
                OrphanReason::ObjectMissing
            } else {
                OrphanReason::Unreachable
            };
            Ok((Some(CommitGraphCondition::Orphaned { reason }), Vec::new()))
        }
        1 => Ok((condition_of(&competing[0].commit_oid).cloned(), Vec::new())),
        _ => {
            let trees: BTreeSet<&str> = competing
                .iter()
                .map(|claim| claim.tree_oid.as_str())
                .collect();
            if trees.len() == 1 {
                let merged = competing.iter().any(|claim| {
                    matches!(
                        condition_of(&claim.commit_oid),
                        Some(CommitGraphCondition::Merged)
                    )
                });
                let condition = if merged {
                    CommitGraphCondition::Merged
                } else {
                    CommitGraphCondition::Live
                };
                return Ok((Some(condition), Vec::new()));
            }
            let mut oids: Vec<&str> = competing
                .iter()
                .map(|claim| {
                    let oid = claim.commit_oid.as_str();
                    &oid[..oid.len().min(SHORT_OID_LEN)]
                })
                .collect();
            oids.sort_unstable();
            Ok((
                None,
                vec![ProjectionDiagnostic {
                    code: DIVERGENT_COMMIT_ASSOCIATION_CODE.to_owned(),
                    message: format!(
                        "revision {} has {} competing landing commits ({}); \
                         none is an ancestor of another and their trees differ",
                        view.revision_id.as_str(),
                        competing.len(),
                        oids.join(", "),
                    ),
                }],
            ))
        }
    }
}

/// The single condition an iterator agrees on, or `None` when it is empty or
/// its members disagree.
fn agreed_condition<'a>(
    mut conditions: impl Iterator<Item = &'a CommitGraphCondition>,
) -> Option<CommitGraphCondition> {
    let first = conditions.next()?.clone();
    if conditions.all(|condition| *condition == first) {
        Some(first)
    } else {
        None
    }
}

/// The short, display label for a full ref: `refs/heads/feat/x` → `feat/x`,
/// `refs/remotes/origin/main` → `origin/main`. Already-short names pass through.
fn short_ref_label(reference: &str) -> String {
    reference
        .strip_prefix("refs/heads/")
        .or_else(|| reference.strip_prefix("refs/remotes/"))
        .unwrap_or(reference)
        .to_owned()
}

/// Honest label for a detached worktree HEAD: never fabricates a branch name.
fn detached_label(oid: &str) -> String {
    format!("(detached @ {})", &oid[..oid.len().min(SHORT_OID_LEN)])
}

#[cfg(test)]
mod tests {
    use std::ffi::OsStr;
    use std::process::Command;

    use tempfile::TempDir;

    use super::*;
    use crate::model::RevisionId;
    use crate::session::{CommitEdgeSource, CurrentCommitAssociation};

    struct LivenessRepo {
        root: TempDir,
    }

    impl LivenessRepo {
        /// main: base → mid → tip; plus a dangling commit (child of tip) whose
        /// branch was deleted, so it exists but no live ref reaches it.
        fn new() -> Self {
            let repo = Self {
                root: TempDir::new().unwrap(),
            };
            repo.git(["init"]);
            repo.git(["config", "user.name", "Shore Tests"]);
            repo.git(["config", "user.email", "shore-tests@example.com"]);
            repo.git(["config", "commit.gpgsign", "false"]);

            repo.commit("base", "base\n");
            repo.git(["branch", "-M", "main"]);
            repo.commit("mid", "mid\n");
            repo.commit("tip", "tip\n");

            // A child of tip on a throwaway branch, then delete the branch.
            repo.git(["checkout", "-b", "tmp"]);
            repo.commit("dangling", "dangling\n");
            repo.git(["checkout", "main"]);
            repo.git(["branch", "-D", "tmp"]);

            repo
        }

        fn path(&self) -> &Path {
            self.root.path()
        }

        fn commit(&self, message: &str, contents: &str) {
            std::fs::write(self.path().join("file.txt"), contents).unwrap();
            self.git(["add", "--all"]);
            self.git(["commit", "-m", message]);
        }

        fn oid(&self, rev: &str) -> String {
            let output = Command::new("git")
                .args(["rev-parse", "--verify", rev])
                .current_dir(self.path())
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "git rev-parse {rev} failed: {}",
                String::from_utf8_lossy(&output.stderr)
            );
            String::from_utf8(output.stdout).unwrap().trim().to_owned()
        }

        fn git<I, S>(&self, args: I)
        where
            I: IntoIterator<Item = S>,
            S: AsRef<OsStr>,
        {
            self.git_at(self.path(), args);
        }

        fn git_at<I, S>(&self, cwd: &Path, args: I)
        where
            I: IntoIterator<Item = S>,
            S: AsRef<OsStr>,
        {
            let status = Command::new("git")
                .args(
                    args.into_iter()
                        .map(|a| a.as_ref().to_owned())
                        .collect::<Vec<_>>(),
                )
                .current_dir(cwd)
                .status()
                .unwrap();
            assert!(status.success());
        }
    }

    fn view_with(oids: &[&str]) -> RevisionCommitRangeView {
        RevisionCommitRangeView {
            revision_id: RevisionId::new("review-unit:sha256:test"),
            anchored: !oids.is_empty(),
            current_commits: oids
                .iter()
                .map(|oid| CurrentCommitAssociation {
                    commit_oid: (*oid).to_owned(),
                    tree_oid: format!("{oid}-tree"),
                    commit_association_id: None,
                    source: CommitEdgeSource::CaptureTarget,
                })
                .collect(),
            current_refs: Vec::new(),
            withdrawn_commits: Vec::new(),
            withdrawn_refs: Vec::new(),
            diagnostics: Vec::new(),
        }
    }

    fn condition_of(
        repo: &LivenessRepo,
        oid: &str,
        integration: Option<&str>,
    ) -> CommitGraphCondition {
        enrich_liveness(&view_with(&[oid]), repo.path(), integration)
            .unwrap()
            .per_commit
            .remove(0)
            .condition
    }

    #[test]
    fn merged_live_orphaned_broad_default() {
        let repo = LivenessRepo::new();
        let mid = repo.oid("main~1");
        let tip = repo.oid("main");

        // mid is an ancestor of the tip (another live tip) → Merged.
        assert_eq!(
            condition_of(&repo, &mid, None),
            CommitGraphCondition::Merged
        );
        // The tip is itself a live tip, contained in no other → Live.
        assert_eq!(condition_of(&repo, &tip, None), CommitGraphCondition::Live);

        // A well-formed but absent object → Orphaned(ObjectMissing).
        let missing = "0".repeat(tip.len());
        assert_eq!(
            condition_of(&repo, &missing, None),
            CommitGraphCondition::Orphaned {
                reason: OrphanReason::ObjectMissing
            }
        );
    }

    #[test]
    fn unreachable_existing_commit_is_orphaned_unreachable() {
        let repo = LivenessRepo::new();
        let dangling = repo.dangling_oid();

        assert!(git_object_exists(repo.path(), &dangling).unwrap());
        assert_eq!(
            condition_of(&repo, &dangling, None),
            CommitGraphCondition::Orphaned {
                reason: OrphanReason::Unreachable
            }
        );
    }

    #[test]
    fn floating_unit_skips_reachability() {
        let repo = LivenessRepo::new();
        let enrichment = enrich_liveness(&view_with(&[]), repo.path(), None).unwrap();
        assert!(enrichment.per_commit.is_empty());
        assert!(enrichment.headline.is_none());
    }

    #[test]
    fn headline_withheld_when_conditions_disagree() {
        let repo = LivenessRepo::new();
        let mid = repo.oid("main~1");
        let dangling = repo.dangling_oid();

        let enrichment =
            enrich_liveness(&view_with(&[&mid, &dangling]), repo.path(), None).unwrap();

        assert_eq!(enrichment.per_commit.len(), 2);
        assert!(enrichment.headline.is_none());
    }

    #[test]
    fn headline_present_when_single_condition_and_no_diagnostics() {
        let repo = LivenessRepo::new();
        let tip = repo.oid("main");

        let enrichment = enrich_liveness(&view_with(&[&tip]), repo.path(), None).unwrap();
        assert_eq!(enrichment.headline, Some(CommitGraphCondition::Live));
    }

    #[test]
    fn detached_worktree_head_is_live_with_detached_label() {
        let repo = LivenessRepo::new();
        let parent = TempDir::new().unwrap();
        let linked = parent.path().join("wt");

        // A detached linked worktree, then a commit on it: its HEAD advances to a
        // commit no branch points at — live only via the worktree HEAD.
        repo.git_at(
            repo.path(),
            [
                "worktree",
                "add",
                "--detach",
                linked.to_str().unwrap(),
                "main",
            ],
        );
        std::fs::write(linked.join("file.txt"), "detached\n").unwrap();
        repo.git_at(&linked, ["add", "--all"]);
        repo.git_at(&linked, ["commit", "-m", "detached work"]);
        let detached_oid = {
            let output = Command::new("git")
                .args(["rev-parse", "--verify", "HEAD"])
                .current_dir(&linked)
                .output()
                .unwrap();
            String::from_utf8(output.stdout).unwrap().trim().to_owned()
        };

        let enrichment = enrich_liveness(&view_with(&[&detached_oid]), repo.path(), None).unwrap();
        let commit = &enrichment.per_commit[0];

        assert_eq!(commit.condition, CommitGraphCondition::Live);
        assert!(
            commit
                .live_branch
                .as_deref()
                .is_some_and(|label| label.starts_with("(detached @ ")),
            "detached worktree HEAD must carry the honest detached label, got {:?}",
            commit.live_branch
        );
    }

    /// The batch's per-commit conditions and headline must match the per-entry
    /// `enrich_liveness` exactly — that equivalence is what lets `list_revisions`
    /// swap the per-pair ancestry probe for shared reachability without moving the
    /// wire. Covers merged, live, orphaned-unreachable, and object-missing in one
    /// view (so the headline is withheld), broad default.
    #[test]
    fn batch_conditions_match_enrich_liveness_broad() {
        let repo = LivenessRepo::new();
        let mid = repo.oid("main~1");
        let tip = repo.oid("main");
        let dangling = repo.dangling_oid();
        let missing = "0".repeat(tip.len());
        let view = view_with(&[&mid, &tip, &dangling, &missing]);

        let direct = enrich_liveness(&view, repo.path(), None).unwrap();
        let batch = LivenessBatch::build(repo.path(), None).unwrap();
        let batched = batch.enrich_broad(repo.path(), &view).unwrap();

        assert_eq!(conditions_of(&direct), conditions_of(&batched));
        assert_eq!(direct.headline, batched.headline);
    }

    /// A branch tip that is itself an ancestor of another tip is `Merged`, not
    /// `Live` (the merged-before-live order) — the one case a naive "a tip is live"
    /// shortcut would get wrong. The batch must agree with `enrich_liveness`.
    #[test]
    fn batch_classifies_a_merged_branch_tip_like_enrich_liveness() {
        let repo = LivenessRepo::new();
        // `old` points at main~1, which is an ancestor of `main` (another tip).
        repo.git(["branch", "old", "main~1"]);
        let old_tip = repo.oid("old");
        let view = view_with(&[&old_tip]);

        let direct = enrich_liveness(&view, repo.path(), None).unwrap();
        let batch = LivenessBatch::build(repo.path(), None).unwrap();
        let batched = batch.enrich_broad(repo.path(), &view).unwrap();
        // merge-status runs `enrich_merge` with no integration ref — that is the broad
        // default and must disambiguate the merged tip identically, not report it open.
        let merge = batch.enrich_merge(repo.path(), &view).unwrap();

        assert_eq!(
            direct.per_commit[0].condition,
            CommitGraphCondition::Merged,
            "a tip that is an ancestor of another tip is merged"
        );
        assert_eq!(conditions_of(&direct), conditions_of(&batched));
        assert_eq!(conditions_of(&direct), conditions_of(&merge));
    }

    /// Narrow (integration-ref) enrichment must also match the per-entry path: a
    /// commit merged into the integration ref vs an unreachable orphan.
    #[test]
    fn batch_conditions_match_enrich_liveness_integration() {
        let repo = LivenessRepo::new();
        let mid = repo.oid("main~1");
        let dangling = repo.dangling_oid();
        let view = view_with(&[&mid, &dangling]);

        let direct = enrich_liveness(&view, repo.path(), Some("refs/heads/main")).unwrap();
        let batch = LivenessBatch::build(repo.path(), Some("refs/heads/main")).unwrap();
        let batched = batch.enrich_merge(repo.path(), &view).unwrap();

        assert_eq!(conditions_of(&direct), conditions_of(&batched));
        assert_eq!(direct.headline, batched.headline);
    }

    /// Under a narrow integration ref, a revision captured at the tip of a *different*
    /// live branch is `Live` (a live tip), not orphaned — the integration check only
    /// decides "merged into the integration ref", and `enrich_liveness` falls through
    /// to the live-tip check for everything else. The batch must do the same.
    #[test]
    fn batch_integration_keeps_a_live_side_branch_tip_live() {
        let repo = LivenessRepo::new();
        // A branch diverging from base: its tip is live but unreachable from main.
        repo.git(["checkout", "-b", "feature", "main~2"]);
        repo.commit("side", "side\n");
        let side_tip = repo.oid("feature");
        repo.git(["checkout", "main"]);
        let view = view_with(&[&side_tip]);

        let direct = enrich_liveness(&view, repo.path(), Some("refs/heads/main")).unwrap();
        let batch = LivenessBatch::build(repo.path(), Some("refs/heads/main")).unwrap();
        let batched = batch.enrich_merge(repo.path(), &view).unwrap();

        assert_eq!(
            direct.per_commit[0].condition,
            CommitGraphCondition::Live,
            "a live side-branch tip is live under a narrow integration ref, not orphaned"
        );
        assert_eq!(conditions_of(&direct), conditions_of(&batched));
    }

    fn conditions_of(enrichment: &LivenessEnrichment) -> Vec<(String, CommitGraphCondition)> {
        enrichment
            .per_commit
            .iter()
            .map(|commit| (commit.commit_oid.clone(), commit.condition.clone()))
            .collect()
    }

    fn claim(oid: &str, tree: &str) -> CurrentCommitAssociation {
        CurrentCommitAssociation {
            commit_oid: oid.to_owned(),
            tree_oid: tree.to_owned(),
            commit_association_id: None,
            source: CommitEdgeSource::Association,
        }
    }

    fn capture_target(oid: &str) -> CurrentCommitAssociation {
        CurrentCommitAssociation {
            commit_oid: oid.to_owned(),
            tree_oid: format!("{oid}-tree"),
            commit_association_id: None,
            source: CommitEdgeSource::CaptureTarget,
        }
    }

    fn view_of_edges(edges: Vec<CurrentCommitAssociation>) -> RevisionCommitRangeView {
        RevisionCommitRangeView {
            revision_id: RevisionId::new("review-unit:sha256:test"),
            anchored: !edges.is_empty(),
            current_commits: edges,
            current_refs: Vec::new(),
            withdrawn_commits: Vec::new(),
            withdrawn_refs: Vec::new(),
            diagnostics: Vec::new(),
        }
    }

    fn tree_of(repo: &LivenessRepo, rev: &str) -> String {
        repo.oid(&format!("{rev}^{{tree}}"))
    }

    /// Successive landings form a chain: the headline follows the tip claim and
    /// nothing diverges, even though the per-OID conditions disagree
    /// (merged vs live) — accretion is history, not competition.
    #[test]
    fn landing_chain_headline_follows_the_tip() {
        let repo = LivenessRepo::new();
        let mid = repo.oid("main~1");
        let tip = repo.oid("main");
        let view = view_of_edges(vec![
            claim(&mid, &tree_of(&repo, "main~1")),
            claim(&tip, &tree_of(&repo, "main")),
        ]);

        let enrichment = enrich_liveness(&view, repo.path(), None).unwrap();

        assert_eq!(enrichment.headline, Some(CommitGraphCondition::Live));
        assert!(enrichment.diagnostics.is_empty());
    }

    /// The standard squash-landing shape: the capture target was rewritten away
    /// (orphaned) and the landed commit is merged. The capture target is
    /// provenance, never a claim, so the headline follows the landing and no
    /// divergence fires.
    #[test]
    fn capture_target_plus_landed_commit_is_not_divergent() {
        let repo = LivenessRepo::new();
        let dangling = repo.dangling_oid();
        let mid = repo.oid("main~1");
        let view = view_of_edges(vec![
            capture_target(&dangling),
            claim(&mid, &tree_of(&repo, "main~1")),
        ]);

        let enrichment = enrich_liveness(&view, repo.path(), None).unwrap();

        assert_eq!(enrichment.headline, Some(CommitGraphCondition::Merged));
        assert!(enrichment.diagnostics.is_empty());
    }

    /// An orphaned claim (a rebased-away earlier landing) never competes: the
    /// surviving live claim carries the headline alone.
    #[test]
    fn orphaned_claim_never_competes() {
        let repo = LivenessRepo::new();
        let dangling = repo.dangling_oid();
        let tip = repo.oid("main");
        let view = view_of_edges(vec![
            claim(&dangling, "rewritten-tree"),
            claim(&tip, &tree_of(&repo, "main")),
        ]);

        let enrichment = enrich_liveness(&view, repo.path(), None).unwrap();

        assert_eq!(enrichment.headline, Some(CommitGraphCondition::Live));
        assert!(enrichment.diagnostics.is_empty());
    }

    /// A claim whose object is gone entirely is excluded before the ancestry
    /// probe (merge-base would refuse the missing OID) and never competes.
    #[test]
    fn missing_object_claim_never_competes_or_errors() {
        let repo = LivenessRepo::new();
        let tip = repo.oid("main");
        let missing = "0".repeat(tip.len());
        let view = view_of_edges(vec![
            claim(&missing, "gone-tree"),
            claim(&tip, &tree_of(&repo, "main")),
        ]);

        let enrichment = enrich_liveness(&view, repo.path(), None).unwrap();

        assert_eq!(enrichment.headline, Some(CommitGraphCondition::Live));
        assert!(enrichment.diagnostics.is_empty());
    }

    /// Every claim orphaned, reasons mixed (unreachable + object-missing): the
    /// headline still reads orphaned — `Unreachable` is the truthful summary
    /// (a missing object is also unreachable from live refs) — never withheld.
    #[test]
    fn all_orphaned_claims_with_mixed_reasons_read_orphaned() {
        let repo = LivenessRepo::new();
        let dangling = repo.dangling_oid();
        let missing = "0".repeat(dangling.len());
        let view = view_of_edges(vec![
            claim(&dangling, "dangling-tree"),
            claim(&missing, "gone-tree"),
        ]);

        let enrichment = enrich_liveness(&view, repo.path(), None).unwrap();

        assert_eq!(
            enrichment.headline,
            Some(CommitGraphCondition::Orphaned {
                reason: OrphanReason::Unreachable
            })
        );
        assert!(enrichment.diagnostics.is_empty());
    }

    /// Two incomparable live claims with distinct trees genuinely compete: the
    /// headline is withheld and `divergent_commit_association` fires — from the
    /// enrichment, with identical verdicts on the direct and batch paths.
    #[test]
    fn incomparable_live_claims_with_distinct_trees_are_divergent() {
        let repo = LivenessRepo::new();
        repo.git(["checkout", "-b", "rival", "main~2"]);
        repo.commit("rival", "rival\n");
        let rival = repo.oid("rival");
        repo.git(["checkout", "main"]);
        let tip = repo.oid("main");
        let view = view_of_edges(vec![
            claim(&tip, &tree_of(&repo, "main")),
            claim(&rival, &tree_of(&repo, "rival")),
        ]);

        let direct = enrich_liveness(&view, repo.path(), None).unwrap();
        let batch = LivenessBatch::build(repo.path(), None).unwrap();
        let batched = batch.enrich_broad(repo.path(), &view).unwrap();

        for enrichment in [&direct, &batched] {
            assert!(enrichment.headline.is_none());
            let divergent = enrichment
                .diagnostics
                .iter()
                .find(|d| d.code == DIVERGENT_COMMIT_ASSOCIATION_CODE)
                .expect("competing landing claims diverge");
            assert!(
                divergent.message.contains("competing landing commits"),
                "message explains the competition: {}",
                divergent.message
            );
        }
        assert_eq!(direct.headline, batched.headline);
        assert_eq!(direct.diagnostics, batched.diagnostics);
    }

    /// Incomparable claims that carry the same tree are a content-equivalent
    /// rewrite, not a divergence: the content landed, so the headline reads
    /// `Merged` when any of them is merged.
    #[test]
    fn content_equivalent_incomparable_claims_are_not_divergent() {
        let repo = LivenessRepo::new();
        repo.git(["checkout", "-b", "rival", "main~2"]);
        repo.commit("rival", "rival\n");
        let rival = repo.oid("rival");
        repo.git(["checkout", "main"]);
        let mid = repo.oid("main~1");
        // Fabricated shared tree: tree equality is string equality on the view.
        let view = view_of_edges(vec![claim(&mid, "sharedtree"), claim(&rival, "sharedtree")]);

        let enrichment = enrich_liveness(&view, repo.path(), None).unwrap();

        assert_eq!(enrichment.headline, Some(CommitGraphCondition::Merged));
        assert!(enrichment.diagnostics.is_empty());
    }

    /// Fold-level diagnostics no longer blank the headline: a view carrying an
    /// unrelated `retraction_target_missing` still reads its landing condition.
    #[test]
    fn unrelated_fold_diagnostics_do_not_withhold_the_headline() {
        let repo = LivenessRepo::new();
        let tip = repo.oid("main");
        let mut view = view_of_edges(vec![claim(&tip, &tree_of(&repo, "main"))]);
        view.diagnostics.push(ProjectionDiagnostic {
            code: "retraction_target_missing".to_owned(),
            message: "unrelated".to_owned(),
        });

        let enrichment = enrich_liveness(&view, repo.path(), None).unwrap();

        assert_eq!(enrichment.headline, Some(CommitGraphCondition::Live));
    }

    #[test]
    fn narrow_merged_when_integration_ref_set() {
        let repo = LivenessRepo::new();
        let mid = repo.oid("main~1");
        let dangling = repo.dangling_oid();

        assert_eq!(
            condition_of(&repo, &mid, Some("refs/heads/main")),
            CommitGraphCondition::Merged
        );
        assert_eq!(
            condition_of(&repo, &dangling, Some("refs/heads/main")),
            CommitGraphCondition::Orphaned {
                reason: OrphanReason::Unreachable
            }
        );
    }

    /// #447: under a narrow integration ref, a commit that IS the integration
    /// ref's own tip is `Merged`, not `Live` — git's own `merge-base
    /// --is-ancestor` treats equality as ancestry, so "is this merged into main"
    /// is yes when the commit is main's exact tip. The per-entry and batch paths
    /// must agree on that condition.
    #[test]
    fn narrow_integration_tip_is_merged() {
        let repo = LivenessRepo::new();
        let tip = repo.oid("main");
        let view = view_with(&[&tip]);

        let direct = enrich_liveness(&view, repo.path(), Some("refs/heads/main")).unwrap();
        let batch = LivenessBatch::build(repo.path(), Some("refs/heads/main")).unwrap();
        let batched = batch.enrich_merge(repo.path(), &view).unwrap();

        assert_eq!(
            direct.per_commit[0].condition,
            CommitGraphCondition::Merged,
            "a commit that is the integration ref's own tip is merged into it"
        );
        assert_eq!(conditions_of(&direct), conditions_of(&batched));
        assert_eq!(direct.headline, batched.headline);
    }

    /// #445 option 1: a broad-merged commit is labeled with the integration/default
    /// branch when that branch reaches it, not with an alphabetically-earlier
    /// feature branch that merely contains main's history. Here `main`'s tip is
    /// also reachable from a feature branch whose refname sorts before "main"; the
    /// ref-order walk would pick that feature branch, but the label must read
    /// "main".
    #[test]
    fn broad_merged_labels_the_default_branch_not_an_earlier_feature_branch() {
        let repo = LivenessRepo::new();
        let tip = repo.oid("main");
        // A feature branch cut from main's tip with one extra commit: it contains
        // the tip and its refname sorts before "main".
        repo.git(["checkout", "-b", "feat-lens", "main"]);
        repo.commit("lens", "lens\n");
        repo.git(["checkout", "main"]);

        let enrichment = enrich_liveness(&view_with(&[&tip]), repo.path(), None).unwrap();
        let commit = &enrichment.per_commit[0];

        assert_eq!(commit.condition, CommitGraphCondition::Merged);
        assert_eq!(
            commit.live_branch.as_deref(),
            Some("main"),
            "a merged commit reachable from the default branch reads its landing \
             branch, not {:?}",
            commit.live_branch
        );
    }

    /// #445 regression: when the default branch shares its tip OID with an
    /// alphabetically-earlier branch, `live_tips` dedups by OID and keeps the
    /// earlier label. The default-branch preference must report the default
    /// branch's **own** label, not the deduped alias.
    #[test]
    fn broad_merged_label_uses_the_default_branch_name_not_a_same_oid_alias() {
        let repo = LivenessRepo::new();
        let mid = repo.oid("main~1");
        // `aaa` points at main's exact tip and sorts before "main", so the deduped
        // live tip for that OID carries the label "aaa".
        repo.git(["branch", "aaa", "main"]);

        let enrichment = enrich_liveness(&view_with(&[&mid]), repo.path(), None).unwrap();
        let commit = &enrichment.per_commit[0];

        assert_eq!(commit.condition, CommitGraphCondition::Merged);
        assert_eq!(
            commit.live_branch.as_deref(),
            Some("main"),
            "the default branch's own label wins over a same-OID alias, got {:?}",
            commit.live_branch
        );
    }

    impl LivenessRepo {
        /// The OID of the dangling commit (child of tip, branch deleted). Found
        /// by scanning the reflog for the unreachable child of `main`.
        fn dangling_oid(&self) -> String {
            // `git fsck` is heavyweight; instead read the reflog of HEAD where the
            // tmp commit was created, then verify it is unreachable from main.
            let output = Command::new("git")
                .args(["log", "-g", "--format=%H"])
                .current_dir(self.path())
                .output()
                .unwrap();
            let reflog = String::from_utf8(output.stdout).unwrap();
            let tip = self.oid("main");
            reflog
                .lines()
                .map(str::to_owned)
                .find(|oid| {
                    *oid != tip
                        && git_is_ancestor(self.path(), oid, &tip).unwrap() == Ancestry::NotAncestor
                        && git_is_ancestor(self.path(), &tip, oid).unwrap() == Ancestry::Ancestor
                })
                .expect("a dangling child of tip is in the reflog")
        }
    }
}
