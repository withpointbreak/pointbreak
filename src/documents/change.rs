//! Change-oriented headless documents built from authoritative domain projections.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use super::{
    AssociationComparisonDocumentV1, ContentAvailabilityV1, RevisionResourceAvailabilityV1,
    RevisionResourceDocumentV1,
};
use crate::error::{Result, ShoreError};
use crate::model::{
    ActorId, ChangeId, EventId, InputRequestId, ReviewFactPortId, ReviewTargetRef, RevisionId,
    RevisionRefV1, TrackId,
};
use crate::session::event::{
    BodyContentType, EventType, FactPortRelationV1, FactRefV1, ReviewFactPortedPayload, ShoreEvent,
    WorkObjectProposal, WorkObjectProposedPayload,
};
use crate::session::{
    BodyContentState, ChangeClaimSupportV1, ChangeDocumentProjectionV1, ChangeLifecycleV1,
    ChangeLinkView, ChangeMembershipClaimViewV1, ChangeProjection, ChangeRelationClaimViewV1,
    ChangeTopologyV1, RevisionRefUnavailableReasonV1,
};

pub const REVIEW_CHANGE_LIST_SCHEMA: &str = "pointbreak.review-change-list";
pub const REVIEW_CHANGE_SCHEMA: &str = "pointbreak.review-change";
pub const REVIEW_CHANGE_REVISION_SCHEMA: &str = "pointbreak.review-change-revision";
pub const ATTENTION_LIST_SCHEMA_V2: &str = "pointbreak.attention-list";
pub use super::inspect::{INSPECT_ATTENTION_SCHEMA_V2, INSPECT_CHANGES_PAGE_SCHEMA};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChangeDeclarationStateV1 {
    Authoritative,
    Incomplete,
    Conflicted,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChangeRevisionCurrencyV1 {
    Current,
    StaleBySupersession,
    MembershipIncomplete,
    MembershipConflicted,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FactFamilyStateV1 {
    Current,
    Stale,
    Withdrawn,
    Conflicted,
    Unavailable,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(
    rename_all = "snake_case",
    rename_all_fields = "camelCase",
    tag = "kind"
)]
pub enum FactContentV1 {
    Observation {
        title: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        body: Option<String>,
    },
    InputRequest {
        title: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        body: Option<String>,
        status: String,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        responses: Vec<FactInputResponseContentV1>,
    },
    Assessment {
        assessment: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        summary: Option<String>,
    },
    Validation {
        check_name: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        command: Option<String>,
        status: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        summary: Option<String>,
    },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FactInputResponseContentV1 {
    pub response_id: String,
    pub outcome: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    pub content_type: BodyContentType,
    pub body_content_state: BodyContentState,
    pub availability: ContentAvailabilityV1,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FactContentPresentationV1 {
    pub content_type: BodyContentType,
    pub body_content_state: BodyContentState,
    pub content: FactContentV1,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FactPresentationV1 {
    pub fact_id: String,
    pub family: String,
    pub origin_revision: RevisionRefV1,
    /// The immutable Review target recorded with this fact. Contextual readers
    /// may use it to place the fact only against this Revision's bound
    /// captured snapshot; it is never inferred from a live worktree.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target: Option<ReviewTargetRef>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context_change_id: Option<ChangeId>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub presented_in_revision: Option<RevisionRefV1>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub port_relation: Option<FactPortRelationV1>,
    pub actor_id: ActorId,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub track_id: Option<TrackId>,
    pub family_state: FactFamilyStateV1,
    pub revision_currency: ChangeRevisionCurrencyV1,
    pub availability: ContentAvailabilityV1,
}

/// Whether one recorded fact-port carrier can contribute continuity in the
/// selected Change and exact target Revision.
///
/// A port is presentation context only. Even an applicable port never moves
/// fact ownership, changes family state, or transfers validation/assessment.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FactPortApplicabilityV1 {
    Applicable,
    Conflicted,
    Unavailable,
}

/// One explicit cross-Revision fact-port carrier.
///
/// This is deliberately separate from [`FactPresentationV1`]. Port writer and
/// track attribution, target-fact identity, rationale, and competing carriers
/// cannot be folded into a fact row without misrepresenting the origin fact.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FactPortPresentationV1 {
    pub port_id: ReviewFactPortId,
    pub origin_revision: RevisionRefV1,
    pub origin_fact: FactRefV1,
    pub target_revision: RevisionRefV1,
    pub relation: FactPortRelationV1,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_fact: Option<FactRefV1>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rationale_content_hash: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context_change_id: Option<ChangeId>,
    pub actor_id: ActorId,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub track_id: Option<TrackId>,
    pub source_event_ids: Vec<EventId>,
    pub applicability: FactPortApplicabilityV1,
    pub diagnostics: Vec<String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RevisionSummarySourceV1 {
    RevisionProposalSummary,
    Absent,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CurrentRevisionPresentationV1 {
    pub revision: RevisionRefV1,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub revision_proposal_summary: Option<String>,
    pub summary_source: RevisionSummarySourceV1,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChangePresentationV1 {
    pub current_revisions: Vec<CurrentRevisionPresentationV1>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct FactPortCarrierV1 {
    port_id: ReviewFactPortId,
    origin_revision: RevisionRefV1,
    origin_fact: FactRefV1,
    target_revision: RevisionRefV1,
    relation: FactPortRelationV1,
    target_fact: Option<FactRefV1>,
    rationale_content_hash: Option<String>,
    context_change_id: Option<ChangeId>,
    actor_id: ActorId,
    track_id: TrackId,
    source_event_ids: Vec<EventId>,
}

/// Body-free document input built from validated events in the same reader
/// generation. Proposal summaries remain here rather than entering
/// `ChangeDocumentProjectionV1`, whose persisted semantic contract is prose-free.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ChangePresentationProjectionV1 {
    pub presentations: BTreeMap<ChangeId, ChangePresentationV1>,
    fact_port_carriers: Vec<FactPortCarrierV1>,
    pub source_projection_stamp: String,
    /// The validated event generation this presentation was built from. The
    /// semantic projection can intentionally omit prose-only event changes, so
    /// the shared presentation stamp must carry this identity separately.
    pub source_event_set_hash: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChangeSummaryV1 {
    pub change_id: ChangeId,
    pub declaration_state: ChangeDeclarationStateV1,
    /// Reserved for a future attributed title-claim family. The activated v1
    /// cohort has no title carrier, so this is always empty rather than
    /// deriving identity from Revision summaries.
    pub title_assertions: Vec<String>,
    pub member_count: usize,
    pub current_revision_refs: Vec<RevisionRefV1>,
    pub topology: ChangeTopologyV1,
    pub lifecycle: ChangeLifecycleV1,
    pub attention_summary: String,
    /// Change-level membership/reference completeness.
    ///
    /// `available` means every member resolves to one exact `RevisionRefV1` in
    /// this projection generation. It does not promise that an exact captured
    /// resource or an externalized fact body still has readable bytes; those
    /// states are reported by the exact Revision and fact documents.
    pub availability_summary: String,
    pub diagnostics: Vec<String>,
    pub projection_stamp: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChangeMemberRevisionV1 {
    pub revision: RevisionRefV1,
    pub supporting_claim_ids: Vec<crate::model::ChangeMembershipClaimId>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UnavailableChangeMemberRevisionV1 {
    pub revision_id: RevisionId,
    pub reason: RevisionRefUnavailableReasonV1,
    pub supporting_claim_ids: Vec<crate::model::ChangeMembershipClaimId>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChangeDetailV1 {
    pub summary: ChangeSummaryV1,
    pub member_revisions: Vec<ChangeMemberRevisionV1>,
    pub unavailable_member_revisions: Vec<UnavailableChangeMemberRevisionV1>,
    pub membership_claims: Vec<ChangeMembershipClaimViewV1>,
    pub membership_withdrawals: Vec<ChangeClaimWithdrawalV1>,
    pub relation_claims: Vec<ChangeRelationClaimViewV1>,
    pub relation_withdrawals: Vec<ChangeClaimWithdrawalV1>,
    pub links: Vec<ChangeLinkView>,
    pub effective_supersedes: Vec<(RevisionRefV1, RevisionRefV1)>,
    pub pending_or_conflicting_edges: Vec<ChangeRelationClaimViewV1>,
    pub current_revision_refs: Vec<RevisionRefV1>,
    pub per_current_revision_qualification: Vec<RevisionQualificationV1>,
    pub operative_obligations: Vec<InputRequestId>,
    pub diagnostics: Vec<String>,
    pub projection_stamp: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChangeClaimWithdrawalV1 {
    pub claim_id: String,
    pub supports: Vec<ChangeClaimSupportV1>,
    pub diagnostics: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RevisionQualificationV1 {
    pub revision: RevisionRefV1,
    pub qualified: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChangeRevisionDetailV1 {
    pub change_id: ChangeId,
    pub revision: RevisionRefV1,
    pub membership_support: Vec<ChangeMembershipClaimViewV1>,
    pub revision_currency: ChangeRevisionCurrencyV1,
    pub relation_classification: String,
    pub exact_revision_document: RevisionResourceDocumentV1,
    pub fact_presentations: Vec<FactPresentationV1>,
    #[serde(default)]
    pub fact_ports: Vec<FactPortPresentationV1>,
    pub associations: Vec<AssociationComparisonDocumentV1>,
    pub availability: RevisionResourceAvailabilityV1,
    pub diagnostics: Vec<String>,
    pub projection_stamp: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChangeListDocumentV1 {
    pub schema: String,
    pub version: u32,
    pub changes: Vec<ChangeSummaryV1>,
    pub diagnostics: Vec<String>,
    pub projection_stamp: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChangeAttentionDocumentV2 {
    pub schema: String,
    pub version: u32,
    pub changes: Vec<ChangeSummaryV1>,
    pub projection_stamp: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChangeListPresentationDocumentV1 {
    #[serde(flatten)]
    pub document: ChangeListDocumentV1,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub presentations: BTreeMap<ChangeId, ChangePresentationV1>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChangeAttentionPresentationDocumentV2 {
    #[serde(flatten)]
    pub document: ChangeAttentionDocumentV2,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub presentations: BTreeMap<ChangeId, ChangePresentationV1>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChangeDetailDocumentV1 {
    pub schema: String,
    pub version: u32,
    #[serde(flatten)]
    pub detail: ChangeDetailV1,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChangeRevisionDocumentV1 {
    pub schema: String,
    pub version: u32,
    #[serde(flatten)]
    pub detail: ChangeRevisionDetailV1,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChangeRevisionPresentationDocumentV1 {
    #[serde(flatten)]
    pub document: ChangeRevisionDocumentV1,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub fact_content_presentations: BTreeMap<String, FactContentPresentationV1>,
}

/// One facade over the semantic and provenance projections. It has no store or
/// SQLite handle, so loose and derived callers cannot diverge in presentation.
#[derive(Clone, Debug)]
pub struct ChangeDocumentFacadeV1 {
    semantic: ChangeProjection,
    provenance: ChangeDocumentProjectionV1,
    presentations: Option<BTreeMap<ChangeId, ChangePresentationV1>>,
    fact_port_carriers: Vec<FactPortCarrierV1>,
    projection_stamp: String,
}

impl ChangeDocumentFacadeV1 {
    pub fn new(semantic: ChangeProjection, provenance: ChangeDocumentProjectionV1) -> Result<Self> {
        if provenance.projection_stamp
            != crate::session::change_document_projection_stamp(&semantic, &provenance)?
        {
            return Err(ShoreError::Message(
                "Change document projection stamp mismatch".to_owned(),
            ));
        }
        for (change_id, view) in &semantic.changes {
            let projected_members = provenance
                .membership_claims
                .iter()
                .filter(|claim| {
                    claim.active
                        && &claim.change_id == change_id
                        && (provenance.revision_refs.contains_key(&claim.revision_id)
                            || provenance
                                .unavailable_revision_refs
                                .contains_key(&claim.revision_id))
                })
                .map(|claim| claim.revision_id.clone())
                .collect::<BTreeSet<_>>();
            if projected_members != view.members {
                return Err(ShoreError::Message(
                    "Change document provenance diverges from semantic membership".to_owned(),
                ));
            }
            for (successor, predecessor) in &view.supersedes {
                let supported = provenance.relation_claims.iter().any(|claim| {
                    claim.active
                        && &claim.change_id == change_id
                        && &claim.successor.revision_id == successor
                        && &claim.predecessor.revision_id == predecessor
                });
                if !supported {
                    return Err(ShoreError::Message(
                        "Change document provenance diverges from effective replacement state"
                            .to_owned(),
                    ));
                }
            }
        }
        Ok(Self {
            semantic,
            projection_stamp: provenance.projection_stamp.clone(),
            provenance,
            presentations: None,
            fact_port_carriers: Vec::new(),
        })
    }

    /// The combined semantic-and-presentation generation used by every
    /// Change-capable Inspector response staged together.
    pub fn projection_stamp(&self) -> &str {
        &self.projection_stamp
    }

    /// Select fact-port authority for one exact contextual Revision from the
    /// same validated event generation as the facade.
    ///
    /// An explicit Change context applies only to that Change. Older unscoped
    /// ports apply wherever both exact endpoint Revisions are active members;
    /// they are never assigned to a Change by event order or lexical identity.
    pub fn fact_port_presentations(
        &self,
        change_id: &ChangeId,
        target_revision: &RevisionRefV1,
    ) -> Result<Vec<FactPortPresentationV1>> {
        let view = self.semantic.changes.get(change_id).ok_or_else(|| {
            ShoreError::Message(format!("Change {} is unavailable", change_id.as_str()))
        })?;
        if self.exact_ref(&target_revision.revision_id).as_ref() != Some(target_revision)
            || !view.members.contains(&target_revision.revision_id)
        {
            return Err(ShoreError::Message(
                "fact-port target is not an exact member of the Change".to_owned(),
            ));
        }
        let mut ports = Vec::new();
        for carrier in self
            .fact_port_carriers
            .iter()
            .filter(|carrier| carrier.target_revision == *target_revision)
        {
            if carrier
                .context_change_id
                .as_ref()
                .is_some_and(|context| context != change_id)
            {
                continue;
            }
            let origin_is_member = self
                .exact_ref(&carrier.origin_revision.revision_id)
                .as_ref()
                == Some(&carrier.origin_revision)
                && view.members.contains(&carrier.origin_revision.revision_id);
            if carrier.context_change_id.is_none() && !origin_is_member {
                continue;
            }
            ports.push(FactPortPresentationV1 {
                port_id: carrier.port_id.clone(),
                origin_revision: carrier.origin_revision.clone(),
                origin_fact: carrier.origin_fact.clone(),
                target_revision: carrier.target_revision.clone(),
                relation: carrier.relation,
                target_fact: carrier.target_fact.clone(),
                rationale_content_hash: carrier.rationale_content_hash.clone(),
                context_change_id: carrier.context_change_id.clone(),
                actor_id: carrier.actor_id.clone(),
                track_id: Some(carrier.track_id.clone()),
                source_event_ids: carrier.source_event_ids.clone(),
                applicability: if origin_is_member {
                    FactPortApplicabilityV1::Applicable
                } else {
                    FactPortApplicabilityV1::Unavailable
                },
                diagnostics: if origin_is_member {
                    Vec::new()
                } else {
                    vec!["fact_port_origin_revision_unavailable".to_owned()]
                },
            });
        }
        let mut semantic_edges = BTreeMap::<String, BTreeSet<String>>::new();
        for port in ports
            .iter()
            .filter(|port| port.applicability == FactPortApplicabilityV1::Applicable)
        {
            semantic_edges
                .entry(fact_port_continuity_key(port)?)
                .or_default()
                .insert(fact_port_semantic_edge(port)?);
        }
        for port in &mut ports {
            if port.applicability == FactPortApplicabilityV1::Applicable
                && semantic_edges
                    .get(&fact_port_continuity_key(port)?)
                    .is_some_and(|edges| edges.len() > 1)
            {
                port.applicability = FactPortApplicabilityV1::Conflicted;
                port.diagnostics
                    .push("fact_port_continuity_conflicted".to_owned());
            }
        }
        Ok(ports)
    }

    /// Bind optional presentation data produced from the exact same validated
    /// event generation. The combined stamp changes when inline proposal copy
    /// changes while the bodyless semantic projection remains unchanged.
    pub(crate) fn with_presentations(
        mut self,
        projection: ChangePresentationProjectionV1,
    ) -> Result<Self> {
        if projection.source_projection_stamp != self.provenance.projection_stamp {
            return Err(ShoreError::Message(
                "Change presentation projection belongs to a different semantic generation"
                    .to_owned(),
            ));
        }
        let expected_change_ids = self
            .semantic
            .changes
            .keys()
            .cloned()
            .collect::<BTreeSet<_>>();
        if projection
            .presentations
            .keys()
            .cloned()
            .collect::<BTreeSet<_>>()
            != expected_change_ids
        {
            return Err(ShoreError::Message(
                "Change presentation projection does not cover the semantic Change set".to_owned(),
            ));
        }
        for (change_id, presentation) in &projection.presentations {
            let view = &self.semantic.changes[change_id];
            let expected = self.current_refs(view);
            let actual = presentation
                .current_revisions
                .iter()
                .map(|current| current.revision.clone())
                .collect::<Vec<_>>();
            if actual != expected
                || presentation.current_revisions.iter().any(|current| {
                    matches!(current.summary_source, RevisionSummarySourceV1::Absent)
                        != current.revision_proposal_summary.is_none()
                })
            {
                return Err(ShoreError::Message(
                    "Change presentation projection diverges from exact current Revision state"
                        .to_owned(),
                ));
            }
        }
        self.projection_stamp = crate::canonical_hash::sha256_json_prefixed(&serde_json::json!({
            "semanticProjectionStamp": self.provenance.projection_stamp,
            "eventSetHash": projection.source_event_set_hash,
            "presentations": projection.presentations,
            "factPortCarriers": projection.fact_port_carriers,
        }))?;
        self.presentations = Some(projection.presentations);
        self.fact_port_carriers = projection.fact_port_carriers;
        Ok(self)
    }

    pub fn list_document(&self) -> ChangeListDocumentV1 {
        self.list_document_with_schema(REVIEW_CHANGE_LIST_SCHEMA)
    }

    /// Build the Inspector page from the same ordered Change summaries as the
    /// cold CLI list. Pagination is intentionally absent in the first capable
    /// cohort; the distinct schema keeps a later bounded page additive without
    /// weakening the CLI document.
    pub fn list_document_for_inspector(&self) -> ChangeListDocumentV1 {
        self.list_document_with_schema(INSPECT_CHANGES_PAGE_SCHEMA)
    }

    pub fn list_document_for_inspector_with_presentations(
        &self,
    ) -> Result<ChangeListPresentationDocumentV1> {
        Ok(ChangeListPresentationDocumentV1 {
            document: self.list_document_for_inspector(),
            presentations: self.presentations.clone().ok_or_else(|| {
                ShoreError::Message("Change presentation projection is unavailable".to_owned())
            })?,
        })
    }

    fn list_document_with_schema(&self, schema: &str) -> ChangeListDocumentV1 {
        ChangeListDocumentV1 {
            schema: schema.to_owned(),
            version: 1,
            changes: self
                .semantic
                .changes
                .values()
                .map(|view| self.summary(view))
                .collect(),
            diagnostics: self.provenance.diagnostics.clone(),
            projection_stamp: self.projection_stamp.clone(),
        }
    }

    /// Build attention from the same Change summary model. Accepted Changes are
    /// omitted; no separate client-side lifecycle policy exists.
    pub fn attention_document(&self, inspect: bool) -> ChangeAttentionDocumentV2 {
        ChangeAttentionDocumentV2 {
            schema: if inspect {
                INSPECT_ATTENTION_SCHEMA_V2
            } else {
                ATTENTION_LIST_SCHEMA_V2
            }
            .to_owned(),
            version: 2,
            changes: self
                .semantic
                .changes
                .values()
                .filter(|view| view.lifecycle != ChangeLifecycleV1::Accepted)
                .map(|view| self.summary(view))
                .collect(),
            projection_stamp: self.projection_stamp.clone(),
        }
    }

    pub fn attention_document_with_presentations(
        &self,
        inspect: bool,
    ) -> Result<ChangeAttentionPresentationDocumentV2> {
        let visible_change_ids = self
            .semantic
            .changes
            .values()
            .filter(|view| view.lifecycle != ChangeLifecycleV1::Accepted)
            .map(|view| view.change_id.clone())
            .collect::<BTreeSet<_>>();
        let presentations = self
            .presentations
            .as_ref()
            .ok_or_else(|| {
                ShoreError::Message("Change presentation projection is unavailable".to_owned())
            })?
            .iter()
            .filter(|(change_id, _)| visible_change_ids.contains(*change_id))
            .map(|(change_id, presentation)| (change_id.clone(), presentation.clone()))
            .collect();
        Ok(ChangeAttentionPresentationDocumentV2 {
            document: self.attention_document(inspect),
            presentations,
        })
    }

    pub fn detail_document(&self, change_id: &ChangeId) -> Result<ChangeDetailDocumentV1> {
        let view = self.semantic.changes.get(change_id).ok_or_else(|| {
            ShoreError::Message(format!("Change {} is unavailable", change_id.as_str()))
        })?;
        let membership_claims = self
            .provenance
            .membership_claims
            .iter()
            .filter(|claim| &claim.change_id == change_id)
            .cloned()
            .collect::<Vec<_>>();
        let relation_claims = self
            .provenance
            .relation_claims
            .iter()
            .filter(|claim| &claim.change_id == change_id)
            .cloned()
            .collect::<Vec<_>>();
        let membership_withdrawals = membership_claims
            .iter()
            .filter(|claim| !claim.withdrawals.is_empty())
            .map(|claim| ChangeClaimWithdrawalV1 {
                claim_id: claim.claim_id.as_str().to_owned(),
                supports: claim.withdrawals.clone(),
                diagnostics: claim.diagnostics.clone(),
            })
            .collect();
        let relation_withdrawals = relation_claims
            .iter()
            .filter(|claim| !claim.withdrawals.is_empty())
            .map(|claim| ChangeClaimWithdrawalV1 {
                claim_id: claim.claim_id.as_str().to_owned(),
                supports: claim.withdrawals.clone(),
                diagnostics: claim.diagnostics.clone(),
            })
            .collect();
        let member_revisions = view
            .members
            .iter()
            .filter_map(|revision_id| {
                self.exact_ref(revision_id)
                    .map(|revision| ChangeMemberRevisionV1 {
                        revision,
                        supporting_claim_ids: membership_claims
                            .iter()
                            .filter(|claim| claim.active && &claim.revision_id == revision_id)
                            .map(|claim| claim.claim_id.clone())
                            .collect(),
                    })
            })
            .collect::<Vec<_>>();
        let unavailable_member_revisions = view
            .members
            .iter()
            .filter_map(|revision_id| {
                self.provenance
                    .unavailable_revision_refs
                    .get(revision_id)
                    .copied()
                    .map(|reason| UnavailableChangeMemberRevisionV1 {
                        revision_id: revision_id.clone(),
                        reason,
                        supporting_claim_ids: membership_claims
                            .iter()
                            .filter(|claim| claim.active && &claim.revision_id == revision_id)
                            .map(|claim| claim.claim_id.clone())
                            .collect(),
                    })
            })
            .collect();
        let effective_supersedes = view
            .supersedes
            .iter()
            .filter_map(|(successor, predecessor)| {
                Some((self.exact_ref(successor)?, self.exact_ref(predecessor)?))
            })
            .collect::<Vec<_>>();
        let current_revision_refs = self.current_refs(view);
        let pending_or_conflicting_edges = relation_claims
            .iter()
            .filter(|claim| {
                claim.active
                    && !effective_supersedes.iter().any(|(successor, predecessor)| {
                        successor == &claim.successor && predecessor == &claim.predecessor
                    })
            })
            .cloned()
            .collect();
        let detail = ChangeDetailV1 {
            summary: self.summary(view),
            member_revisions,
            unavailable_member_revisions,
            membership_claims,
            membership_withdrawals,
            relation_claims,
            relation_withdrawals,
            links: self
                .semantic
                .links
                .iter()
                .filter(|link| {
                    &link.left_change_id == change_id || &link.right_change_id == change_id
                })
                .cloned()
                .collect(),
            effective_supersedes,
            pending_or_conflicting_edges,
            per_current_revision_qualification: current_revision_refs
                .iter()
                .cloned()
                .map(|revision| RevisionQualificationV1 {
                    qualified: view
                        .qualified_current_revisions
                        .contains(&revision.revision_id),
                    revision,
                })
                .collect(),
            current_revision_refs,
            operative_obligations: view.operative_obligations.iter().cloned().collect(),
            diagnostics: view.diagnostics.clone(),
            projection_stamp: self.projection_stamp.clone(),
        };
        Ok(ChangeDetailDocumentV1 {
            schema: REVIEW_CHANGE_SCHEMA.to_owned(),
            version: 1,
            detail,
        })
    }

    pub fn contextual_revision_document(
        &self,
        change_id: &ChangeId,
        revision: &RevisionRefV1,
        exact_revision_document: RevisionResourceDocumentV1,
        fact_presentations: Vec<FactPresentationV1>,
        associations: Vec<AssociationComparisonDocumentV1>,
    ) -> Result<ChangeRevisionDocumentV1> {
        let fact_ports = self.fact_port_presentations(change_id, revision)?;
        self.contextual_revision_document_with_fact_ports(
            change_id,
            revision,
            exact_revision_document,
            fact_presentations,
            fact_ports,
            associations,
        )
    }

    fn contextual_revision_document_with_fact_ports(
        &self,
        change_id: &ChangeId,
        revision: &RevisionRefV1,
        exact_revision_document: RevisionResourceDocumentV1,
        mut fact_presentations: Vec<FactPresentationV1>,
        canonical_fact_ports: Vec<FactPortPresentationV1>,
        associations: Vec<AssociationComparisonDocumentV1>,
    ) -> Result<ChangeRevisionDocumentV1> {
        exact_revision_document.validate_integrity()?;
        if exact_revision_document
            .projection_stamp
            .as_deref()
            .is_some_and(|stamp| stamp != self.projection_stamp.as_str())
        {
            return Err(ShoreError::Message(
                "exact Revision resource belongs to a different facade generation".to_owned(),
            ));
        }
        let view = self.semantic.changes.get(change_id).ok_or_else(|| {
            ShoreError::Message(format!("Change {} is unavailable", change_id.as_str()))
        })?;
        if self.exact_ref(&revision.revision_id).as_ref() != Some(revision)
            || !view.members.contains(&revision.revision_id)
            || exact_revision_document.resource.revision != *revision
        {
            return Err(ShoreError::Message(
                "exact Revision is not an integrity-qualified member of the Change".to_owned(),
            ));
        }
        for association in &associations {
            association.validate_integrity()?;
        }
        if associations
            .iter()
            .any(|association| association.comparison.revision != *revision)
        {
            return Err(ShoreError::Message(
                "association comparison does not target the contextual exact Revision".to_owned(),
            ));
        }
        let mut presented_fact_keys = BTreeSet::new();
        for fact in &fact_presentations {
            let key = serde_json::to_string(&serde_json::json!({
                "originRevision": fact.origin_revision,
                "factId": fact.fact_id,
                "family": fact.family,
                "presentedInRevision": fact.presented_in_revision,
            }))?;
            if !presented_fact_keys.insert(key) {
                return Err(ShoreError::Message(
                    "an exact contextual fact presentation is duplicated".to_owned(),
                ));
            }
        }
        let fact_ports =
            bind_fact_port_hydration(canonical_fact_ports, &fact_presentations, revision);
        let applicable_port_origins = fact_ports
            .iter()
            .filter(|port| port.applicability == FactPortApplicabilityV1::Applicable)
            .map(|port| {
                (
                    port.origin_revision.clone(),
                    fact_ref_id(&port.origin_fact).to_owned(),
                )
            })
            .collect::<Vec<_>>();
        let currency = if view.current_revisions.contains(&revision.revision_id) {
            ChangeRevisionCurrencyV1::Current
        } else {
            ChangeRevisionCurrencyV1::StaleBySupersession
        };
        for fact in &mut fact_presentations {
            if self.exact_ref(&fact.origin_revision.revision_id).as_ref()
                != Some(&fact.origin_revision)
                || !view.members.contains(&fact.origin_revision.revision_id)
            {
                return Err(ShoreError::Message(
                    "fact origin is not an exact member of the contextual Change".to_owned(),
                ));
            }
            fact.context_change_id = Some(change_id.clone());
            fact.revision_currency = if view
                .current_revisions
                .contains(&fact.origin_revision.revision_id)
            {
                ChangeRevisionCurrencyV1::Current
            } else {
                ChangeRevisionCurrencyV1::StaleBySupersession
            };
            if fact.origin_revision == *revision {
                if fact.presented_in_revision.is_some() || fact.port_relation.is_some() {
                    return Err(ShoreError::Message(
                        "an origin-local fact cannot claim a presentation port".to_owned(),
                    ));
                }
            } else if fact.presented_in_revision.as_ref() != Some(revision)
                || fact.port_relation.is_some()
            {
                return Err(ShoreError::Message(
                    "a cross-Revision fact presentation requires the exact target and sibling port authority"
                        .to_owned(),
                ));
            } else if !applicable_port_origins.iter().any(|(origin, fact_id)| {
                origin == &fact.origin_revision && fact_id == &fact.fact_id
            }) {
                return Err(ShoreError::Message(
                    "a cross-Revision fact presentation requires an applicable recorded port"
                        .to_owned(),
                ));
            }
        }
        for port in fact_ports
            .iter()
            .filter(|port| port.applicability == FactPortApplicabilityV1::Applicable)
        {
            let origin_present = fact_presentations.iter().any(|fact| {
                fact.origin_revision == port.origin_revision
                    && fact.fact_id == fact_ref_id(&port.origin_fact)
                    && fact.family == fact_ref_family(&port.origin_fact)
                    && fact.presented_in_revision.as_ref() == Some(revision)
            });
            let target_present = port.target_fact.as_ref().is_none_or(|target_fact| {
                fact_presentations.iter().any(|fact| {
                    fact.origin_revision == *revision
                        && fact.fact_id == fact_ref_id(target_fact)
                        && fact.family == fact_ref_family(target_fact)
                })
            });
            if !origin_present || !target_present {
                return Err(ShoreError::Message(
                    "applicable fact port is missing an exact endpoint fact presentation"
                        .to_owned(),
                ));
            }
        }
        let membership_support = self
            .provenance
            .membership_claims
            .iter()
            .filter(|claim| {
                claim.active
                    && &claim.change_id == change_id
                    && claim.revision_id == revision.revision_id
            })
            .cloned()
            .collect();
        Ok(ChangeRevisionDocumentV1 {
            schema: REVIEW_CHANGE_REVISION_SCHEMA.to_owned(),
            version: 1,
            detail: ChangeRevisionDetailV1 {
                change_id: change_id.clone(),
                revision: revision.clone(),
                membership_support,
                revision_currency: currency,
                relation_classification: if currency == ChangeRevisionCurrencyV1::Current {
                    "current".to_owned()
                } else {
                    "superseded".to_owned()
                },
                availability: exact_revision_document.availability,
                exact_revision_document,
                fact_presentations,
                fact_ports,
                associations,
                diagnostics: view.diagnostics.clone(),
                projection_stamp: self.projection_stamp.clone(),
            },
        })
    }

    pub fn contextual_revision_document_with_fact_content(
        &self,
        change_id: &ChangeId,
        revision: &RevisionRefV1,
        exact_revision_document: RevisionResourceDocumentV1,
        fact_presentations: Vec<FactPresentationV1>,
        associations: Vec<AssociationComparisonDocumentV1>,
        fact_content_presentations: BTreeMap<String, FactContentPresentationV1>,
    ) -> Result<ChangeRevisionPresentationDocumentV1> {
        let fact_ports = self.fact_port_presentations(change_id, revision)?;
        self.contextual_revision_document_with_fact_content_and_ports(
            change_id,
            revision,
            exact_revision_document,
            fact_presentations,
            fact_ports,
            associations,
            fact_content_presentations,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn contextual_revision_document_with_fact_content_and_ports(
        &self,
        change_id: &ChangeId,
        revision: &RevisionRefV1,
        exact_revision_document: RevisionResourceDocumentV1,
        fact_presentations: Vec<FactPresentationV1>,
        canonical_fact_ports: Vec<FactPortPresentationV1>,
        associations: Vec<AssociationComparisonDocumentV1>,
        fact_content_presentations: BTreeMap<String, FactContentPresentationV1>,
    ) -> Result<ChangeRevisionPresentationDocumentV1> {
        let facts_by_id = fact_presentations
            .iter()
            .map(|fact| (fact.fact_id.as_str(), fact))
            .collect::<BTreeMap<_, _>>();
        if facts_by_id.len() != fact_presentations.len()
            || facts_by_id.len() != fact_content_presentations.len()
            || fact_content_presentations
                .keys()
                .any(|fact_id| !facts_by_id.contains_key(fact_id.as_str()))
        {
            return Err(ShoreError::Message(
                "rich fact content must cover every exact contextual fact exactly once".to_owned(),
            ));
        }
        for (fact_id, content) in &fact_content_presentations {
            let fact = facts_by_id[fact_id.as_str()];
            let family_matches = matches!(
                (fact.family.as_str(), &content.content),
                ("observation", FactContentV1::Observation { .. })
                    | ("input_request", FactContentV1::InputRequest { .. })
                    | ("assessment", FactContentV1::Assessment { .. })
                    | ("validation", FactContentV1::Validation { .. })
            );
            if !family_matches
                || !fact_content_availability_is_consistent(
                    fact.availability,
                    content.body_content_state,
                    fact_content_body(&content.content),
                )
                || !fact_input_responses_are_consistent(&content.content)
            {
                return Err(ShoreError::Message(format!(
                    "rich fact content is inconsistent with fact {fact_id}"
                )));
            }
        }
        Ok(ChangeRevisionPresentationDocumentV1 {
            document: self.contextual_revision_document_with_fact_ports(
                change_id,
                revision,
                exact_revision_document,
                fact_presentations,
                canonical_fact_ports,
                associations,
            )?,
            fact_content_presentations,
        })
    }

    fn summary(&self, view: &crate::session::ChangeView) -> ChangeSummaryV1 {
        let current_revision_refs = self.current_refs(view);
        let exact_member_count = view
            .members
            .iter()
            .filter(|revision_id| self.exact_ref(revision_id).is_some())
            .count();
        ChangeSummaryV1 {
            change_id: view.change_id.clone(),
            declaration_state: declaration_state(view),
            title_assertions: Vec::new(),
            member_count: view.members.len(),
            current_revision_refs,
            topology: view.topology,
            lifecycle: view.lifecycle,
            attention_summary: match view.lifecycle {
                ChangeLifecycleV1::Accepted => "clear",
                ChangeLifecycleV1::InProgress => "in_progress",
                ChangeLifecycleV1::Incomplete => "incomplete",
                ChangeLifecycleV1::Conflicted => "conflicted",
            }
            .to_owned(),
            availability_summary: if exact_member_count == view.members.len() {
                "available"
            } else {
                "incomplete"
            }
            .to_owned(),
            diagnostics: view.diagnostics.clone(),
            projection_stamp: self.projection_stamp.clone(),
        }
    }

    fn current_refs(&self, view: &crate::session::ChangeView) -> Vec<RevisionRefV1> {
        view.current_revisions
            .iter()
            .filter_map(|revision_id| self.exact_ref(revision_id))
            .collect()
    }

    fn exact_ref(&self, revision_id: &RevisionId) -> Option<RevisionRefV1> {
        let refs = self.provenance.revision_refs.get(revision_id)?;
        (refs.len() == 1).then(|| refs[0].clone())
    }
}

fn fact_content_availability_is_consistent(
    availability: ContentAvailabilityV1,
    state: BodyContentState,
    body: Option<&str>,
) -> bool {
    match availability {
        ContentAvailabilityV1::Available => state == BodyContentState::Present,
        ContentAvailabilityV1::Removed => state.is_removed() && body.is_none(),
        ContentAvailabilityV1::Missing
        | ContentAvailabilityV1::Mismatch
        | ContentAvailabilityV1::NonTextual => state == BodyContentState::Present && body.is_none(),
    }
}

fn fact_input_responses_are_consistent(content: &FactContentV1) -> bool {
    let FactContentV1::InputRequest { responses, .. } = content else {
        return true;
    };
    responses.iter().all(|response| {
        fact_content_availability_is_consistent(
            response.availability,
            response.body_content_state,
            response.reason.as_deref(),
        )
    })
}

fn declaration_state(view: &crate::session::ChangeView) -> ChangeDeclarationStateV1 {
    let diagnostics: BTreeSet<_> = view.diagnostics.iter().map(String::as_str).collect();
    if diagnostics.contains("change_declaration_missing") {
        ChangeDeclarationStateV1::Incomplete
    } else if diagnostics.contains("change_declaration_conflict")
        || diagnostics.contains("change_declaration_identity_mismatch")
    {
        ChangeDeclarationStateV1::Conflicted
    } else {
        ChangeDeclarationStateV1::Authoritative
    }
}

/// Build card/list presentation solely from validated inline journal events.
///
/// This function deliberately has no store, content-backend, or resource
/// loader parameter. A list caller therefore cannot hydrate captured resources
/// or externalized fact bodies by accident. Rich fact hydration remains on the
/// explicitly selected exact-Revision path.
pub(crate) fn change_presentation_projection(
    semantic: &ChangeProjection,
    provenance: &ChangeDocumentProjectionV1,
    events: &[ShoreEvent],
    event_set_hash: &str,
) -> Result<ChangePresentationProjectionV1> {
    let mut proposal_summaries = BTreeMap::<RevisionRefV1, BTreeSet<Option<String>>>::new();
    for event in events {
        if event.event_type != EventType::WorkObjectProposed {
            continue;
        }
        let payload: WorkObjectProposedPayload = serde_json::from_value(event.payload.clone())?;
        if let WorkObjectProposal::Revision {
            revision,
            summary,
            object_artifact_content_hash,
            ..
        } = payload.work_object
        {
            let Ok(reference) = RevisionRefV1::new(revision.id, object_artifact_content_hash)
            else {
                continue;
            };
            proposal_summaries
                .entry(reference)
                .or_default()
                .insert(summary);
        }
    }

    let mut presentations = BTreeMap::new();
    for (change_id, view) in &semantic.changes {
        let current_revisions = view
            .current_revisions
            .iter()
            .filter_map(|revision_id| exact_ref_from_projection(provenance, revision_id))
            .map(|revision| {
                let summaries = proposal_summaries
                    .get(&revision)
                    .into_iter()
                    .flat_map(|summaries| summaries.iter().cloned())
                    .collect::<BTreeSet<_>>();
                if summaries.len() > 1 {
                    return Err(ShoreError::Message(format!(
                        "conflicting proposal summaries for exact Revision {}",
                        revision.revision_id.as_str()
                    )));
                }
                let revision_proposal_summary = (summaries.len() == 1)
                    .then(|| summaries.iter().next().cloned().flatten())
                    .flatten();
                Ok(CurrentRevisionPresentationV1 {
                    summary_source: if revision_proposal_summary.is_some() {
                        RevisionSummarySourceV1::RevisionProposalSummary
                    } else {
                        RevisionSummarySourceV1::Absent
                    },
                    revision,
                    revision_proposal_summary,
                })
            })
            .collect::<Result<Vec<_>>>()?;

        presentations.insert(
            change_id.clone(),
            ChangePresentationV1 { current_revisions },
        );
    }
    Ok(ChangePresentationProjectionV1 {
        presentations,
        fact_port_carriers: normalize_fact_port_carriers(events)?,
        source_projection_stamp: provenance.projection_stamp.clone(),
        source_event_set_hash: event_set_hash.to_owned(),
    })
}

/// Normalize every human-readable fact family from one exact Revision read.
///
/// This is document policy rather than CLI policy: cold CLI and warm Inspector
/// adapters must expose the same family names, enum spellings, availability,
/// and typed content for the same `RevisionShowResult`.
#[doc(hidden)]
pub fn normalize_fact_presentations(
    result: &crate::session::RevisionShowResult,
    exact: &RevisionRefV1,
) -> (
    Vec<FactPresentationV1>,
    BTreeMap<String, FactContentPresentationV1>,
) {
    let mut facts = Vec::new();
    let mut content = BTreeMap::new();
    for view in &result.observations {
        let fact_id = view.id.as_str().to_owned();
        content.insert(
            fact_id.clone(),
            FactContentPresentationV1 {
                content_type: view.body_content_type,
                body_content_state: view.body_content_state,
                content: FactContentV1::Observation {
                    title: view.title.clone(),
                    body: view.body.clone(),
                },
            },
        );
        facts.push(normalized_fact(
            &fact_id,
            "observation",
            exact,
            Some(view.target.clone()),
            &view.writer.actor_id,
            Some(view.track_id.clone()),
            if view.status == crate::session::ObservationStatus::Active {
                FactFamilyStateV1::Current
            } else {
                FactFamilyStateV1::Stale
            },
            (
                view.body_content_state,
                result.body_content_availability(view.body_content_hash.as_deref()),
            ),
        ));
    }
    for view in &result.input_requests {
        let fact_id = view.id.as_str().to_owned();
        content.insert(
            fact_id.clone(),
            FactContentPresentationV1 {
                content_type: view.body_content_type,
                body_content_state: view.body_content_state,
                content: FactContentV1::InputRequest {
                    title: view.title.clone(),
                    body: view.body.clone(),
                    status: view.status.as_str().to_owned(),
                    responses: view
                        .responses
                        .iter()
                        .map(|response| FactInputResponseContentV1 {
                            response_id: response.id.as_str().to_owned(),
                            outcome: input_request_response_outcome_wire(response.outcome)
                                .to_owned(),
                            reason: response.reason.clone(),
                            content_type: response.reason_content_type,
                            body_content_state: response.reason_content_state,
                            availability: if response.reason_content_state.is_removed() {
                                ContentAvailabilityV1::Removed
                            } else {
                                result.body_content_availability(
                                    response.reason_content_hash.as_deref(),
                                )
                            },
                        })
                        .collect(),
                },
            },
        );
        facts.push(normalized_fact(
            &fact_id,
            "input_request",
            exact,
            Some(view.target.clone()),
            &view.writer.actor_id,
            Some(view.track_id.clone()),
            FactFamilyStateV1::Current,
            (
                view.body_content_state,
                result.body_content_availability(view.body_content_hash.as_deref()),
            ),
        ));
    }
    for view in &result.assessments {
        let fact_id = view.id.as_str().to_owned();
        content.insert(
            fact_id.clone(),
            FactContentPresentationV1 {
                content_type: view.summary_content_type,
                body_content_state: view.summary_content_state,
                content: FactContentV1::Assessment {
                    assessment: review_assessment_wire(view.assessment).to_owned(),
                    summary: view.summary.clone(),
                },
            },
        );
        facts.push(normalized_fact(
            &fact_id,
            "assessment",
            exact,
            Some(view.target.clone()),
            &view.writer.actor_id,
            Some(view.track_id.clone()),
            if view.status == crate::session::AssessmentRecordStatus::Current {
                FactFamilyStateV1::Current
            } else {
                FactFamilyStateV1::Stale
            },
            (
                view.summary_content_state,
                result.body_content_availability(view.summary_content_hash.as_deref()),
            ),
        ));
    }
    for view in &result.validation_checks {
        let fact_id = view.id.as_str().to_owned();
        content.insert(
            fact_id.clone(),
            FactContentPresentationV1 {
                content_type: view.summary_content_type,
                body_content_state: view.summary_content_state,
                content: FactContentV1::Validation {
                    check_name: view.check_name.clone(),
                    command: view.command.clone(),
                    status: validation_status_wire(view.status).to_owned(),
                    summary: view.summary.clone(),
                },
            },
        );
        facts.push(normalized_fact(
            &fact_id,
            "validation",
            exact,
            None,
            &view.writer.actor_id,
            Some(view.track_id.clone()),
            if view.superseded_by_revisions.is_empty() {
                FactFamilyStateV1::Current
            } else {
                FactFamilyStateV1::Stale
            },
            (
                view.summary_content_state,
                result.body_content_availability(view.summary_content_hash.as_deref()),
            ),
        ));
    }
    facts.sort_by(|left, right| left.fact_id.cmp(&right.fact_id));
    (facts, content)
}

/// Bind every explicit fact-port carrier to the same validated event generation
/// as the Change presentation facade. Contextual applicability is deliberately
/// deferred until a caller names one Change and one exact target Revision.
fn normalize_fact_port_carriers(events: &[ShoreEvent]) -> Result<Vec<FactPortCarrierV1>> {
    let mut by_port_id = BTreeMap::<String, FactPortCarrierV1>::new();
    for event in events
        .iter()
        .filter(|event| event.event_type == EventType::ReviewFactPorted)
    {
        let payload: ReviewFactPortedPayload = serde_json::from_value(event.payload.clone())?;
        let track_id = event
            .target
            .track_id
            .as_ref()
            .ok_or_else(|| ShoreError::InvalidEvent {
                message: "review_fact_ported requires an attributed review track".to_owned(),
            })?;
        payload.validate_attribution(&event.writer.actor_id, track_id)?;
        let carrier = FactPortCarrierV1 {
            port_id: payload.port_id,
            origin_revision: payload.origin_revision,
            origin_fact: payload.origin_fact,
            target_revision: payload.target_revision,
            relation: payload.relation,
            target_fact: payload.target_fact,
            rationale_content_hash: payload.rationale_content_hash,
            context_change_id: payload.context_change_id,
            actor_id: event.writer.actor_id.clone(),
            track_id: track_id.clone(),
            source_event_ids: vec![event.event_id.clone()],
        };
        let key = carrier.port_id.as_str().to_owned();
        match by_port_id.entry(key) {
            std::collections::btree_map::Entry::Vacant(entry) => {
                entry.insert(carrier);
            }
            std::collections::btree_map::Entry::Occupied(mut entry) => {
                if !same_fact_port_carrier(entry.get(), &carrier) {
                    return Err(ShoreError::Message(
                        "one fact-port identity has conflicting carrier material".to_owned(),
                    ));
                }
                entry
                    .get_mut()
                    .source_event_ids
                    .extend(carrier.source_event_ids);
            }
        }
    }
    for carrier in by_port_id.values_mut() {
        carrier.source_event_ids.sort();
        carrier.source_event_ids.dedup();
    }
    Ok(by_port_id.into_values().collect())
}

fn same_fact_port_carrier(left: &FactPortCarrierV1, right: &FactPortCarrierV1) -> bool {
    left.port_id == right.port_id
        && left.origin_revision == right.origin_revision
        && left.origin_fact == right.origin_fact
        && left.target_revision == right.target_revision
        && left.relation == right.relation
        && left.target_fact == right.target_fact
        && left.rationale_content_hash == right.rationale_content_hash
        && left.context_change_id == right.context_change_id
        && left.actor_id == right.actor_id
        && left.track_id == right.track_id
}

fn fact_port_continuity_key(port: &FactPortPresentationV1) -> Result<String> {
    Ok(serde_json::to_string(&serde_json::json!({
        "originRevision": port.origin_revision,
        "originFact": port.origin_fact,
        "targetRevision": port.target_revision,
    }))?)
}

fn fact_port_semantic_edge(port: &FactPortPresentationV1) -> Result<String> {
    Ok(serde_json::to_string(&serde_json::json!({
        "relation": port.relation,
        "targetFact": port.target_fact,
    }))?)
}

fn bind_fact_port_hydration(
    mut ports: Vec<FactPortPresentationV1>,
    fact_presentations: &[FactPresentationV1],
    target_revision: &RevisionRefV1,
) -> Vec<FactPortPresentationV1> {
    for port in ports
        .iter_mut()
        .filter(|port| port.applicability == FactPortApplicabilityV1::Applicable)
    {
        let origin_present = fact_presentations.iter().any(|fact| {
            fact.origin_revision == port.origin_revision
                && fact.fact_id == fact_ref_id(&port.origin_fact)
                && fact.family == fact_ref_family(&port.origin_fact)
                && fact.presented_in_revision.as_ref() == Some(target_revision)
        });
        let target_present = port.target_fact.as_ref().is_none_or(|target_fact| {
            fact_presentations.iter().any(|fact| {
                fact.origin_revision == *target_revision
                    && fact.fact_id == fact_ref_id(target_fact)
                    && fact.family == fact_ref_family(target_fact)
            })
        });
        if !origin_present {
            port.applicability = FactPortApplicabilityV1::Unavailable;
            port.diagnostics
                .push("fact_port_origin_fact_unavailable".to_owned());
        }
        if !target_present {
            port.applicability = FactPortApplicabilityV1::Unavailable;
            port.diagnostics
                .push("fact_port_target_fact_unavailable".to_owned());
        }
    }
    ports
}

pub(crate) fn fact_ref_id(fact: &FactRefV1) -> &str {
    match fact {
        FactRefV1::Observation { observation_id } => observation_id.as_str(),
        FactRefV1::InputRequest { input_request_id } => input_request_id.as_str(),
    }
}

pub(crate) fn fact_ref_family(fact: &FactRefV1) -> &'static str {
    match fact {
        FactRefV1::Observation { .. } => "observation",
        FactRefV1::InputRequest { .. } => "input_request",
    }
}

fn input_request_response_outcome_wire(
    outcome: crate::session::event::InputRequestResponseOutcome,
) -> &'static str {
    match outcome {
        crate::session::event::InputRequestResponseOutcome::Approved => "approved",
        crate::session::event::InputRequestResponseOutcome::Rejected => "rejected",
        crate::session::event::InputRequestResponseOutcome::Dismissed => "dismissed",
        crate::session::event::InputRequestResponseOutcome::Superseded => "superseded",
        crate::session::event::InputRequestResponseOutcome::Abandoned => "abandoned",
    }
}

fn review_assessment_wire(assessment: crate::session::event::ReviewAssessment) -> &'static str {
    match assessment {
        crate::session::event::ReviewAssessment::Accepted => "accepted",
        crate::session::event::ReviewAssessment::AcceptedWithFollowUp => "accepted_with_follow_up",
        crate::session::event::ReviewAssessment::NeedsChanges => "needs_changes",
        crate::session::event::ReviewAssessment::NeedsClarification => "needs_clarification",
    }
}

fn validation_status_wire(status: crate::model::ValidationStatus) -> &'static str {
    match status {
        crate::model::ValidationStatus::Passed => "passed",
        crate::model::ValidationStatus::Failed => "failed",
        crate::model::ValidationStatus::Errored => "errored",
        crate::model::ValidationStatus::Skipped => "skipped",
    }
}

fn normalized_fact(
    fact_id: &str,
    family: &str,
    exact: &RevisionRefV1,
    target: Option<ReviewTargetRef>,
    actor_id: &ActorId,
    track_id: Option<TrackId>,
    family_state: FactFamilyStateV1,
    content: (BodyContentState, ContentAvailabilityV1),
) -> FactPresentationV1 {
    let (body_content_state, content_availability) = content;
    FactPresentationV1 {
        fact_id: fact_id.to_owned(),
        family: family.to_owned(),
        origin_revision: exact.clone(),
        target,
        context_change_id: None,
        presented_in_revision: None,
        port_relation: None,
        actor_id: actor_id.clone(),
        track_id,
        family_state,
        revision_currency: ChangeRevisionCurrencyV1::Current,
        availability: if body_content_state.is_removed() {
            ContentAvailabilityV1::Removed
        } else {
            content_availability
        },
    }
}

fn fact_content_body(content: &FactContentV1) -> Option<&str> {
    match content {
        FactContentV1::Observation { body, .. } | FactContentV1::InputRequest { body, .. } => {
            body.as_deref()
        }
        FactContentV1::Assessment { summary, .. } | FactContentV1::Validation { summary, .. } => {
            summary.as_deref()
        }
    }
}

fn exact_ref_from_projection(
    projection: &ChangeDocumentProjectionV1,
    revision_id: &RevisionId,
) -> Option<RevisionRefV1> {
    let references = projection.revision_refs.get(revision_id)?;
    (references.len() == 1).then(|| references[0].clone())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::documents::{
        AssociationComparisonRefV1, AssociationComparisonStateV1, AssociationProofAvailabilityV1,
        RevisionResourceProjectionV1, RevisionResourceRefV1,
    };
    use crate::model::{
        ChangeMembershipClaimId, CommitAssociationId, EngagementId, EventId, JournalId, ObjectId,
        ObservationId,
    };
    use crate::session::event::{
        EventPayload, EventTarget, ReviewFactPortDraftV1, Revision, Writer,
        build_review_fact_ported,
    };
    use crate::session::{ChangeClaimSupportV1, ChangeMembershipClaimViewV1, ChangeView};

    fn reference(name: &str, byte: char) -> RevisionRefV1 {
        RevisionRefV1::new(
            RevisionId::new(format!("rev:sha256:{name}")),
            format!("sha256:{}", byte.to_string().repeat(64)),
        )
        .unwrap()
    }

    fn event<P: EventPayload>(payload: P, key: &str, target: EventTarget) -> ShoreEvent {
        ShoreEvent::new(
            payload.event_type(),
            key,
            target,
            Writer::shore_local("presentation-test"),
            payload,
            "2026-08-06T00:00:00Z",
        )
        .unwrap()
    }

    fn proposal_event(revision: &RevisionRefV1, summary: Option<&str>, key: &str) -> ShoreEvent {
        event(
            WorkObjectProposedPayload {
                engagement_id: EngagementId::new("engagement:sha256:presentation"),
                work_object: WorkObjectProposal::Revision {
                    revision: Revision {
                        id: revision.revision_id.clone(),
                        object_id: ObjectId::new("obj:sha256:presentation"),
                        git_provenance: None,
                    },
                    summary: summary.map(str::to_owned),
                    object_artifact_content_hash: revision.object_artifact_content_hash.clone(),
                    supersedes: Vec::new(),
                },
            },
            key,
            EventTarget::for_revision(
                JournalId::new("journal:presentation"),
                revision.revision_id.clone(),
                None,
            )
            .unwrap(),
        )
    }

    fn facade() -> (ChangeId, RevisionRefV1, ChangeDocumentFacadeV1) {
        let change_id = ChangeId::new("change:sha256:one");
        let revision = reference("one", 'a');
        let view = ChangeView {
            change_id: change_id.clone(),
            members: [revision.revision_id.clone()].into(),
            current_revisions: [revision.revision_id.clone()].into(),
            supersedes: BTreeSet::new(),
            topology: ChangeTopologyV1::Initial,
            lifecycle: ChangeLifecycleV1::InProgress,
            qualified_current_revisions: BTreeSet::new(),
            operative_obligations: BTreeSet::new(),
            diagnostics: Vec::new(),
        };
        let support = ChangeClaimSupportV1 {
            event_id: EventId::new("event:sha256:support"),
            actor_id: ActorId::new("actor:author"),
            track_id: None,
        };
        let claim = ChangeMembershipClaimViewV1 {
            claim_id: ChangeMembershipClaimId::new("membership:sha256:one"),
            change_id: change_id.clone(),
            revision_id: revision.revision_id.clone(),
            supports: vec![support],
            withdrawals: Vec::new(),
            active: true,
            diagnostics: Vec::new(),
        };
        let semantic = ChangeProjection {
            changes: [(change_id.clone(), view)].into(),
            links: Vec::new(),
        };
        let mut provenance = ChangeDocumentProjectionV1 {
            revision_refs: [(revision.revision_id.clone(), vec![revision.clone()])].into(),
            unavailable_revision_refs: Default::default(),
            membership_claims: vec![claim],
            relation_claims: Vec::new(),
            diagnostics: Vec::new(),
            projection_stamp: String::new(),
        };
        provenance.projection_stamp =
            crate::session::change_document_projection_stamp(&semantic, &provenance).unwrap();
        let facade = ChangeDocumentFacadeV1::new(semantic, provenance).unwrap();
        (change_id, revision, facade)
    }

    fn resource(revision: &RevisionRefV1) -> RevisionResourceDocumentV1 {
        RevisionResourceDocumentV1::unavailable(
            RevisionResourceRefV1 {
                revision: revision.clone(),
                object_id: ObjectId::new(format!("obj:sha256:{}", revision.revision_id.as_str())),
            },
            RevisionResourceProjectionV1 {
                track_id: None,
                include_body: true,
            },
            ContentAvailabilityV1::Missing,
        )
        .unwrap()
    }

    fn fact(origin_revision: RevisionRefV1) -> FactPresentationV1 {
        FactPresentationV1 {
            fact_id: "observation:sha256:one".to_owned(),
            family: "observation".to_owned(),
            origin_revision,
            target: None,
            context_change_id: None,
            presented_in_revision: None,
            port_relation: None,
            actor_id: ActorId::new("actor:author"),
            track_id: None,
            family_state: FactFamilyStateV1::Current,
            revision_currency: ChangeRevisionCurrencyV1::Current,
            availability: ContentAvailabilityV1::Available,
        }
    }

    fn observation(id: &str) -> FactRefV1 {
        FactRefV1::Observation {
            observation_id: ObservationId::new(id),
        }
    }

    fn fact_port_event(
        origin: &RevisionRefV1,
        target: &RevisionRefV1,
        origin_fact: FactRefV1,
        relation: FactPortRelationV1,
        target_fact: Option<FactRefV1>,
        context_change_id: Option<ChangeId>,
        key: &str,
    ) -> ShoreEvent {
        let writer = Writer::shore_local("presentation-test");
        let track_id = TrackId::new("track:fact-port");
        let payload = build_review_fact_ported(
            ReviewFactPortDraftV1 {
                origin_revision: origin.clone(),
                origin_fact,
                target_revision: target.clone(),
                relation,
                target_fact,
                rationale_content_hash: None,
                context_change_id,
            },
            &writer.actor_id,
            &track_id,
        )
        .unwrap();
        ShoreEvent::new(
            EventType::ReviewFactPorted,
            key,
            EventTarget::for_revision(
                JournalId::new("journal:fact-port"),
                origin.revision_id.clone(),
                Some(track_id),
            )
            .unwrap(),
            writer,
            payload,
            "2026-08-06T00:00:00Z",
        )
        .unwrap()
    }

    fn facade_with_port_events(
        facade: &ChangeDocumentFacadeV1,
        events: &[ShoreEvent],
    ) -> ChangeDocumentFacadeV1 {
        facade
            .clone()
            .with_presentations(
                change_presentation_projection(
                    &facade.semantic,
                    &facade.provenance,
                    events,
                    "sha256:fact-port-event-set",
                )
                .unwrap(),
            )
            .unwrap()
    }

    fn with_additional_change(
        facade: &ChangeDocumentFacadeV1,
        change_id: ChangeId,
        members: Vec<RevisionRefV1>,
    ) -> ChangeDocumentFacadeV1 {
        let mut semantic = facade.semantic.clone();
        let member_ids = members
            .iter()
            .map(|member| member.revision_id.clone())
            .collect::<BTreeSet<_>>();
        semantic.changes.insert(
            change_id.clone(),
            ChangeView {
                change_id: change_id.clone(),
                members: member_ids.clone(),
                current_revisions: member_ids,
                supersedes: BTreeSet::new(),
                topology: ChangeTopologyV1::ParallelCurrent,
                lifecycle: ChangeLifecycleV1::InProgress,
                qualified_current_revisions: BTreeSet::new(),
                operative_obligations: BTreeSet::new(),
                diagnostics: Vec::new(),
            },
        );
        let mut provenance = facade.provenance.clone();
        for (index, member) in members.into_iter().enumerate() {
            provenance
                .revision_refs
                .entry(member.revision_id.clone())
                .or_insert_with(|| vec![member.clone()]);
            provenance
                .membership_claims
                .push(ChangeMembershipClaimViewV1 {
                    claim_id: ChangeMembershipClaimId::new(format!(
                        "membership:sha256:{}-{index}",
                        change_id.as_str()
                    )),
                    change_id: change_id.clone(),
                    revision_id: member.revision_id,
                    supports: vec![ChangeClaimSupportV1 {
                        event_id: EventId::new(format!("event:sha256:additional-{index}")),
                        actor_id: ActorId::new("actor:author"),
                        track_id: None,
                    }],
                    withdrawals: Vec::new(),
                    active: true,
                    diagnostics: Vec::new(),
                });
        }
        provenance.projection_stamp =
            crate::session::change_document_projection_stamp(&semantic, &provenance).unwrap();
        ChangeDocumentFacadeV1::new(semantic, provenance).unwrap()
    }

    fn with_second_member(
        facade: &ChangeDocumentFacadeV1,
        change_id: &ChangeId,
        revision: RevisionRefV1,
    ) -> ChangeDocumentFacadeV1 {
        let mut semantic = facade.semantic.clone();
        let view = semantic.changes.get_mut(change_id).unwrap();
        view.members.insert(revision.revision_id.clone());
        view.current_revisions.insert(revision.revision_id.clone());
        view.topology = ChangeTopologyV1::ParallelCurrent;
        let mut provenance = facade.provenance.clone();
        provenance
            .revision_refs
            .insert(revision.revision_id.clone(), vec![revision.clone()]);
        provenance
            .membership_claims
            .push(ChangeMembershipClaimViewV1 {
                claim_id: ChangeMembershipClaimId::new("membership:sha256:two"),
                change_id: change_id.clone(),
                revision_id: revision.revision_id,
                supports: vec![ChangeClaimSupportV1 {
                    event_id: EventId::new("event:sha256:support-two"),
                    actor_id: ActorId::new("actor:author"),
                    track_id: None,
                }],
                withdrawals: Vec::new(),
                active: true,
                diagnostics: Vec::new(),
            });
        provenance.projection_stamp =
            crate::session::change_document_projection_stamp(&semantic, &provenance).unwrap();
        ChangeDocumentFacadeV1::new(semantic, provenance).unwrap()
    }

    #[test]
    fn detail_exposes_claim_provenance_without_client_inference() {
        let (change_id, _, facade) = facade();
        let detail = facade.detail_document(&change_id).unwrap();
        assert_eq!(detail.detail.member_revisions.len(), 1);
        assert_eq!(detail.detail.membership_claims[0].supports.len(), 1);
    }

    #[test]
    fn detail_types_unavailable_legacy_members_and_per_revision_qualification() {
        let (change_id, revision, facade) = facade();
        let legacy_id = RevisionId::new("review-unit:sha256:legacy");
        let mut semantic = facade.semantic.clone();
        let view = semantic.changes.get_mut(&change_id).unwrap();
        view.members.insert(legacy_id.clone());
        view.current_revisions.insert(legacy_id.clone());
        view.topology = ChangeTopologyV1::Incomplete;
        view.lifecycle = ChangeLifecycleV1::Incomplete;
        view.qualified_current_revisions
            .insert(revision.revision_id.clone());
        view.diagnostics
            .push("change_revision_ref_unavailable".to_owned());
        let mut provenance = facade.provenance.clone();
        provenance.unavailable_revision_refs.insert(
            legacy_id.clone(),
            RevisionRefUnavailableReasonV1::InvalidRevisionId,
        );
        provenance
            .membership_claims
            .push(ChangeMembershipClaimViewV1 {
                claim_id: ChangeMembershipClaimId::new("membership:sha256:legacy"),
                change_id: change_id.clone(),
                revision_id: legacy_id.clone(),
                supports: Vec::new(),
                withdrawals: Vec::new(),
                active: true,
                diagnostics: Vec::new(),
            });
        provenance.projection_stamp =
            crate::session::change_document_projection_stamp(&semantic, &provenance).unwrap();

        let detail = ChangeDocumentFacadeV1::new(semantic, provenance)
            .unwrap()
            .detail_document(&change_id)
            .unwrap()
            .detail;

        assert_eq!(detail.unavailable_member_revisions.len(), 1);
        assert_eq!(
            detail.unavailable_member_revisions[0].reason,
            RevisionRefUnavailableReasonV1::InvalidRevisionId
        );
        assert_eq!(detail.per_current_revision_qualification.len(), 1);
        assert!(detail.per_current_revision_qualification[0].qualified);
    }

    #[test]
    fn facade_constructor_rejects_every_projection_substitution_axis() {
        let (_, _, facade) = facade();
        let mut bad_stamp = facade.provenance.clone();
        bad_stamp.projection_stamp = "sha256:substituted".to_owned();
        assert!(ChangeDocumentFacadeV1::new(facade.semantic.clone(), bad_stamp).is_err());

        let mut missing_member = facade.provenance.clone();
        missing_member.membership_claims.clear();
        missing_member.projection_stamp =
            crate::session::change_document_projection_stamp(&facade.semantic, &missing_member)
                .unwrap();
        assert!(ChangeDocumentFacadeV1::new(facade.semantic.clone(), missing_member).is_err());

        let successor = reference("successor", 'b');
        let mut semantic = facade.semantic.clone();
        let view = semantic.changes.values_mut().next().unwrap();
        let predecessor = view.members.iter().next().unwrap().clone();
        view.members.insert(successor.revision_id.clone());
        view.current_revisions = [successor.revision_id.clone()].into();
        view.supersedes
            .insert((successor.revision_id.clone(), predecessor));
        view.topology = ChangeTopologyV1::Replacement;
        let mut unsupported_edge = facade.provenance.clone();
        unsupported_edge
            .revision_refs
            .insert(successor.revision_id.clone(), vec![successor.clone()]);
        unsupported_edge
            .membership_claims
            .push(ChangeMembershipClaimViewV1 {
                claim_id: ChangeMembershipClaimId::new("membership:sha256:successor"),
                change_id: view.change_id.clone(),
                revision_id: successor.revision_id,
                supports: Vec::new(),
                withdrawals: Vec::new(),
                active: true,
                diagnostics: Vec::new(),
            });
        unsupported_edge.projection_stamp =
            crate::session::change_document_projection_stamp(&semantic, &unsupported_edge).unwrap();
        assert!(ChangeDocumentFacadeV1::new(semantic, unsupported_edge).is_err());
    }

    #[test]
    fn contextual_revision_rejects_every_cross_resource_substitution_axis() {
        let (change_id, revision, facade) = facade();
        let other = reference("other", 'b');

        let wrong_hash = reference("one", 'c');
        assert!(
            facade
                .contextual_revision_document(
                    &change_id,
                    &wrong_hash,
                    resource(&wrong_hash),
                    Vec::new(),
                    Vec::new(),
                )
                .is_err()
        );
        assert!(
            facade
                .contextual_revision_document(
                    &change_id,
                    &other,
                    resource(&other),
                    Vec::new(),
                    Vec::new(),
                )
                .is_err()
        );
        assert!(
            facade
                .contextual_revision_document(
                    &change_id,
                    &revision,
                    resource(&other),
                    Vec::new(),
                    Vec::new(),
                )
                .is_err()
        );

        let wrong_association = AssociationComparisonDocumentV1::new(
            AssociationComparisonRefV1 {
                revision: other.clone(),
                association_id: CommitAssociationId::new("assoc-commit:sha256:other"),
                commit_oid: "1".repeat(40),
                comparison_base: "0".repeat(40),
                view_kind: "landing".to_owned(),
                proof_ref: None,
            },
            AssociationComparisonStateV1::Unknown,
            AssociationProofAvailabilityV1::NotRequested,
            Vec::new(),
        )
        .unwrap();
        assert!(
            facade
                .contextual_revision_document(
                    &change_id,
                    &revision,
                    resource(&revision),
                    Vec::new(),
                    vec![wrong_association],
                )
                .is_err()
        );
        assert!(
            facade
                .contextual_revision_document(
                    &change_id,
                    &revision,
                    resource(&revision),
                    vec![fact(other.clone())],
                    Vec::new(),
                )
                .is_err()
        );

        let mut local_with_port = fact(revision.clone());
        local_with_port.presented_in_revision = Some(revision.clone());
        local_with_port.port_relation = Some(FactPortRelationV1::ContextOnly);
        assert!(
            facade
                .contextual_revision_document(
                    &change_id,
                    &revision,
                    resource(&revision),
                    vec![local_with_port],
                    Vec::new(),
                )
                .is_err()
        );

        let two_member = with_second_member(&facade, &change_id, other.clone());
        assert!(
            two_member
                .contextual_revision_document(
                    &change_id,
                    &revision,
                    resource(&revision),
                    vec![fact(other)],
                    Vec::new(),
                )
                .is_err()
        );
    }

    #[test]
    fn facade_owns_fact_port_hydration_state() {
        let origin = reference("origin", 'b');
        let target = reference("target", 'c');
        let canonical = FactPortPresentationV1 {
            port_id: ReviewFactPortId::new("fact-port:sha256:authority"),
            origin_revision: origin.clone(),
            origin_fact: FactRefV1::Observation {
                observation_id: crate::model::ObservationId::new("observation:sha256:one"),
            },
            target_revision: target.clone(),
            relation: FactPortRelationV1::ContextOnly,
            target_fact: None,
            rationale_content_hash: None,
            context_change_id: Some(ChangeId::new("change:sha256:context")),
            actor_id: ActorId::new("actor:author"),
            track_id: Some(TrackId::new("track:reviewer")),
            source_event_ids: vec![EventId::new("event:sha256:port")],
            applicability: FactPortApplicabilityV1::Applicable,
            diagnostics: Vec::new(),
        };

        let mut expected_unavailable = canonical.clone();
        expected_unavailable.applicability = FactPortApplicabilityV1::Unavailable;
        expected_unavailable
            .diagnostics
            .push("fact_port_origin_fact_unavailable".to_owned());
        assert_eq!(
            bind_fact_port_hydration(vec![canonical.clone()], &[], &target),
            vec![expected_unavailable],
            "the facade derives unavailability when the exact origin fact was not hydrated"
        );

        let mut cross_revision_fact = fact(origin);
        cross_revision_fact.presented_in_revision = Some(target.clone());
        assert_eq!(
            bind_fact_port_hydration(vec![canonical.clone()], &[cross_revision_fact], &target),
            vec![canonical],
            "exact endpoint facts preserve the canonical carrier without caller-authored state"
        );
    }

    #[test]
    fn explicit_fact_port_context_never_leaks_to_another_change() {
        let (first_change, target, facade) = facade();
        let origin = reference("origin", 'b');
        let facade = with_second_member(&facade, &first_change, origin.clone());
        let second_change = ChangeId::new("change:sha256:second");
        let facade = with_additional_change(
            &facade,
            second_change.clone(),
            vec![origin.clone(), target.clone()],
        );
        let facade = facade_with_port_events(
            &facade,
            &[fact_port_event(
                &origin,
                &target,
                observation("observation:sha256:origin"),
                FactPortRelationV1::ContextOnly,
                None,
                Some(first_change.clone()),
                "fact-port:explicit-context",
            )],
        );

        assert_eq!(
            facade
                .fact_port_presentations(&first_change, &target)
                .unwrap()
                .len(),
            1
        );
        assert!(
            facade
                .fact_port_presentations(&second_change, &target)
                .unwrap()
                .is_empty(),
            "an explicit Change context may not be inferred into a sibling Change"
        );
    }

    #[test]
    fn unscoped_fact_ports_require_both_exact_endpoints_as_active_members() {
        let (change_id, target, facade) = facade();
        let origin = reference("origin", 'b');
        let facade = with_second_member(&facade, &change_id, origin.clone());
        let target_only_change = ChangeId::new("change:sha256:target-only");
        let facade =
            with_additional_change(&facade, target_only_change.clone(), vec![target.clone()]);
        let facade = facade_with_port_events(
            &facade,
            &[fact_port_event(
                &origin,
                &target,
                observation("observation:sha256:origin"),
                FactPortRelationV1::ContextOnly,
                None,
                None,
                "fact-port:unscoped",
            )],
        );

        assert_eq!(
            facade
                .fact_port_presentations(&change_id, &target)
                .unwrap()
                .len(),
            1
        );
        assert!(
            facade
                .fact_port_presentations(&target_only_change, &target)
                .unwrap()
                .is_empty(),
            "unscoped continuity is not an implicit Change membership claim"
        );
    }

    #[test]
    fn competing_fact_port_semantics_are_conflicted_without_a_selected_winner() {
        let (change_id, target, facade) = facade();
        let origin = reference("origin", 'b');
        let facade = with_second_member(&facade, &change_id, origin.clone());
        let origin_fact = observation("observation:sha256:origin");
        let target_fact = observation("observation:sha256:target");
        let facade = facade_with_port_events(
            &facade,
            &[
                fact_port_event(
                    &origin,
                    &target,
                    origin_fact.clone(),
                    FactPortRelationV1::ContextOnly,
                    None,
                    Some(change_id.clone()),
                    "fact-port:context-only",
                ),
                fact_port_event(
                    &origin,
                    &target,
                    origin_fact,
                    FactPortRelationV1::ReanchoredAs,
                    Some(target_fact),
                    Some(change_id.clone()),
                    "fact-port:reanchored",
                ),
            ],
        );

        let ports = facade.fact_port_presentations(&change_id, &target).unwrap();
        assert_eq!(ports.len(), 2);
        assert!(ports.iter().all(|port| {
            port.applicability == FactPortApplicabilityV1::Conflicted
                && port
                    .diagnostics
                    .iter()
                    .any(|diagnostic| diagnostic == "fact_port_continuity_conflicted")
        }));
    }

    #[test]
    fn duplicate_fact_port_carriers_coalesce_their_distinct_event_ids() {
        let (change_id, target, facade) = facade();
        let origin = reference("origin", 'b');
        let facade = with_second_member(&facade, &change_id, origin.clone());
        let first = fact_port_event(
            &origin,
            &target,
            observation("observation:sha256:origin"),
            FactPortRelationV1::ContextOnly,
            None,
            Some(change_id.clone()),
            "fact-port:duplicate-one",
        );
        let second = fact_port_event(
            &origin,
            &target,
            observation("observation:sha256:origin"),
            FactPortRelationV1::ContextOnly,
            None,
            Some(change_id.clone()),
            "fact-port:duplicate-two",
        );
        let facade = facade_with_port_events(&facade, &[first.clone(), second.clone()]);

        let ports = facade.fact_port_presentations(&change_id, &target).unwrap();
        assert_eq!(ports.len(), 1);
        assert_eq!(ports[0].source_event_ids.len(), 2);
        assert_eq!(
            ports[0].source_event_ids,
            vec![first.event_id.clone(), second.event_id.clone()]
        );
    }

    #[test]
    fn divergent_material_claiming_one_fact_port_identity_fails_closed() {
        let (change_id, target, facade) = facade();
        let origin = reference("origin", 'b');
        let facade = with_second_member(&facade, &change_id, origin.clone());
        let first = fact_port_event(
            &origin,
            &target,
            observation("observation:sha256:origin"),
            FactPortRelationV1::ContextOnly,
            None,
            Some(change_id),
            "fact-port:valid",
        );
        let mut divergent = first.clone();
        divergent.event_id = EventId::new("event:sha256:divergent-port");
        divergent.payload["relation"] = serde_json::json!("resolved_by");
        divergent.payload_hash = crate::canonical_hash::sha256_json_prefixed(&divergent.payload)
            .expect("mutated test payload hashes");

        let error = change_presentation_projection(
            &facade.semantic,
            &facade.provenance,
            &[first, divergent],
            "sha256:divergent-port-set",
        )
        .expect_err("divergent carrier material must not select one port identity");
        assert!(
            error
                .to_string()
                .contains("fact-port identity or attribution mismatch")
        );
    }

    #[test]
    fn fact_port_endpoint_hydration_requires_one_cross_origin_fact_per_exact_endpoint() {
        let (change_id, target, facade) = facade();
        let origin = reference("origin", 'b');
        let facade = with_second_member(&facade, &change_id, origin.clone());
        let facade = facade_with_port_events(
            &facade,
            &[fact_port_event(
                &origin,
                &target,
                observation("observation:sha256:one"),
                FactPortRelationV1::ContextOnly,
                None,
                Some(change_id.clone()),
                "fact-port:hydration",
            )],
        );
        let mut cross_origin = fact(origin.clone());
        cross_origin.presented_in_revision = Some(target.clone());
        let local = fact(target.clone());

        let document = facade
            .contextual_revision_document(
                &change_id,
                &target,
                resource(&target),
                vec![local.clone(), cross_origin.clone()],
                Vec::new(),
            )
            .unwrap();
        assert_eq!(document.detail.fact_ports.len(), 1);
        assert_eq!(
            document.detail.fact_ports[0].applicability,
            FactPortApplicabilityV1::Applicable
        );
        assert_eq!(
            document
                .detail
                .fact_presentations
                .iter()
                .filter(|fact| fact.origin_revision == origin)
                .count(),
            1,
            "one origin-owned fact is presented once at the exact target"
        );

        let duplicate_error = facade
            .contextual_revision_document(
                &change_id,
                &target,
                resource(&target),
                vec![local.clone(), cross_origin.clone(), cross_origin],
                Vec::new(),
            )
            .expect_err("duplicate cross-origin facts cannot fabricate endpoint hydration");
        assert!(
            duplicate_error
                .to_string()
                .contains("exact contextual fact presentation is duplicated")
        );

        let local_duplicate_error = facade
            .contextual_revision_document(
                &change_id,
                &target,
                resource(&target),
                vec![local.clone(), local],
                Vec::new(),
            )
            .expect_err("origin-local facts must also remain exact and unique");
        assert!(
            local_duplicate_error
                .to_string()
                .contains("exact contextual fact presentation is duplicated")
        );
    }

    #[test]
    fn change_summary_and_attention_documents_have_deterministic_golden_shapes() {
        let (_, revision, facade) = facade();
        let projection_stamp = facade.provenance.projection_stamp.clone();
        let list = serde_json::to_value(facade.list_document()).unwrap();
        assert_eq!(
            list,
            serde_json::json!({
                "schema": "pointbreak.review-change-list",
                "version": 1,
                "changes": [{
                    "changeId": "change:sha256:one",
                    "declarationState": "authoritative",
                    "titleAssertions": [],
                    "memberCount": 1,
                    "currentRevisionRefs": [{
                        "revisionId": revision.revision_id,
                        "objectArtifactContentHash": revision.object_artifact_content_hash,
                    }],
                    "topology": "initial",
                    "lifecycle": "in_progress",
                    "attentionSummary": "in_progress",
                    "availabilitySummary": "available",
                    "diagnostics": [],
                    "projectionStamp": projection_stamp.clone(),
                }],
                "diagnostics": [],
                "projectionStamp": projection_stamp,
            })
        );
        let attention = facade.attention_document(false);
        assert_eq!(attention.schema, ATTENTION_LIST_SCHEMA_V2);
        assert_eq!(attention.version, 2);
        assert_eq!(attention.changes.len(), 1);
        assert_eq!(
            facade.attention_document(true).schema,
            INSPECT_ATTENTION_SCHEMA_V2
        );
    }

    #[test]
    fn change_summary_availability_is_independent_of_exact_resource_bytes() {
        let (change_id, revision, facade) = facade();
        assert_eq!(
            facade.list_document().changes[0].availability_summary,
            "available",
            "the Change summary reports exact member-reference completeness"
        );

        for exact_availability in [
            ContentAvailabilityV1::Removed,
            ContentAvailabilityV1::Missing,
        ] {
            let exact = RevisionResourceDocumentV1::unavailable(
                RevisionResourceRefV1 {
                    revision: revision.clone(),
                    object_id: ObjectId::new("obj:sha256:unavailable"),
                },
                RevisionResourceProjectionV1 {
                    track_id: None,
                    include_body: true,
                },
                exact_availability,
            )
            .unwrap();
            let contextual = facade
                .contextual_revision_document(&change_id, &revision, exact, Vec::new(), Vec::new())
                .unwrap();

            assert_eq!(contextual.detail.availability, exact_availability);
            assert_eq!(
                facade.list_document().changes[0].availability_summary,
                "available",
                "removed or missing exact bytes must not rewrite Change membership completeness"
            );
        }
    }

    #[test]
    fn contextual_revision_preserves_exact_fact_origin_and_currency() {
        let (change_id, revision, facade) = facade();
        let fact = FactPresentationV1 {
            fact_id: "observation:sha256:one".to_owned(),
            family: "observation".to_owned(),
            origin_revision: revision.clone(),
            target: None,
            context_change_id: Some(change_id.clone()),
            presented_in_revision: None,
            port_relation: None,
            actor_id: ActorId::new("actor:author"),
            track_id: None,
            family_state: FactFamilyStateV1::Current,
            revision_currency: ChangeRevisionCurrencyV1::Current,
            availability: ContentAvailabilityV1::Available,
        };
        let document = facade
            .contextual_revision_document(
                &change_id,
                &revision,
                resource(&revision),
                vec![fact],
                Vec::new(),
            )
            .unwrap();
        assert_eq!(document.detail.revision, revision);
        assert_eq!(
            document.detail.fact_presentations[0].origin_revision,
            document.detail.revision
        );
        assert_eq!(
            document.detail.fact_presentations[0].revision_currency,
            ChangeRevisionCurrencyV1::Current
        );
    }

    #[test]
    fn additive_presentations_keep_change_titles_empty_and_every_current_revision_exact() {
        let (change_id, revision, facade) = facade();
        let other = reference("two", 'b');
        let facade = with_second_member(&facade, &change_id, other.clone());
        let source_projection_stamp = facade.provenance.projection_stamp.clone();
        let facade = facade
            .with_presentations(ChangePresentationProjectionV1 {
                fact_port_carriers: Vec::new(),
                presentations: [(
                    change_id.clone(),
                    ChangePresentationV1 {
                        current_revisions: vec![
                            CurrentRevisionPresentationV1 {
                                revision: revision.clone(),
                                revision_proposal_summary: Some("first proposal".to_owned()),
                                summary_source: RevisionSummarySourceV1::RevisionProposalSummary,
                            },
                            CurrentRevisionPresentationV1 {
                                revision: other.clone(),
                                revision_proposal_summary: None,
                                summary_source: RevisionSummarySourceV1::Absent,
                            },
                        ],
                    },
                )]
                .into(),
                source_projection_stamp,
                source_event_set_hash: "sha256:event-set".to_owned(),
            })
            .unwrap();

        let list = facade
            .list_document_for_inspector_with_presentations()
            .unwrap();
        let attention = facade.attention_document_with_presentations(true).unwrap();
        assert!(list.document.changes[0].title_assertions.is_empty());
        assert_eq!(list.presentations, attention.presentations);
        assert_eq!(
            list.presentations
                .get(&change_id)
                .unwrap()
                .current_revisions
                .iter()
                .map(|current| current.revision.clone())
                .collect::<Vec<_>>(),
            vec![revision, other]
        );
        assert_eq!(
            list.document.projection_stamp,
            attention.document.projection_stamp
        );
        assert_ne!(
            list.document.projection_stamp,
            facade.provenance.projection_stamp
        );
    }

    #[test]
    fn presentation_documents_are_flat_additions_on_one_shared_generation() {
        fn keys(value: &serde_json::Value) -> BTreeSet<&str> {
            value
                .as_object()
                .expect("document object")
                .keys()
                .map(String::as_str)
                .collect()
        }

        let (change_id, revision, facade) = facade();
        let proposal = proposal_event(&revision, Some("Readable exact state"), "proposal:wire");
        let presentations = change_presentation_projection(
            &facade.semantic,
            &facade.provenance,
            &[proposal],
            "sha256:shared-event-set",
        )
        .unwrap();
        let facade = facade.with_presentations(presentations).unwrap();

        let list = serde_json::to_value(
            facade
                .list_document_for_inspector_with_presentations()
                .unwrap(),
        )
        .unwrap();
        assert_eq!(
            keys(&list),
            BTreeSet::from([
                "changes",
                "diagnostics",
                "presentations",
                "projectionStamp",
                "schema",
                "version",
            ])
        );
        assert!(list.get("document").is_none());

        for (inspect, schema) in [
            (false, ATTENTION_LIST_SCHEMA_V2),
            (true, INSPECT_ATTENTION_SCHEMA_V2),
        ] {
            let attention = serde_json::to_value(
                facade
                    .attention_document_with_presentations(inspect)
                    .unwrap(),
            )
            .unwrap();
            assert_eq!(attention["schema"], schema);
            assert_eq!(
                keys(&attention),
                BTreeSet::from([
                    "changes",
                    "presentations",
                    "projectionStamp",
                    "schema",
                    "version",
                ])
            );
            assert!(attention.get("document").is_none());
            assert_eq!(attention["projectionStamp"], list["projectionStamp"]);
            assert_eq!(
                attention["changes"][0]["projectionStamp"],
                list["projectionStamp"]
            );
        }

        let detail = facade.detail_document(&change_id).unwrap();
        assert_eq!(detail.detail.projection_stamp, list["projectionStamp"]);
        assert_eq!(
            detail.detail.summary.projection_stamp,
            list["projectionStamp"]
        );

        let observation = fact(revision.clone());
        let contextual = serde_json::to_value(
            facade
                .contextual_revision_document_with_fact_content(
                    &change_id,
                    &revision,
                    resource(&revision),
                    vec![observation.clone()],
                    Vec::new(),
                    [(
                        observation.fact_id,
                        FactContentPresentationV1 {
                            content_type: BodyContentType::TextMarkdown,
                            body_content_state: BodyContentState::Present,
                            content: FactContentV1::Observation {
                                title: "Readable fact".to_owned(),
                                body: Some("Exact contextual body".to_owned()),
                            },
                        },
                    )]
                    .into(),
                )
                .unwrap(),
        )
        .unwrap();
        assert_eq!(
            keys(&contextual),
            BTreeSet::from([
                "associations",
                "availability",
                "changeId",
                "diagnostics",
                "exactRevisionDocument",
                "factContentPresentations",
                "factPorts",
                "factPresentations",
                "membershipSupport",
                "projectionStamp",
                "relationClassification",
                "revision",
                "revisionCurrency",
                "schema",
                "version",
            ])
        );
        assert!(contextual.get("document").is_none());
        assert_eq!(contextual["projectionStamp"], list["projectionStamp"]);
        assert_eq!(
            list["changes"][0]["projectionStamp"],
            list["projectionStamp"]
        );
    }

    #[test]
    fn inline_proposal_presentations_are_typed_and_permutation_duplicate_stable() {
        let (change_id, revision, facade) = facade();
        let summarized = proposal_event(&revision, Some("Readable exact state"), "proposal:one");
        let duplicate = proposal_event(
            &revision,
            Some("Readable exact state"),
            "proposal:duplicate",
        );
        let inputs = vec![summarized.clone(), duplicate];
        let projected = change_presentation_projection(
            &facade.semantic,
            &facade.provenance,
            &inputs,
            "sha256:event-set",
        )
        .unwrap();
        let mut permuted = inputs.clone();
        permuted.reverse();
        assert_eq!(
            projected,
            change_presentation_projection(
                &facade.semantic,
                &facade.provenance,
                &permuted,
                "sha256:event-set",
            )
            .unwrap()
        );
        let presentation = &projected.presentations[&change_id];
        assert_eq!(
            presentation.current_revisions[0]
                .revision_proposal_summary
                .as_deref(),
            Some("Readable exact state")
        );
        assert_eq!(
            presentation.current_revisions[0].summary_source,
            RevisionSummarySourceV1::RevisionProposalSummary
        );

        let absent = change_presentation_projection(
            &facade.semantic,
            &facade.provenance,
            &[proposal_event(&revision, None, "proposal:absent")],
            "sha256:event-set",
        )
        .unwrap();
        assert_eq!(
            absent.presentations[&change_id].current_revisions[0].summary_source,
            RevisionSummarySourceV1::Absent
        );
        assert!(
            absent.presentations[&change_id].current_revisions[0]
                .revision_proposal_summary
                .is_none()
        );
    }

    #[test]
    fn list_presentation_builder_has_no_body_or_resource_reader_input() {
        let (_, revision, facade) = facade();
        let inputs = [proposal_event(
            &revision,
            Some("inline only"),
            "proposal:inline",
        )];
        let projection = change_presentation_projection(
            &facade.semantic,
            &facade.provenance,
            &inputs,
            "sha256:event-set",
        )
        .unwrap();
        assert_eq!(projection.presentations.len(), 1);
    }

    #[test]
    fn distinct_proposal_summaries_for_one_exact_revision_fail_closed() {
        let (_, revision, facade) = facade();
        let error = change_presentation_projection(
            &facade.semantic,
            &facade.provenance,
            &[
                proposal_event(&revision, Some("first"), "proposal:first"),
                proposal_event(&revision, Some("second"), "proposal:second"),
            ],
            "sha256:event-set",
        )
        .expect_err("conflicting exact-revision proposal summaries must not be selected");

        assert!(
            error
                .to_string()
                .contains("conflicting proposal summaries for exact Revision")
        );
    }

    #[test]
    fn presentation_stamp_includes_event_generation_identity() {
        let (change_id, revision, facade) = facade();
        let presentation = ChangePresentationV1 {
            current_revisions: vec![CurrentRevisionPresentationV1 {
                revision,
                revision_proposal_summary: Some("stable".to_owned()),
                summary_source: RevisionSummarySourceV1::RevisionProposalSummary,
            }],
        };
        let bind = |event_set_hash: &str| {
            facade
                .clone()
                .with_presentations(ChangePresentationProjectionV1 {
                    fact_port_carriers: Vec::new(),
                    presentations: [(change_id.clone(), presentation.clone())].into(),
                    source_projection_stamp: facade.provenance.projection_stamp.clone(),
                    source_event_set_hash: event_set_hash.to_owned(),
                })
                .unwrap()
                .list_document_for_inspector()
                .projection_stamp
        };

        assert_ne!(bind("sha256:event-set-a"), bind("sha256:event-set-b"));
    }

    #[test]
    fn rich_fact_content_is_typed_without_weakening_exact_context() {
        let (change_id, revision, facade) = facade();
        let observation = fact(revision.clone());
        let content = FactContentPresentationV1 {
            content_type: crate::session::event::BodyContentType::TextMarkdown,
            body_content_state: crate::session::BodyContentState::Present,
            content: FactContentV1::Observation {
                title: "Actionable finding".to_owned(),
                body: Some("Use the exact Revision.".to_owned()),
            },
        };

        let document = facade
            .contextual_revision_document_with_fact_content(
                &change_id,
                &revision,
                resource(&revision),
                vec![observation],
                Vec::new(),
                [("observation:sha256:one".to_owned(), content)].into(),
            )
            .unwrap();
        let fact = &document.document.detail.fact_presentations[0];
        assert_eq!(fact.origin_revision, revision);
        assert_eq!(fact.context_change_id.as_ref(), Some(&change_id));
        assert_eq!(fact.availability, ContentAvailabilityV1::Available);
        assert!(matches!(
            document
                .fact_content_presentations
                .get(&fact.fact_id)
                .map(|presentation| &presentation.content),
            Some(FactContentV1::Observation { title, .. }) if title == "Actionable finding"
        ));
    }

    #[test]
    fn rich_fact_content_requires_exact_typed_and_available_fact_coverage() {
        let (change_id, revision, facade) = facade();
        let observation = fact(revision.clone());
        let observation_content = FactContentPresentationV1 {
            content_type: BodyContentType::TextPlain,
            body_content_state: BodyContentState::Present,
            content: FactContentV1::Observation {
                title: "finding".to_owned(),
                body: Some("body".to_owned()),
            },
        };

        assert!(
            facade
                .contextual_revision_document_with_fact_content(
                    &change_id,
                    &revision,
                    resource(&revision),
                    vec![observation.clone()],
                    Vec::new(),
                    BTreeMap::new(),
                )
                .is_err(),
            "a missing companion entry must fail closed"
        );
        assert!(
            facade
                .contextual_revision_document_with_fact_content(
                    &change_id,
                    &revision,
                    resource(&revision),
                    vec![observation.clone()],
                    Vec::new(),
                    [
                        (observation.fact_id.clone(), observation_content.clone()),
                        (
                            "observation:sha256:extra".to_owned(),
                            observation_content.clone(),
                        ),
                    ]
                    .into(),
                )
                .is_err(),
            "a surplus companion entry must fail closed"
        );
        assert!(
            facade
                .contextual_revision_document_with_fact_content(
                    &change_id,
                    &revision,
                    resource(&revision),
                    vec![observation.clone()],
                    Vec::new(),
                    [(
                        observation.fact_id.clone(),
                        FactContentPresentationV1 {
                            content_type: BodyContentType::TextPlain,
                            body_content_state: BodyContentState::Present,
                            content: FactContentV1::Assessment {
                                assessment: "accepted".to_owned(),
                                summary: None,
                            },
                        },
                    )]
                    .into(),
                )
                .is_err(),
            "a content variant from another fact family must fail closed"
        );
        assert!(
            facade
                .contextual_revision_document_with_fact_content(
                    &change_id,
                    &revision,
                    resource(&revision),
                    vec![observation.clone()],
                    Vec::new(),
                    [(
                        observation.fact_id,
                        FactContentPresentationV1 {
                            content_type: BodyContentType::TextPlain,
                            body_content_state: BodyContentState::SuppressedPresent,
                            content: FactContentV1::Observation {
                                title: "finding".to_owned(),
                                body: None,
                            },
                        },
                    )]
                    .into(),
                )
                .is_err(),
            "available metadata cannot pair with removed body state"
        );
    }

    #[test]
    fn rich_fact_content_accepts_typed_unavailability_and_checks_response_reasons() {
        let (change_id, revision, facade) = facade();
        let mut request = fact(revision.clone());
        request.family = "input_request".to_owned();
        request.availability = ContentAvailabilityV1::NonTextual;
        let request_content = FactContentPresentationV1 {
            content_type: BodyContentType::TextPlain,
            body_content_state: BodyContentState::Present,
            content: FactContentV1::InputRequest {
                title: "decision".to_owned(),
                body: None,
                status: "responded".to_owned(),
                responses: vec![FactInputResponseContentV1 {
                    response_id: "input-response:sha256:one".to_owned(),
                    outcome: "approved".to_owned(),
                    reason: None,
                    content_type: BodyContentType::TextPlain,
                    body_content_state: BodyContentState::Present,
                    availability: ContentAvailabilityV1::Missing,
                }],
            },
        };

        assert!(
            facade
                .contextual_revision_document_with_fact_content(
                    &change_id,
                    &revision,
                    resource(&revision),
                    vec![request.clone()],
                    Vec::new(),
                    [(request.fact_id.clone(), request_content.clone())].into(),
                )
                .is_ok(),
            "typed unavailability with no text is valid"
        );

        let mut inconsistent = request_content;
        let FactContentV1::InputRequest { responses, .. } = &mut inconsistent.content else {
            unreachable!()
        };
        responses[0].reason = Some("must not survive a missing state".to_owned());
        assert!(
            facade
                .contextual_revision_document_with_fact_content(
                    &change_id,
                    &revision,
                    resource(&revision),
                    vec![request.clone()],
                    Vec::new(),
                    [(request.fact_id, inconsistent)].into(),
                )
                .is_err(),
            "an unavailable response reason cannot carry text"
        );
    }
}
