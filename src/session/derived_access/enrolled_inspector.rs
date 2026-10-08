//! Private enrolled Inspector documents. This adapter separates SQL selection
//! from filesystem hydration and retains one request pin through composition.
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use rusqlite::Connection;

use super::changes::{
    self, ChangePageLens, DerivedAttentionPageV1, DerivedChangePageRequestV1, DerivedChangePageV1,
    DerivedExactRevisionReadV1, ExactRevisionReadPlanV1,
};
use super::enrolled::RequestPin;
use super::locator::LocatorRead;
use super::sqlite::{HydratedLocatorRow, SqliteLocator, SqliteSemantic, hydrate_locator_bytes};
use crate::documents::{
    ChangeDocumentFacadeV1, FactPortApplicabilityV1, FactPortCarrierSourceV1,
    INSPECT_READER_PROFILE_SCHEMA, ReaderProfileAvailabilityV1, ReaderProfileDocumentV1,
    attention_presentation_for_change,
};
use crate::error::{Result, ShoreError};
use crate::model::{ChangeId, RevisionId, RevisionRefV1};
use crate::session::append_progress::{data_work_charge, require_quiescent_data};
use crate::session::derived_access::cursor::TruthCursor;
use crate::session::event::ShoreEvent;
use crate::session::projection::change::ChangeProjectionFact;
use crate::session::store::backend::StoreBackend;
use crate::session::{
    ChangeDocumentProjectionV1, ChangeProjection, RevisionShowOptions, RevisionShowResult,
};

const MAX_SELECTED: usize = 8192;
fn fail(value: impl std::fmt::Display) -> ShoreError {
    ShoreError::Message(value.to_string())
}
fn bounded(count: usize) -> Result<()> {
    if count > MAX_SELECTED {
        return Err(fail("enrolled selected row/closure limit"));
    }
    data_work_charge(count, 0)
}
fn bounded_ids(ids: impl IntoIterator<Item = String>) -> Result<Vec<String>> {
    let mut selected = BTreeSet::new();
    for id in ids {
        selected.insert(id);
        if selected.len() > MAX_SELECTED {
            return Err(fail("selected carrier union limit"));
        }
    }
    Ok(selected.into_iter().collect())
}

#[cfg(test)]
mod selection_tests {
    use super::*;

