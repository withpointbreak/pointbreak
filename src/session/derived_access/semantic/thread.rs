//! Bodyless fork-tolerant thread-family output and normalized-fact reducer.

use std::collections::{BTreeMap, BTreeSet};

use serde::Serialize;

use super::{SemanticFact, SemanticFactKind, SemanticModelError};
use crate::error::Result as ProductResult;
use crate::model::{EngagementId, RevisionId};
use crate::session::event::ShoreEvent;
use crate::session::projection::{
    ChangeProjection, EngagementGrouping, EngagementLifecycle, EngagementView, SupersessionView,
};
use crate::session::state::ProjectionDiagnostic;

/// Thread documents over `events` through the Change-scoped replacement graph.
///
/// `changes` is the store-wide Change projection: replacement authority is
/// per store, so a caller that selected only part of the store still decides
/// it over every Change. With `scope`, `events` must hold the scope's
/// dependency closure (thread heads are component-wide and a Change relation
/// crosses engagements), and only the threads containing a scope Revision are
/// kept.
pub(crate) fn thread_documents(
    events: &[ShoreEvent],
    changes: &ChangeProjection,
    scope: Option<&BTreeSet<RevisionId>>,
) -> ProductResult<serde_json::Value> {
    #[derive(Serialize)]
    #[serde(rename_all = "camelCase")]
    struct ThreadDocuments<'a> {
        supersession: &'a SupersessionView,
        engagements: &'a EngagementGrouping,
    }

    let supersession = thread_supersession(SupersessionView::from_events(events)?, changes, scope);
    let engagements = EngagementGrouping::from_view(events, &supersession)?;
    Ok(serde_json::to_value(ThreadDocuments {
        supersession: &supersession,
        engagements: &engagements,
    })?)
}

/// [`thread_documents`] over compact facts; `facts`, `changes` and `scope`
/// follow the same contract.
pub(crate) fn thread_documents_from_facts(
    facts: &[SemanticFact],
    changes: &ChangeProjection,
    scope: Option<&BTreeSet<RevisionId>>,
) -> std::result::Result<serde_json::Value, SemanticModelError> {
    #[derive(Serialize)]
    #[serde(rename_all = "camelCase")]
    struct ThreadDocuments<'a> {
        supersession: &'a SupersessionView,
        engagements: &'a EngagementGrouping,
    }

    let mut captures = BTreeMap::<RevisionId, &super::RevisionFact>::new();
    for fact in facts {
        let SemanticFactKind::Revision(revision) = &fact.kind else {
            continue;
        };
        let id = RevisionId::new(
            fact.revision_id
                .as_deref()
                .ok_or(SemanticModelError::MissingField("revision_id"))?,
        );
        captures.insert(id.clone(), revision);
    }
    let supersession = thread_supersession(supersession_from_facts(facts)?, changes, scope);
    let current_assessments = current_assessments(facts)?;
    let mut diagnostics = supersession.diagnostics.clone();
    let mut engagements = Vec::new();
    for component in &supersession.components {
        let Some(canonical) = component
            .iter()
            .find_map(|revision| captures.get(revision))
            .map(|capture| EngagementId::new(capture.engagement_id.clone()))
        else {
            continue;
        };
        let hints = component
            .iter()
            .filter_map(|revision| captures.get(revision))
            .map(|capture| EngagementId::new(capture.engagement_id.clone()))
            .collect::<BTreeSet<_>>();
        if hints.len() > 1 {
            diagnostics.push(ProjectionDiagnostic {
                code: "engagements_merged".to_owned(),
                message: format!(
                    "a capture bridged separate engagements, now merged: {}",
                    hints
                        .iter()
                        .map(EngagementId::as_str)
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
            });
        }
        let heads = component
            .intersection(&supersession.heads)
            .cloned()
            .collect::<BTreeSet<_>>();
        let lifecycle = if heads.len() == 1
            && heads.iter().next().is_some_and(|head| {
                current_assessments.get(head).is_some_and(|assessments| {
                    assessments.len() == 1
                        && assessments[0] == crate::session::event::ReviewAssessment::Accepted
                })
            }) {
            EngagementLifecycle::Accepted
        } else {
            EngagementLifecycle::InProgress
        };
        engagements.push(EngagementView {
            engagement_id: canonical,
            revisions: component.clone(),
            heads,
            lifecycle,
        });
    }
    let engagements = EngagementGrouping {
        engagements,
        diagnostics,
    };
    Ok(serde_json::to_value(ThreadDocuments {
        supersession: &supersession,
        engagements: &engagements,
    })?)
}

/// The replacement view thread documents read: `legacy` re-read through the
/// store-wide `changes`, narrowed to the threads of `scope` when one is given.
/// A store without Change claims reads its proposal-borne view unchanged and is
/// never narrowed (its selection already is the scope).
fn thread_supersession(
    legacy: SupersessionView,
    changes: &ChangeProjection,
    scope: Option<&BTreeSet<RevisionId>>,
) -> SupersessionView {
    if changes.changes.is_empty() {
        return legacy;
    }
    let replacement = legacy.change_aware(changes);
    match scope {
        Some(scope) => replacement.threads_containing(scope),
        None => replacement,
    }
}

/// The proposal-borne supersession view over compact Revision facts: the
/// historical migration input every Change-aware reader starts from.
pub(crate) fn supersession_from_facts(
    facts: &[SemanticFact],
) -> std::result::Result<SupersessionView, SemanticModelError> {
    let mut edges = Vec::new();
    for fact in facts {
        let SemanticFactKind::Revision(revision) = &fact.kind else {
            continue;
        };
        let revision_id = fact
            .revision_id
            .as_deref()
            .ok_or(SemanticModelError::MissingField("revision_id"))?;
        edges.push((
            RevisionId::new(revision_id),
            revision
                .supersedes
                .iter()
                .cloned()
                .map(RevisionId::new)
                .collect(),
        ));
    }
    Ok(SupersessionView::from_edges(edges))
}

fn current_assessments(
    facts: &[SemanticFact],
) -> std::result::Result<
    BTreeMap<RevisionId, Vec<crate::session::event::ReviewAssessment>>,
    SemanticModelError,
> {
    let mut representatives = BTreeMap::<String, &SemanticFact>::new();
    for fact in facts {
        if !matches!(fact.kind, SemanticFactKind::Assessment(_)) {
            continue;
        }
        let id = fact
            .semantic_id
            .as_deref()
            .ok_or(SemanticModelError::MissingField("semantic_id"))?;
        representatives
            .entry(id.to_owned())
            .and_modify(|current| {
                if fact.event_id < current.event_id {
                    *current = fact;
                }
            })
            .or_insert(fact);
    }
    let replaced = representatives
        .values()
        .filter_map(|fact| match &fact.kind {
            SemanticFactKind::Assessment(assessment) => Some(&assessment.replaces),
            _ => None,
        })
        .flatten()
        .collect::<BTreeSet<_>>();
    let mut current = BTreeMap::<RevisionId, Vec<_>>::new();
    for (id, fact) in representatives {
        if replaced.contains(&id) {
            continue;
        }
        let SemanticFactKind::Assessment(assessment) = &fact.kind else {
            continue;
        };
        let revision = RevisionId::new(
            fact.revision_id
                .as_deref()
                .ok_or(SemanticModelError::MissingField("revision_id"))?,
        );
        current
            .entry(revision)
            .or_default()
            .push(assessment.assessment);
    }
    Ok(current)
}
