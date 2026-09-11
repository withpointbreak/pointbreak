//! Descriptive evidence from one completed write, never authority for a later operation.
use serde::{Deserialize, Serialize};

use crate::session::ProjectionDiagnostic;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WriteAcknowledgementV1 {
    pub authority_outcome: AuthorityWriteOutcomeV1,
    pub derived: DerivedWriteAcknowledgementV1,
    pub legacy_projection_state: LegacyProjectionStateV1,
    pub operation_receipt: OperationReceiptAcknowledgementV1,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuthorityWriteOutcomeV1 {
    Created,
    Existing,
    Mixed,
    Unchanged,
}

impl AuthorityWriteOutcomeV1 {
    pub fn from_counts(created: usize, existing: usize) -> Self {
        match (created > 0, existing > 0) {
            (true, false) => Self::Created,
            (false, true) => Self::Existing,
            (true, true) => Self::Mixed,
            (false, false) => Self::Unchanged,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DerivedWriteAcknowledgementV1 {
    pub availability: DerivedWriteAvailabilityV1,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub token: Option<DerivedVisibilityTokenV1>,
}

impl DerivedWriteAcknowledgementV1 {
    pub fn new(
        availability: DerivedWriteAvailabilityV1,
        token: Option<DerivedVisibilityTokenV1>,
    ) -> Result<Self, &'static str> {
        if matches!(
            availability,
            DerivedWriteAvailabilityV1::Current | DerivedWriteAvailabilityV1::CatchingUp
        ) != token.is_some()
        {
            return Err("only current and catching_up require a derived token");
        }
        Ok(Self {
            availability,
            token,
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DerivedWriteAvailabilityV1 {
    Off,
    Current,
    CatchingUp,
    Unavailable,
    NotObserved,
}

/// A call-bound observation. Head sequences are comparable only within one generation and epoch.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DerivedVisibilityTokenV1 {
    pub generation_id: String,
    pub epoch: u64,
    pub head_sequence: u64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LegacyProjectionStateV1 {
    Refreshed,
    RefreshFailed,
    NotAttempted,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OperationReceiptAcknowledgementV1 {
    pub state: OperationReceiptStateV1,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub receipt_id: Option<String>,
}

impl OperationReceiptAcknowledgementV1 {
    pub fn new(
        state: OperationReceiptStateV1,
        receipt_id: Option<String>,
    ) -> Result<Self, &'static str> {
        if (state != OperationReceiptStateV1::NotRecorded) != receipt_id.is_some() {
            return Err("only recorded and existing require a receipt ID");
        }
        Ok(Self { state, receipt_id })
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OperationReceiptStateV1 {
    Recorded,
    Existing,
    NotRecorded,
}

#[derive(Debug)]
pub(crate) struct DerivedWriteAggregate {
    pub(crate) derived: DerivedWriteAcknowledgementV1,
    pub(crate) diagnostics: Vec<ProjectionDiagnostic>,
    observed_token: Option<DerivedVisibilityTokenV1>,
}

impl Default for DerivedWriteAggregate {
    fn default() -> Self {
        Self {
            derived: DerivedWriteAcknowledgementV1::new(
                DerivedWriteAvailabilityV1::NotObserved,
                None,
            )
            .unwrap(),
            diagnostics: Vec::new(),
            observed_token: None,
        }
    }
}

impl DerivedWriteAggregate {
    pub(crate) fn add(
        &mut self,
        derived: DerivedWriteAcknowledgementV1,
        diagnostics: impl IntoIterator<Item = ProjectionDiagnostic>,
    ) {
        for diagnostic in diagnostics {
            self.push_diagnostic(diagnostic);
        }
        let mut conflict = false;
        if let Some(token) = derived.token {
            if let Some(previous) = &mut self.observed_token {
                if previous.generation_id != token.generation_id || previous.epoch != token.epoch {
                    conflict = true;
                } else {
                    previous.head_sequence = previous.head_sequence.max(token.head_sequence);
                }
            } else {
                self.observed_token = Some(token);
            }
        }
        let availability = if conflict {
            self.push_diagnostic(ProjectionDiagnostic {
                code: "derived_write_token_conflict".into(),
                message: "write observations refer to different derived generations or epochs"
                    .into(),
            });
            DerivedWriteAvailabilityV1::Unavailable
        } else if rank(&derived.availability) > rank(&self.derived.availability) {
            derived.availability
        } else {
            self.derived.availability.clone()
        };
        let token = if matches!(
            availability,
            DerivedWriteAvailabilityV1::Current | DerivedWriteAvailabilityV1::CatchingUp
        ) {
            self.observed_token.clone()
        } else {
            None
        };
        self.derived = DerivedWriteAcknowledgementV1::new(availability, token)
            .expect("aggregated token agrees with availability");
    }

    fn push_diagnostic(&mut self, diagnostic: ProjectionDiagnostic) {
        if !self
            .diagnostics
            .iter()
            .any(|previous| previous.code == diagnostic.code)
        {
            self.diagnostics.push(diagnostic);
        }
    }
}

fn rank(availability: &DerivedWriteAvailabilityV1) -> u8 {
    use DerivedWriteAvailabilityV1::*;
    match availability {
        NotObserved => 0,
        Off => 1,
        Current => 2,
        CatchingUp => 3,
        Unavailable => 4,
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    const PRODUCERS: &[(&str, &str, &str)] = &[
        ("capture.rs", "CaptureResult", "pointbreak.review-capture"),
        (
            "capture.rs",
            "ChangeCaptureReceiptV1",
            "pointbreak.change-capture-receipt.v1",
        ),
        (
            "association/mod.rs",
            "AssociateCommitResult",
            "pointbreak.review-association-commit",
        ),
        (
            "association/mod.rs",
            "WithdrawCommitResult",
            "pointbreak.review-association-commit-withdrawn",
        ),
        (
            "association/mod.rs",
            "AssociateRefResult",
            "pointbreak.review-association-ref",
        ),
        (
            "association/mod.rs",
            "WithdrawRefResult",
            "pointbreak.review-association-ref-withdrawn",
        ),
        ("ingest.rs", "IngestEventsResult", ""),
        (
            "assessment/add.rs",
            "AssessmentAddResult",
            "pointbreak.review-assessment-add",
        ),
        (
            "observation/add.rs",
            "ObservationAddResult",
            "pointbreak.review-observation-add",
        ),
        (
            "artifact_removal/mod.rs",
            "RemoveResult",
            "pointbreak.store-remove",
        ),
        (
            "input_request/open.rs",
            "InputRequestOpenResult",
            "pointbreak.review-input-request-open",
        ),
        (
            "input_request/respond.rs",
            "InputRequestRespondResult",
            "pointbreak.review-input-request-respond",
        ),
        (
            "event_signature/mod.rs",
            "EventSignatureRecordResult",
            "pointbreak.review-endorse",
        ),
        (
            "validation/add.rs",
            "ValidationAddResult",
            "pointbreak.review-validation-add",
        ),
        (
            "landing.rs",
            "LandCommitResultV1",
            "pointbreak.association-land.v1",
        ),
        (
            "fact_port.rs",
            "FactPortResultV1",
            "pointbreak.review-fact-port.v1",
        ),
        (
            "../store/bundle.rs",
            "StoreLinkResult",
            "pointbreak.store-link",
        ),
        (
            "../store/bundle.rs",
            "MigrateToCommonDirResult",
            "pointbreak.store-migrate",
        ),
    ];

    #[test]
    fn acknowledgement_live_producer_matrix() {
        use std::collections::BTreeSet;
        use std::path::Path;
        fn sources(dir: &Path, result: &mut Vec<(std::path::PathBuf, String)>) {
            for entry in std::fs::read_dir(dir).unwrap() {
                let path = entry.unwrap().path();
                if path.is_dir() {
                    sources(&path, result);
                } else if path.extension().is_some_and(|extension| extension == "rs") {
                    result.push((path.clone(), std::fs::read_to_string(path).unwrap()));
                }
            }
        }
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        let workflow = root.join("src/session/workflow");
        let mut files = Vec::new();
        sources(&workflow, &mut files);
        files.push((
            workflow.join("../store/bundle.rs"),
            std::fs::read_to_string(workflow.join("../store/bundle.rs")).unwrap(),
        ));
        let sites: BTreeSet<_> = files
            .iter()
            .filter(|(_, source)| {
                source.lines().any(|line| {
                    line.contains("= publish_legacy_state_projection(")
                        || line.trim() == "Durability::Projection,"
                })
            })
            .map(|(path, _)| path.strip_prefix(&workflow).unwrap().to_str().unwrap())
            .collect();
        assert_eq!(sites, PRODUCERS.iter().map(|row| row.0).collect());
        assert_eq!(sites.len(), 13);
        assert_eq!(PRODUCERS.len(), 18);
        assert_eq!(
            PRODUCERS
                .iter()
                .map(|row| row.1)
                .collect::<BTreeSet<_>>()
                .len(),
            18
        );
        let mut all_source = Vec::new();
        sources(&root.join("src"), &mut all_source);
        // Omit this matrix itself: each inventory name must resolve to a real declaration/schema.
        let all_source = all_source
            .into_iter()
            .filter(|(path, _)| !path.ends_with("acknowledgement.rs"))
            .map(|(_, source)| source)
            .collect::<Vec<_>>()
            .join("\n");
        for (_, result, schema) in PRODUCERS {
            assert!(
                all_source.contains(&format!("struct {result} {{")),
                "missing result {result}"
            );
            if !schema.is_empty() {
                assert!(
                    all_source.contains(&format!("\"{schema}\"")),
                    "missing schema {schema}"
                );
            }
        }
    }

    fn token(generation: &str, epoch: u64, sequence: u64) -> DerivedVisibilityTokenV1 {
        DerivedVisibilityTokenV1 {
            generation_id: generation.into(),
            epoch,
            head_sequence: sequence,
        }
    }

    #[test]
    fn acknowledgement_constructor_and_wire_contract() {
        use DerivedWriteAvailabilityV1::*;
        for availability in [Off, Current, CatchingUp, Unavailable, NotObserved] {
            let needs_token = matches!(availability, Current | CatchingUp);
            assert_eq!(
                DerivedWriteAcknowledgementV1::new(availability.clone(), None).is_ok(),
                !needs_token
            );
            assert_eq!(
                DerivedWriteAcknowledgementV1::new(availability, Some(token("g", 3, 9))).is_ok(),
                needs_token
            );
        }
        use OperationReceiptStateV1::*;
        for state in [Recorded, Existing, NotRecorded] {
            let needs_id = state != NotRecorded;
            assert_eq!(
                OperationReceiptAcknowledgementV1::new(state.clone(), None).is_ok(),
                !needs_id
            );
            assert_eq!(
                OperationReceiptAcknowledgementV1::new(state, Some("op".into())).is_ok(),
                needs_id
            );
        }
        let ack = WriteAcknowledgementV1 {
            authority_outcome: AuthorityWriteOutcomeV1::Mixed,
            derived: DerivedWriteAcknowledgementV1::new(CatchingUp, Some(token("g", 3, 9)))
                .unwrap(),
            legacy_projection_state: LegacyProjectionStateV1::RefreshFailed,
            operation_receipt: OperationReceiptAcknowledgementV1::new(Recorded, Some("op".into()))
                .unwrap(),
        };
        let wire = json!({"acknowledgement": ack});
        assert_eq!(
            wire,
            json!({"acknowledgement": {"authorityOutcome":"mixed", "derived":{"availability":"catching_up","token":{"generationId":"g","epoch":3,"headSequence":9}},"legacyProjectionState":"refresh_failed","operationReceipt":{"state":"recorded","receiptId":"op"}}})
        );
        assert_eq!(
            serde_json::from_value::<WriteAcknowledgementV1>(wire["acknowledgement"].clone())
                .unwrap(),
            ack
        );
    }

    #[test]
    fn acknowledgement_authority_counts() {
        use AuthorityWriteOutcomeV1::*;
        for (created, existing, expected) in [
            (0, 0, Unchanged),
            (2, 0, Created),
            (0, 2, Existing),
            (1, 2, Mixed),
        ] {
            assert_eq!(
                AuthorityWriteOutcomeV1::from_counts(created, existing),
                expected
            );
        }
    }

    #[test]
    fn acknowledgement_aggregation_preserves_tokens_and_deduplicates_diagnostics() {
        use DerivedWriteAvailabilityV1::*;
        let mut aggregate = DerivedWriteAggregate::default();
        for (availability, sequence) in [(Current, 2), (CatchingUp, 5), (Current, 9)] {
            aggregate.add(
                DerivedWriteAcknowledgementV1::new(availability, Some(token("g", 3, sequence)))
                    .unwrap(),
                [ProjectionDiagnostic {
                    code: "pending".into(),
                    message: sequence.to_string(),
                }],
            );
        }
        assert_eq!(aggregate.derived.availability, CatchingUp);
        assert_eq!(aggregate.derived.token, Some(token("g", 3, 9)));
        assert_eq!(aggregate.diagnostics.len(), 1);
        assert_eq!(aggregate.diagnostics[0].message, "2");
        aggregate.add(DerivedWriteAcknowledgementV1::new(Off, None).unwrap(), []);
        assert_eq!(aggregate.derived.availability, CatchingUp);
        aggregate.add(
            DerivedWriteAcknowledgementV1::new(Unavailable, None).unwrap(),
            [],
        );
        assert_eq!(aggregate.derived.availability, Unavailable);
        assert!(aggregate.derived.token.is_none());
    }

    #[test]
    fn acknowledgement_conflicting_generation_or_epoch_fails_closed() {
        for other in [token("other", 3, 9), token("g", 4, 9)] {
            let mut aggregate = DerivedWriteAggregate::default();
            for t in [token("g", 3, 2), other] {
                aggregate.add(
                    DerivedWriteAcknowledgementV1::new(
                        DerivedWriteAvailabilityV1::Current,
                        Some(t),
                    )
                    .unwrap(),
                    [],
                );
            }
            assert_eq!(
                aggregate.derived.availability,
                DerivedWriteAvailabilityV1::Unavailable
            );
            assert!(aggregate.derived.token.is_none());
            assert_eq!(aggregate.diagnostics.len(), 1);
        }
    }
}