    #[test]
    fn carrier_union_deduplicates_shared_support_and_refuses_overflow() {
        let selected = bounded_ids(
            (0..MAX_SELECTED).flat_map(|i| [format!("event-{i:05}"), format!("event-{i:05}")]),
        )
        .unwrap();
        assert_eq!(selected.len(), MAX_SELECTED);
        assert_eq!(selected[0], "event-00000");
        assert!(bounded_ids((0..=MAX_SELECTED).map(|i| format!("event-{i}"))).is_err());
    }
}
#[doc(hidden)]
#[derive(Clone)]
pub struct OwnedInspectorGeneration {
    pub projection: ChangeProjection,
    pub document_projection: ChangeDocumentProjectionV1,
    pub facade: ChangeDocumentFacadeV1,
    proposal_summary_conflicts: BTreeSet<RevisionRefV1>,
}
#[doc(hidden)]
pub enum OwnedInspectorPage {
    Changes(DerivedChangePageV1),
    Attention(DerivedAttentionPageV1),
}
#[doc(hidden)]
pub struct OwnedInspectorReader<'a> {
    pub(super) pin: RequestPin<'a>,
    repo: &'a Path,
    backend: StoreBackend,
    generation: Option<OwnedInspectorGeneration>,
}
impl<'a> OwnedInspectorReader<'a> {
    pub(in crate::session) fn from_pin(pin: RequestPin<'a>, repo: &'a Path) -> Result<Self> {
        let backend = pin.context().backend()?;
        Ok(Self {
            pin,
            repo,
            backend,
            generation: None,
        })
    }
    pub(in crate::session) fn finish(self) -> Result<()> {
        self.pin.finish()
    }
    pub fn status_document(&self) -> serde_json::Value {
        serde_json::json!({"schema":"pointbreak.inspect-derived-access-status","version":1,
            "active":true,"availability":"current","namespace":"stable",
            "generationId":self.pin.receipt().publication_generation_id,
            "rebuildInFlight":false,"rebuildPaused":false,"servingCurrent":true,
            "fallbackInFlight":false,"actions":[]})
    }
    pub fn profile(&self) -> ReaderProfileDocumentV1 {
        let receipt = self.pin.receipt();
        ReaderProfileDocumentV1 {
            schema: INSPECT_READER_PROFILE_SCHEMA.into(),
            version: 1,
            availability: ReaderProfileAvailabilityV1::Ready,
            authority_cursor: self.pin.checkpoint().authority_cursor.clone(),
            minimum_reader_profile: Some(receipt.minimum_reader_profile.clone()),
            activation_id: Some(receipt.publication_activation_id.clone()),
            manifest_hash: Some(receipt.publication_manifest_hash.clone()),
            completion_id: Some(receipt.publication_completion_id.clone()),
            commit_graph_stamp: None,
            documents: crate::documents::change_revision_document_registry()
                .iter()
                .map(|(schema, version)| ((*schema).into(), *version))
                .collect(),
        }
    }
    pub fn generation(&mut self) -> Result<OwnedInspectorGeneration> {
        if let Some(generation) = &self.generation {
            return Ok(generation.clone());
        }
        let generation = self.pin.phase(|db, checkpoint, receipt| {
            let count: i64 = db
                .query_row(
                    "SELECT count(*) FROM semantic_change_fact WHERE sequence <= ?1",
                    [i64::try_from(checkpoint.truth_cursor.sequence).map_err(fail)?],
                    |row| {
                        crate::session::derived_access::owned_row_charge(row)?;
                        row.get(0)
                    },
                )
                .map_err(fail)?;
            if !(0..=MAX_SELECTED as i64).contains(&count) {
                return Err(fail("Change metadata closure limit"));
            }
            let LocatorRead::Ready(materialized) =
                SqliteSemantic::materialized_change_page_projection_on(db, checkpoint.truth_cursor)
                    .map_err(fail)?
            else {
                return Err(fail("enrolled Change projection stale"));
            };
            bounded(materialized.projection.changes.len())?;
            let facts = super::sqlite::query_materialized_change_document_facts(
                db,
                checkpoint.truth_cursor.epoch,
                checkpoint.truth_cursor.sequence,
            )
            .map_err(fail)?;
            bounded(facts.len())?;
            let sources = facts
                .iter()
                .filter_map(|fact| match &fact.change {
                    ChangeProjectionFact::FactPort { port } => Some((port, &fact.support)),
                    _ => None,
                })
                .map(|(port, support)| {
                    Ok(FactPortCarrierSourceV1 {
                        payload: port.clone(),
                        event_id: support.event_id.clone(),
                        actor_id: support.actor_id.clone(),
                        track_id: support
                            .track_id
                            .clone()
                            .ok_or_else(|| fail("fact port has no track"))?,
                    })
                })
                .collect::<Result<Vec<_>>>()?;
            let stamp = super::semantic::change::derived_change_generation_stamp_v1(
                receipt,
                checkpoint,
                &materialized.projection,
                &materialized.document_projection,
            )
            .map_err(fail)?;
            let conflicts =
                super::sqlite::query_proposal_summary_conflicts(db, checkpoint.truth_cursor)
                    .map_err(fail)?;
            let proposal_summary_conflicts =
                crate::documents::current_revision_refs_matching_projection(
                    &materialized.projection,
                    &materialized.document_projection,
                    &conflicts.into_keys().collect(),
                );
            let facade = ChangeDocumentFacadeV1::new(
                materialized.projection.clone(),
                materialized.document_projection.clone(),
            )?
            .with_ordering(materialized.ordering)?
            .with_generation_stamp(stamp)?
            .with_fact_port_sources(sources)?;
            Ok(OwnedInspectorGeneration {
                projection: materialized.projection,
                document_projection: materialized.document_projection,
                facade,
                proposal_summary_conflicts,
            })
        })?;
        self.generation = Some(generation.clone());
        Ok(generation)
    }
    pub(super) fn hydrate(&mut self, event_ids: &[String]) -> Result<Vec<HydratedLocatorRow>> {
        bounded(event_ids.len())?;
        let rows = self.pin.phase(|db, checkpoint, _| {
            SqliteLocator::selected_rows_on(db, event_ids, checkpoint.truth_cursor).map_err(fail)
        })?;
        self.hydrate_detached(event_ids, rows)
    }
    pub(super) fn hydrate_detached(
        &self,
        event_ids: &[String],
        rows: Vec<Option<super::locator::LocatorRow>>,
    ) -> Result<Vec<HydratedLocatorRow>> {
        require_quiescent_data()?;
        if rows.len() != event_ids.len() {
            return Err(fail("selected locator count mismatch"));
        }
        event_ids
            .iter()
            .zip(rows)
            .map(|(id, row)| {
                let row = row
                    .ok_or_else(|| fail(format!("selected authoritative event {id} is absent")))?;
                if row.event_id != *id {
                    return Err(fail("selected locator identity mismatch"));
                }
                let bytes = self
                    .pin
                    .context()
                    .selected_event_bytes(&row.logical_reread_key)?;
                hydrate_locator_bytes(row, bytes).map_err(fail)
            })
            .collect()
    }
    fn support(&mut self, selected: &[ShoreEvent]) -> Result<Vec<ShoreEvent>> {
        let plan = self.pin.phase(|db, checkpoint, _| {
            super::support::support_event_plan_readonly(db, selected, checkpoint.truth_cursor)
                .map_err(fail)
        })?;
        let events = self
            .hydrate(&plan.all_event_ids())?
            .into_iter()
            .map(|row| row.event)
            .collect::<Vec<_>>();
        super::fact_reads::validate_support_events(&plan, selected, &events).map_err(fail)?;
        Ok(events)
    }
    pub fn page(
        &mut self,
        request: &DerivedChangePageRequestV1,
        attention: bool,
    ) -> Result<OwnedInspectorPage> {
        let generation = self.generation()?;
        let facade = &generation.facade;
        let lens = if attention {
            ChangePageLens::Attention
        } else {
            ChangePageLens::Changes
        };
        let order = request.order_for(lens);
        let summaries = facade.list_document_for_inspector().changes;
        let candidate_ids = changes::select_bodyless_change_candidates(
            lens,
            &summaries,
            facade.projection_stamp(),
            request,
            order,
        )?;
        let query = match request {
            DerivedChangePageRequestV1::Bare => None,
            DerivedChangePageRequestV1::Bounded(selection) => selection.summary_query(),
        };
        let provisional = if query.is_none() {
            Some(changes::select_bodyless_change_page(
                lens,
                &summaries,
                facade.projection_stamp(),
                request,
                order,
            )?)
        } else {
            None
        };
        let wanted = provisional
            .as_ref()
            .map(|selection| &selection.change_ids)
            .unwrap_or(&candidate_ids)
            .iter()
            .cloned()
            .collect::<BTreeSet<_>>();
        let revisions = summaries
            .iter()
            .filter(|summary| wanted.contains(&summary.change_id))
            .flat_map(|summary| summary.current_revision_refs.iter().cloned())
            .collect::<BTreeSet<_>>();
        bounded(revisions.len())?;
        let locators = self.pin.phase(|db, checkpoint, _| {
            SqliteSemantic::proposal_carrier_locators_readonly_on(
                db,
                &revisions,
                checkpoint.truth_cursor,
            )
            .map_err(fail)
        })?;
        if locators.values().any(Vec::is_empty) {
            return Err(fail("selected exact Revision lacks proposal carrier"));
        }
        let located = locators.values().flatten().collect::<Vec<_>>();
        let ids = located
            .iter()
            .map(|locator| locator.event_id.as_str().to_owned())
            .collect::<Vec<_>>();
        let hydrated = self.hydrate(&ids)?;
        #[cfg(any(test, feature = "longitudinal-counting"))]
        crate::bench_support::longitudinal::record_change_proposal_carriers_opened(hydrated.len());
        let mut events = Vec::new();
        for (expected, row) in located.into_iter().zip(hydrated) {
            if row.row.cursor != expected.cursor
                || crate::canonical_hash::sha256_bytes_hex(row.row.logical_reread_key.as_bytes())
                    != expected.logical_reread_key_hash
                || row.row.replay_key != expected.replay_key
                || row.row.event_type != expected.event_type
                || row.row.payload_hash != expected.payload_hash
                || row.row.validation_witness != expected.validation_witness
                || changes::exact_revision_from_proposal(&row.event)? != expected.revision
            {
                return Err(fail("proposal carrier differs from selected exact binding"));
            }
            events.push(row.event);
        }
        let selection = match provisional {
            Some(selection) => selection,
            None => {
                let matches = facade.search_change_ids_with_proposal_presentations(
                    &candidate_ids,
                    &events,
                    query.unwrap(),
                )?;
                changes::paginate_bodyless_change_candidates(
                    &matches,
                    &summaries,
                    facade.projection_stamp(),
                    request,
                    order,
                )?
            }
        };
        let selected = selection
            .change_ids
            .iter()
            .cloned()
            .collect::<BTreeSet<_>>();
        let revisions = summaries
            .iter()
            .filter(|summary| selected.contains(&summary.change_id))
            .flat_map(|summary| summary.current_revision_refs.iter().cloned())
            .collect::<BTreeSet<_>>();
        let events = events
            .into_iter()
            .filter_map(
                |event| match changes::exact_revision_from_proposal(&event) {
                    Ok(exact) if revisions.contains(&exact) => Some(Ok(event)),
                    Ok(_) => None,
                    Err(error) => Some(Err(error)),
                },
            )
            .collect::<Result<Vec<_>>>()?;
        let _support = self.support(&events)?;
        if attention {
            let document = facade.selected_attention_document_for_inspector_with_presentations(
                &selection.change_ids,
                &events,
                facade.projection_stamp(),
                order,
            )?;
            let attention_presentations = selection
                .change_ids
                .iter()
                .map(|id| {
                    attention_presentation_for_change(&facade.detail_document(id)?.detail)
                        .map(|value| (id.clone(), value))
                })
                .collect::<Result<_>>()?;
            Ok(OwnedInspectorPage::Attention(DerivedAttentionPageV1 {
                document,
                attention_presentations,
                window: selection.window,
            }))
        } else {
            Ok(OwnedInspectorPage::Changes(DerivedChangePageV1 {
                document: facade.selected_list_document_for_inspector_with_presentations(
                    &selection.change_ids,
                    &events,
                    facade.projection_stamp(),
                    order,
                )?,
                window: selection.window,
            }))
        }
    }
    fn components(
        &mut self,
        revisions: &BTreeSet<RevisionRefV1>,
        plan: &ExactRevisionReadPlanV1,
    ) -> Result<BTreeMap<RevisionRefV1, RevisionShowResult>> {
        bounded(revisions.len())?;
        let selections = self.pin.phase(|db, checkpoint, _| {
            revisions
                .iter()
                .map(|exact| {
                    component_ids(db, &exact.revision_id, checkpoint.truth_cursor)
                        .map(|ids| (exact.clone(), ids))
                })
                .collect::<Result<BTreeMap<_, _>>>()
        })?;
        let all = bounded_ids(selections.values().flatten().cloned())?;
        let hydrated = self
            .hydrate(&all)?
            .into_iter()
            .map(|row| (row.event.event_id.as_str().to_owned(), row.event))
            .collect::<BTreeMap<_, _>>();
        let selected = selections
            .into_iter()
            .map(|(exact, ids)| {
                let events = ids
                    .iter()
                    .map(|id| {
                        hydrated
                            .get(id)
                            .cloned()
                            .ok_or_else(|| fail("selected component absent"))
                    })
                    .collect::<Result<Vec<_>>>()?;
                super::detail_reads::validate_selected_component_events(
                    &events,
                    &exact.revision_id,
                )
                .map_err(fail)?;
                Ok((exact, events))
            })
            .collect::<Result<BTreeMap<_, _>>>()?;
        let plans = self.pin.phase(|db, checkpoint, _| {
            selected
                .iter()
                .map(|(exact, events)| {
                    super::support::support_event_plan_readonly(db, events, checkpoint.truth_cursor)
                        .map(|plan| (exact.clone(), plan))
                        .map_err(fail)
                })
                .collect::<Result<BTreeMap<_, _>>>()
        })?;
        let support_ids = bounded_ids(plans.values().flat_map(|plan| plan.all_event_ids()))?;
        let support = self
            .hydrate(&support_ids)?
            .into_iter()
            .map(|row| (row.event.event_id.as_str().to_owned(), row.event))
            .collect::<BTreeMap<_, _>>();
        let mut results = BTreeMap::new();
        for (exact, mut events) in selected {
            let selected_plan = &plans[&exact];
            let extra = selected_plan
                .all_event_ids()
                .iter()
                .map(|id| {
                    support
                        .get(id)
                        .cloned()
                        .ok_or_else(|| fail("component support absent"))
                })
                .collect::<Result<Vec<_>>>()?;
            super::fact_reads::validate_support_events(selected_plan, &events, &extra)
                .map_err(fail)?;
            events.extend(extra);
            super::fact_reads::normalize_events(&mut events);
            require_quiescent_data()?;
            let result = crate::session::workflow::show_revision_from_selected_events(
                RevisionShowOptions::new(self.repo)
                    .with_revision_id(exact.revision_id.clone())
                    .with_exact(true)
                    .with_include_body(plan.include_body)
                    .with_read_for_display(plan.read_for_display),
                &self.backend,
                events,
            )?;
            if result.revision.object_artifact_content_hash != exact.object_artifact_content_hash {
                return Err(fail("exact Revision artifact hash mismatch"));
            }
            results.insert(exact, result);
        }
        Ok(results)
    }
    pub fn exact(
        &mut self,
        change_id: &ChangeId,
        plan: &ExactRevisionReadPlanV1,
    ) -> Result<DerivedExactRevisionReadV1> {
        let generation = self.generation()?;
        let view = generation
            .projection
            .changes
            .get(change_id)
            .cloned()
            .ok_or_else(|| fail("Change unavailable"))?;
        let mut planned = plan.revisions.iter().cloned().collect::<BTreeSet<_>>();
        if let Some(base) = &plan.fact_port_context {
            planned.insert(base.clone());
        }
        let mut results = self.components(&planned, plan)?;
        if let Some(base) = &plan.fact_port_context {
            let base_result = results
                .get(base)
                .ok_or_else(|| fail("context base missing"))?;
            let (facts, _) = crate::documents::normalize_fact_presentations(base_result, base);
            let mut origins = BTreeSet::new();
            for port in generation.facade.fact_port_presentations(change_id, base)? {
                if port.applicability != FactPortApplicabilityV1::Applicable {
                    continue;
                }
                if let Some(target) = &port.target_fact {
                    let (family, id) = match target {
                        crate::session::event::FactRefV1::Observation { observation_id } => {
                            ("observation", observation_id.as_str())
                        }
                        crate::session::event::FactRefV1::InputRequest { input_request_id } => {
                            ("input_request", input_request_id.as_str())
                        }
                    };
                    if !facts.iter().any(|fact| {
                        fact.origin_revision == *base && fact.family == family && fact.fact_id == id
                    }) {
                        continue;
                    }
                }
                if !results.contains_key(&port.origin_revision) {
                    origins.insert(port.origin_revision);
                }
            }
            if !origins.is_empty() {
                results.extend(self.components(&origins, plan)?);
            }
        }
        Ok(DerivedExactRevisionReadV1::new(
            view,
            generation.document_projection,
            generation.facade.clone(),
            generation.facade.projection_stamp().into(),
            results,
        ))
    }
}

