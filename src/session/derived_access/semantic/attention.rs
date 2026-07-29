//! Bodyless attention-family records and output.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use super::{SemanticFact, SemanticFactKind, SemanticModelError};
use crate::error::Result as ProductResult;
use crate::model::{
    ActorId, AssessmentId, InputRequestId, ObservationId, RevisionId, TrackId, ValidationCheckId,
    ValidationStatus,
};
use crate::session::event::{AssertionMode, ReviewAssessment, ShoreEvent};
use crate::session::projection::SupersessionView;
use crate::session::workflow::assessment::collect_assessment_records_by_revision;
use crate::session::workflow::attention::{
    AttentionAssessmentRecord, AttentionDetail, AttentionFreshness, AttentionFreshnessState,
    AttentionItem, AttentionTier, attention_from_events,
};
use crate::session::workflow::input_request::{
    collect_input_request_projection_records, open_input_request_ids,
};
use crate::session::{compare_event_instants, parse_event_instant};

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CurrentAssessmentFact {
    pub(crate) revision_id: String,
    pub(crate) assessment_id: String,
    pub(crate) assessment: ReviewAssessment,
    pub(crate) event_id: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct OpenRequestFact {
    pub(crate) revision_id: String,
    pub(crate) input_request_id: String,
    pub(crate) mode: AssertionMode,
    pub(crate) event_id: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AttentionSemanticSnapshot {
    pub(crate) current_assessments: Vec<CurrentAssessmentFact>,
    pub(crate) open_requests: Vec<OpenRequestFact>,
    pub(crate) items: Vec<AttentionItem>,
    pub(crate) diagnostics: Vec<crate::session::ProjectionDiagnostic>,
}

impl AttentionSemanticSnapshot {
    pub(crate) fn from_events(events: &[ShoreEvent]) -> ProductResult<Self> {
        let by_revision = collect_assessment_records_by_revision(events)?;
        let mut current_assessments = Vec::new();
        for (revision_id, records) in by_revision {
            let replaced: BTreeSet<_> = records
                .values()
                .flat_map(|record| record.payload.replaces_assessment_ids.iter().cloned())
                .collect();
            for record in records.into_values() {
                if !replaced.contains(&record.payload.assessment_id) {
                    current_assessments.push(CurrentAssessmentFact {
                        revision_id: revision_id.as_str().to_owned(),
                        assessment_id: record.payload.assessment_id.as_str().to_owned(),
                        assessment: record.payload.assessment,
                        event_id: record.event.event_id.as_str().to_owned(),
                    });
                }
            }
        }
        current_assessments.sort_by(|left, right| {
            left.revision_id
                .cmp(&right.revision_id)
                .then_with(|| left.assessment_id.cmp(&right.assessment_id))
                .then_with(|| left.event_id.cmp(&right.event_id))
        });

        let records = collect_input_request_projection_records(events)?;
        let open_ids = open_input_request_ids(&records);
        let mut open_requests = records
            .request_records
            .into_iter()
            .filter(|(request_id, _)| open_ids.contains(request_id))
            .filter_map(|(request_id, record)| {
                record
                    .event
                    .subject_revision_id()
                    .ok()
                    .flatten()
                    .map(|revision_id| OpenRequestFact {
                        revision_id: revision_id.as_str().to_owned(),
                        input_request_id: request_id.as_str().to_owned(),
                        mode: record.event.assertion_mode,
                        event_id: record.event.event_id.as_str().to_owned(),
                    })
            })
            .collect::<Vec<_>>();
        open_requests.sort_by(|left, right| {
            left.revision_id
                .cmp(&right.revision_id)
                .then_with(|| left.input_request_id.cmp(&right.input_request_id))
        });

        let attention = attention_from_events(events, None)?;
        Ok(Self {
            current_assessments,
            open_requests,
            items: attention.items,
            diagnostics: attention.diagnostics,
        })
    }

    pub(crate) fn from_facts(
        facts: &[SemanticFact],
    ) -> std::result::Result<Self, SemanticModelError> {
        let supersession = super::thread::supersession_from_facts(facts)?;
        Self::from_facts_with_supersession(facts, &supersession)
    }

    pub(crate) fn from_facts_with_supersession(
        facts: &[SemanticFact],
        supersession: &SupersessionView,
    ) -> std::result::Result<Self, SemanticModelError> {
        let current = current_assessment_records(facts)?;
        let (requests, open_request_ids) = open_request_records(facts)?;
        let mut items = Vec::new();

        for request in &requests {
            let detail = AttentionDetail::OpenInputRequest {
                input_request_id: InputRequestId::new(request.input_request_id.clone()),
                mode: request.mode,
                reason_code: request.reason_code,
                title: request.title.clone(),
                track_id: TrackId::new(request.track_id.clone()),
                opened_by: ActorId::new(request.opened_by.clone()),
            };
            items.push(AttentionItem {
                id: format!("open_input_request:{}", request.input_request_id),
                tier: tier_for(&detail),
                revision_id: Some(RevisionId::new(request.revision_id.clone())),
                freshness: freshness_for(
                    supersession,
                    &RevisionId::new(request.revision_id.clone()),
                ),
                observed_at: request.observed_at.clone(),
                detail,
            });
        }
        ambiguous_items(&current, supersession, &mut items);
        competing_head_items(facts, supersession, &mut items)?;
        stale_items(&current, supersession, &mut items);
        failed_validation_items(facts, &current, supersession, &mut items)?;
        follow_up_items(&current, &open_request_ids, supersession, &mut items);
        items.sort_by(|left, right| {
            tier_rank(left.tier)
                .cmp(&tier_rank(right.tier))
                .then_with(|| compare_event_instants(&left.observed_at, &right.observed_at))
                .then_with(|| left.id.cmp(&right.id))
        });

        let mut current_assessments = current
            .iter()
            .flat_map(|(revision_id, records)| {
                records.iter().map(|record| CurrentAssessmentFact {
                    revision_id: revision_id.as_str().to_owned(),
                    assessment_id: record.assessment_id.as_str().to_owned(),
                    assessment: record.assessment,
                    event_id: record.event_id.clone(),
                })
            })
            .collect::<Vec<_>>();
        current_assessments.sort_by(|left, right| {
            left.revision_id
                .cmp(&right.revision_id)
                .then_with(|| left.assessment_id.cmp(&right.assessment_id))
                .then_with(|| left.event_id.cmp(&right.event_id))
        });
        let open_requests = requests
            .into_iter()
            .map(|request| OpenRequestFact {
                revision_id: request.revision_id,
                input_request_id: request.input_request_id,
                mode: request.mode,
                event_id: request.event_id,
            })
            .collect();
        Ok(Self {
            current_assessments,
            open_requests,
            items,
            diagnostics: supersession.diagnostics.clone(),
        })
    }
}

#[derive(Clone)]
struct AssessmentRecord {
    event_id: String,
    assessment_id: AssessmentId,
    assessment: ReviewAssessment,
    track_id: TrackId,
    recorded_by: ActorId,
    recorded_at: String,
    related_observation_ids: Vec<ObservationId>,
    related_input_request_ids: Vec<InputRequestId>,
    revision_scoped: bool,
}

impl AssessmentRecord {
    fn as_attention(&self) -> AttentionAssessmentRecord {
        AttentionAssessmentRecord {
            assessment_id: self.assessment_id.clone(),
            assessment: self.assessment,
            track_id: self.track_id.clone(),
            recorded_by: self.recorded_by.clone(),
            recorded_at: self.recorded_at.clone(),
            related_observation_ids: self.related_observation_ids.clone(),
            related_input_request_ids: self.related_input_request_ids.clone(),
            revision_scoped: self.revision_scoped,
        }
    }
}

struct RequestRecord {
    revision_id: String,
    input_request_id: String,
    mode: AssertionMode,
    reason_code: crate::session::event::InputRequestReasonCode,
    title: String,
    track_id: String,
    opened_by: String,
    observed_at: String,
    event_id: String,
}

fn current_assessment_records(
    facts: &[SemanticFact],
) -> std::result::Result<
    std::collections::BTreeMap<RevisionId, Vec<AssessmentRecord>>,
    SemanticModelError,
> {
    let mut representatives = std::collections::BTreeMap::<String, &SemanticFact>::new();
    for fact in facts {
        if !matches!(fact.kind, SemanticFactKind::Assessment(_)) {
            continue;
        }
        let id = required(&fact.semantic_id, "semantic_id")?;
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
            SemanticFactKind::Assessment(assessment) => Some(assessment.replaces.iter()),
            _ => None,
        })
        .flatten()
        .cloned()
        .collect::<BTreeSet<_>>();
    let mut current = std::collections::BTreeMap::<RevisionId, Vec<AssessmentRecord>>::new();
    for (id, fact) in representatives {
        if replaced.contains(&id) {
            continue;
        }
        let SemanticFactKind::Assessment(assessment) = &fact.kind else {
            continue;
        };
        let revision_id = RevisionId::new(required(&fact.revision_id, "revision_id")?);
        let related_observation_ids = assessment
            .related_observations
            .iter()
            .cloned()
            .map(ObservationId::new)
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();
        let related_input_request_ids = assessment
            .related_requests
            .iter()
            .cloned()
            .map(InputRequestId::new)
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();
        current
            .entry(revision_id)
            .or_default()
            .push(AssessmentRecord {
                event_id: fact.event_id.clone(),
                assessment_id: AssessmentId::new(id),
                assessment: assessment.assessment,
                track_id: TrackId::new(required(&fact.track_id, "track_id")?),
                recorded_by: ActorId::new(fact.actor_id.clone()),
                recorded_at: fact.occurred_at.clone(),
                related_observation_ids,
                related_input_request_ids,
                revision_scoped: assessment.revision_scoped,
            });
    }
    for records in current.values_mut() {
        records.sort_by(|left, right| {
            left.recorded_at.cmp(&right.recorded_at).then_with(|| {
                left.assessment_id
                    .as_str()
                    .cmp(right.assessment_id.as_str())
            })
        });
    }
    Ok(current)
}

fn open_request_records(
    facts: &[SemanticFact],
) -> std::result::Result<(Vec<RequestRecord>, BTreeSet<InputRequestId>), SemanticModelError> {
    let mut requests = std::collections::BTreeMap::<String, &SemanticFact>::new();
    let mut responses = std::collections::BTreeMap::<String, &SemanticFact>::new();
    for fact in facts {
        match &fact.kind {
            SemanticFactKind::InputRequestOpened(_) => {
                replace_lowest(
                    &mut requests,
                    required(&fact.semantic_id, "semantic_id")?,
                    fact,
                );
            }
            SemanticFactKind::InputRequestResponded(_) => {
                replace_lowest(
                    &mut responses,
                    required(&fact.semantic_id, "semantic_id")?,
                    fact,
                );
            }
            _ => {}
        }
    }
    let responded_ids = responses
        .values()
        .filter_map(|fact| match &fact.kind {
            SemanticFactKind::InputRequestResponded(response) => Some(response.request_id.clone()),
            _ => None,
        })
        .collect::<BTreeSet<_>>();
    let mut open = Vec::new();
    let mut open_ids = BTreeSet::new();
    for (id, fact) in requests {
        if responded_ids.contains(&id) {
            continue;
        }
        let Some(revision_id) = fact.revision_id.clone() else {
            continue;
        };
        let SemanticFactKind::InputRequestOpened(request) = &fact.kind else {
            continue;
        };
        open_ids.insert(InputRequestId::new(id.clone()));
        open.push(RequestRecord {
            revision_id,
            input_request_id: id,
            mode: fact.assertion_mode,
            reason_code: request.reason_code,
            title: request.title.clone(),
            track_id: required(&fact.track_id, "track_id")?.to_owned(),
            opened_by: fact.actor_id.clone(),
            observed_at: fact.occurred_at.clone(),
            event_id: fact.event_id.clone(),
        });
    }
    open.sort_by(|left, right| {
        left.revision_id
            .cmp(&right.revision_id)
            .then_with(|| left.input_request_id.cmp(&right.input_request_id))
    });
    Ok((open, open_ids))
}

fn replace_lowest<'a>(
    map: &mut std::collections::BTreeMap<String, &'a SemanticFact>,
    id: &str,
    fact: &'a SemanticFact,
) {
    map.entry(id.to_owned())
        .and_modify(|current| {
            if fact.event_id < current.event_id {
                *current = fact;
            }
        })
        .or_insert(fact);
}

