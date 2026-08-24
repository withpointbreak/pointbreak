//! Minimal freshness and incrementally derivable session state.
#![cfg_attr(not(test), allow(dead_code))]

use std::collections::{BTreeMap, BTreeSet};

use serde::Serialize;

use super::{SemanticFact, SemanticFactKind, SemanticModelError};
use crate::canonical_hash::sha256_json_prefixed;
use crate::session::derived_access::cursor::TruthCursor;
use crate::session::event::{AssertionMode, EventType, ShoreEvent};
use crate::session::projection::SessionState;

const DEFAULT_JOURNAL_ID: &str = "journal:default";
const DUPLICATE_EVENT_LIMIT: usize = 5;

#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SemanticStateSnapshot {
    pub(crate) journal_id: String,
    pub(crate) current_revision_id: Option<String>,
    pub(crate) current_object_id: Option<String>,
    pub(crate) revision_count: usize,
    pub(crate) event_count: usize,
    pub(crate) event_set_hash: Option<String>,
    pub(crate) observation_count: usize,
    pub(crate) assessment_count: usize,
    pub(crate) validation_check_count: usize,
    pub(crate) input_request_count: usize,
    pub(crate) open_input_request_count: usize,
    pub(crate) open_operative_input_request_count: usize,
    pub(crate) diagnostics: Vec<serde_json::Value>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct MaterializedSemanticState {
    pub(crate) journal_id: String,
    pub(crate) current_revision_id: Option<String>,
    pub(crate) current_object_id: Option<String>,
    pub(crate) revision_count: usize,
    pub(crate) event_count: usize,
    pub(crate) observation_count: usize,
    pub(crate) assessment_count: usize,
    pub(crate) validation_check_count: usize,
    pub(crate) input_request_count: usize,
    pub(crate) open_input_request_count: usize,
    pub(crate) open_operative_input_request_count: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct MaterializedSemanticDuplicate {
    pub(crate) family: String,
    pub(crate) semantic_id: String,
    pub(crate) event_ids: Vec<String>,
    pub(crate) event_count: usize,
}

impl SemanticStateSnapshot {
    pub(crate) fn from_events(events: &[ShoreEvent]) -> crate::error::Result<Self> {
        let state = SessionState::from_events(events)?;
        Ok(Self {
            journal_id: state.journal_id.as_str().to_owned(),
            current_revision_id: state
                .current_revision_id
                .map(|value| value.as_str().to_owned()),
            current_object_id: state
                .current_object_id
                .map(|value| value.as_str().to_owned()),
            revision_count: state.revision_count,
            event_count: state.event_count,
            event_set_hash: state.event_set_hash,
            observation_count: state.observation_count,
            assessment_count: state.assessment_count,
            validation_check_count: state.validation_check_count,
            input_request_count: state.input_request_count,
            open_input_request_count: state.open_input_request_count,
            open_operative_input_request_count: state.open_operative_input_request_count,
            diagnostics: state
                .diagnostics
                .iter()
                .map(serde_json::to_value)
                .collect::<std::result::Result<Vec<_>, _>>()?,
        })
    }

    pub(crate) fn from_facts(facts: &[SemanticFact]) -> Result<Self, SemanticModelError> {
        let mut journal_id = DEFAULT_JOURNAL_ID.to_owned();
        let mut captures = BTreeMap::<String, String>::new();
        let mut semantic_events = BTreeMap::<(&str, String), BTreeSet<String>>::new();
        let mut request_modes = BTreeMap::<String, AssertionMode>::new();
        let mut responded_requests = BTreeSet::<String>::new();

        for fact in facts {
            if fact.event_type == EventType::ReviewInitialized.as_str()
                || journal_id == DEFAULT_JOURNAL_ID
            {
                journal_id.clone_from(&fact.journal_id);
            }
            match &fact.kind {
                SemanticFactKind::Revision(revision) => {
                    captures.insert(
                        required(&fact.revision_id, "revision_id")?.to_owned(),
                        revision.object_id.clone(),
                    );
                }
                SemanticFactKind::Observation => {
                    insert_semantic(&mut semantic_events, "observation", fact)?;
                }
                SemanticFactKind::Assessment(_) => {
                    insert_semantic(&mut semantic_events, "assessment", fact)?;
                }
                SemanticFactKind::InputRequestOpened(_) => {
                    insert_semantic(&mut semantic_events, "request", fact)?;
                    request_modes
                        .entry(required(&fact.semantic_id, "semantic_id")?.to_owned())
                        .or_insert(fact.assertion_mode);
                }
                SemanticFactKind::InputRequestResponded(response) => {
                    insert_semantic(&mut semantic_events, "response", fact)?;
                    responded_requests.insert(response.request_id.clone());
                }
                SemanticFactKind::Validation(_) => {
                    insert_semantic(&mut semantic_events, "validation", fact)?;
                }
                _ => {}
            }
        }

        let current = (captures.len() == 1)
            .then(|| captures.iter().next())
            .flatten();
        let open_input_request_count = request_modes
            .keys()
            .filter(|id| !responded_requests.contains(*id))
            .count();
        let open_operative_input_request_count = request_modes
            .iter()
            .filter(|(id, mode)| {
                **mode == AssertionMode::Operative && !responded_requests.contains(*id)
            })
            .count();
        let diagnostics = duplicate_diagnostics(&semantic_events);

        Ok(Self {
            journal_id,
            current_revision_id: current.map(|(id, _)| id.clone()),
            current_object_id: current.map(|(_, id)| id.clone()),
            revision_count: captures.len(),
            event_count: facts.len(),
            event_set_hash: Some(event_set_hash(facts)?),
            observation_count: count_family(&semantic_events, "observation"),
            assessment_count: count_family(&semantic_events, "assessment"),
            validation_check_count: count_family(&semantic_events, "validation"),
            input_request_count: request_modes.len(),
            open_input_request_count,
            open_operative_input_request_count,
            diagnostics,
        })
    }

    /// Assemble the ordinary-operation state from incrementally maintained
    /// counters and bounded duplicate rows.
    ///
    /// `eventSetHash` is deliberately absent here. It is the exhaustive
    /// rebuild/audit receipt and is computed only by `from_events` or
    /// `from_facts`, never by a fixed-output ordinary read.
    pub(crate) fn from_materialized(
        state: MaterializedSemanticState,
        duplicates: &[MaterializedSemanticDuplicate],
    ) -> Self {
        Self {
            journal_id: state.journal_id,
            current_revision_id: state.current_revision_id,
            current_object_id: state.current_object_id,
            revision_count: state.revision_count,
            event_count: state.event_count,
            event_set_hash: None,
            observation_count: state.observation_count,
            assessment_count: state.assessment_count,
            validation_check_count: state.validation_check_count,
            input_request_count: state.input_request_count,
            open_input_request_count: state.open_input_request_count,
            open_operative_input_request_count: state.open_operative_input_request_count,
            diagnostics: materialized_duplicate_diagnostics(duplicates),
        }
    }
}

fn required<'a>(
    value: &'a Option<String>,
    label: &'static str,
) -> Result<&'a str, SemanticModelError> {
    value
        .as_deref()
        .ok_or(SemanticModelError::MissingField(label))
}