/// Same component graph as the existing recursive SQL, traversed with bounded
/// Rust sets and indexed readonly probes in ONE native phase.
fn component_ids(
    db: &Connection,
    revision: &RevisionId,
    observed: TruthCursor,
) -> Result<Vec<String>> {
    let epoch = i64::try_from(observed.epoch).map_err(fail)?;
    let sequence = i64::try_from(observed.sequence).map_err(fail)?;
    let forward = "SELECT edge.superseded_revision_id FROM product_revision AS revision INDEXED BY product_revision_identity CROSS JOIN semantic_representative AS representative CROSS JOIN locator_event_text AS locator CROSS JOIN product_revision_edge AS edge WHERE revision.revision_id=?3 AND representative.family_id=1 AND representative.sequence=revision.sequence AND locator.sequence=revision.sequence AND edge.sequence=revision.sequence AND locator.epoch=?1 AND revision.sequence<=?2";
    let backward = "SELECT revision.revision_id FROM product_revision_edge AS edge INDEXED BY product_revision_edge_target CROSS JOIN product_revision AS revision CROSS JOIN semantic_representative AS representative CROSS JOIN locator_event_text AS locator WHERE edge.superseded_revision_id=?3 AND revision.sequence=edge.sequence AND representative.family_id=1 AND representative.sequence=revision.sequence AND locator.sequence=revision.sequence AND locator.epoch=?1 AND revision.sequence<=?2";
    let mut pending = BTreeSet::from([revision.as_str().to_owned()]);
    let mut visited = BTreeSet::new();
    let mut forward = db.prepare(forward).map_err(fail)?;
    let mut backward = db.prepare(backward).map_err(fail)?;
    while let Some(id) = pending.pop_first() {
        if !visited.insert(id.clone()) {
            continue;
        }
        if visited.len() > MAX_SELECTED {
            return Err(fail("component closure limit"));
        }
        data_work_charge(1, 0)?;
        for statement in [&mut forward, &mut backward] {
            let rows = statement
                .query_map(rusqlite::params![epoch, sequence, &id], |row| {
                    crate::session::derived_access::owned_row_charge(row)?;
                    row.get::<_, String>(0)
                })
                .map_err(fail)?;
            for row in rows {
                let next = row.map_err(fail)?;
                if !visited.contains(&next) {
                    pending.insert(next);
                }
            }
        }
        if pending.len() > MAX_SELECTED {
            return Err(fail("component pending limit"));
        }
    }
    let mut statement = db.prepare("SELECT locator.replay_key,locator.event_id FROM semantic_event_fact_text AS event JOIN locator_event_text AS locator ON locator.sequence=event.sequence WHERE locator.epoch=?1 AND locator.sequence<=?2 AND event.revision_id=?3 ORDER BY locator.replay_key,locator.event_id").map_err(fail)?;
    let mut events = BTreeSet::new();
    for id in visited {
        let rows = statement
            .query_map(rusqlite::params![epoch, sequence, id], |row| {
                crate::session::derived_access::owned_row_charge(row)?;
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })
            .map_err(fail)?;
        for row in rows {
            events.insert(row.map_err(fail)?);
            if events.len() > MAX_SELECTED {
                return Err(fail("component carrier limit"));
            }
        }
    }
    Ok(events.into_iter().map(|(_, id)| id).collect())
}

