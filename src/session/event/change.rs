#![allow(
    dead_code,
    reason = "some claim constructors are reserved for explicit low-level Change workflows"
)]

use serde::{Deserialize, Serialize};

#[cfg(test)]
use super::type_code;
use super::{EventPayload, EventType};
use crate::canonical_hash::sha256_json_prefixed;
use crate::error::Result;
use crate::model::{
    ChangeDeclarationClaimId, ChangeId, ChangeIdentityDescriptorV1, ChangeLinkClaimId,
    ChangeMembershipClaimId, ChangeMembershipWithdrawalId, ChangeRevisionRelationClaimId,
    ChangeRevisionRelationWithdrawalId, RevisionId, RevisionRefV1, id_prefix,
};

const CHANGE_DECLARED_SCHEMA_V1: &str = "pointbreak.change-declared";
const MEMBERSHIP_ASSERTED_SCHEMA_V1: &str = "pointbreak.change-membership-asserted";
const MEMBERSHIP_WITHDRAWN_SCHEMA_V1: &str = "pointbreak.change-membership-withdrawn";
const CHANGE_LINK_ASSERTED_SCHEMA_V1: &str = "pointbreak.change-link-asserted";
const REVISION_RELATION_ASSERTED_SCHEMA_V1: &str = "pointbreak.change-revision-relation-asserted";
const REVISION_RELATION_WITHDRAWN_SCHEMA_V1: &str = "pointbreak.change-revision-relation-withdrawn";

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ChangeDeclaredPayload {
    pub schema: String,
    pub version: u32,
    pub declaration_claim_id: ChangeDeclarationClaimId,
    pub change_id: ChangeId,
    pub identity_descriptor: ChangeIdentityDescriptorV1,
    pub claim_nonce: String,
}

impl EventPayload for ChangeDeclaredPayload {
    fn event_type(&self) -> EventType {
        EventType::ChangeDeclared
    }
}

