//! Bodyless revision-family output and normalized-fact reducer.

use std::collections::{BTreeMap, BTreeSet};

use serde::Serialize;

use super::{SemanticFact, SemanticFactKind, SemanticModelError};
use crate::error::Result as ProductResult;
use crate::model::{
    CommitAssociationId, CommitWithdrawalId, RefAssociationId, RefWithdrawalId, RevisionId,
};
use crate::session::event::ShoreEvent;
use crate::session::projection::{
    CommitEdgeSource, CurrentCommitAssociation, CurrentRefAssociation,
    RevisionCommitRangeProjection, RevisionCommitRangeView, RevisionsByBase, SupersessionView,
    WithdrawnCommitAssociation, WithdrawnRefAssociation,
};
use crate::session::state::ProjectionDiagnostic;

pub(crate) fn revision_documents(events: &[ShoreEvent]) -> ProductResult<serde_json::Value> {
    #[derive(Serialize)]
    #[serde(rename_all = "camelCase")]
    struct RevisionDocuments<'a> {
        supersession: &'a SupersessionView,
        revisions_by_base: &'a RevisionsByBase,
        commit_ranges: &'a RevisionCommitRangeProjection,
    }

    let supersession = SupersessionView::from_events(events)?;
    let revisions_by_base = RevisionsByBase::from_events(events)?;
    let commit_ranges = RevisionCommitRangeProjection::from_events(events)?;
    Ok(serde_json::to_value(RevisionDocuments {
        supersession: &supersession,
        revisions_by_base: &revisions_by_base,
        commit_ranges: &commit_ranges,
    })?)
}

pub(crate) fn revision_documents_from_facts(
    facts: &[SemanticFact],
) -> std::result::Result<serde_json::Value, SemanticModelError> {
    #[derive(Serialize)]
    #[serde(rename_all = "camelCase")]
    struct RevisionDocuments<'a> {
        supersession: &'a SupersessionView,
        revisions_by_base: &'a RevisionsByBase,
        commit_ranges: &'a RevisionCommitRangeProjection,
    }

    let revision_facts = facts.iter().filter_map(|fact| match &fact.kind {
        SemanticFactKind::Revision(revision) => Some((fact, revision)),
        _ => None,
    });
    let mut edges = Vec::new();
    let mut base_buckets = BTreeMap::<String, BTreeSet<RevisionId>>::new();
    let mut builders = BTreeMap::<RevisionId, CommitRangeBuilder>::new();
    for (fact, revision) in revision_facts {
        let id = revision_id(fact)?;
        edges.push((
            id.clone(),
            revision
                .supersedes
                .iter()
                .cloned()
                .map(RevisionId::new)
                .collect(),
        ));
        if let Some(base) = &revision.base_commit_oid {
            base_buckets
                .entry(base.clone())
                .or_default()
                .insert(id.clone());
        }
        builders.entry(id).or_default().capture_target = revision
            .capture_commit_oid
            .clone()
            .zip(revision.capture_tree_oid.clone());
    }

    for fact in facts {
        let Some(revision_id) = fact.revision_id.as_deref().map(RevisionId::new) else {
            continue;
        };
        match &fact.kind {
            SemanticFactKind::CommitAssociated(association) => {
                builders
                    .entry(revision_id)
                    .or_default()
                    .associated_commits
                    .insert(
                        CommitAssociationId::new(semantic_id(fact)?),
                        (association.commit_oid.clone(), association.tree_oid.clone()),
                    );
            }
            SemanticFactKind::CommitWithdrawn(withdrawal) => {
                builders
                    .entry(revision_id)
                    .or_default()
                    .withdrawn_commits
                    .insert(
                        CommitAssociationId::new(withdrawal.association_id.clone()),
                        CommitWithdrawalId::new(semantic_id(fact)?),
                    );
            }
            SemanticFactKind::RefAssociated(association) => {
                builders
                    .entry(revision_id)
                    .or_default()
                    .associated_refs
                    .insert(
                        RefAssociationId::new(semantic_id(fact)?),
                        (association.ref_name.clone(), association.head_oid.clone()),
                    );
            }
            SemanticFactKind::RefWithdrawn(withdrawal) => {
                builders
                    .entry(revision_id)
                    .or_default()
                    .withdrawn_refs
                    .insert(
                        RefAssociationId::new(withdrawal.association_id.clone()),
                        RefWithdrawalId::new(semantic_id(fact)?),
                    );
            }
            _ => {}
        }
    }

    let supersession = SupersessionView::from_edges(edges);
    let revisions_by_base = RevisionsByBase {
        buckets: base_buckets,
    };
    let commit_ranges = RevisionCommitRangeProjection {
        units: builders
            .into_iter()
            .map(|(id, builder)| (id.clone(), builder.finish(id)))
            .collect(),
    };
    Ok(serde_json::to_value(RevisionDocuments {
        supersession: &supersession,
        revisions_by_base: &revisions_by_base,
        commit_ranges: &commit_ranges,
    })?)
}