fn ambiguous_items(
    current: &std::collections::BTreeMap<RevisionId, Vec<AssessmentRecord>>,
    supersession: &SupersessionView,
    items: &mut Vec<AttentionItem>,
) {
    for (revision_id, records) in current {
        if records.len() < 2 {
            continue;
        }
        let freshness = freshness_for(supersession, revision_id);
        if freshness.state == AttentionFreshnessState::Superseded
            && thread_heads_all_assessed(supersession, current, revision_id)
        {
            continue;
        }
        let detail = AttentionDetail::AmbiguousAssessment {
            assessments: records.iter().map(AssessmentRecord::as_attention).collect(),
        };
        items.push(AttentionItem {
            id: format!("ambiguous_assessment:{}", revision_id.as_str()),
            tier: tier_for(&detail),
            revision_id: Some(revision_id.clone()),
            freshness,
            observed_at: records
                .iter()
                .map(|record| record.recorded_at.clone())
                .max()
                .unwrap_or_default(),
            detail,
        });
    }
}

fn competing_head_items(
    facts: &[SemanticFact],
    supersession: &SupersessionView,
    items: &mut Vec<AttentionItem>,
) -> std::result::Result<(), SemanticModelError> {
    let mut captured_at = std::collections::BTreeMap::<RevisionId, &SemanticFact>::new();
    for fact in facts {
        if !matches!(fact.kind, SemanticFactKind::Revision(_)) {
            continue;
        }
        let id = RevisionId::new(required(&fact.revision_id, "revision_id")?);
        captured_at
            .entry(id)
            .and_modify(|current| {
                if fact.event_id < current.event_id {
                    *current = fact;
                }
            })
            .or_insert(fact);
    }
    for component in &supersession.components {
        let heads = component
            .intersection(&supersession.heads)
            .cloned()
            .collect::<Vec<_>>();
        if heads.len() < 2 {
            continue;
        }
        let detail = AttentionDetail::CompetingHeads {
            head_revision_ids: heads.clone(),
            thread_revision_count: component.len(),
        };
        items.push(AttentionItem {
            id: format!(
                "competing_heads:{}",
                component
                    .iter()
                    .next()
                    .expect("non-empty component")
                    .as_str()
            ),
            tier: tier_for(&detail),
            revision_id: None,
            freshness: current_freshness(),
            observed_at: heads
                .iter()
                .filter_map(|head| captured_at.get(head))
                .map(|fact| fact.occurred_at.clone())
                .max()
                .unwrap_or_default(),
            detail,
        });
    }
    Ok(())
}

