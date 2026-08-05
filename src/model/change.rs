use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use crate::canonical_hash::sha256_json_prefixed;
use crate::error::Result;
use crate::model::{ChangeId, ChangeMembershipClaimId, RevisionId, id_prefix};

const CHANGE_IDENTITY_SCHEMA_V1: &str = "pointbreak.change-identity.v1";

/// Canonical identity material for one stable Review-domain Change.
///
/// Opaque nonce is the default isolation mode. Root Revision is an explicit
/// rendezvous mode and must never be inferred from content, Git, or topology.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ChangeIdentityDescriptorV1 {
    OpaqueNonce {
        schema: String,
        nonce: String,
    },
    RootRevision {
        schema: String,
        revision_id: RevisionId,
    },
}

impl ChangeIdentityDescriptorV1 {
    pub fn opaque_nonce(nonce: [u8; 32]) -> Self {
        Self::OpaqueNonce {
            schema: CHANGE_IDENTITY_SCHEMA_V1.to_owned(),
            nonce: lowercase_hex(&nonce),
        }
    }

    pub fn root_revision(revision_id: RevisionId) -> Self {
        Self::RootRevision {
            schema: CHANGE_IDENTITY_SCHEMA_V1.to_owned(),
            revision_id,
        }
    }

    pub fn validate(&self) -> Result<()> {
        match self {
            Self::OpaqueNonce { schema, nonce } => {
                validate_schema(schema)?;
                if nonce.len() != 64
                    || !nonce
                        .bytes()
                        .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
                {
                    return Err(crate::error::ShoreError::Message(
                        "opaque Change nonce must be 32 lowercase-hex bytes".to_owned(),
                    ));
                }
            }
            Self::RootRevision {
                schema,
                revision_id,
            } => {
                validate_schema(schema)?;
                if !revision_id.as_str().starts_with("rev:") {
                    return Err(crate::error::ShoreError::Message(
                        "root Change rendezvous requires a rev: RevisionId".to_owned(),
                    ));
                }
            }
        }
        Ok(())
    }
}

pub fn derive_change_id(descriptor: &ChangeIdentityDescriptorV1) -> Result<ChangeId> {
    descriptor.validate()?;
    Ok(ChangeId::new(format!(
        "{}:{}",
        id_prefix::CHANGE,
        sha256_json_prefixed(&serde_json::to_value(descriptor)?)?
    )))
}

pub fn derive_membership_claim_id(
    change_id: &ChangeId,
    revision_id: &RevisionId,
    claim_nonce: [u8; 32],
) -> Result<ChangeMembershipClaimId> {
    let hash = sha256_json_prefixed(&serde_json::json!({
        "family": "change_membership_asserted_v1",
        "changeId": change_id,
        "revisionId": revision_id,
        "claimNonce": lowercase_hex(&claim_nonce),
    }))?;
    Ok(ChangeMembershipClaimId::new(format!(
        "{}:{hash}",
        id_prefix::CHANGE_MEMBERSHIP
    )))
}

pub(crate) fn current_revisions(
    members: &BTreeSet<RevisionId>,
    supersedes: &BTreeSet<(RevisionId, RevisionId)>,
) -> BTreeSet<RevisionId> {
    let replaced: BTreeSet<_> = supersedes
        .iter()
        .map(|(_, predecessor)| predecessor)
        .collect();
    members
        .iter()
        .filter(|member| !replaced.contains(member))
        .cloned()
        .collect()
}

pub(crate) fn revision_graph_has_cycle(
    members: &BTreeSet<RevisionId>,
    supersedes: &BTreeSet<(RevisionId, RevisionId)>,
) -> bool {
    fn visit(
        node: &RevisionId,
        supersedes: &BTreeSet<(RevisionId, RevisionId)>,
        visiting: &mut BTreeSet<RevisionId>,
        visited: &mut BTreeSet<RevisionId>,
    ) -> bool {
        if visiting.contains(node) {
            return true;
        }
        if !visited.insert(node.clone()) {
            return false;
        }
        visiting.insert(node.clone());
        let cycle = supersedes
            .iter()
            .filter(|(successor, _)| successor == node)
            .any(|(_, predecessor)| visit(predecessor, supersedes, visiting, visited));
        visiting.remove(node);
        cycle
    }

    let mut visited = BTreeSet::new();
    members
        .iter()
        .any(|member| visit(member, supersedes, &mut BTreeSet::new(), &mut visited))
}

pub(crate) fn replacement_heads_diverge(
    current: &BTreeSet<RevisionId>,
    supersedes: &BTreeSet<(RevisionId, RevisionId)>,
) -> bool {
    let ancestors: Vec<BTreeSet<RevisionId>> = current
        .iter()
        .map(|head| {
            let mut found = BTreeSet::new();
            let mut pending = vec![head.clone()];
            while let Some(node) = pending.pop() {
                for (_, predecessor) in supersedes
                    .iter()
                    .filter(|(successor, _)| successor == &node)
                {
                    if found.insert(predecessor.clone()) {
                        pending.push(predecessor.clone());
                    }
                }
            }
            found
        })
        .collect();
    (0..ancestors.len()).any(|left| {
        (left + 1..ancestors.len()).any(|right| !ancestors[left].is_disjoint(&ancestors[right]))
    })
}

pub(crate) fn lowercase_hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        write!(&mut output, "{byte:02x}").expect("writing to String cannot fail");
    }
    output
}

fn validate_schema(schema: &str) -> Result<()> {
    if schema == CHANGE_IDENTITY_SCHEMA_V1 {
        Ok(())
    } else {
        Err(crate::error::ShoreError::Message(format!(
            "unsupported Change identity schema: {schema}"
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{RevisionId, RevisionRefV1};

    #[test]
    fn opaque_change_identity_is_distinct_while_root_rendezvous_converges() {
        let first = ChangeIdentityDescriptorV1::opaque_nonce([0x11; 32]);
        let second = ChangeIdentityDescriptorV1::opaque_nonce([0x22; 32]);
        assert_ne!(
            derive_change_id(&first).unwrap(),
            derive_change_id(&second).unwrap()
        );

        let root = RevisionId::new(
            "rev:sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        );
        let left = ChangeIdentityDescriptorV1::root_revision(root.clone());
        let right = ChangeIdentityDescriptorV1::root_revision(root);
        assert_eq!(
            derive_change_id(&left).unwrap(),
            derive_change_id(&right).unwrap()
        );
    }

    #[test]
    fn exact_revision_reference_requires_prefixed_sha256_artifact_identity() {
        let revision = RevisionId::new(
            "rev:sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        );
        assert!(RevisionRefV1::new(revision.clone(), "sha256:short").is_err());
        assert!(
            RevisionRefV1::new(
                revision,
                "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
            )
            .is_ok()
        );
    }

    #[test]
    fn claim_ids_are_nonce_distinct_and_retry_stable() {
        let change =
            derive_change_id(&ChangeIdentityDescriptorV1::opaque_nonce([0x33; 32])).unwrap();
        let revision = RevisionId::new(
            "rev:sha256:cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc",
        );
        let first = derive_membership_claim_id(&change, &revision, [0x44; 32]).unwrap();
        let retry = derive_membership_claim_id(&change, &revision, [0x44; 32]).unwrap();
        let independent = derive_membership_claim_id(&change, &revision, [0x55; 32]).unwrap();
        assert_eq!(first, retry);
        assert_ne!(first, independent);
    }
}