impl ChangeDeclaredPayload {
    pub fn validate(&self) -> Result<()> {
        validate_payload_header(&self.schema, self.version, CHANGE_DECLARED_SCHEMA_V1)?;
        validate_claim_nonce(&self.claim_nonce)?;
        if crate::model::derive_change_id(&self.identity_descriptor)? != self.change_id {
            return invalid_change_payload("Change declaration identity mismatch");
        }
        let expected = ChangeDeclarationClaimId::new(prefixed_claim_id(
            id_prefix::CHANGE_DECLARATION,
            "change_declared_v1",
            &serde_json::json!({
                "changeId": self.change_id,
                "identityDescriptor": self.identity_descriptor,
            }),
            &self.claim_nonce,
        )?);
        if expected != self.declaration_claim_id {
            return invalid_change_payload("Change declaration claim id mismatch");
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ChangeMembershipAssertedPayload {
    pub schema: String,
    pub version: u32,
    pub membership_claim_id: ChangeMembershipClaimId,
    pub change_id: ChangeId,
    pub revision_id: RevisionId,
    pub claim_nonce: String,
}

impl EventPayload for ChangeMembershipAssertedPayload {
    fn event_type(&self) -> EventType {
        EventType::ChangeMembershipAsserted
    }
}

impl ChangeMembershipAssertedPayload {
    pub fn validate(&self) -> Result<()> {
        validate_payload_header(&self.schema, self.version, MEMBERSHIP_ASSERTED_SCHEMA_V1)?;
        validate_claim_nonce(&self.claim_nonce)?;
        let expected = ChangeMembershipClaimId::new(format!(
            "{}:{}",
            id_prefix::CHANGE_MEMBERSHIP,
            sha256_json_prefixed(&serde_json::json!({
                "family": "change_membership_asserted_v1",
                "changeId": self.change_id,
                "revisionId": self.revision_id,
                "claimNonce": self.claim_nonce,
            }))?
        ));
        if expected != self.membership_claim_id {
            return invalid_change_payload("Change membership claim id mismatch");
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ChangeMembershipWithdrawnPayload {
    pub schema: String,
    pub version: u32,
    pub membership_withdrawal_id: ChangeMembershipWithdrawalId,
    pub membership_claim_id: ChangeMembershipClaimId,
    pub claim_nonce: String,
}

impl EventPayload for ChangeMembershipWithdrawnPayload {
    fn event_type(&self) -> EventType {
        EventType::ChangeMembershipWithdrawn
    }
}

impl ChangeMembershipWithdrawnPayload {
    pub fn validate(&self) -> Result<()> {
        validate_payload_header(&self.schema, self.version, MEMBERSHIP_WITHDRAWN_SCHEMA_V1)?;
        validate_claim_nonce(&self.claim_nonce)?;
        let expected = ChangeMembershipWithdrawalId::new(prefixed_claim_id(
            id_prefix::CHANGE_MEMBERSHIP_WITHDRAWAL,
            "change_membership_withdrawn_v1",
            &serde_json::json!({"membershipClaimId": self.membership_claim_id}),
            &self.claim_nonce,
        )?);
        if expected != self.membership_withdrawal_id {
            return invalid_change_payload("Change membership withdrawal id mismatch");
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChangeLinkRelationV1 {
    SameWork,
    RelatedWork,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ChangeLinkAssertedPayload {
    pub schema: String,
    pub version: u32,
    pub link_claim_id: ChangeLinkClaimId,
    pub left_change_id: ChangeId,
    pub right_change_id: ChangeId,
    pub relation: ChangeLinkRelationV1,
    pub claim_nonce: String,
}

impl EventPayload for ChangeLinkAssertedPayload {
    fn event_type(&self) -> EventType {
        EventType::ChangeLinkAsserted
    }
}

impl ChangeLinkAssertedPayload {
    pub fn validate(&self) -> Result<()> {
        validate_payload_header(&self.schema, self.version, CHANGE_LINK_ASSERTED_SCHEMA_V1)?;
        validate_claim_nonce(&self.claim_nonce)?;
        if self.left_change_id >= self.right_change_id {
            return invalid_change_payload("Change links require a canonical distinct id pair");
        }
        let expected = ChangeLinkClaimId::new(prefixed_claim_id(
            id_prefix::CHANGE_LINK,
            "change_link_asserted_v1",
            &serde_json::json!({
                "leftChangeId": self.left_change_id,
                "rightChangeId": self.right_change_id,
                "relation": self.relation,
            }),
            &self.claim_nonce,
        )?);
        if expected != self.link_claim_id {
            return invalid_change_payload("Change link claim id mismatch");
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChangeRevisionRelationV1 {
    Supersedes,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ChangeRevisionRelationAssertedPayload {
    pub schema: String,
    pub version: u32,
    pub relation_claim_id: ChangeRevisionRelationClaimId,
    pub change_id: ChangeId,
    pub successor: RevisionRefV1,
    pub predecessor: RevisionRefV1,
    pub relation: ChangeRevisionRelationV1,
    pub claim_nonce: String,
}

impl EventPayload for ChangeRevisionRelationAssertedPayload {
    fn event_type(&self) -> EventType {
        EventType::ChangeRevisionRelationAsserted
    }
}

impl ChangeRevisionRelationAssertedPayload {
    pub fn validate(&self) -> Result<()> {
        validate_payload_header(
            &self.schema,
            self.version,
            REVISION_RELATION_ASSERTED_SCHEMA_V1,
        )?;
        validate_claim_nonce(&self.claim_nonce)?;
        let expected = ChangeRevisionRelationClaimId::new(prefixed_claim_id(
            id_prefix::CHANGE_REVISION_RELATION,
            "change_revision_relation_asserted_v1",
            &serde_json::json!({
                "changeId": self.change_id,
                "successor": self.successor,
                "predecessor": self.predecessor,
                "relation": self.relation,
            }),
            &self.claim_nonce,
        )?);
        if expected != self.relation_claim_id {
            return invalid_change_payload("Change Revision-relation claim id mismatch");
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ChangeRevisionRelationWithdrawnPayload {
    pub schema: String,
    pub version: u32,
    pub relation_withdrawal_id: ChangeRevisionRelationWithdrawalId,
    pub relation_claim_id: ChangeRevisionRelationClaimId,
    pub claim_nonce: String,
}

impl EventPayload for ChangeRevisionRelationWithdrawnPayload {
    fn event_type(&self) -> EventType {
        EventType::ChangeRevisionRelationWithdrawn
    }
}

impl ChangeRevisionRelationWithdrawnPayload {
    pub fn validate(&self) -> Result<()> {
        validate_payload_header(
            &self.schema,
            self.version,
            REVISION_RELATION_WITHDRAWN_SCHEMA_V1,
        )?;
        validate_claim_nonce(&self.claim_nonce)?;
        let expected = ChangeRevisionRelationWithdrawalId::new(prefixed_claim_id(
            id_prefix::CHANGE_REVISION_RELATION_WITHDRAWAL,
            "change_revision_relation_withdrawn_v1",
            &serde_json::json!({"relationClaimId": self.relation_claim_id}),
            &self.claim_nonce,
        )?);
        if expected != self.relation_withdrawal_id {
            return invalid_change_payload("Change Revision-relation withdrawal id mismatch");
        }
        Ok(())
    }
}

pub(crate) fn build_change_declared(
    descriptor: ChangeIdentityDescriptorV1,
    claim_nonce: [u8; 32],
) -> Result<ChangeDeclaredPayload> {
    let change_id = crate::model::derive_change_id(&descriptor)?;
    let claim_nonce = crate::model::lowercase_hex(&claim_nonce);
    let declaration_claim_id = ChangeDeclarationClaimId::new(prefixed_claim_id(
        id_prefix::CHANGE_DECLARATION,
        "change_declared_v1",
        &serde_json::json!({"changeId": change_id, "identityDescriptor": descriptor}),
        &claim_nonce,
    )?);
    Ok(ChangeDeclaredPayload {
        schema: CHANGE_DECLARED_SCHEMA_V1.to_owned(),
        version: 1,
        declaration_claim_id,
        change_id,
        identity_descriptor: descriptor,
        claim_nonce,
    })
}

pub(crate) fn build_membership_asserted(
    change_id: &ChangeId,
    revision_id: &RevisionId,
    claim_nonce: [u8; 32],
) -> Result<ChangeMembershipAssertedPayload> {
    let claim_nonce_text = crate::model::lowercase_hex(&claim_nonce);
    let membership_claim_id =
        crate::model::derive_membership_claim_id(change_id, revision_id, claim_nonce)?;
    Ok(ChangeMembershipAssertedPayload {
        schema: MEMBERSHIP_ASSERTED_SCHEMA_V1.to_owned(),
        version: 1,
        membership_claim_id,
        change_id: change_id.clone(),
        revision_id: revision_id.clone(),
        claim_nonce: claim_nonce_text,
    })
}

pub(crate) fn build_membership_withdrawn(
    membership_claim_id: &ChangeMembershipClaimId,
    claim_nonce: [u8; 32],
) -> Result<ChangeMembershipWithdrawnPayload> {
    let claim_nonce = crate::model::lowercase_hex(&claim_nonce);
    let membership_withdrawal_id = ChangeMembershipWithdrawalId::new(prefixed_claim_id(
        id_prefix::CHANGE_MEMBERSHIP_WITHDRAWAL,
        "change_membership_withdrawn_v1",
        &serde_json::json!({"membershipClaimId": membership_claim_id}),
        &claim_nonce,
    )?);
    Ok(ChangeMembershipWithdrawnPayload {
        schema: MEMBERSHIP_WITHDRAWN_SCHEMA_V1.to_owned(),
        version: 1,
        membership_withdrawal_id,
        membership_claim_id: membership_claim_id.clone(),
        claim_nonce,
    })
}

pub(crate) fn build_change_link_asserted(
    first: &ChangeId,
    second: &ChangeId,
    relation: ChangeLinkRelationV1,
    claim_nonce: [u8; 32],
) -> Result<ChangeLinkAssertedPayload> {
    let (left_change_id, right_change_id) = if first <= second {
        (first.clone(), second.clone())
    } else {
        (second.clone(), first.clone())
    };
    let claim_nonce = crate::model::lowercase_hex(&claim_nonce);
    let link_claim_id = ChangeLinkClaimId::new(prefixed_claim_id(
        id_prefix::CHANGE_LINK,
        "change_link_asserted_v1",
        &serde_json::json!({
            "leftChangeId": left_change_id,
            "rightChangeId": right_change_id,
            "relation": relation,
        }),
        &claim_nonce,
    )?);
    let payload = ChangeLinkAssertedPayload {
        schema: CHANGE_LINK_ASSERTED_SCHEMA_V1.to_owned(),
        version: 1,
        link_claim_id,
        left_change_id,
        right_change_id,
        relation,
        claim_nonce,
    };
    payload.validate()?;
    Ok(payload)
}

pub(crate) fn build_revision_relation_asserted(
    change_id: &ChangeId,
    successor: RevisionRefV1,
    predecessor: RevisionRefV1,
    claim_nonce: [u8; 32],
) -> Result<ChangeRevisionRelationAssertedPayload> {
    let claim_nonce = crate::model::lowercase_hex(&claim_nonce);
    let relation = ChangeRevisionRelationV1::Supersedes;
    let relation_claim_id = ChangeRevisionRelationClaimId::new(prefixed_claim_id(
        id_prefix::CHANGE_REVISION_RELATION,
        "change_revision_relation_asserted_v1",
        &serde_json::json!({
            "changeId": change_id,
            "successor": successor,
            "predecessor": predecessor,
            "relation": relation,
        }),
        &claim_nonce,
    )?);
    Ok(ChangeRevisionRelationAssertedPayload {
        schema: REVISION_RELATION_ASSERTED_SCHEMA_V1.to_owned(),
        version: 1,
        relation_claim_id,
        change_id: change_id.clone(),
        successor,
        predecessor,
        relation,
        claim_nonce,
    })
}

pub(crate) fn build_revision_relation_withdrawn(
    relation_claim_id: &ChangeRevisionRelationClaimId,
    claim_nonce: [u8; 32],
) -> Result<ChangeRevisionRelationWithdrawnPayload> {
    let claim_nonce = crate::model::lowercase_hex(&claim_nonce);
    let relation_withdrawal_id = ChangeRevisionRelationWithdrawalId::new(prefixed_claim_id(
        id_prefix::CHANGE_REVISION_RELATION_WITHDRAWAL,
        "change_revision_relation_withdrawn_v1",
        &serde_json::json!({"relationClaimId": relation_claim_id}),
        &claim_nonce,
    )?);
    Ok(ChangeRevisionRelationWithdrawnPayload {
        schema: REVISION_RELATION_WITHDRAWN_SCHEMA_V1.to_owned(),
        version: 1,
        relation_withdrawal_id,
        relation_claim_id: relation_claim_id.clone(),
        claim_nonce,
    })
}

fn prefixed_claim_id(
    prefix: &str,
    family: &str,
    coordinates: &serde_json::Value,
    claim_nonce: &str,
) -> Result<String> {
    let hash = sha256_json_prefixed(&serde_json::json!({
        "family": family,
        "coordinates": coordinates,
        "claimNonce": claim_nonce,
    }))?;
    Ok(format!("{prefix}:{hash}"))
}

fn validate_payload_header(schema: &str, version: u32, expected_schema: &str) -> Result<()> {
    if schema == expected_schema && version == 1 {
        Ok(())
    } else {
        invalid_change_payload("unsupported Change payload schema/version")
    }
}

fn validate_claim_nonce(nonce: &str) -> Result<()> {
    if nonce.len() == 64
        && nonce
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    {
        Ok(())
    } else {
        invalid_change_payload("Change claim nonce must be 32 lowercase-hex bytes")
    }
}

fn invalid_change_payload<T>(message: &str) -> Result<T> {
    Err(crate::error::ShoreError::InvalidEvent {
        message: message.to_owned(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{ChangeIdentityDescriptorV1, RevisionId, derive_change_id};

    #[test]
    fn change_payloads_use_the_frozen_t17_through_t22_codes() {
        assert_eq!(type_code(EventType::ChangeDeclared), "t:17");
        assert_eq!(type_code(EventType::ChangeMembershipAsserted), "t:18");
        assert_eq!(type_code(EventType::ChangeMembershipWithdrawn), "t:19");
        assert_eq!(type_code(EventType::ChangeLinkAsserted), "t:20");
        assert_eq!(type_code(EventType::ChangeRevisionRelationAsserted), "t:21");
        assert_eq!(
            type_code(EventType::ChangeRevisionRelationWithdrawn),
            "t:22"
        );
    }

    #[test]
    fn independently_attributed_membership_support_survives_exact_claim_withdrawal() {
        let change_id =
            derive_change_id(&ChangeIdentityDescriptorV1::opaque_nonce([0x10; 32])).unwrap();
        let revision_id = RevisionId::new(
            "rev:sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        );
        let first = build_membership_asserted(&change_id, &revision_id, [0x11; 32]).unwrap();
        let second = build_membership_asserted(&change_id, &revision_id, [0x12; 32]).unwrap();
        let withdrawal =
            build_membership_withdrawn(&first.membership_claim_id, [0x13; 32]).unwrap();
        assert_ne!(first.membership_claim_id, second.membership_claim_id);
        assert_eq!(withdrawal.membership_claim_id, first.membership_claim_id);
    }

    #[test]
    fn change_links_canonicalize_the_undirected_pair() {
        let left = derive_change_id(&ChangeIdentityDescriptorV1::opaque_nonce([0x20; 32])).unwrap();
        let right =
            derive_change_id(&ChangeIdentityDescriptorV1::opaque_nonce([0x21; 32])).unwrap();
        let forward =
            build_change_link_asserted(&left, &right, ChangeLinkRelationV1::SameWork, [0x22; 32])
                .unwrap();
        let reverse =
            build_change_link_asserted(&right, &left, ChangeLinkRelationV1::SameWork, [0x22; 32])
                .unwrap();
        assert_eq!(forward, reverse);
        assert!(forward.left_change_id < forward.right_change_id);
        assert!(
            build_change_link_asserted(&left, &left, ChangeLinkRelationV1::RelatedWork, [0x23; 32])
                .is_err()
        );
    }
}