fn insert_semantic(
    events: &mut BTreeMap<(&'static str, String), BTreeSet<String>>,
    family: &'static str,
    fact: &SemanticFact,
) -> Result<(), SemanticModelError> {
    events
        .entry((
            family,
            required(&fact.semantic_id, "semantic_id")?.to_owned(),
        ))
        .or_default()
        .insert(fact.event_id.clone());
    Ok(())
}

fn count_family(
    events: &BTreeMap<(&'static str, String), BTreeSet<String>>,
    family: &str,
) -> usize {
    events.keys().filter(|(kind, _)| *kind == family).count()
}

fn duplicate_diagnostics(
    events: &BTreeMap<(&'static str, String), BTreeSet<String>>,
) -> Vec<serde_json::Value> {
    let mut diagnostics = Vec::new();
    let mut duplicate_groups = events.iter().collect::<Vec<_>>();
    duplicate_groups.sort_by(|left, right| {
        duplicate_family_rank((left.0).0)
            .cmp(&duplicate_family_rank((right.0).0))
            .then_with(|| (left.0).1.cmp(&(right.0).1))
    });
    for ((family, semantic_id), event_ids) in duplicate_groups {
        if event_ids.len() < 2 {
            continue;
        }
        let (code, label) = match *family {
            "observation" => ("duplicate_semantic_observation_event", "observation"),
            "request" => (
                "duplicate_semantic_input_request_open_event",
                "input request",
            ),
            "response" => (
                "duplicate_semantic_input_request_response_event",
                "input request response",
            ),
            "assessment" => ("duplicate_semantic_assessment_event", "assessment"),
            "validation" => ("duplicate_semantic_validation_event", "validation check"),
            _ => continue,
        };
        let mut event_id_list = event_ids
            .iter()
            .take(DUPLICATE_EVENT_LIMIT)
            .map(String::as_str)
            .collect::<Vec<_>>();
        if event_ids.len() > DUPLICATE_EVENT_LIMIT {
            event_id_list.push("...");
        }
        diagnostics.push(serde_json::json!({
            "code": code,
            "message": format!(
                "duplicate {label} semantic id {semantic_id} appears in events: {}",
                event_id_list.join(", ")
            ),
        }));
    }
    diagnostics
}

fn materialized_duplicate_diagnostics(
    duplicates: &[MaterializedSemanticDuplicate],
) -> Vec<serde_json::Value> {
    let mut duplicates = duplicates.to_vec();
    duplicates.sort_by(|left, right| {
        duplicate_family_rank(&left.family)
            .cmp(&duplicate_family_rank(&right.family))
            .then_with(|| left.semantic_id.cmp(&right.semantic_id))
    });
    duplicates
        .into_iter()
        .filter(|duplicate| duplicate.event_count >= 2)
        .filter_map(|duplicate| {
            let (code, label) = duplicate_label(&duplicate.family)?;
            let mut event_ids = duplicate.event_ids;
            if duplicate.event_count > DUPLICATE_EVENT_LIMIT {
                event_ids.push("...".to_owned());
            }
            Some(serde_json::json!({
                "code": code,
                "message": format!(
                    "duplicate {label} semantic id {} appears in events: {}",
                    duplicate.semantic_id,
                    event_ids.join(", ")
                ),
            }))
        })
        .collect()
}

fn duplicate_family_rank(family: &str) -> u8 {
    // Match SessionState::finish exactly. Public fact documents inherited this
    // family order before materialized diagnostics existed, so lexical SQL/model
    // order would change otherwise byte-identical derived responses.
    match family {
        "observation" => 0,
        "request" => 1,
        "response" => 2,
        "assessment" => 3,
        "validation" => 4,
        _ => 5,
    }
}

fn duplicate_label(family: &str) -> Option<(&'static str, &'static str)> {
    match family {
        "observation" => Some(("duplicate_semantic_observation_event", "observation")),
        "request" => Some((
            "duplicate_semantic_input_request_open_event",
            "input request",
        )),
        "response" => Some((
            "duplicate_semantic_input_request_response_event",
            "input request response",
        )),
        "assessment" => Some(("duplicate_semantic_assessment_event", "assessment")),
        "validation" => Some(("duplicate_semantic_validation_event", "validation check")),
        _ => None,
    }
}

fn event_set_hash(facts: &[SemanticFact]) -> Result<String, SemanticModelError> {
    #[derive(Serialize)]
    #[serde(rename_all = "camelCase")]
    struct Material<'a> {
        schema: &'static str,
        events: Vec<Entry<'a>>,
    }
    #[derive(Serialize)]
    #[serde(rename_all = "camelCase")]
    struct Entry<'a> {
        event_id: &'a str,
        payload_hash: &'a str,
    }
    let mut events = facts
        .iter()
        .map(|fact| Entry {
            event_id: &fact.event_id,
            payload_hash: &fact.payload_hash,
        })
        .collect::<Vec<_>>();
    events.sort_by_key(|entry| (entry.event_id, entry.payload_hash));
    Ok(sha256_json_prefixed(&serde_json::to_value(Material {
        schema: "shore.event-set.v1",
        events,
    })?)?)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum DerivedAccessFreshness {
    Current {
        as_of: TruthCursor,
    },
    CatchUpRequired {
        applied: TruthCursor,
        observed: TruthCursor,
    },
    EpochMismatch {
        applied: TruthCursor,
        observed: TruthCursor,
    },
}

impl DerivedAccessFreshness {
    pub(crate) fn between(
        applied: TruthCursor,
        observed: TruthCursor,
    ) -> Result<Self, FreshnessModelError> {
        if applied.epoch != observed.epoch {
            return Ok(Self::EpochMismatch { applied, observed });
        }
        if applied.sequence > observed.sequence {
            return Err(FreshnessModelError::AppliedAhead { applied, observed });
        }
        if applied == observed {
            Ok(Self::Current { as_of: observed })
        } else {
            Ok(Self::CatchUpRequired { applied, observed })
        }
    }

    pub(crate) fn new_event_count(self) -> Option<u64> {
        match self {
            Self::Current { .. } => Some(0),
            Self::CatchUpRequired { applied, observed } => {
                Some(observed.sequence - applied.sequence)
            }
            Self::EpochMismatch { .. } => None,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub(crate) enum FreshnessModelError {
    #[error("derived cursor {applied:?} is ahead of observed truth {observed:?}")]
    AppliedAhead {
        applied: TruthCursor,
        observed: TruthCursor,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn materialized_duplicate_diagnostics_follow_authoritative_family_order() {
        let duplicate = |family: &str| MaterializedSemanticDuplicate {
            family: family.to_owned(),
            semantic_id: format!("{family}:sha256:test"),
            event_ids: vec!["evt:sha256:a".to_owned(), "evt:sha256:b".to_owned()],
            event_count: 2,
        };
        let diagnostics = materialized_duplicate_diagnostics(&[
            duplicate("validation"),
            duplicate("assessment"),
            duplicate("response"),
            duplicate("request"),
            duplicate("observation"),
        ]);

        assert_eq!(
            diagnostics
                .iter()
                .map(|diagnostic| diagnostic["code"].as_str().unwrap())
                .collect::<Vec<_>>(),
            vec![
                "duplicate_semantic_observation_event",
                "duplicate_semantic_input_request_open_event",
                "duplicate_semantic_input_request_response_event",
                "duplicate_semantic_assessment_event",
                "duplicate_semantic_validation_event",
            ]
        );
    }
}