fn stale_items(
    current: &std::collections::BTreeMap<RevisionId, Vec<AssessmentRecord>>,
    supersession: &SupersessionView,
    items: &mut Vec<AttentionItem>,
) {
    for (revision_id, records) in current {
        let freshness = freshness_for(supersession, revision_id);
        if freshness.state != AttentionFreshnessState::Superseded
            || thread_heads_all_assessed(supersession, current, revision_id)
        {
            continue;
        }
        for record in records {
            let detail = AttentionDetail::StaleAssessment {
                assessment_id: record.assessment_id.clone(),
                assessment: record.assessment,
                track_id: record.track_id.clone(),
                recorded_by: record.recorded_by.clone(),
                head_revision_ids: supersession.heads_for(revision_id).into_iter().collect(),
            };
            items.push(AttentionItem {
                id: format!("stale_assessment:{}", record.assessment_id.as_str()),
                tier: tier_for(&detail),
                revision_id: Some(revision_id.clone()),
                freshness: freshness.clone(),
                observed_at: record.recorded_at.clone(),
                detail,
            });
        }
    }
}

fn failed_validation_items(
    facts: &[SemanticFact],
    current: &std::collections::BTreeMap<RevisionId, Vec<AssessmentRecord>>,
    supersession: &SupersessionView,
    items: &mut Vec<AttentionItem>,
) -> std::result::Result<(), SemanticModelError> {
    let mut representatives = std::collections::BTreeMap::<String, &SemanticFact>::new();
    for fact in facts {
        if matches!(fact.kind, SemanticFactKind::Validation(_)) {
            replace_lowest(
                &mut representatives,
                required(&fact.semantic_id, "semantic_id")?,
                fact,
            );
        }
    }
    let mut groups =
        std::collections::BTreeMap::<(RevisionId, TrackId, String), Vec<&SemanticFact>>::new();
    for fact in representatives.into_values() {
        let SemanticFactKind::Validation(validation) = &fact.kind else {
            continue;
        };
        groups
            .entry((
                RevisionId::new(required(&fact.revision_id, "revision_id")?),
                TrackId::new(required(&fact.track_id, "track_id")?),
                validation.check_name.clone(),
            ))
            .or_default()
            .push(fact);
    }
    for ((revision_id, _, _), mut group) in groups {
        if !supersession.heads.contains(&revision_id) {
            continue;
        }
        let Some(max_time) = group
            .iter()
            .filter_map(|fact| match &fact.kind {
                SemanticFactKind::Validation(validation)
                    if validation.status != ValidationStatus::Skipped =>
                {
                    Some(
                        validation
                            .completed_at
                            .as_deref()
                            .unwrap_or(&fact.occurred_at)
                            .to_owned(),
                    )
                }
                _ => None,
            })
            .max()
        else {
            continue;
        };
        if assessment_subsumes_failure(
            current.get(&revision_id).map(Vec::as_slice).unwrap_or(&[]),
            &max_time,
        ) {
            continue;
        }
        group.sort_by_key(|fact| fact.semantic_id.as_deref().unwrap_or_default());
        for fact in group {
            let SemanticFactKind::Validation(validation) = &fact.kind else {
                continue;
            };
            let sort_time = validation
                .completed_at
                .as_deref()
                .unwrap_or(&fact.occurred_at);
            if sort_time != max_time
                || !matches!(
                    validation.status,
                    ValidationStatus::Failed | ValidationStatus::Errored
                )
            {
                continue;
            }
            let id = ValidationCheckId::new(required(&fact.semantic_id, "semantic_id")?);
            let detail = AttentionDetail::FailedValidation {
                validation_check_id: id.clone(),
                check_name: validation.check_name.clone(),
                status: validation.status,
                exit_code: validation.exit_code,
                track_id: TrackId::new(required(&fact.track_id, "track_id")?),
                recorded_by: ActorId::new(fact.actor_id.clone()),
                log_artifact_content_hashes: validation.log_artifact_content_hashes.clone(),
            };
            items.push(AttentionItem {
                id: format!("failed_validation:{}", id.as_str()),
                tier: tier_for(&detail),
                revision_id: Some(revision_id.clone()),
                freshness: freshness_for(supersession, &revision_id),
                observed_at: sort_time.to_owned(),
                detail,
            });
        }
    }
    Ok(())
}

