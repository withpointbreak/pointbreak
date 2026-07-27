//! Bodyless fork-tolerant thread-family output and normalized-fact reducer.

use std::collections::{BTreeMap, BTreeSet};

use serde::Serialize;

use super::{SemanticFact, SemanticFactKind, SemanticModelError};
use crate::error::Result as ProductResult;
use crate::model::{EngagementId, RevisionId};
use crate::session::event::ShoreEvent;
use crate::session::projection::{
    EngagementGrouping, EngagementLifecycle, EngagementView, SupersessionView,
};
use crate::session::state::ProjectionDiagnostic;

pub(crate) fn thread_documents(events: &[ShoreEvent]) -> ProductResult<serde_json::Value> {
    #[derive(Serialize)]
    #[serde(rename_all = "camelCase")]
    struct ThreadDocuments<'a> {
        supersession: &'a SupersessionView,
        engagements: &'a EngagementGrouping,
    }

    let supersession = SupersessionView::from_events(events)?;
    let engagements = EngagementGrouping::from_events(events)?;
    Ok(serde_json::to_value(ThreadDocuments {
        supersession: &supersession,
        engagements: &engagements,
    })?)
}

pub(crate) fn thread_documents_from_facts(
    facts: &[SemanticFact],
) -> std::result::Result<serde_json::Value, SemanticModelError> {
    #[derive(Serialize)]
    #[serde(rename_all = "camelCase")]
    struct ThreadDocuments<'a> {
        supersession: &'a SupersessionView,
        engagements: &'a EngagementGrouping,
    }

    let mut captures = BTreeMap::<RevisionId, &super::RevisionFact>::new();
    let mut edges = Vec::new();
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
        edges.push((
            id,
            revision
                .supersedes
                .iter()
                .cloned()
                .map(RevisionId::new)
                .collect(),
        ));
    }
    let supersession = SupersessionView::from_edges(edges);
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
