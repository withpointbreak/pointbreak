#![allow(
    dead_code,
    reason = "the bulk-adoption executor is deliberately not routed"
)]

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::canonical_hash::{sha256_bytes_hex, sha256_json_prefixed};
use crate::error::Result;
use crate::model::{
    ChangeId, ChangeIdentityDescriptorV1, JournalId, RevisionId, RevisionRefV1, current_revisions,
    replacement_heads_diverge, revision_graph_has_cycle,
};
use crate::session::AuthorityCursorV2;
use crate::session::event::{
    EventPayload, EventTarget, ShoreEvent, Writer, build_change_declared,
    build_membership_asserted, build_revision_relation_asserted,
};
use crate::session::store::{BulkAdoptionManifestV1, ReservedCohortRecordV1};

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct LegacyRevisionV1 {
    revision: RevisionRefV1,
    legacy_group: String,
    supersedes: Vec<RevisionId>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct LegacyRootInventoryV1 {
    root_identity_hash: String,
    journal_id: JournalId,
    source_authority_cursor: AuthorityCursorV2,
    revisions: Vec<LegacyRevisionV1>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "snake_case")]
enum OverlapIdentityDecisionV1 {
    Shared {
        identity_descriptor: ChangeIdentityDescriptorV1,
    },
    Distinct,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct RetainedChangeAllocationV1 {
    change_id: ChangeId,
    identity_descriptor: ChangeIdentityDescriptorV1,
    members: BTreeSet<RevisionId>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct RetainedAdoptionManifestV1 {
    manifest_hash: String,
    allocations: Vec<RetainedChangeAllocationV1>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct OwnerManifestDecisionsV1 {
    approved_anomaly_ids: BTreeSet<String>,
    overlap_identity: BTreeMap<RevisionId, OverlapIdentityDecisionV1>,
    writer: Writer,
    occurred_at: String,
}

impl Default for OwnerManifestDecisionsV1 {
    fn default() -> Self {
        Self {
            approved_anomaly_ids: BTreeSet::new(),
            overlap_identity: BTreeMap::new(),
            writer: Writer::shore_local("bulk-adoption-proposal-v1"),
            occurred_at: "1970-01-01T00:00:00Z".to_owned(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct MigrationAnomalyV1 {
    anomaly_id: String,
    code: String,
    root_identity_hashes: BTreeSet<String>,
    revision_ids: BTreeSet<RevisionId>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AdoptionGroupProposalV1 {
    change_id: ChangeId,
    members: BTreeSet<RevisionId>,
    admitted_relations: BTreeSet<(RevisionId, RevisionId)>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct RootAdoptionProposalV1 {
    root_identity_hash: String,
    covered_revisions: BTreeSet<RevisionId>,
    groups: Vec<AdoptionGroupProposalV1>,
    planned_events: Vec<ShoreEvent>,
    manifest: Option<BulkAdoptionManifestV1>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct BulkAdoptionProposalV1 {
    roots: Vec<RootAdoptionProposalV1>,
    anomalies: Vec<MigrationAnomalyV1>,
    requires_owner_decision: bool,
}

fn plan_bulk_adoption(
    roots: &[LegacyRootInventoryV1],
    retained_manifests: &[RetainedAdoptionManifestV1],
    decisions: &OwnerManifestDecisionsV1,
) -> Result<BulkAdoptionProposalV1> {
    let mut roots = roots.to_vec();
    roots.sort_by(|left, right| left.root_identity_hash.cmp(&right.root_identity_hash));
    for root in &mut roots {
        for revision in &mut root.revisions {
            revision.supersedes.sort();
            revision.supersedes.dedup();
        }
        root.revisions.sort_by(|left, right| {
            (
                &left.revision.revision_id,
                &left.revision.object_artifact_content_hash,
                &left.legacy_group,
                &left.supersedes,
            )
                .cmp(&(
                    &right.revision.revision_id,
                    &right.revision.object_artifact_content_hash,
                    &right.legacy_group,
                    &right.supersedes,
                ))
        });
    }
    let mut retained_manifests = retained_manifests.to_vec();
    retained_manifests.sort_by(|left, right| left.manifest_hash.cmp(&right.manifest_hash));
    for manifest in &mut retained_manifests {
        if !is_prefixed_sha256(&manifest.manifest_hash) {
            return Err(crate::error::ShoreError::Message(
                "retained adoption manifest requires a prefixed SHA-256 identity".to_owned(),
            ));
        }
        manifest.allocations.sort_by(|left, right| {
            (&left.change_id, &left.members).cmp(&(&right.change_id, &right.members))
        });
        for allocation in &manifest.allocations {
            if crate::model::derive_change_id(&allocation.identity_descriptor)?
                != allocation.change_id
            {
                return Err(crate::error::ShoreError::Message(
                    "retained adoption allocation has a mismatched Change identity".to_owned(),
                ));
            }
        }
    }

    let mut anomalies = Vec::new();
    let mut locations: BTreeMap<RevisionId, BTreeSet<String>> = BTreeMap::new();
    let mut root_revisions: BTreeMap<
        RevisionId,
        Vec<(String, RevisionRefV1, BTreeSet<RevisionId>)>,
    > = BTreeMap::new();
    let mut retained_allocations: BTreeMap<RevisionId, Vec<RetainedChangeAllocationV1>> =
        BTreeMap::new();
    for root in &roots {
        for revision in &root.revisions {
            locations
                .entry(revision.revision.revision_id.clone())
                .or_default()
                .insert(root.root_identity_hash.clone());
            root_revisions
                .entry(revision.revision.revision_id.clone())
                .or_default()
                .push((
                    root.root_identity_hash.clone(),
                    revision.revision.clone(),
                    revision.supersedes.iter().cloned().collect(),
                ));
        }
    }
    for manifest in &retained_manifests {
        let source = format!("retained-manifest:{}", manifest.manifest_hash);
        for allocation in &manifest.allocations {
            for revision_id in &allocation.members {
                locations
                    .entry(revision_id.clone())
                    .or_default()
                    .insert(source.clone());
                retained_allocations
                    .entry(revision_id.clone())
                    .or_default()
                    .push(allocation.clone());
            }
        }
    }
    for (revision_id, root_hashes) in locations.iter().filter(|(_, roots)| roots.len() > 1) {
        anomalies.push(anomaly(
            "cross_root_revision_overlap",
            root_hashes.clone(),
            [revision_id.clone()].into(),
        )?);
        let occurrences = root_revisions
            .get(revision_id)
            .map(Vec::as_slice)
            .unwrap_or_default();
        let artifact_hashes: BTreeSet<_> = occurrences
            .iter()
            .map(|(_, revision, _)| revision.object_artifact_content_hash.clone())
            .collect();
        if artifact_hashes.len() > 1 {
            anomalies.push(anomaly(
                "cross_root_revision_disagreement",
                root_hashes.clone(),
                [revision_id.clone()].into(),
            )?);
        }
        let relation_sets: BTreeSet<_> = occurrences
            .iter()
            .map(|(_, _, supersedes)| supersedes.clone())
            .collect();
        if relation_sets.len() > 1 {
            let mut implicated = [revision_id.clone()].into_iter().collect::<BTreeSet<_>>();
            implicated.extend(
                occurrences
                    .iter()
                    .flat_map(|(_, _, supersedes)| supersedes.iter().cloned()),
            );
            anomalies.push(anomaly(
                "cross_root_relation_disagreement",
                root_hashes.clone(),
                implicated,
            )?);
        }
        if let Some(OverlapIdentityDecisionV1::Shared {
            identity_descriptor,
        }) = decisions.overlap_identity.get(revision_id)
            && retained_allocations
                .get(revision_id)
                .is_some_and(|allocations| {
                    !allocations
                        .iter()
                        .any(|allocation| allocation.identity_descriptor == *identity_descriptor)
                })
        {
            anomalies.push(anomaly(
                "retained_identity_mismatch",
                root_hashes.clone(),
                [revision_id.clone()].into(),
            )?);
        }
    }

    let mut root_components = Vec::new();
    for root in &roots {
        let mut by_id = BTreeMap::new();
        for revision in &root.revisions {
            if let Some(existing) =
                by_id.insert(revision.revision.revision_id.clone(), revision.clone())
                && existing.revision != revision.revision
            {
                anomalies.push(anomaly(
                    "revision_identity_collision",
                    [root.root_identity_hash.clone()].into(),
                    [revision.revision.revision_id.clone()].into(),
                )?);
            }
        }
        let mut adjacency: BTreeMap<RevisionId, BTreeSet<RevisionId>> = by_id
            .keys()
            .cloned()
            .map(|revision| (revision, BTreeSet::new()))
            .collect();
        let mut admitted = BTreeSet::new();
        for revision in by_id.values() {
            for predecessor in &revision.supersedes {
                if by_id.contains_key(predecessor) {
                    adjacency
                        .get_mut(&revision.revision.revision_id)
                        .expect("known revision")
                        .insert(predecessor.clone());
                    adjacency
                        .get_mut(predecessor)
                        .expect("known predecessor")
                        .insert(revision.revision.revision_id.clone());
                    admitted.insert((revision.revision.revision_id.clone(), predecessor.clone()));
                } else {
                    anomalies.push(anomaly(
                        "dangling_relation",
                        [root.root_identity_hash.clone()].into(),
                        [revision.revision.revision_id.clone(), predecessor.clone()].into(),
                    )?);
                }
            }
        }
        let components = connected_components(&adjacency);
        for members in &components {
            let groups: BTreeSet<_> = members
                .iter()
                .filter_map(|revision| by_id.get(revision))
                .map(|revision| revision.legacy_group.clone())
                .collect();
            if groups.len() > 1 {
                anomalies.push(anomaly(
                    "legacy_group_bridge",
                    [root.root_identity_hash.clone()].into(),
                    members.clone(),
                )?);
            }
            let component_edges: BTreeSet<_> = admitted
                .iter()
                .filter(|(successor, predecessor)| {
                    members.contains(successor) && members.contains(predecessor)
                })
                .cloned()
                .collect();
            if revision_graph_has_cycle(members, &component_edges) {
                anomalies.push(anomaly(
                    "relation_cycle",
                    [root.root_identity_hash.clone()].into(),
                    members.clone(),
                )?);
            }
            let current = current_revisions(members, &component_edges);
            if replacement_heads_diverge(&current, &component_edges) {
                anomalies.push(anomaly(
                    "replacement_divergent_group",
                    [root.root_identity_hash.clone()].into(),
                    members.clone(),
                )?);
            }
            let shared_descriptors: BTreeSet<_> = members
                .iter()
                .filter_map(|revision| match decisions.overlap_identity.get(revision) {
                    Some(OverlapIdentityDecisionV1::Shared {
                        identity_descriptor,
                    }) => serde_json::to_string(identity_descriptor).ok(),
                    Some(OverlapIdentityDecisionV1::Distinct) | None => None,
                })
                .collect();
            if shared_descriptors.len() > 1 {
                anomalies.push(anomaly(
                    "overlap_identity_decision_conflict",
                    [root.root_identity_hash.clone()].into(),
                    members.clone(),
                )?);
            }
        }
        root_components.push((root.clone(), by_id, components, admitted));
    }

    anomalies.sort_by(|left, right| left.anomaly_id.cmp(&right.anomaly_id));
    anomalies.dedup_by(|left, right| left.anomaly_id == right.anomaly_id);
    let unresolved = anomalies.iter().any(|item| {
        if matches!(
            item.code.as_str(),
            "overlap_identity_decision_conflict" | "retained_identity_mismatch"
        ) {
            true
        } else if item.code == "cross_root_revision_overlap" {
            item.revision_ids
                .iter()
                .any(|revision| !decisions.overlap_identity.contains_key(revision))
        } else {
            !decisions.approved_anomaly_ids.contains(&item.anomaly_id)
        }
    });

    let mut proposals = Vec::new();
    for (root, by_id, components, admitted) in root_components {
        let mut groups = Vec::new();
        let mut planned_events = Vec::new();
        for members in components {
            let shared_descriptor = members.iter().find_map(|revision| {
                match decisions.overlap_identity.get(revision) {
                    Some(OverlapIdentityDecisionV1::Shared {
                        identity_descriptor,
                    }) => Some(identity_descriptor.clone()),
                    Some(OverlapIdentityDecisionV1::Distinct) | None => None,
                }
            });
            let descriptor = if let Some(descriptor) = shared_descriptor {
                descriptor
            } else {
                ChangeIdentityDescriptorV1::opaque_nonce(deterministic_nonce(&serde_json::json!({
                    "rootIdentityHash": root.root_identity_hash,
                    "members": members,
                }))?)
            };
            let declaration = build_change_declared(
                descriptor,
                deterministic_nonce(
                    &serde_json::json!({"claim": "declaration", "members": members}),
                )?,
            )?;
            let change_id = declaration.change_id.clone();
            planned_events.push(planned_event(
                &root,
                &decisions.writer,
                &decisions.occurred_at,
                declaration,
            )?);
            for revision_id in &members {
                planned_events.push(planned_event(
                    &root,
                    &decisions.writer,
                    &decisions.occurred_at,
                    build_membership_asserted(
                        &change_id,
                        revision_id,
                        deterministic_nonce(&serde_json::json!({
                            "claim": "membership",
                            "changeId": change_id,
                            "revisionId": revision_id,
                        }))?,
                    )?,
                )?);
            }
            let relations: BTreeSet<_> = admitted
                .iter()
                .filter(|(successor, predecessor)| {
                    members.contains(successor) && members.contains(predecessor)
                })
                .cloned()
                .collect();
            for (successor, predecessor) in &relations {
                planned_events.push(planned_event(
                    &root,
                    &decisions.writer,
                    &decisions.occurred_at,
                    build_revision_relation_asserted(
                        &change_id,
                        by_id
                            .get(successor)
                            .expect("component successor")
                            .revision
                            .clone(),
                        by_id
                            .get(predecessor)
                            .expect("component predecessor")
                            .revision
                            .clone(),
                        deterministic_nonce(&serde_json::json!({
                            "claim": "relation",
                            "changeId": change_id,
                            "successor": successor,
                            "predecessor": predecessor,
                        }))?,
                    )?,
                )?);
            }
            groups.push(AdoptionGroupProposalV1 {
                change_id,
                members,
                admitted_relations: relations,
            });
        }
        planned_events.sort_by(|left, right| left.idempotency_key.cmp(&right.idempotency_key));
        let manifest = if unresolved {
            None
        } else {
            let records = planned_events
                .iter()
                .map(reserved_record)
                .collect::<Result<Vec<_>>>()?;
            Some(BulkAdoptionManifestV1::from_reserved_records(
                root.source_authority_cursor.clone(),
                records,
            )?)
        };
        proposals.push(RootAdoptionProposalV1 {
            root_identity_hash: root.root_identity_hash,
            covered_revisions: by_id.into_keys().collect(),
            groups,
            planned_events,
            manifest,
        });
    }
    Ok(BulkAdoptionProposalV1 {
        roots: proposals,
        anomalies,
        requires_owner_decision: unresolved,
    })
}

fn planned_event<P: EventPayload>(
    root: &LegacyRootInventoryV1,
    writer: &Writer,
    occurred_at: &str,
    payload: P,
) -> Result<ShoreEvent> {
    let payload_value = serde_json::to_value(&payload)?;
    let payload_hash = sha256_json_prefixed(&payload_value)?;
    ShoreEvent::new(
        payload.event_type(),
        format!(
            "bulk-adoption:{}:{payload_hash}",
            payload.event_type().as_str()
        ),
        EventTarget::for_journal(root.journal_id.clone()),
        writer.clone(),
        payload,
        occurred_at,
    )
}

fn reserved_record(event: &ShoreEvent) -> Result<ReservedCohortRecordV1> {
    // EventStore persists serde's exact compact struct encoding. Reservations
    // bind those bytes, not a separately canonicalized representation.
    let bytes = serde_json::to_vec(event)?;
    let family = match event.event_type {
        crate::session::event::EventType::ChangeDeclared => "change_declared_v1",
        crate::session::event::EventType::ChangeMembershipAsserted => {
            "change_membership_asserted_v1"
        }
        crate::session::event::EventType::ChangeRevisionRelationAsserted => {
            "change_revision_relation_asserted_v1"
        }
        _ => {
            return Err(crate::error::ShoreError::Message(
                "bulk-adoption planner emitted an unsupported event family".to_owned(),
            ));
        }
    };
    Ok(ReservedCohortRecordV1 {
        logical_key: event.idempotency_key.clone(),
        record_family: family.to_owned(),
        record_hash: format!("sha256:{}", sha256_bytes_hex(&bytes)),
    })
}

fn deterministic_nonce(value: &serde_json::Value) -> Result<[u8; 32]> {
    let hash = sha256_json_prefixed(value)?;
    let hex = hash.strip_prefix("sha256:").expect("canonical hash prefix");
    let mut nonce = [0_u8; 32];
    for (index, byte) in nonce.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&hex[index * 2..index * 2 + 2], 16)
            .expect("canonical hash is lowercase hexadecimal");
    }
    Ok(nonce)
}

fn is_prefixed_sha256(value: &str) -> bool {
    value.strip_prefix("sha256:").is_some_and(|hex| {
        hex.len() == 64
            && hex
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    })
}

fn anomaly(
    code: &str,
    root_identity_hashes: BTreeSet<String>,
    revision_ids: BTreeSet<RevisionId>,
) -> Result<MigrationAnomalyV1> {
    let anomaly_id = sha256_json_prefixed(&serde_json::json!({
        "code": code,
        "rootIdentityHashes": root_identity_hashes,
        "revisionIds": revision_ids,
    }))?;
    Ok(MigrationAnomalyV1 {
        anomaly_id,
        code: code.to_owned(),
        root_identity_hashes,
        revision_ids,
    })
}

fn connected_components(
    adjacency: &BTreeMap<RevisionId, BTreeSet<RevisionId>>,
) -> Vec<BTreeSet<RevisionId>> {
    let mut remaining: BTreeSet<_> = adjacency.keys().cloned().collect();
    let mut result = Vec::new();
    while let Some(start) = remaining.pop_first() {
        let mut component = BTreeSet::new();
        let mut pending = vec![start];
        while let Some(node) = pending.pop() {
            if !component.insert(node.clone()) {
                continue;
            }
            remaining.remove(&node);
            pending.extend(adjacency.get(&node).into_iter().flatten().cloned());
        }
        result.push(component);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{JournalId, RevisionId, RevisionRefV1};
    use crate::session::AuthorityCursorV2;

    fn cursor() -> AuthorityCursorV2 {
        AuthorityCursorV2 {
            schema: "pointbreak.authority-cursor.v2".to_owned(),
            journal_record_count: 3,
            event_count: 3,
            journal_record_set_hash: format!("sha256:{}", "1".repeat(64)),
            event_set_hash: format!("sha256:{}", "2".repeat(64)),
            capability_set_hash: format!("sha256:{}", "3".repeat(64)),
        }
    }

    fn revision(name: &str, byte: char, supersedes: &[&str]) -> LegacyRevisionV1 {
        LegacyRevisionV1 {
            revision: RevisionRefV1::new(
                RevisionId::new(format!("rev:sha256:{name}")),
                format!("sha256:{}", byte.to_string().repeat(64)),
            )
            .unwrap(),
            legacy_group: "engagement:one".to_owned(),
            supersedes: supersedes
                .iter()
                .map(|name| RevisionId::new(format!("rev:sha256:{name}")))
                .collect(),
        }
    }

    fn root(revisions: Vec<LegacyRevisionV1>) -> LegacyRootInventoryV1 {
        LegacyRootInventoryV1 {
            root_identity_hash: format!("sha256:{}", "4".repeat(64)),
            journal_id: JournalId::new("journal:test"),
            source_authority_cursor: cursor(),
            revisions,
        }
    }

    #[test]
    fn component_and_singleton_proposal_is_permutation_stable_and_covers_every_revision() {
        let input = root(vec![
            revision("a", 'a', &[]),
            revision("b", 'b', &["a"]),
            revision("c", 'c', &[]),
        ]);
        let first = plan_bulk_adoption(
            std::slice::from_ref(&input),
            &[],
            &OwnerManifestDecisionsV1::default(),
        )
        .unwrap();
        let mut reversed = input;
        reversed.revisions.reverse();
        let second =
            plan_bulk_adoption(&[reversed], &[], &OwnerManifestDecisionsV1::default()).unwrap();
        assert_eq!(first, second);
        assert_eq!(first.roots[0].covered_revisions.len(), 3);
        assert_eq!(first.roots[0].groups.len(), 2);
        assert!(first.roots[0].manifest.is_some());
    }

    #[test]
    fn dangling_relation_and_cross_root_overlap_require_explicit_owner_decisions() {
        let dangling = root(vec![revision("a", 'a', &["missing"])]);
        let mut other = root(vec![revision("a", 'a', &[])]);
        other.root_identity_hash = format!("sha256:{}", "5".repeat(64));
        other.journal_id = JournalId::new("journal:other");
        let proposal = plan_bulk_adoption(
            &[dangling.clone(), other.clone()],
            &[],
            &OwnerManifestDecisionsV1::default(),
        )
        .unwrap();
        assert!(proposal.requires_owner_decision);
        assert!(proposal.roots.iter().all(|root| root.manifest.is_none()));
        assert!(
            proposal
                .anomalies
                .iter()
                .any(|item| item.code == "dangling_relation")
        );
        assert!(
            proposal
                .anomalies
                .iter()
                .any(|item| item.code == "cross_root_revision_overlap")
        );
        assert!(
            proposal
                .anomalies
                .iter()
                .any(|item| item.code == "cross_root_relation_disagreement")
        );

        let decisions = OwnerManifestDecisionsV1 {
            approved_anomaly_ids: proposal
                .anomalies
                .iter()
                .filter(|item| item.code != "cross_root_revision_overlap")
                .map(|item| item.anomaly_id.clone())
                .collect(),
            overlap_identity: [(
                RevisionId::new("rev:sha256:a"),
                OverlapIdentityDecisionV1::Shared {
                    identity_descriptor: ChangeIdentityDescriptorV1::root_revision(
                        RevisionId::new("rev:sha256:a"),
                    ),
                },
            )]
            .into(),
            ..OwnerManifestDecisionsV1::default()
        };
        let approved = plan_bulk_adoption(&[dangling, other], &[], &decisions).unwrap();
        assert!(!approved.requires_owner_decision);
        assert!(approved.roots.iter().all(|root| root.manifest.is_some()));
        assert_eq!(
            approved.roots[0].groups[0].change_id,
            approved.roots[1].groups[0].change_id
        );
    }

    #[test]
    fn retained_manifest_overlap_requires_and_reuses_an_explicit_identity_decision() {
        let revision_id = RevisionId::new("rev:sha256:a");
        let descriptor = ChangeIdentityDescriptorV1::root_revision(revision_id.clone());
        let retained_change_id = crate::model::derive_change_id(&descriptor).unwrap();
        let retained = RetainedAdoptionManifestV1 {
            manifest_hash: format!("sha256:{}", "9".repeat(64)),
            allocations: vec![RetainedChangeAllocationV1 {
                change_id: retained_change_id.clone(),
                identity_descriptor: descriptor.clone(),
                members: [revision_id.clone()].into(),
            }],
        };
        let input = root(vec![revision("a", 'a', &[])]);

        let unresolved = plan_bulk_adoption(
            std::slice::from_ref(&input),
            std::slice::from_ref(&retained),
            &OwnerManifestDecisionsV1::default(),
        )
        .unwrap();
        assert!(unresolved.requires_owner_decision);
        assert!(unresolved.roots[0].manifest.is_none());

        let decisions = OwnerManifestDecisionsV1 {
            overlap_identity: [(
                revision_id,
                OverlapIdentityDecisionV1::Shared {
                    identity_descriptor: descriptor,
                },
            )]
            .into(),
            ..OwnerManifestDecisionsV1::default()
        };
        let resolved = plan_bulk_adoption(&[input], &[retained], &decisions).unwrap();
        assert!(!resolved.requires_owner_decision);
        assert!(resolved.roots[0].manifest.is_some());
        assert_eq!(resolved.roots[0].groups[0].change_id, retained_change_id);
    }
}