fn follow_up_items(
    current: &std::collections::BTreeMap<RevisionId, Vec<AssessmentRecord>>,
    open_request_ids: &BTreeSet<InputRequestId>,
    supersession: &SupersessionView,
    items: &mut Vec<AttentionItem>,
) {
    for (revision_id, records) in current {
        for record in records {
            if record.assessment != ReviewAssessment::AcceptedWithFollowUp {
                continue;
            }
            let open = record
                .related_input_request_ids
                .iter()
                .filter(|id| open_request_ids.contains(*id))
                .cloned()
                .collect::<Vec<_>>();
            if open.is_empty() {
                continue;
            }
            let detail = AttentionDetail::FollowUpOutstanding {
                assessment_id: record.assessment_id.clone(),
                track_id: record.track_id.clone(),
                recorded_by: record.recorded_by.clone(),
                open_input_request_ids: open,
            };
            items.push(AttentionItem {
                id: format!("follow_up_outstanding:{}", record.assessment_id.as_str()),
                tier: tier_for(&detail),
                revision_id: Some(revision_id.clone()),
                freshness: freshness_for(supersession, revision_id),
                observed_at: record.recorded_at.clone(),
                detail,
            });
        }
    }
}

fn thread_heads_all_assessed(
    supersession: &SupersessionView,
    current: &std::collections::BTreeMap<RevisionId, Vec<AssessmentRecord>>,
    revision_id: &RevisionId,
) -> bool {
    if supersession
        .component_of(revision_id)
        .is_none_or(|component| {
            component
                .intersection(&supersession.cycle_revisions)
                .next()
                .is_some()
        })
    {
        return false;
    }
    let heads = supersession.heads_for(revision_id);
    !heads.is_empty()
        && heads.iter().all(|head| {
            current
                .get(head)
                .is_some_and(|records| records.iter().any(|record| record.revision_scoped))
        })
}