fn revision_id(fact: &SemanticFact) -> std::result::Result<RevisionId, SemanticModelError> {
    fact.revision_id
        .as_deref()
        .map(RevisionId::new)
        .ok_or(SemanticModelError::MissingField("revision_id"))
}

fn semantic_id(fact: &SemanticFact) -> std::result::Result<String, SemanticModelError> {
    fact.semantic_id
        .clone()
        .ok_or(SemanticModelError::MissingField("semantic_id"))
}

#[derive(Default)]
struct CommitRangeBuilder {
    capture_target: Option<(String, String)>,
    associated_commits: BTreeMap<CommitAssociationId, (String, String)>,
    withdrawn_commits: BTreeMap<CommitAssociationId, CommitWithdrawalId>,
    associated_refs: BTreeMap<RefAssociationId, (String, String)>,
    withdrawn_refs: BTreeMap<RefAssociationId, RefWithdrawalId>,
}

impl CommitRangeBuilder {
    fn finish(self, revision_id: RevisionId) -> RevisionCommitRangeView {
        let mut diagnostics = Vec::new();
        let mut withdrawn_commits = self.withdrawn_commits;
        let mut current_commits = Vec::new();
        let mut withdrawn_commit_views = Vec::new();
        if let Some((commit_oid, tree_oid)) = self.capture_target {
            current_commits.push(CurrentCommitAssociation {
                commit_oid,
                tree_oid,
                commit_association_id: None,
                source: CommitEdgeSource::CaptureTarget,
            });
        }
        for (association_id, (commit_oid, tree_oid)) in self.associated_commits {
            match withdrawn_commits.remove(&association_id) {
                Some(withdrawal_id) => {
                    withdrawn_commit_views.push(WithdrawnCommitAssociation {
                        commit_oid,
                        tree_oid,
                        commit_association_id: association_id,
                        commit_withdrawal_id: withdrawal_id,
                    });
                }
                None => current_commits.push(CurrentCommitAssociation {
                    commit_oid,
                    tree_oid,
                    commit_association_id: Some(association_id),
                    source: CommitEdgeSource::Association,
                }),
            }
        }

        let mut withdrawn_refs = self.withdrawn_refs;
        let mut current_refs = Vec::new();
        let mut withdrawn_ref_views = Vec::new();
        for (association_id, (ref_name, head_oid)) in self.associated_refs {
            match withdrawn_refs.remove(&association_id) {
                Some(withdrawal_id) => withdrawn_ref_views.push(WithdrawnRefAssociation {
                    ref_association_id: association_id,
                    ref_name,
                    head_oid,
                    ref_withdrawal_id: withdrawal_id,
                }),
                None => current_refs.push(CurrentRefAssociation {
                    ref_association_id: association_id,
                    ref_name,
                    head_oid,
                }),
            }
        }

        for missing in withdrawn_commits
            .keys()
            .map(|id| id.as_str())
            .chain(withdrawn_refs.keys().map(|id| id.as_str()))
        {
            diagnostics.push(ProjectionDiagnostic {
                code: "retraction_target_missing".to_owned(),
                message: format!(
                    "revision {} withdraws association {missing}, which has no matching association",
                    revision_id.as_str()
                ),
            });
        }

        current_commits.sort_by(|left, right| left.commit_oid.cmp(&right.commit_oid));
        withdrawn_commit_views.sort_by(|left, right| left.commit_oid.cmp(&right.commit_oid));
        current_refs.sort_by(|left, right| {
            left.ref_name
                .cmp(&right.ref_name)
                .then_with(|| left.head_oid.cmp(&right.head_oid))
        });
        withdrawn_ref_views.sort_by(|left, right| left.ref_name.cmp(&right.ref_name));

        let mut oids_by_tree = BTreeMap::<&str, BTreeSet<&str>>::new();
        for commit in current_commits
            .iter()
            .filter(|commit| commit.source == CommitEdgeSource::Association)
        {
            oids_by_tree
                .entry(&commit.tree_oid)
                .or_default()
                .insert(&commit.commit_oid);
        }
        for (tree_oid, oids) in oids_by_tree {
            if oids.len() > 1 {
                diagnostics.push(ProjectionDiagnostic {
                    code: "rewritten_commit_association".to_owned(),
                    message: format!(
                        "revision {} has {} content-equivalent commit associations \
                         for tree {tree_oid} — a rewritten landing; withdraw the stale edge",
                        revision_id.as_str(),
                        oids.len()
                    ),
                });
            }
        }

        RevisionCommitRangeView {
            revision_id,
            anchored: !current_commits.is_empty(),
            current_commits,
            current_refs,
            withdrawn_commits: withdrawn_commit_views,
            withdrawn_refs: withdrawn_ref_views,
            diagnostics,
        }
    }
}
