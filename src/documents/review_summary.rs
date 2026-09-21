// Document builders for the `pointbreak summary show` and `summary check` commands.
use serde::Serialize;

use crate::documents::DiagnosticDocument;
use crate::session::{
    ActorRelation, COUNTED_INPUT_DIGEST_ALGORITHM, Count, CountedInputReceipt,
    EventVerificationStatus, FirstCaptureResults, Measure, Population, ReceiptCheckResult,
    ReceiptEntryDifference, ReceiptEntryOutcome, ReviewSummaryResult, RoundProfile,
    format_rfc3339_utc_millis,
};

/// Emitted schema for `pointbreak summary show`.
pub const REVIEW_SUMMARY_SCHEMA: &str = "pointbreak.review-summary";
/// Emitted schema for `pointbreak summary check`.
pub const REVIEW_SUMMARY_CHECK_SCHEMA: &str = "pointbreak.review-summary-check";
/// Names the population and measure definitions a summary was computed under.
pub const REVIEW_SUMMARY_METRIC_DEFINITIONS: &str = "pointbreak.review-summary/1";

/// Documented body for `pointbreak.review-summary`: counts with their
/// denominators, the evidence basis of every figure, and the counted-input
/// receipt. No ratio, score, or individual appears anywhere in it.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReviewSummaryBody {
    population: PopulationDocument,
    review_rounds: ReviewRoundsDocument,
    landed_work_coverage: LandedWorkCoverageDocument,
    record_internal_measures: RecordInternalMeasuresDocument,
    provenance: ProvenanceDocument,
}

/// Build the `pointbreak.review-summary` document from a summary read over the
/// store's facts. `computed_at` is supplied by the caller so the builder stays
/// pure; `include_entries` inlines every counted-input receipt entry.
pub fn review_summary_document(
    result: &ReviewSummaryResult,
    include_entries: bool,
    computed_at: String,
) -> DiagnosticDocument<ReviewSummaryBody> {
    review_summary_document_with_identity(result, None, include_entries, computed_at)
}

/// Build the `pointbreak.review-summary` document from a summary read over a
/// derived generation. `projection_stamp` names that local generation and binds
/// no event content; the receipt is computed from the counted events' recorded
/// bytes either way.
pub fn derived_review_summary_document(
    result: &ReviewSummaryResult,
    projection_stamp: String,
    include_entries: bool,
    computed_at: String,
) -> DiagnosticDocument<ReviewSummaryBody> {
    review_summary_document_with_identity(
        result,
        Some(projection_stamp),
        include_entries,
        computed_at,
    )
}

fn review_summary_document_with_identity(
    result: &ReviewSummaryResult,
    projection_stamp: Option<String>,
    include_entries: bool,
    computed_at: String,
) -> DiagnosticDocument<ReviewSummaryBody> {
    let summary = &result.summary;
    let basis = if projection_stamp.is_some() {
        "projection"
    } else {
        "factSet"
    };
    let event_set_hash = projection_stamp
        .is_none()
        .then(|| result.event_set_hash.clone());
    DiagnosticDocument::new(
        REVIEW_SUMMARY_SCHEMA,
        ReviewSummaryBody {
            population: PopulationDocument::new(&summary.population),
            review_rounds: ReviewRoundsDocument {
                unit: "currentMemberRevisionsPerChange",
                profile: MeasureDocument::from_measure(&summary.profile, RoundProfileDocument::new),
                first_capture: MeasureDocument::from_measure(
                    &summary.first_capture,
                    FirstCaptureDocument::new,
                ),
            },
            landed_work_coverage: landed_work_coverage_unavailable(),
            record_internal_measures: RecordInternalMeasuresDocument {
                denominator_unit: "capturedRevisions",
                assessed_capture_share: MeasureDocument::from_measure(
                    &summary.assessed_capture,
                    |count| AssessedDocument {
                        assessed: count.count,
                        of: count.of,
                    },
                ),
                commit_associated_capture_share: MeasureDocument::from_measure(
                    &summary.commit_associated_capture,
                    |count| AssociatedDocument {
                        associated: count.count,
                        of: count.of,
                    },
                ),
                first_capture_acceptance: MeasureDocument::from_measure(
                    &summary.first_capture_acceptance,
                    |count: &Count| AcceptanceDocument {
                        accepting: count.count,
                        of: count.of,
                        unit: "assessedFirstCaptures",
                    },
                ),
                actor_distinct_assessment_share: MeasureDocument::from_measure(
                    &summary.actor_relation,
                    ActorRelationDocument::new,
                ),
            },
            provenance: ProvenanceDocument {
                basis,
                event_set_hash,
                projection_stamp,
                event_count: result.event_count,
                metric_definitions: REVIEW_SUMMARY_METRIC_DEFINITIONS,
                computed_at,
                counted_inputs: CountedInputsDocument::new(&result.receipt, include_entries),
            },
        },
        result.diagnostics.clone(),
    )
}