impl OwnedInspectorReader<'_> {
    pub fn trust_set(&self) -> Result<crate::session::TrustSet> {
        self.pin.context().trust_set()
    }
    pub fn timeline(
        &mut self,
        request: &super::timeline::DerivedTimelinePageRequestV1,
        trust_set: &crate::session::TrustSet,
    ) -> Result<super::changes::DerivedChangeOutcomeV1<super::timeline::DerivedTimelinePageV1>>
    {
        use super::changes::{
            DerivedChangeOutcomeV1 as Outcome, DerivedProjectionFailureCodeV1 as Code,
        };
        let generation = self.generation()?;
        let source_stamp = generation.facade.projection_stamp().to_owned();
        let stamp = crate::session::projection::event_history::timeline_projection_stamp_v1(
            &source_stamp,
            &trust_set.identity_sha256()?,
        )?;
        if request
            .position()
            .expected_projection_stamp()
            .is_some_and(|expected| expected != stamp)
        {
            return Ok(Outcome::projection_unavailable(
                Code::ProjectionStale,
                "derived Timeline continuation belongs to a different live checkpoint or TrustSet",
            ));
        }
        if let Some(conflict) = generation.proposal_summary_conflicts.iter().next() {
            return Ok(Outcome::projection_unavailable(
                Code::ProjectionInvalid,
                format!(
                    "conflicting proposal summaries for exact Revision {}",
                    conflict.revision_id.as_str()
                ),
            ));
        }
        let checkpoint = self.pin.checkpoint().clone();
        match super::timeline::enrolled::prepare_timeline_page(
            self,
            &generation.document_projection,
            checkpoint.truth_cursor,
            checkpoint.authority_cursor,
            source_stamp,
            stamp,
            request,
            trust_set,
            &mut |_| {},
        ) {
            Ok(page) => Ok(Outcome::Ready(page)),
            Err(super::timeline::TimelinePageError::RequestInvalid(reason)) => {
                Err(ShoreError::WorkflowInputInvalid { reason })
            }
            Err(super::timeline::TimelinePageError::Stale(reason)) => Ok(
                Outcome::projection_unavailable(Code::ProjectionStale, reason),
            ),
            Err(super::timeline::TimelinePageError::Invalid(reason)) => Ok(
                Outcome::projection_unavailable(Code::ProjectionInvalid, reason),
            ),
        }
    }
}