fn assessment_subsumes_failure(records: &[AssessmentRecord], failure_time: &str) -> bool {
    if records.is_empty()
        || !records.iter().all(|record| {
            matches!(
                record.assessment,
                ReviewAssessment::Accepted | ReviewAssessment::AcceptedWithFollowUp
            )
        })
    {
        return false;
    }
    let Some(failure) = parse_event_instant(failure_time) else {
        return false;
    };
    records.iter().any(|record| {
        record.revision_scoped
            && parse_event_instant(&record.recorded_at).is_some_and(|value| value > failure)
    })
}

fn freshness_for(supersession: &SupersessionView, revision_id: &RevisionId) -> AttentionFreshness {
    let superseded_by = supersession
        .stale_by_superseding_revision(revision_id)
        .into_iter()
        .collect::<Vec<_>>();
    if superseded_by.is_empty() {
        current_freshness()
    } else {
        AttentionFreshness {
            state: AttentionFreshnessState::Superseded,
            superseded_by,
        }
    }
}

fn current_freshness() -> AttentionFreshness {
    AttentionFreshness {
        state: AttentionFreshnessState::Current,
        superseded_by: Vec::new(),
    }
}

fn tier_for(detail: &AttentionDetail) -> AttentionTier {
    match detail {
        AttentionDetail::OpenInputRequest {
            mode: AssertionMode::Advisory,
            ..
        } => AttentionTier::Secondary,
        _ => AttentionTier::Primary,
    }
}

fn tier_rank(tier: AttentionTier) -> u8 {
    match tier {
        AttentionTier::Primary => 0,
        AttentionTier::Secondary => 1,
    }
}

fn required<'a>(
    value: &'a Option<String>,
    label: &'static str,
) -> std::result::Result<&'a str, SemanticModelError> {
    value
        .as_deref()
        .ok_or(SemanticModelError::MissingField(label))
}
