//! Change-oriented headless documents built from authoritative domain projections.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use super::{
    AssociationComparisonDocumentV1, ContentAvailabilityV1, RevisionResourceAvailabilityV1,
    RevisionResourceDocumentV1,
};
use crate::error::{Result, ShoreError};
use crate::model::{ActorId, ChangeId, InputRequestId, RevisionId, RevisionRefV1, TrackId};
use crate::session::event::FactPortRelationV1;
use crate::session::{
    ChangeClaimSupportV1, ChangeDocumentProjectionV1, ChangeLifecycleV1, ChangeLinkView,
    ChangeMembershipClaimViewV1, ChangeProjection, ChangeRelationClaimViewV1, ChangeTopologyV1,
    RevisionRefUnavailableReasonV1,
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
#[serde(rename_all = "camelCase")]
pub struct FactPresentationV1 {
    pub fact_id: String,
    pub family: String,
    pub origin_revision: RevisionRefV1,
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

/// One facade over the semantic and provenance projections. It has no store or
/// SQLite handle, so loose and derived callers cannot diverge in presentation.
#[derive(Clone, Debug)]
pub struct ChangeDocumentFacadeV1 {
    semantic: ChangeProjection,
    provenance: ChangeDocumentProjectionV1,
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
            provenance,
        })
    }

    pub fn list_document(&self) -> ChangeListDocumentV1 {
        ChangeListDocumentV1 {
            schema: REVIEW_CHANGE_LIST_SCHEMA.to_owned(),
            version: 1,
            changes: self
                .semantic
                .changes
                .values()
                .map(|view| self.summary(view))
                .collect(),
            diagnostics: self.provenance.diagnostics.clone(),
            projection_stamp: self.provenance.projection_stamp.clone(),
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
            projection_stamp: self.provenance.projection_stamp.clone(),
        }
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
            projection_stamp: self.provenance.projection_stamp.clone(),
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
        mut fact_presentations: Vec<FactPresentationV1>,
        associations: Vec<AssociationComparisonDocumentV1>,
    ) -> Result<ChangeRevisionDocumentV1> {
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
        if associations
            .iter()
            .any(|association| association.comparison.revision != *revision)
        {
            return Err(ShoreError::Message(
                "association comparison does not target the contextual exact Revision".to_owned(),
            ));
        }
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
                || fact.port_relation.is_none()
            {
                return Err(ShoreError::Message(
                    "a cross-Revision fact presentation requires the exact target and typed port"
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
                associations,
                diagnostics: view.diagnostics.clone(),
                projection_stamp: self.provenance.projection_stamp.clone(),
            },
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
            projection_stamp: self.provenance.projection_stamp.clone(),
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::documents::{
        AssociationComparisonRefV1, AssociationComparisonStateV1, AssociationProofAvailabilityV1,
        RevisionResourceProjectionV1, RevisionResourceRefV1,
    };
    use crate::model::{ChangeMembershipClaimId, CommitAssociationId, EventId, ObjectId};
    use crate::session::{ChangeClaimSupportV1, ChangeMembershipClaimViewV1, ChangeView};

    fn reference(name: &str, byte: char) -> RevisionRefV1 {
        RevisionRefV1::new(
            RevisionId::new(format!("rev:sha256:{name}")),
            format!("sha256:{}", byte.to_string().repeat(64)),
        )
        .unwrap()
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
        RevisionResourceDocumentV1::available(
            RevisionResourceRefV1 {
                revision: revision.clone(),
                object_id: ObjectId::new(format!("obj:sha256:{}", revision.revision_id.as_str())),
            },
            RevisionResourceProjectionV1 {
                track_id: None,
                include_body: true,
            },
            &revision.object_artifact_content_hash,
            serde_json::json!({"exact": true}),
        )
        .unwrap()
    }

    fn fact(origin_revision: RevisionRefV1) -> FactPresentationV1 {
        FactPresentationV1 {
            fact_id: "observation:sha256:one".to_owned(),
            family: "observation".to_owned(),
            origin_revision,
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
    fn contextual_revision_preserves_exact_fact_origin_and_currency() {
        let (change_id, revision, facade) = facade();
        let fact = FactPresentationV1 {
            fact_id: "observation:sha256:one".to_owned(),
            family: "observation".to_owned(),
            origin_revision: revision.clone(),
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
                crate::documents::RevisionResourceDocumentV1::available(
                    crate::documents::RevisionResourceRefV1 {
                        revision: revision.clone(),
                        object_id: crate::model::ObjectId::new("obj:sha256:one"),
                    },
                    crate::documents::RevisionResourceProjectionV1 {
                        track_id: None,
                        include_body: true,
                    },
                    &revision.object_artifact_content_hash,
                    serde_json::json!({"exact": true}),
                )
                .unwrap(),
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
}
