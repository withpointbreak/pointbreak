//! Authoritative proposal-carrier hydration for one derived Change detail.
//!
//! The derived Change detail composes from the materialized generation, which
//! carries no proposal prose. Its current-Revision presentation entries (#755)
//! therefore hydrate each current exact Revision's proposal carriers at the
//! generation's pinned truth cursor and validate every carrier against its
//! compact locator, exactly as the Change page producer does, before the
//! shared document fold reads a summary.

use std::collections::BTreeSet;

use super::cursor::TruthCursor;
use super::locator::LocatorRead;
use super::service::DerivedAccessService;
#[cfg(any(test, feature = "longitudinal-counting"))]
use crate::bench_support::longitudinal::{
    record_change_proposal_carriers_opened, record_change_proposal_carriers_validated,
};
use crate::canonical_hash::sha256_bytes_hex;
use crate::model::RevisionRefV1;
use crate::session::event::ShoreEvent;

/// Why hydration could not produce a validated carrier set.
pub(super) enum DetailProposalHydrationError {
    /// The derived view moved past the pinned cursor; retry the read.
    Stale(&'static str),
    /// A carrier is missing or disagrees with its compact locator.
    Invalid(String),
}

/// Hydrate and validate every proposal carrier for `revisions` at `as_of`.
pub(super) fn hydrate_current_revision_proposals(
    service: &DerivedAccessService,
    revisions: &BTreeSet<RevisionRefV1>,
    as_of: TruthCursor,
) -> Result<Vec<ShoreEvent>, DetailProposalHydrationError> {
    if revisions.is_empty() {
        return Ok(Vec::new());
    }
    let locators = match service
        .proposal_carrier_locators_for_exact_revisions(revisions, as_of)
        .map_err(|error| DetailProposalHydrationError::Invalid(error.to_string()))?
    {
        LocatorRead::Ready(locators) => locators,
        LocatorRead::CatchUpRequired { .. } => {
            return Err(DetailProposalHydrationError::Stale(
                "derived Change proposal locators moved during detail composition",
            ));
        }
    };
    if let Some(missing) = revisions
        .iter()
        .find(|revision| locators.get(*revision).is_none_or(Vec::is_empty))
    {
        return Err(DetailProposalHydrationError::Invalid(format!(
            "selected current exact Revision {} has no authoritative proposal carrier",
            missing.revision_id.as_str()
        )));
    }
    let located = locators
        .iter()
        .flat_map(|(revision, locators)| locators.iter().map(move |locator| (revision, locator)))
        .collect::<Vec<_>>();
    let event_ids = located
        .iter()
        .map(|(_, locator)| locator.event_id.as_str().to_owned())
        .collect::<Vec<_>>();
    let hydrated = match service
        .semantic_ids_hydrated_at(&event_ids, as_of)
        .map_err(|error| DetailProposalHydrationError::Invalid(error.to_string()))?
    {
        LocatorRead::Ready(hydrated) => hydrated,
        LocatorRead::CatchUpRequired { .. } => {
            return Err(DetailProposalHydrationError::Stale(
                "derived Change proposal carriers moved during detail composition",
            ));
        }
    };
    #[cfg(any(test, feature = "longitudinal-counting"))]
    record_change_proposal_carriers_opened(event_ids.len());
    if hydrated.len() != located.len() {
        return Err(DetailProposalHydrationError::Invalid(
            "authoritative proposal hydration returned the wrong carrier count".to_owned(),
        ));
    }
    let mut events = Vec::with_capacity(hydrated.len());
    for ((expected_revision, locator), hydrated) in located.into_iter().zip(hydrated) {
        let hydrated = hydrated.ok_or_else(|| {
            DetailProposalHydrationError::Invalid(format!(
                "selected authoritative proposal carrier {} is absent",
                locator.event_id.as_str()
            ))
        })?;
        if hydrated.row.cursor != locator.cursor
            || sha256_bytes_hex(hydrated.row.logical_reread_key.as_bytes())
                != locator.logical_reread_key_hash
            || hydrated.row.replay_key != locator.replay_key
            || hydrated.row.event_id != locator.event_id.as_str()
            || hydrated.row.event_type != locator.event_type
            || hydrated.row.payload_hash != locator.payload_hash
            || hydrated.row.validation_witness != locator.validation_witness
            || hydrated.event.event_id != locator.event_id
            || hydrated.event.payload_hash != locator.payload_hash
        {
            return Err(DetailProposalHydrationError::Invalid(format!(
                "authoritative proposal carrier {} differs from its compact locator",
                locator.event_id.as_str()
            )));
        }
        let actual_revision = super::changes::exact_revision_from_proposal(&hydrated.event)
            .map_err(|error| DetailProposalHydrationError::Invalid(error.to_string()))?;
        if actual_revision != *expected_revision || locator.revision != *expected_revision {
            return Err(DetailProposalHydrationError::Invalid(format!(
                "authoritative proposal carrier {} has the wrong exact Revision binding",
                locator.event_id.as_str()
            )));
        }
        #[cfg(any(test, feature = "longitudinal-counting"))]
        record_change_proposal_carriers_validated(1);
        events.push(hydrated.event);
    }
    Ok(events)
}