/// Documented body for `pointbreak.review-summary-check`: how each fact a saved
/// receipt lists compares with the store now. It speaks only for the listed
/// facts, never for the store as a whole.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReviewSummaryCheckBody {
    receipt: CheckedReceiptDocument,
    digest_matches_entries: bool,
    matched: usize,
    changed: usize,
    missing: usize,
    differing: Vec<ReceiptDifferenceDocument>,
    completeness: &'static str,
}

/// Build the `pointbreak.review-summary-check` document from a receipt re-check.
pub fn review_summary_check_document(
    result: &ReceiptCheckResult,
) -> DiagnosticDocument<ReviewSummaryCheckBody> {
    DiagnosticDocument::new(
        REVIEW_SUMMARY_CHECK_SCHEMA,
        ReviewSummaryCheckBody {
            receipt: CheckedReceiptDocument {
                algorithm: result.algorithm.clone(),
                digest: result.recorded_digest.clone(),
                count: result.recorded_count,
            },
            digest_matches_entries: result.digest_matches_entries,
            matched: result.matched,
            changed: result.changed,
            missing: result.missing,
            differing: result
                .differing
                .iter()
                .map(ReceiptDifferenceDocument::new)
                .collect(),
            completeness: "notProven",
        },
        result.diagnostics.clone(),
    )
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct CheckedReceiptDocument {
    algorithm: String,
    digest: String,
    count: usize,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ReceiptDifferenceDocument {
    event_id: String,
    outcome: &'static str,
    recorded_payload_hash: String,
    recorded_event_record_hash: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    current_payload_hash: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    current_event_record_hash: Option<String>,
}

impl ReceiptDifferenceDocument {
    fn new(difference: &ReceiptEntryDifference) -> Self {
        Self {
            event_id: difference.event_id.clone(),
            outcome: match difference.outcome {
                ReceiptEntryOutcome::Matched => "matched",
                ReceiptEntryOutcome::Changed => "changed",
                ReceiptEntryOutcome::Missing => "missing",
            },
            recorded_payload_hash: difference.recorded_payload_hash.clone(),
            recorded_event_record_hash: difference.recorded_event_record_hash.clone(),
            current_payload_hash: difference.current_payload_hash.clone(),
            current_event_record_hash: difference.current_event_record_hash.clone(),
        }
    }
}

/// A measure is either computed, carrying its counts and denominator, or
/// unavailable, carrying only its reasons: never a zero standing in for absence.
#[derive(Serialize)]
#[serde(tag = "state", rename_all = "camelCase")]
enum MeasureDocument<T> {
    Computed(T),
    Unavailable { reasons: Vec<&'static str> },
}

impl<T> MeasureDocument<T> {
    fn from_measure<M>(measure: &Measure<M>, computed: impl FnOnce(&M) -> T) -> Self {
        match measure {
            Measure::Computed(value) => Self::Computed(computed(value)),
            Measure::Unavailable { reasons } => Self::Unavailable {
                reasons: reasons.clone(),
            },
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct PopulationDocument {
    definition: &'static str,
    membership_basis: &'static str,
    changes_seen: u64,
    changes_counted: u64,
    memberships: CurrentAndHistoricalDocument,
    captured_revisions: CurrentAndHistoricalDocument,
    #[serde(skip_serializing_if = "Option::is_none")]
    observed_window: Option<ObservedWindowDocument>,
    exclusions: Vec<ExclusionDocument>,
}

impl PopulationDocument {
    fn new(population: &Population) -> Self {
        let backfill = &population.migration_backfill;
        Self {
            definition: "reviewChanges",
            membership_basis: "current",
            changes_seen: population.changes_seen,
            changes_counted: population.changes_counted,
            memberships: CurrentAndHistoricalDocument {
                current: population.memberships.current,
                historical: population.memberships.historical,
            },
            captured_revisions: CurrentAndHistoricalDocument {
                current: population.captured_revisions.current,
                historical: population.captured_revisions.historical,
            },
            observed_window: population
                .observed_window
                .map(|window| ObservedWindowDocument {
                    from: format_rfc3339_utc_millis(window.from_millis),
                    to: format_rfc3339_utc_millis(window.to_millis),
                }),
            exclusions: vec![
                ExclusionDocument::MigrationBackfill {
                    basis: if backfill.manifest_hash.is_some() {
                        "storeActivationManifest"
                    } else {
                        "noActivationManifest"
                    },
                    manifest_hash: backfill.manifest_hash.clone(),
                    changes: backfill.changes,
                    memberships: backfill.memberships,
                },
                ExclusionDocument::NoCurrentMembers {
                    changes: population.no_current_members,
                },
            ],
        }
    }
}

#[derive(Serialize)]
struct CurrentAndHistoricalDocument {
    current: u64,
    historical: u64,
}

#[derive(Serialize)]
struct ObservedWindowDocument {
    from: String,
    to: String,
}

#[derive(Serialize)]
#[serde(
    tag = "reason",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
enum ExclusionDocument {
    MigrationBackfill {
        basis: &'static str,
        #[serde(skip_serializing_if = "Option::is_none")]
        manifest_hash: Option<String>,
        changes: u64,
        memberships: u64,
    },
    NoCurrentMembers {
        changes: u64,
    },
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ReviewRoundsDocument {
    unit: &'static str,
    profile: MeasureDocument<RoundProfileDocument>,
    first_capture: MeasureDocument<FirstCaptureDocument>,
}

#[derive(Serialize)]
struct RoundProfileDocument {
    of: u64,
    distribution: Vec<RoundCountDocument>,
}

impl RoundProfileDocument {
    fn new(profile: &RoundProfile) -> Self {
        Self {
            of: profile.of,
            distribution: profile
                .distribution
                .iter()
                .map(|row| RoundCountDocument {
                    rounds: row.rounds,
                    changes: row.changes,
                })
                .collect(),
        }
    }
}

#[derive(Serialize)]
struct RoundCountDocument {
    rounds: u64,
    changes: u64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct FirstCaptureDocument {
    of: u64,
    accepted: u64,
    needs_changes: u64,
    other_verdict: u64,
    unassessed: u64,
}

impl FirstCaptureDocument {
    fn new(results: &FirstCaptureResults) -> Self {
        Self {
            of: results.of,
            accepted: results.accepted,
            needs_changes: results.needs_changes,
            other_verdict: results.other_verdict,
            unassessed: results.unassessed,
        }
    }
}

/// The six landed-work coverage rungs. Each is denominated in commits on the
/// integration branch (or, for proved landing, in associations) and needs that
/// branch's history, which this read does not walk.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct LandedWorkCoverageDocument {
    tip_exact: CoverageRungDocument,
    associated_range: CoverageRungDocument,
    assessed_range: CoverageRungDocument,
    accepting_verdict_range: CoverageRungDocument,
    distinct_identity: CoverageRungDocument,
    proved_landing: CoverageRungDocument,
}

#[derive(Serialize)]
struct CoverageRungDocument {
    state: &'static str,
    reasons: Vec<&'static str>,
    unit: &'static str,
}

fn landed_work_coverage_unavailable() -> LandedWorkCoverageDocument {
    let rung = |reasons: &[&'static str], unit: &'static str| CoverageRungDocument {
        state: "unavailable",
        reasons: reasons.to_vec(),
        unit,
    };
    let not_walked = ["integrationRefNotWalked"];
    let commits = "commitsOnIntegrationRef";
    LandedWorkCoverageDocument {
        tip_exact: rung(&not_walked, commits),
        associated_range: rung(&not_walked, commits),
        assessed_range: rung(&not_walked, commits),
        accepting_verdict_range: rung(&not_walked, commits),
        distinct_identity: rung(&["integrationRefNotWalked", "signingKeysNotRead"], commits),
        proved_landing: rung(&not_walked, "associations"),
    }
}

/// Measures of the record against itself, denominated in captured Revisions.
/// None of them says anything about landed work.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct RecordInternalMeasuresDocument {
    denominator_unit: &'static str,
    assessed_capture_share: MeasureDocument<AssessedDocument>,
    commit_associated_capture_share: MeasureDocument<AssociatedDocument>,
    first_capture_acceptance: MeasureDocument<AcceptanceDocument>,
    actor_distinct_assessment_share: MeasureDocument<ActorRelationDocument>,
}

#[derive(Serialize)]
struct AssessedDocument {
    assessed: u64,
    of: u64,
}

#[derive(Serialize)]
struct AssociatedDocument {
    associated: u64,
    of: u64,
}

#[derive(Serialize)]
struct AcceptanceDocument {
    accepting: u64,
    of: u64,
    unit: &'static str,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ActorRelationDocument {
    actor_distinct: u64,
    actor_same: u64,
    undetermined: u64,
    of: u64,
    key_evidence: KeyEvidenceDocument,
}

impl ActorRelationDocument {
    fn new(relation: &ActorRelation) -> Self {
        Self {
            actor_distinct: relation.actor_distinct,
            actor_same: relation.actor_same,
            undetermined: relation.undetermined,
            of: relation.of,
            key_evidence: KeyEvidenceDocument {
                state: "unavailable",
                reasons: vec!["signingKeysNotCompared"],
            },
        }
    }
}

/// Signing keys are a separate kind of evidence from actor ids, and this read
/// does not compare them.
#[derive(Serialize)]
struct KeyEvidenceDocument {
    state: &'static str,
    reasons: Vec<&'static str>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ProvenanceDocument {
    basis: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    event_set_hash: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    projection_stamp: Option<String>,
    event_count: usize,
    metric_definitions: &'static str,
    computed_at: String,
    counted_inputs: CountedInputsDocument,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct CountedInputsDocument {
    algorithm: &'static str,
    digest: String,
    count: usize,
    by_kind: ByKindDocument,
    verification: VerificationDocument,
    completeness: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    entries: Option<Vec<CountedInputEntryDocument>>,
}

impl CountedInputsDocument {
    fn new(receipt: &CountedInputReceipt, include_entries: bool) -> Self {
        Self {
            algorithm: COUNTED_INPUT_DIGEST_ALGORITHM,
            digest: receipt.digest.clone(),
            count: receipt.entries.len(),
            by_kind: ByKindDocument {
                membership_claims: receipt.by_kind.membership_claims,
                membership_withdrawals: receipt.by_kind.membership_withdrawals,
                captures: receipt.by_kind.captures,
                assessments: receipt.by_kind.assessments,
                commit_associations: receipt.by_kind.commit_associations,
            },
            verification: VerificationDocument {
                valid: receipt.verification.valid,
                untrusted_key: receipt.verification.untrusted_key,
                invalid: receipt.verification.invalid,
                unsigned: receipt.verification.unsigned,
                allowed_signers_configured: receipt.allowed_signers_configured,
            },
            completeness: "notProven",
            entries: include_entries.then(|| {
                receipt
                    .entries
                    .iter()
                    .map(|entry| CountedInputEntryDocument {
                        event_id: entry.event_id.clone(),
                        payload_hash: entry.payload_hash.clone(),
                        event_record_hash: entry.event_record_hash.clone(),
                        verification_status: entry.verification_status,
                    })
                    .collect()
            }),
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ByKindDocument {
    membership_claims: usize,
    membership_withdrawals: usize,
    captures: usize,
    assessments: usize,
    commit_associations: usize,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct VerificationDocument {
    valid: usize,
    untrusted_key: usize,
    invalid: usize,
    unsigned: usize,
    allowed_signers_configured: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct CountedInputEntryDocument {
    event_id: String,
    payload_hash: String,
    event_record_hash: String,
    verification_status: EventVerificationStatus,
}

#[cfg(test)]
mod tests {
    use serde_json::Value;

    use super::*;
    use crate::session::event::ReviewAssessment;
    use crate::session::{
        AssessmentInput, CaptureInput, CommitAssociationInput, CountedEventRef, CountedInputEntry,
        CountedInputKindCounts, MembershipClaimInput, ReviewSummaryInputs, VerificationTally,
        compute_review_summary,
    };

    const EMAIL_ACTOR: &str = "actor:git-email:someone@example.com";

    fn source(id: &str) -> CountedEventRef {
        CountedEventRef {
            event_id: format!("evt:sha256:{id}"),
            payload_hash: format!("sha256:payload-{id}"),
        }
    }

    fn populated_inputs() -> ReviewSummaryInputs {
        ReviewSummaryInputs {
            memberships: vec![
                MembershipClaimInput {
                    source: source("m1"),
                    claim_id: "claim:1".to_owned(),
                    change_id: "change:c1".to_owned(),
                    revision_id: "rev:r1".to_owned(),
                    backfill: false,
                },
                MembershipClaimInput {
                    source: source("m2"),
                    claim_id: "claim:2".to_owned(),
                    change_id: "change:old".to_owned(),
                    revision_id: "rev:r-old".to_owned(),
                    backfill: true,
                },
            ],
            withdrawals: Vec::new(),
            captures: vec![CaptureInput {
                source: source("c1"),
                revision_id: "rev:r1".to_owned(),
                actor: EMAIL_ACTOR.to_owned(),
                captured_at_millis: Some(1_760_000_000_000),
            }],
            assessments: vec![AssessmentInput {
                source: source("a1"),
                assessment_id: "assess:1".to_owned(),
                revision_id: Some("rev:r1".to_owned()),
                actor: EMAIL_ACTOR.to_owned(),
                verdict: ReviewAssessment::Accepted,
                replaces: Vec::new(),
            }],
            commit_associations: vec![CommitAssociationInput {
                source: source("x1"),
                revision_id: "rev:r1".to_owned(),
            }],
            manifest_hash: Some("sha256:manifest".to_owned()),
        }
    }

    fn receipt() -> CountedInputReceipt {
        let entries = ["a1", "c1", "m1", "x1"]
            .into_iter()
            .map(|id| CountedInputEntry {
                event_id: format!("evt:sha256:{id}"),
                payload_hash: format!("sha256:payload-{id}"),
                event_record_hash: format!("sha256:record-{id}"),
                verification_status: EventVerificationStatus::Unsigned,
            })
            .collect::<Vec<_>>();
        CountedInputReceipt {
            digest: "sha256:digest".to_owned(),
            entries,
            by_kind: CountedInputKindCounts {
                membership_claims: 1,
                captures: 1,
                assessments: 1,
                commit_associations: 1,
                ..CountedInputKindCounts::default()
            },
            verification: VerificationTally {
                unsigned: 4,
                ..VerificationTally::default()
            },
            allowed_signers_configured: false,
        }
    }

    fn result_from(inputs: &ReviewSummaryInputs) -> ReviewSummaryResult {
        ReviewSummaryResult {
            summary: compute_review_summary(inputs),
            receipt: receipt(),
            event_set_hash: "sha256:event-set".to_owned(),
            event_count: 7,
            diagnostics: Vec::new(),
        }
    }

    fn document(inputs: &ReviewSummaryInputs, include_entries: bool) -> Value {
        serde_json::to_value(review_summary_document(
            &result_from(inputs),
            include_entries,
            "2026-09-19T00:00:00.000Z".to_owned(),
        ))
        .unwrap()
    }

    fn keys(value: &Value) -> Vec<String> {
        let mut keys: Vec<_> = value.as_object().unwrap().keys().cloned().collect();
        keys.sort();
        keys
    }

    fn walk(value: &Value, key: Option<&str>, visit: &mut impl FnMut(Option<&str>, &Value)) {
        visit(key, value);
        match value {
            Value::Object(map) => {
                for (child_key, child) in map {
                    walk(child, Some(child_key), visit);
                }
            }
            Value::Array(items) => {
                for item in items {
                    walk(item, key, visit);
                }
            }
            _ => {}
        }
    }

    #[test]
    fn review_summary_document_serializes_computed_measures() {
        let value = document(&populated_inputs(), false);

        assert_eq!(value["schema"], "pointbreak.review-summary");
        assert_eq!(value["version"], 1);
        assert_eq!(
            value["population"],
            serde_json::json!({
                "definition": "reviewChanges",
                "membershipBasis": "current",
                "changesSeen": 2,
                "changesCounted": 1,
                "memberships": {"current": 1, "historical": 1},
                "capturedRevisions": {"current": 1, "historical": 1},
                "observedWindow": {
                    "from": "2025-10-09T08:53:20.000Z",
                    "to": "2025-10-09T08:53:20.000Z"
                },
                "exclusions": [
                    {
                        "reason": "migrationBackfill",
                        "basis": "storeActivationManifest",
                        "manifestHash": "sha256:manifest",
                        "changes": 1,
                        "memberships": 1
                    },
                    {"reason": "noCurrentMembers", "changes": 0}
                ]
            })
        );
        assert_eq!(
            value["reviewRounds"],
            serde_json::json!({
                "unit": "currentMemberRevisionsPerChange",
                "profile": {
                    "state": "computed",
                    "of": 1,
                    "distribution": [{"rounds": 1, "changes": 1}]
                },
                "firstCapture": {
                    "state": "computed",
                    "of": 1,
                    "accepted": 1,
                    "needsChanges": 0,
                    "otherVerdict": 0,
                    "unassessed": 0
                }
            })
        );
        assert_eq!(
            value["recordInternalMeasures"],
            serde_json::json!({
                "denominatorUnit": "capturedRevisions",
                "assessedCaptureShare": {"state": "computed", "assessed": 1, "of": 1},
                "commitAssociatedCaptureShare": {"state": "computed", "associated": 1, "of": 1},
                "firstCaptureAcceptance": {
                    "state": "computed",
                    "accepting": 1,
                    "of": 1,
                    "unit": "assessedFirstCaptures"
                },
                "actorDistinctAssessmentShare": {
                    "state": "computed",
                    "actorDistinct": 0,
                    "actorSame": 1,
                    "undetermined": 0,
                    "of": 1,
                    "keyEvidence": {
                        "state": "unavailable",
                        "reasons": ["signingKeysNotCompared"]
                    }
                }
            })
        );
    }

    #[test]
    fn review_summary_document_renders_every_rung_unavailable() {
        let value = document(&populated_inputs(), false);
        let coverage = &value["landedWorkCoverage"];

        assert_eq!(
            keys(coverage),
            [
                "acceptingVerdictRange",
                "assessedRange",
                "associatedRange",
                "distinctIdentity",
                "provedLanding",
                "tipExact"
            ]
        );
        for rung in [
            "tipExact",
            "associatedRange",
            "assessedRange",
            "acceptingVerdictRange",
        ] {
            assert_eq!(
                coverage[rung],
                serde_json::json!({
                    "state": "unavailable",
                    "reasons": ["integrationRefNotWalked"],
                    "unit": "commitsOnIntegrationRef"
                })
            );
        }
        assert_eq!(
            coverage["distinctIdentity"],
            serde_json::json!({
                "state": "unavailable",
                "reasons": ["integrationRefNotWalked", "signingKeysNotRead"],
                "unit": "commitsOnIntegrationRef"
            })
        );
        assert_eq!(
            coverage["provedLanding"],
            serde_json::json!({
                "state": "unavailable",
                "reasons": ["integrationRefNotWalked"],
                "unit": "associations"
            })
        );
        let coverage_keys = keys(coverage);
        let measure_keys = keys(&value["recordInternalMeasures"]);
        assert!(
            coverage_keys.iter().all(|key| !measure_keys.contains(key)),
            "the two disclosure blocks share no key"
        );
    }

    #[test]
    fn review_summary_document_unavailable_measures_carry_only_state_and_reasons() {
        let value = document(&ReviewSummaryInputs::default(), false);

        let measures = [
            &value["reviewRounds"]["profile"],
            &value["reviewRounds"]["firstCapture"],
            &value["recordInternalMeasures"]["assessedCaptureShare"],
            &value["recordInternalMeasures"]["commitAssociatedCaptureShare"],
            &value["recordInternalMeasures"]["firstCaptureAcceptance"],
            &value["recordInternalMeasures"]["actorDistinctAssessmentShare"],
        ];
        for measure in measures {
            assert_eq!(
                measure,
                &serde_json::json!({"state": "unavailable", "reasons": ["noCountedChanges"]})
            );
        }
        assert!(value["population"].get("observedWindow").is_none());
        assert_eq!(
            value["population"]["exclusions"][0],
            serde_json::json!({
                "reason": "migrationBackfill",
                "basis": "noActivationManifest",
                "changes": 0,
                "memberships": 0
            })
        );
    }

    #[test]
    fn review_summary_document_carries_counts_and_no_ratio_or_float() {
        for value in [
            document(&populated_inputs(), true),
            document(&ReviewSummaryInputs::default(), true),
        ] {
            walk(&value, None, &mut |key, node| {
                if let Value::Number(number) = node {
                    assert!(number.is_u64() || number.is_i64(), "float at {key:?}");
                    let key = key.unwrap_or_default().to_ascii_lowercase();
                    for word in ["rate", "ratio", "percent", "share"] {
                        assert!(!key.contains(word), "numeric {word} field {key}");
                    }
                }
                if let Some(key) = key {
                    assert!(
                        !key.to_ascii_lowercase().contains("score"),
                        "score key {key}"
                    );
                }
            });
        }
    }

    #[test]
    fn review_summary_document_names_no_individual() {
        let text = serde_json::to_string(&document(&populated_inputs(), true)).unwrap();

        for needle in ["actor:", "did:key:", "@"] {
            assert!(!text.contains(needle), "document contains {needle}");
        }
    }

    #[test]
    fn review_summary_document_provenance_is_fact_set_based() {
        let without = document(&populated_inputs(), false);
        let with = document(&populated_inputs(), true);
        let provenance = &without["provenance"];

        assert_eq!(provenance["basis"], "factSet");
        assert_eq!(provenance["eventSetHash"], "sha256:event-set");
        assert!(provenance.get("projectionStamp").is_none());
        assert_eq!(provenance["eventCount"], 7);
        assert_eq!(
            provenance["metricDefinitions"],
            "pointbreak.review-summary/1"
        );
        assert_eq!(provenance["computedAt"], "2026-09-19T00:00:00.000Z");
        assert_eq!(
            provenance["countedInputs"],
            serde_json::json!({
                "algorithm": "shore.event-set.canonical-map.v1",
                "digest": "sha256:digest",
                "count": 4,
                "byKind": {
                    "membershipClaims": 1,
                    "membershipWithdrawals": 0,
                    "captures": 1,
                    "assessments": 1,
                    "commitAssociations": 1
                },
                "verification": {
                    "valid": 0,
                    "untrustedKey": 0,
                    "invalid": 0,
                    "unsigned": 4,
                    "allowedSignersConfigured": false
                },
                "completeness": "notProven"
            })
        );
        let entries = with["provenance"]["countedInputs"]["entries"]
            .as_array()
            .unwrap();
        assert_eq!(entries.len(), 4);
        assert_eq!(
            entries[0],
            serde_json::json!({
                "eventId": "evt:sha256:a1",
                "payloadHash": "sha256:payload-a1",
                "eventRecordHash": "sha256:record-a1",
                "verificationStatus": "unsigned"
            })
        );
    }

    #[test]
    fn review_summary_document_is_registered() {
        assert!(crate::documents::document_registry().contains(&("pointbreak.review-summary", 1)));
    }

    fn check_result() -> ReceiptCheckResult {
        ReceiptCheckResult {
            algorithm: "shore.event-set.canonical-map.v1".to_owned(),
            recorded_digest: "sha256:digest".to_owned(),
            recorded_count: 3,
            digest_matches_entries: true,
            matched: 1,
            changed: 1,
            missing: 1,
            differing: vec![
                ReceiptEntryDifference {
                    event_id: "evt:sha256:c1".to_owned(),
                    outcome: ReceiptEntryOutcome::Changed,
                    recorded_payload_hash: "sha256:payload-c1".to_owned(),
                    recorded_event_record_hash: "sha256:record-c1".to_owned(),
                    current_payload_hash: Some("sha256:payload-c1b".to_owned()),
                    current_event_record_hash: Some("sha256:record-c1b".to_owned()),
                },
                ReceiptEntryDifference {
                    event_id: "evt:sha256:m1".to_owned(),
                    outcome: ReceiptEntryOutcome::Missing,
                    recorded_payload_hash: "sha256:payload-m1".to_owned(),
                    recorded_event_record_hash: "sha256:record-m1".to_owned(),
                    current_payload_hash: None,
                    current_event_record_hash: None,
                },
            ],
            diagnostics: Vec::new(),
        }
    }

    #[test]
    fn review_summary_check_document_reports_counts_and_differences() {
        let value = serde_json::to_value(review_summary_check_document(&check_result())).unwrap();

        assert_eq!(value["schema"], "pointbreak.review-summary-check");
        assert_eq!(value["version"], 1);
        assert_eq!(
            value["receipt"],
            serde_json::json!({
                "algorithm": "shore.event-set.canonical-map.v1",
                "digest": "sha256:digest",
                "count": 3
            })
        );
        assert_eq!(value["digestMatchesEntries"], true);
        assert_eq!(
            (&value["matched"], &value["changed"], &value["missing"]),
            (
                &serde_json::json!(1),
                &serde_json::json!(1),
                &serde_json::json!(1)
            )
        );
        assert_eq!(value["completeness"], "notProven");
        assert_eq!(
            value["differing"],
            serde_json::json!([
                {
                    "eventId": "evt:sha256:c1",
                    "outcome": "changed",
                    "recordedPayloadHash": "sha256:payload-c1",
                    "recordedEventRecordHash": "sha256:record-c1",
                    "currentPayloadHash": "sha256:payload-c1b",
                    "currentEventRecordHash": "sha256:record-c1b"
                },
                {
                    "eventId": "evt:sha256:m1",
                    "outcome": "missing",
                    "recordedPayloadHash": "sha256:payload-m1",
                    "recordedEventRecordHash": "sha256:record-m1"
                }
            ])
        );
    }

    #[test]
    fn review_summary_check_document_carries_no_ratio_or_identifier() {
        let value = serde_json::to_value(review_summary_check_document(&check_result())).unwrap();
        let mut leaves = 0;
        walk(&value, None, &mut |_, leaf| {
            leaves += 1;
            assert!(!leaf.is_f64(), "no float in the document: {leaf}");
            if let Some(text) = leaf.as_str() {
                for forbidden in ["actor:", "did:key:", "@"] {
                    assert!(!text.contains(forbidden), "{forbidden} in {text}");
                }
            }
        });
        assert!(leaves > 0);
    }

    #[test]
    fn review_summary_check_document_is_registered() {
        assert!(
            crate::documents::document_registry().contains(&("pointbreak.review-summary-check", 1))
        );
    }
}
