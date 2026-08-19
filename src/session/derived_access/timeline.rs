//! Change-aware Timeline selection over the shared product-history projection.
//!
//! Token parsing and signing stay in the Inspector binary. These neutral types
//! carry only the authenticated selection, an optional exact live stamp, and
//! unsigned neighboring boundaries across the session facade.
#![cfg_attr(not(test), allow(dead_code))]

use std::collections::{BTreeMap, BTreeSet};

use rusqlite::OptionalExtension;
use rusqlite::types::Value;

#[cfg(any(test, feature = "longitudinal-counting"))]
use crate::bench_support::longitudinal::{
    record_timeline_correlation_support_carriers, record_timeline_entries_emitted,
    record_timeline_exhaustive_candidates, record_timeline_removal_support_carriers,
    record_timeline_revision_candidate_carriers, record_timeline_selected_carriers,
    record_timeline_signature_support_carriers, record_timeline_sqlite_candidates,
    record_timeline_sqlite_facet_rows, record_timeline_sqlite_window_rows,
    record_timeline_trust_support_carriers,
};
use crate::canonical_hash::sha256_bytes_hex;
use crate::documents::{
    EventHistoryCompletionV1, EventHistoryDocumentV1, EventHistoryEntryV1, EventHistoryOrderV1,
    INSPECT_EVENT_HISTORY_SCHEMA,
};
use crate::model::{ChangeId, EventId, RevisionId, RevisionRefV1, TrackId};
use crate::session::derived_access::cursor::TruthCursor;
use crate::session::derived_access::history::{
    SelectedHistoryKey, query_selected_count, query_selected_facets, query_selected_index,
    query_selected_key, query_selected_window,
};
use crate::session::derived_access::locator::LocatorRead;
use crate::session::derived_access::service::DerivedAccessService;
use crate::session::derived_access::sqlite::{
    HydratedLocatorRow, ProductHistoryFact, ProposalCarrierLocator,
};
use crate::session::derived_access::support::{SupportEventPlan, support_event_plan};
use crate::session::event::{EventType, ShoreEvent};
use crate::session::projection::event_history::{
    EventHistoryEntryDraftV1, bind_selected_event_history_trust,
    project_selected_event_history_without_trust,
};
use crate::session::workflow::{
    MatchKind, QueryClause, QueryDiagnosticCode, QuerySurface, event_history_search_record,
    match_kind_for, matches_query, parse_search_query_for, range_bound, resolve_assessment_value,
    resolve_type_value,
};
use crate::session::{AuthorityCursorV2, TrustSet};

const DEFAULT_LIMIT: usize = 100;
const MAXIMUM_LIMIT: usize = 100;
const MAXIMUM_QUERY_BYTES: usize = 256;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
#[doc(hidden)]
pub enum DerivedTimelineOrderV1 {
    Asc,
    #[default]
    Desc,
}

#[derive(Clone, Debug, Eq, PartialEq)]
#[doc(hidden)]
pub struct DerivedTimelinePageKeyV1 {
    occurred_at: String,
    event_id: EventId,
}

impl DerivedTimelinePageKeyV1 {
    pub fn new(
        occurred_at: impl Into<String>,
        event_id: EventId,
    ) -> Result<Self, DerivedTimelinePageRequestError> {
        let occurred_at = occurred_at.into();
        if occurred_at.is_empty() || event_id.as_str().is_empty() {
            return Err(DerivedTimelinePageRequestError::InvalidBoundary);
        }
        Ok(Self {
            occurred_at,
            event_id,
        })
    }

    pub fn occurred_at(&self) -> &str {
        &self.occurred_at
    }

    pub fn event_id(&self) -> &EventId {
        &self.event_id
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
#[doc(hidden)]
pub enum DerivedTimelinePageBoundaryV1 {
    SelectedOrderStart,
    Key(DerivedTimelinePageKeyV1),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[doc(hidden)]
pub enum DerivedTimelineTraversalV1 {
    Previous,
    Next,
}

#[derive(Clone, Debug, Eq, PartialEq)]
#[doc(hidden)]
pub enum DerivedTimelinePagePositionV1 {
    Initial,
    At(EventId),
    Continuation {
        traversal: DerivedTimelineTraversalV1,
        boundary: DerivedTimelinePageBoundaryV1,
        expected_projection_stamp: String,
    },
}

impl DerivedTimelinePagePositionV1 {
    pub fn continuation(
        traversal: DerivedTimelineTraversalV1,
        boundary: DerivedTimelinePageBoundaryV1,
        expected_projection_stamp: impl Into<String>,
    ) -> Result<Self, DerivedTimelinePageRequestError> {
        let expected_projection_stamp = expected_projection_stamp.into();
        if expected_projection_stamp.is_empty() {
            return Err(DerivedTimelinePageRequestError::MissingProjectionStamp);
        }
        if matches!(
            (&traversal, &boundary),
            (
                DerivedTimelineTraversalV1::Next,
                DerivedTimelinePageBoundaryV1::SelectedOrderStart
            )
        ) {
            return Err(DerivedTimelinePageRequestError::InvalidBoundary);
        }
        Ok(Self::Continuation {
            traversal,
            boundary,
            expected_projection_stamp,
        })
    }

    pub fn expected_projection_stamp(&self) -> Option<&str> {
        match self {
            Self::Continuation {
                expected_projection_stamp,
                ..
            } => Some(expected_projection_stamp),
            Self::Initial | Self::At(_) => None,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
#[doc(hidden)]
pub struct DerivedTimelineExactRevisionV1 {
    reference: RevisionRefV1,
}

impl DerivedTimelineExactRevisionV1 {
    pub fn new(reference: RevisionRefV1) -> Self {
        Self { reference }
    }

    pub fn reference(&self) -> &RevisionRefV1 {
        &self.reference
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
#[doc(hidden)]
pub struct DerivedTimelinePageRequestV1 {
    limit: usize,
    order: DerivedTimelineOrderV1,
    query: Option<String>,
    event_types: Vec<EventType>,
    track: Option<String>,
    change: Option<ChangeId>,
    revision: Option<DerivedTimelineExactRevisionV1>,
    position: DerivedTimelinePagePositionV1,
}

impl DerivedTimelinePageRequestV1 {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        limit: usize,
        order: DerivedTimelineOrderV1,
        query: Option<String>,
        event_types: Vec<EventType>,
        track: Option<String>,
        change: Option<ChangeId>,
        revision: Option<DerivedTimelineExactRevisionV1>,
        position: DerivedTimelinePagePositionV1,
    ) -> Result<Self, DerivedTimelinePageRequestError> {
        if !(1..=MAXIMUM_LIMIT).contains(&limit) {
            return Err(DerivedTimelinePageRequestError::InvalidLimit);
        }
        if track.as_ref().is_some_and(String::is_empty) {
            return Err(DerivedTimelinePageRequestError::EmptyTrack);
        }
        let query = normalize_query(query)?;
        let mut observed = BTreeSet::new();
        for event_type in &event_types {
            if !observed.insert(event_type.as_str()) {
                return Err(DerivedTimelinePageRequestError::DuplicateEventType);
            }
        }
        Ok(Self {
            limit,
            order,
            query,
            event_types,
            track,
            change,
            revision,
            position,
        })
    }

    pub fn initial() -> Self {
        Self {
            limit: DEFAULT_LIMIT,
            order: DerivedTimelineOrderV1::Desc,
            query: None,
            event_types: Vec::new(),
            track: None,
            change: None,
            revision: None,
            position: DerivedTimelinePagePositionV1::Initial,
        }
    }

    pub fn limit(&self) -> usize {
        self.limit
    }

    pub fn order(&self) -> DerivedTimelineOrderV1 {
        self.order
    }

    pub fn query(&self) -> Option<&str> {
        self.query.as_deref()
    }

    pub fn event_types(&self) -> &[EventType] {
        &self.event_types
    }

    pub fn track(&self) -> Option<&str> {
        self.track.as_deref()
    }

    pub fn change(&self) -> Option<&ChangeId> {
        self.change.as_ref()
    }

    pub fn revision(&self) -> Option<&DerivedTimelineExactRevisionV1> {
        self.revision.as_ref()
    }

    pub fn position(&self) -> &DerivedTimelinePagePositionV1 {
        &self.position
    }
}

fn normalize_query(
    query: Option<String>,
) -> Result<Option<String>, DerivedTimelinePageRequestError> {
    let Some(query) = query else {
        return Ok(None);
    };
    let query = query.trim().to_lowercase();
    if query.is_empty() {
        return Err(DerivedTimelinePageRequestError::EmptyQuery);
    }
    if query.len() > MAXIMUM_QUERY_BYTES {
        return Err(DerivedTimelinePageRequestError::QueryTooLong);
    }
    let parsed = parse_search_query_for(&query, QuerySurface::ChangeTimeline);
    if let Some(diagnostic) = parsed.diagnostics.iter().find(|diagnostic| {
        matches!(
            diagnostic.code,
            QueryDiagnosticCode::UnsupportedQualifier | QueryDiagnosticCode::UnsupportedValue
        )
    }) {
        return Err(DerivedTimelinePageRequestError::InvalidQuery(
            diagnostic.message.clone(),
        ));
    }
    Ok(Some(query))
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum TimelineQueryClass {
    Bodyless,
    Exhaustive,
}

pub(super) fn classify_query(request: &DerivedTimelinePageRequestV1) -> TimelineQueryClass {
    let Some(query) = request.query() else {
        return TimelineQueryClass::Bodyless;
    };
    let parsed = parse_search_query_for(query, QuerySurface::ChangeTimeline);
    if parsed
        .clauses
        .iter()
        .any(|clause| matches!(clause, QueryClause::Text { .. }))
    {
        TimelineQueryClass::Exhaustive
    } else {
        TimelineQueryClass::Bodyless
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum TimelinePageError {
    RequestInvalid(String),
    Stale(String),
    Invalid(String),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum TimelineReadBoundary {
    SnapshotPinned,
    SupportExpansionStarted,
    CarrierLocatorsSelected,
    CarrierHydrationMidpoint,
    TrustBindingComplete,
}

impl From<String> for TimelinePageError {
    fn from(message: String) -> Self {
        Self::Invalid(message)
    }
}

const TIMELINE_RELATION: &str = "timeline_event";

fn timeline_cte(as_of: TruthCursor) -> String {
    format!(
        "WITH timeline_event AS (
             SELECT locator.sequence,
                    locator.event_id,
                    locator.normalized_occurred_at,
                    event.occurred_at,
                    locator.event_type,
                    locator.track_id,
                    event.actor_id,
                    history.request_state,
                    assessment.assessment,
                    validation.status AS validation_status
             FROM product_history_event AS history
             JOIN locator_event_text AS locator ON locator.sequence = history.sequence
             JOIN semantic_event_fact_text AS event ON event.sequence = history.sequence
             LEFT JOIN semantic_assessment_fact AS assessment
               ON assessment.sequence = history.sequence
             LEFT JOIN semantic_validation_fact AS validation
               ON validation.sequence = history.sequence
             WHERE locator.epoch = {}
               AND locator.sequence <= {}
         )",
        as_of.epoch, as_of.sequence
    )
}

#[derive(Clone, Debug)]
struct TimelinePredicate {
    sql: String,
    parameters: Vec<Value>,
    notices: Vec<String>,
}

fn timeline_predicate(
    request: &DerivedTimelinePageRequestV1,
    include_url_types: bool,
) -> TimelinePredicate {
    let mut predicates = vec!["1 = 1".to_owned()];
    let mut parameters = Vec::new();
    if let Some(track) = request.track() {
        predicates.push("coalesce(track_id, '') = ?".to_owned());
        parameters.push(track.to_owned().into());
    }
    if let Some(change) = request.change() {
        predicates.push(
            "EXISTS (
                 SELECT 1
                 FROM product_history_change_correlation AS correlation
                 WHERE correlation.sequence = timeline_event.sequence
                   AND correlation.change_id = ?
             )"
            .to_owned(),
        );
        parameters.push(change.as_str().to_owned().into());
    }
    if let Some(revision) = request.revision() {
        predicates.push(
            "EXISTS (
                 SELECT 1
                 FROM product_history_revision_reference AS reference
                 WHERE reference.sequence = timeline_event.sequence
                   AND reference.resolution = 'exact'
                   AND reference.revision_id = ?
                   AND reference.object_artifact_content_hash = ?
             )"
            .to_owned(),
        );
        parameters.extend([
            revision.reference().revision_id.as_str().to_owned().into(),
            revision
                .reference()
                .object_artifact_content_hash
                .clone()
                .into(),
        ]);
    }
    if include_url_types && !request.event_types().is_empty() {
        push_values_predicate(
            &mut predicates,
            &mut parameters,
            "event_type",
            request
                .event_types()
                .iter()
                .map(|event_type| event_type.as_str().to_owned()),
        );
    }

    let parsed = parse_search_query_for(
        request.query().unwrap_or_default(),
        QuerySurface::ChangeTimeline,
    );
    for clause in &parsed.clauses {
        let QueryClause::Field {
            field,
            value,
            negate,
        } = clause
        else {
            continue;
        };
        let (predicate, mut clause_parameters) = field_predicate(field, value);
        predicates.push(if *negate {
            format!("NOT ({predicate})")
        } else {
            predicate
        });
        parameters.append(&mut clause_parameters);
    }
    TimelinePredicate {
        sql: predicates.join(" AND "),
        parameters,
        notices: parsed
            .diagnostics
            .into_iter()
            .map(|diagnostic| diagnostic.message)
            .collect(),
    }
}

fn field_predicate(field: &str, value: &str) -> (String, Vec<Value>) {
    match (field, match_kind_for(field)) {
        ("type", MatchKind::Exact) => {
            let values = value
                .split(',')
                .map(resolve_type_value)
                .map(str::to_owned)
                .collect::<Vec<_>>();
            let placeholders = std::iter::repeat_n("?", values.len())
                .collect::<Vec<_>>()
                .join(", ");
            (
                format!("event_type IN ({placeholders})"),
                values.into_iter().map(Value::from).collect(),
            )
        }
        ("check", MatchKind::Exact) => (
            "lower(coalesce(validation_status, '')) = ?".to_owned(),
            vec![value.to_owned().into()],
        ),
        ("assessment", MatchKind::Exact) => (
            "lower(coalesce(assessment, '')) = ?".to_owned(),
            vec![resolve_assessment_value(value).to_owned().into()],
        ),
        ("track", MatchKind::SetMember) => (
            "lower(coalesce(track_id, '')) = ?".to_owned(),
            vec![value.to_owned().into()],
        ),
        ("actor", MatchKind::SetMember) => (
            "lower(coalesce(actor_id, '')) = ?".to_owned(),
            vec![value.to_owned().into()],
        ),
        ("is", MatchKind::SetMember) => (
            "coalesce(request_state, '') = ?".to_owned(),
            vec![value.to_owned().into()],
        ),
        ("tag", MatchKind::SetMember) => (
            "EXISTS (
                 SELECT 1 FROM product_history_tag_value AS tag
                 WHERE tag.sequence = timeline_event.sequence
                   AND tag.tag_value = ?
             )"
            .to_owned(),
            vec![value.to_owned().into()],
        ),
        ("revision", MatchKind::Substring) => (
            "EXISTS (
                 SELECT 1 FROM product_history_revision_reference AS reference
                 WHERE reference.sequence = timeline_event.sequence
                   AND instr(lower(reference.revision_id), ?) > 0
             )"
            .to_owned(),
            vec![value.to_owned().into()],
        ),
        ("change", MatchKind::Substring) => (
            "EXISTS (
                 SELECT 1 FROM product_history_change_correlation AS correlation
                 WHERE correlation.sequence = timeline_event.sequence
                   AND instr(lower(correlation.change_id), ?) > 0
             )"
            .to_owned(),
            vec![value.to_owned().into()],
        ),
        ("snapshot", MatchKind::Substring) => (
            "EXISTS (
                 SELECT 1 FROM product_history_revision_reference AS reference
                 WHERE reference.sequence = timeline_event.sequence
                   AND reference.resolution = 'exact'
                   AND instr(lower(reference.object_artifact_content_hash), ?) > 0
             )"
            .to_owned(),
            vec![value.to_owned().into()],
        ),
        ("before", MatchKind::RangeBefore) => (
            "lower(normalized_occurred_at) < ?".to_owned(),
            vec![range_bound(value).into()],
        ),
        ("after", MatchKind::RangeAfter) => (
            "lower(normalized_occurred_at) > ?".to_owned(),
            vec![range_bound(value).into()],
        ),
        _ => (
            // The Change Timeline grammar is closed. Keeping an impossible
            // predicate here makes any future field fail closed until its
            // normalized semantics are deliberately added.
            "0 = 1".to_owned(),
            Vec::new(),
        ),
    }
}

fn push_values_predicate(
    predicates: &mut Vec<String>,
    parameters: &mut Vec<Value>,
    column: &str,
    values: impl IntoIterator<Item = String>,
) {
    let values = values.into_iter().collect::<Vec<_>>();
    let placeholders = std::iter::repeat_n("?", values.len())
        .collect::<Vec<_>>()
        .join(", ");
    predicates.push(format!("{column} IN ({placeholders})"));
    parameters.extend(values.into_iter().map(Value::from));
}

#[derive(Clone, Debug)]
struct BodylessTimelineSelection {
    keys: Vec<SelectedHistoryKey>,
    facets: BTreeMap<String, usize>,
    match_count: usize,
    offset: usize,
    match_index: Option<usize>,
    adjacent: DerivedTimelineAdjacentWindowV1,
    query_notices: Vec<String>,
}

fn select_bodyless_timeline(
    connection: &rusqlite::Connection,
    as_of: TruthCursor,
    request: &DerivedTimelinePageRequestV1,
) -> Result<BodylessTimelineSelection, TimelinePageError> {
    let cte = timeline_cte(as_of);
    let page = timeline_predicate(request, true);
    let facet = timeline_predicate(request, false);
    let match_count = query_selected_count(
        connection,
        &cte,
        TIMELINE_RELATION,
        &page.sql,
        &page.parameters,
    )?;
    let facets = query_selected_facets(
        connection,
        &cte,
        TIMELINE_RELATION,
        &facet.sql,
        &facet.parameters,
    )?;
    let descending = request.order() == DerivedTimelineOrderV1::Desc;
    let (offset, match_index) =
        resolve_timeline_offset(connection, &cte, &page, request, descending, match_count)?;
    let keys = query_selected_window(
        connection,
        &cte,
        TIMELINE_RELATION,
        &page.sql,
        &page.parameters,
        descending,
        request.limit(),
        offset,
    )?;
    let adjacent = adjacent_windows(
        connection,
        &cte,
        &page,
        request,
        descending,
        offset,
        match_count,
        &keys,
    )?;
    Ok(BodylessTimelineSelection {
        keys,
        facets,
        match_count,
        offset,
        match_index,
        adjacent,
        query_notices: page.notices,
    })
}

fn resolve_timeline_offset(
    connection: &rusqlite::Connection,
    cte: &str,
    predicate: &TimelinePredicate,
    request: &DerivedTimelinePageRequestV1,
    descending: bool,
    match_count: usize,
) -> Result<(usize, Option<usize>), TimelinePageError> {
    match request.position() {
        DerivedTimelinePagePositionV1::Initial => Ok((0, None)),
        DerivedTimelinePagePositionV1::At(event_id) => {
            let key = query_selected_key(
                connection,
                cte,
                TIMELINE_RELATION,
                &predicate.sql,
                &predicate.parameters,
                event_id.as_str(),
            )?
            .ok_or_else(|| {
                TimelinePageError::RequestInvalid(
                    "Timeline locator does not match this query".to_owned(),
                )
            })?;
            let index = query_selected_index(
                connection,
                cte,
                TIMELINE_RELATION,
                &predicate.sql,
                &predicate.parameters,
                &key,
                descending,
                false,
            )?;
            Ok((index / request.limit() * request.limit(), Some(index)))
        }
        DerivedTimelinePagePositionV1::Continuation {
            boundary: DerivedTimelinePageBoundaryV1::SelectedOrderStart,
            ..
        } => Ok((0, None)),
        DerivedTimelinePagePositionV1::Continuation {
            boundary: DerivedTimelinePageBoundaryV1::Key(boundary),
            ..
        } => {
            let key = query_selected_key(
                connection,
                cte,
                TIMELINE_RELATION,
                &predicate.sql,
                &predicate.parameters,
                boundary.event_id().as_str(),
            )?
            .ok_or_else(|| {
                TimelinePageError::RequestInvalid(
                    "continuation boundary is absent from this Timeline".to_owned(),
                )
            })?;
            let occurred_at = query_raw_occurred_at(
                connection,
                cte,
                &predicate.sql,
                &predicate.parameters,
                boundary.event_id().as_str(),
            )?
            .ok_or_else(|| {
                TimelinePageError::RequestInvalid(
                    "continuation boundary is absent from this Timeline".to_owned(),
                )
            })?;
            if occurred_at != boundary.occurred_at() {
                return Err(TimelinePageError::RequestInvalid(
                    "continuation boundary is absent from this Timeline".to_owned(),
                ));
            }
            let offset = query_selected_index(
                connection,
                cte,
                TIMELINE_RELATION,
                &predicate.sql,
                &predicate.parameters,
                &key,
                descending,
                true,
            )?;
            if offset % request.limit() != 0 {
                return Err(TimelinePageError::RequestInvalid(
                    "continuation boundary is not page-aligned".to_owned(),
                ));
            }
            if offset > match_count {
                return Err(TimelinePageError::RequestInvalid(
                    "continuation boundary is outside this Timeline".to_owned(),
                ));
            }
            Ok((offset, None))
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn adjacent_windows(
    connection: &rusqlite::Connection,
    cte: &str,
    predicate: &TimelinePredicate,
    request: &DerivedTimelinePageRequestV1,
    descending: bool,
    offset: usize,
    match_count: usize,
    keys: &[SelectedHistoryKey],
) -> Result<DerivedTimelineAdjacentWindowV1, String> {
    let previous = if offset == 0 {
        None
    } else {
        let previous_start = offset.saturating_sub(request.limit());
        if previous_start == 0 {
            Some(DerivedTimelinePageBoundaryV1::SelectedOrderStart)
        } else {
            Some(DerivedTimelinePageBoundaryV1::Key(key_at_offset(
                connection,
                cte,
                predicate,
                descending,
                previous_start - 1,
            )?))
        }
    };
    let end = offset.saturating_add(keys.len());
    let next = if end < match_count {
        let key = keys
            .last()
            .ok_or_else(|| "Timeline page has no continuation anchor".to_owned())?;
        Some(DerivedTimelinePageBoundaryV1::Key(timeline_page_key(
            connection, cte, predicate, key,
        )?))
    } else {
        None
    };
    Ok(DerivedTimelineAdjacentWindowV1::new(previous, next))
}

fn key_at_offset(
    connection: &rusqlite::Connection,
    cte: &str,
    predicate: &TimelinePredicate,
    descending: bool,
    offset: usize,
) -> Result<DerivedTimelinePageKeyV1, String> {
    let mut keys = query_selected_window(
        connection,
        cte,
        TIMELINE_RELATION,
        &predicate.sql,
        &predicate.parameters,
        descending,
        1,
        offset,
    )?;
    let key = keys
        .pop()
        .ok_or_else(|| "Timeline adjacent boundary is outside this query".to_owned())?;
    timeline_page_key(connection, cte, predicate, &key)
}

fn timeline_page_key(
    connection: &rusqlite::Connection,
    cte: &str,
    predicate: &TimelinePredicate,
    key: &SelectedHistoryKey,
) -> Result<DerivedTimelinePageKeyV1, String> {
    let occurred_at = query_raw_occurred_at(
        connection,
        cte,
        &predicate.sql,
        &predicate.parameters,
        &key.event_id,
    )?
    .ok_or_else(|| "Timeline adjacent boundary disappeared".to_owned())?;
    DerivedTimelinePageKeyV1::new(occurred_at, EventId::new(key.event_id.clone()))
        .map_err(|error| error.to_string())
}

fn query_raw_occurred_at(
    connection: &rusqlite::Connection,
    cte: &str,
    predicate: &str,
    parameters: &[Value],
    event_id: &str,
) -> Result<Option<String>, String> {
    let sql = format!(
        "{cte}
         SELECT occurred_at FROM {TIMELINE_RELATION}
         WHERE {predicate} AND event_id = ?"
    );
    let mut values = parameters.to_vec();
    values.push(event_id.to_owned().into());
    connection
        .query_row(&sql, rusqlite::params_from_iter(values.iter()), |row| {
            row.get(0)
        })
        .optional()
        .map_err(|error| error.to_string())
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct NormalizedRevisionReference {
    source_kind: String,
    reference_role: String,
    resolution: String,
    revision_id: String,
    object_artifact_content_hash: Option<String>,
    historical_change_eligible: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
struct NormalizedChangeCorrelation {
    change_id: String,
    correlation_role: String,
    source_kind: String,
    source_id: String,
    support_sequence: u64,
}

#[derive(Clone, Debug)]
struct TimelineEventExpectation {
    locator: LocatorExpectation,
    request_state: Option<String>,
    tag_values: BTreeSet<String>,
    revision_references: Vec<NormalizedRevisionReference>,
    correlations: Vec<NormalizedChangeCorrelation>,
}

#[derive(Clone, Debug)]
struct LocatorExpectation {
    cursor: TruthCursor,
    logical_reread_key_hash: String,
    replay_key: String,
    event_id: String,
    normalized_occurred_at: String,
    event_type: String,
    journal_id: String,
    subject_id: Option<String>,
    track_id: Option<String>,
    payload_hash: String,
    validation_witness: String,
}

impl LocatorExpectation {
    fn validate(&self, hydrated: &HydratedLocatorRow) -> Result<(), String> {
        let row = &hydrated.row;
        if row.cursor != self.cursor
            || sha256_bytes_hex(row.logical_reread_key.as_bytes()) != self.logical_reread_key_hash
            || row.replay_key != self.replay_key
            || row.event_id != self.event_id
            || row.normalized_occurred_at != self.normalized_occurred_at
            || row.event_type != self.event_type
            || row.journal_id != self.journal_id
            || row.subject_id != self.subject_id
            || row.track_id != self.track_id
            || row.payload_hash != self.payload_hash
            || row.validation_witness != self.validation_witness
            || hydrated.event.event_id.as_str() != self.event_id
            || hydrated.event.payload_hash != self.payload_hash
        {
            return Err(format!(
                "authoritative Timeline carrier {} differs from its compact locator",
                self.event_id
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Default)]
struct TimelineSupportSelection {
    candidate_proposals: BTreeMap<String, Vec<ProposalCarrierLocator>>,
    correlation_support_event_ids: BTreeSet<String>,
}

#[derive(Clone, Debug)]
struct ValidatedTimelineCarriers {
    selected: Vec<ShoreEvent>,
    selected_expectations: BTreeMap<String, TimelineEventExpectation>,
    primary_by_id: BTreeMap<String, ShoreEvent>,
    primary_sequence_by_id: BTreeMap<String, u64>,
    candidate_bindings: BTreeMap<String, BTreeSet<RevisionRefV1>>,
}

fn load_timeline_expectations(
    connection: &rusqlite::Connection,
    event_ids: &[String],
    as_of: TruthCursor,
) -> Result<BTreeMap<String, TimelineEventExpectation>, String> {
    replace_timeline_lookup(connection, event_ids.iter())?;
    let mut expectations = BTreeMap::new();
    let sql = "SELECT locator.epoch, locator.sequence,
                      receipt.logical_reread_key_hash, locator.replay_key,
                      locator.event_id, locator.normalized_occurred_at,
                      locator.event_type, locator.journal_id, locator.subject_id,
                      locator.track_id, locator.payload_hash,
                      receipt.validation_witness, history.request_state,
                      receipt.epoch
               FROM locator_event_text AS locator
               JOIN cursor_receipt_text AS receipt ON receipt.sequence = locator.sequence
               JOIN product_history_event AS history ON history.sequence = locator.sequence
               JOIN temp.pointbreak_timeline_lookup AS selected
                 ON selected.value = locator.event_id
               WHERE locator.epoch = ?1 AND locator.sequence <= ?2
               ORDER BY locator.sequence";
    let mut statement = connection.prepare(sql).map_err(|error| error.to_string())?;
    let rows = statement
        .query_map(
            [
                to_sql_integer(as_of.epoch)?,
                to_sql_integer(as_of.sequence)?,
            ],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, String>(5)?,
                    row.get::<_, String>(6)?,
                    row.get::<_, String>(7)?,
                    row.get::<_, Option<String>>(8)?,
                    row.get::<_, Option<String>>(9)?,
                    row.get::<_, String>(10)?,
                    row.get::<_, String>(11)?,
                    row.get::<_, Option<String>>(12)?,
                    row.get::<_, i64>(13)?,
                ))
            },
        )
        .map_err(|error| error.to_string())?;
    for row in rows {
        let (
            epoch,
            sequence,
            logical_reread_key_hash,
            replay_key,
            event_id,
            normalized_occurred_at,
            event_type,
            journal_id,
            subject_id,
            track_id,
            payload_hash,
            validation_witness,
            request_state,
            receipt_epoch,
        ) = row.map_err(|error| error.to_string())?;
        let epoch = to_u64(epoch)?;
        let sequence = to_u64(sequence)?;
        if receipt_epoch != i64::try_from(epoch).map_err(|_| "epoch overflow".to_owned())? {
            return Err(format!(
                "Timeline carrier {event_id} has a mismatched cursor receipt"
            ));
        }
        expectations.insert(
            event_id.clone(),
            TimelineEventExpectation {
                locator: LocatorExpectation {
                    cursor: TruthCursor::new(epoch, sequence),
                    logical_reread_key_hash,
                    replay_key,
                    event_id,
                    normalized_occurred_at,
                    event_type,
                    journal_id,
                    subject_id,
                    track_id,
                    payload_hash,
                    validation_witness,
                },
                request_state,
                tag_values: BTreeSet::new(),
                revision_references: Vec::new(),
                correlations: Vec::new(),
            },
        );
    }
    if expectations.len() != event_ids.len() {
        return Err("selected Timeline rows have missing or duplicate compact locators".to_owned());
    }

    let mut statement = connection
        .prepare(
            "SELECT locator.event_id, reference.source_kind,
                    reference.reference_role, reference.resolution,
                    reference.revision_id,
                    reference.object_artifact_content_hash,
                    reference.historical_change_eligible
             FROM product_history_revision_reference AS reference
             JOIN locator_event_text AS locator ON locator.sequence = reference.sequence
             JOIN temp.pointbreak_timeline_lookup AS selected
               ON selected.value = locator.event_id
             ORDER BY locator.sequence, reference.source_kind, reference.revision_id",
        )
        .map_err(|error| error.to_string())?;
    let rows = statement
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                NormalizedRevisionReference {
                    source_kind: row.get(1)?,
                    reference_role: row.get(2)?,
                    resolution: row.get(3)?,
                    revision_id: row.get(4)?,
                    object_artifact_content_hash: row.get(5)?,
                    historical_change_eligible: row.get::<_, i64>(6)? != 0,
                },
            ))
        })
        .map_err(|error| error.to_string())?;
    for row in rows {
        let (event_id, reference) = row.map_err(|error| error.to_string())?;
        expectations
            .get_mut(&event_id)
            .ok_or_else(|| format!("Revision reference selects unknown event {event_id}"))?
            .revision_references
            .push(reference);
    }

    let mut statement = connection
        .prepare(
            "SELECT locator.event_id, correlation.change_id,
                    correlation.correlation_role, correlation.source_kind,
                    correlation.source_id, correlation.support_sequence
             FROM product_history_change_correlation AS correlation
             JOIN locator_event_text AS locator ON locator.sequence = correlation.sequence
             JOIN temp.pointbreak_timeline_lookup AS selected
               ON selected.value = locator.event_id
             ORDER BY locator.sequence, correlation.change_id,
                      correlation.correlation_role, correlation.source_kind,
                      correlation.source_id, correlation.support_sequence",
        )
        .map_err(|error| error.to_string())?;
    let rows = statement
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                NormalizedChangeCorrelation {
                    change_id: row.get(1)?,
                    correlation_role: row.get(2)?,
                    source_kind: row.get(3)?,
                    source_id: row.get(4)?,
                    support_sequence: to_u64_sql(row.get(5)?)?,
                },
            ))
        })
        .map_err(|error| error.to_string())?;
    for row in rows {
        let (event_id, correlation) = row.map_err(|error| error.to_string())?;
        expectations
            .get_mut(&event_id)
            .ok_or_else(|| format!("Change correlation selects unknown event {event_id}"))?
            .correlations
            .push(correlation);
    }

    let mut statement = connection
        .prepare(
            "SELECT locator.event_id, tag.tag_value
             FROM product_history_tag_value AS tag
             JOIN locator_event_text AS locator ON locator.sequence = tag.sequence
             JOIN temp.pointbreak_timeline_lookup AS selected
               ON selected.value = locator.event_id
             ORDER BY locator.sequence, tag.tag_value",
        )
        .map_err(|error| error.to_string())?;
    let rows = statement
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })
        .map_err(|error| error.to_string())?;
    for row in rows {
        let (event_id, tag) = row.map_err(|error| error.to_string())?;
        expectations
            .get_mut(&event_id)
            .ok_or_else(|| format!("Timeline tag selects unknown event {event_id}"))?
            .tag_values
            .insert(tag);
    }
    Ok(expectations)
}

fn select_first_level_support(
    connection: &rusqlite::Connection,
    expectations: &BTreeMap<String, TimelineEventExpectation>,
    as_of: TruthCursor,
) -> Result<TimelineSupportSelection, String> {
    let candidate_revision_ids = expectations
        .values()
        .flat_map(|expectation| &expectation.revision_references)
        .filter(|reference| reference.reference_role == "candidate")
        .map(|reference| reference.revision_id.clone())
        .collect::<BTreeSet<_>>();
    let candidate_proposals =
        query_candidate_proposal_locators(connection, &candidate_revision_ids, as_of)?;
    let support_sequences = expectations
        .values()
        .flat_map(|expectation| &expectation.correlations)
        .map(|correlation| correlation.support_sequence)
        .collect::<BTreeSet<_>>();
    let correlation_support_event_ids =
        event_ids_for_sequences(connection, &support_sequences, as_of)?;
    Ok(TimelineSupportSelection {
        candidate_proposals,
        correlation_support_event_ids,
    })
}

fn query_candidate_proposal_locators(
    connection: &rusqlite::Connection,
    revision_ids: &BTreeSet<String>,
    as_of: TruthCursor,
) -> Result<BTreeMap<String, Vec<ProposalCarrierLocator>>, String> {
    if revision_ids.is_empty() {
        return Ok(BTreeMap::new());
    }
    replace_timeline_lookup(connection, revision_ids.iter())?;
    let mut statement = connection
        .prepare(
            "SELECT locator.epoch, locator.sequence,
                    receipt.logical_reread_key_hash, locator.replay_key,
                    locator.event_id, locator.event_type, locator.payload_hash,
                    receipt.validation_witness, proposal.revision_id,
                    proposal.object_artifact_content_hash, receipt.epoch
             FROM semantic_revision_proposal_carrier AS proposal
             JOIN locator_event_text AS locator ON locator.sequence = proposal.sequence
             JOIN cursor_receipt_text AS receipt ON receipt.sequence = proposal.sequence
             JOIN temp.pointbreak_timeline_lookup AS selected
               ON selected.value = proposal.revision_id
             WHERE locator.epoch = ?1 AND locator.sequence <= ?2
             ORDER BY proposal.revision_id, locator.sequence",
        )
        .map_err(|error| error.to_string())?;
    let rows = statement
        .query_map(
            [
                to_sql_integer(as_of.epoch)?,
                to_sql_integer(as_of.sequence)?,
            ],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, String>(5)?,
                    row.get::<_, String>(6)?,
                    row.get::<_, String>(7)?,
                    row.get::<_, String>(8)?,
                    row.get::<_, String>(9)?,
                    row.get::<_, i64>(10)?,
                ))
            },
        )
        .map_err(|error| error.to_string())?;
    let mut grouped = revision_ids
        .iter()
        .cloned()
        .map(|revision_id| (revision_id, Vec::new()))
        .collect::<BTreeMap<_, _>>();
    for row in rows {
        let (
            epoch,
            sequence,
            logical_reread_key_hash,
            replay_key,
            event_id,
            event_type,
            payload_hash,
            validation_witness,
            revision_id,
            artifact_hash,
            receipt_epoch,
        ) = row.map_err(|error| error.to_string())?;
        let epoch = to_u64(epoch)?;
        if receipt_epoch != i64::try_from(epoch).map_err(|_| "epoch overflow".to_owned())? {
            return Err(format!(
                "proposal carrier {event_id} has a mismatched receipt epoch"
            ));
        }
        let revision = RevisionRefV1::new(RevisionId::new(revision_id.clone()), artifact_hash)
            .map_err(|error| error.to_string())?;
        grouped
            .get_mut(&revision_id)
            .ok_or_else(|| format!("proposal carrier selects unexpected Revision {revision_id}"))?
            .push(ProposalCarrierLocator {
                cursor: TruthCursor::new(epoch, to_u64(sequence)?),
                logical_reread_key_hash,
                replay_key,
                event_id: EventId::new(event_id),
                event_type,
                payload_hash,
                validation_witness,
                revision,
            });
    }
    Ok(grouped)
}

fn event_ids_for_sequences(
    connection: &rusqlite::Connection,
    sequences: &BTreeSet<u64>,
    as_of: TruthCursor,
) -> Result<BTreeSet<String>, String> {
    if sequences.is_empty() {
        return Ok(BTreeSet::new());
    }
    connection
        .execute_batch(
            "CREATE TEMP TABLE IF NOT EXISTS pointbreak_timeline_sequence_lookup (
                 value INTEGER PRIMARY KEY
             ) STRICT, WITHOUT ROWID;
             DELETE FROM temp.pointbreak_timeline_sequence_lookup;",
        )
        .map_err(|error| error.to_string())?;
    let mut insert = connection
        .prepare("INSERT INTO temp.pointbreak_timeline_sequence_lookup (value) VALUES (?1)")
        .map_err(|error| error.to_string())?;
    for sequence in sequences {
        insert
            .execute([to_sql_integer(*sequence)?])
            .map_err(|error| error.to_string())?;
    }
    drop(insert);
    let mut statement = connection
        .prepare(
            "SELECT locator.event_id
             FROM locator_event_text AS locator
             JOIN temp.pointbreak_timeline_sequence_lookup AS selected
               ON selected.value = locator.sequence
             WHERE locator.epoch = ?1 AND locator.sequence <= ?2
             ORDER BY locator.event_id",
        )
        .map_err(|error| error.to_string())?;
    let rows = statement
        .query_map(
            [
                to_sql_integer(as_of.epoch)?,
                to_sql_integer(as_of.sequence)?,
            ],
            |row| row.get::<_, String>(0),
        )
        .map_err(|error| error.to_string())?;
    let event_ids = rows
        .collect::<Result<BTreeSet<_>, _>>()
        .map_err(|error| error.to_string())?;
    if event_ids.len() != sequences.len() {
        return Err("Timeline correlation support sequence is absent".to_owned());
    }
    Ok(event_ids)
}

fn replace_timeline_lookup<'a>(
    connection: &rusqlite::Connection,
    values: impl IntoIterator<Item = &'a String>,
) -> Result<(), String> {
    connection
        .execute_batch(
            "CREATE TEMP TABLE IF NOT EXISTS pointbreak_timeline_lookup (
                 value TEXT PRIMARY KEY
             ) STRICT, WITHOUT ROWID;
             DELETE FROM temp.pointbreak_timeline_lookup;",
        )
        .map_err(|error| error.to_string())?;
    let mut insert = connection
        .prepare("INSERT INTO temp.pointbreak_timeline_lookup (value) VALUES (?1)")
        .map_err(|error| error.to_string())?;
    for value in values {
        insert.execute([value]).map_err(|error| error.to_string())?;
    }
    Ok(())
}

fn hydrate_validated(
    service: &DerivedAccessService,
    connection: &rusqlite::Connection,
    event_ids: &[String],
    as_of: TruthCursor,
) -> Result<Vec<HydratedLocatorRow>, TimelinePageError> {
    let expectations = query_locator_expectations(connection, event_ids, as_of)?;
    let hydrated = match service
        .semantic_ids_hydrated_at(event_ids, as_of)
        .map_err(|error| error.to_string())?
    {
        LocatorRead::Ready(rows) => rows,
        LocatorRead::CatchUpRequired { .. } => {
            return Err(TimelinePageError::Stale(
                "derived Timeline became stale during carrier hydration".to_owned(),
            ));
        }
    };
    if hydrated.len() != event_ids.len() {
        return Err(TimelinePageError::Invalid(
            "authoritative Timeline hydration returned the wrong carrier count".to_owned(),
        ));
    }
    event_ids
        .iter()
        .zip(hydrated)
        .map(|(event_id, hydrated)| {
            let hydrated = hydrated.ok_or_else(|| {
                format!("selected authoritative Timeline carrier {event_id} is absent")
            })?;
            expectations
                .get(event_id)
                .ok_or_else(|| format!("compact Timeline locator {event_id} is absent"))?
                .validate(&hydrated)?;
            Ok(hydrated)
        })
        .collect::<Result<Vec<_>, String>>()
        .map_err(TimelinePageError::Invalid)
}

fn validate_timeline_carriers(
    service: &DerivedAccessService,
    connection: &rusqlite::Connection,
    selected_event_ids: &[String],
    as_of: TruthCursor,
    hook: &mut impl FnMut(TimelineReadBoundary),
) -> Result<ValidatedTimelineCarriers, TimelinePageError> {
    let selected_expectations = load_timeline_expectations(connection, selected_event_ids, as_of)?;
    hook(TimelineReadBoundary::SupportExpansionStarted);
    let support = select_first_level_support(connection, &selected_expectations, as_of)?;
    let mut primary_event_ids = selected_event_ids.iter().cloned().collect::<BTreeSet<_>>();
    primary_event_ids.extend(support.correlation_support_event_ids.iter().cloned());
    primary_event_ids.extend(
        support
            .candidate_proposals
            .values()
            .flatten()
            .map(|locator| locator.event_id.as_str().to_owned()),
    );
    #[cfg(any(test, feature = "longitudinal-counting"))]
    {
        let selected = selected_event_ids.iter().cloned().collect::<BTreeSet<_>>();
        let candidates = support
            .candidate_proposals
            .values()
            .flatten()
            .map(|locator| locator.event_id.as_str().to_owned())
            .collect::<BTreeSet<_>>();
        let candidate_only = candidates
            .difference(&selected)
            .cloned()
            .collect::<BTreeSet<_>>();
        let selected_or_candidate = selected
            .union(&candidates)
            .cloned()
            .collect::<BTreeSet<_>>();
        let correlation_only = support
            .correlation_support_event_ids
            .difference(&selected_or_candidate)
            .count();
        record_timeline_selected_carriers(selected.len());
        record_timeline_revision_candidate_carriers(candidate_only.len());
        record_timeline_correlation_support_carriers(correlation_only);
    }
    let primary_event_ids = primary_event_ids.into_iter().collect::<Vec<_>>();
    hook(TimelineReadBoundary::CarrierLocatorsSelected);
    let primary_hydrated = hydrate_validated(service, connection, &primary_event_ids, as_of)?;
    hook(TimelineReadBoundary::CarrierHydrationMidpoint);
    let primary_by_id = primary_hydrated
        .iter()
        .map(|hydrated| {
            (
                hydrated.event.event_id.as_str().to_owned(),
                hydrated.event.clone(),
            )
        })
        .collect::<BTreeMap<_, _>>();
    let primary_sequence_by_id = primary_hydrated
        .iter()
        .map(|hydrated| {
            (
                hydrated.event.event_id.as_str().to_owned(),
                hydrated.row.cursor.sequence,
            )
        })
        .collect::<BTreeMap<_, _>>();
    let primary_locator_by_id = primary_hydrated
        .iter()
        .map(|hydrated| (hydrated.event.event_id.as_str().to_owned(), &hydrated.row))
        .collect::<BTreeMap<_, _>>();

    let mut candidate_bindings = BTreeMap::new();
    for (revision_id, locators) in &support.candidate_proposals {
        let mut bindings = BTreeSet::new();
        for locator in locators {
            let event_id = locator.event_id.as_str();
            let event = primary_by_id
                .get(event_id)
                .ok_or_else(|| format!("candidate proposal carrier {event_id} is absent"))?;
            let row = primary_locator_by_id
                .get(event_id)
                .ok_or_else(|| format!("candidate proposal locator {event_id} is absent"))?;
            if row.cursor != locator.cursor
                || sha256_bytes_hex(row.logical_reread_key.as_bytes())
                    != locator.logical_reread_key_hash
                || row.replay_key != locator.replay_key
                || row.event_type != locator.event_type
                || row.payload_hash != locator.payload_hash
                || row.validation_witness != locator.validation_witness
            {
                return Err(format!(
                    "candidate proposal carrier {event_id} differs from its compact Revision locator"
                )
                .into());
            }
            let actual = super::changes::exact_revision_from_proposal(event)
                .map_err(|error| error.to_string())?;
            if actual != locator.revision || actual.revision_id.as_str() != revision_id {
                return Err(format!(
                    "candidate proposal carrier {event_id} has the wrong exact Revision binding"
                )
                .into());
            }
            bindings.insert(actual);
        }
        candidate_bindings.insert(revision_id.clone(), bindings);
    }

    let primary_events = primary_hydrated
        .iter()
        .map(|hydrated| hydrated.event.clone())
        .collect::<Vec<_>>();
    let support_plan = support_event_plan(connection, &primary_events, as_of)?;
    let secondary_event_ids = support_plan
        .removal_event_ids
        .iter()
        .chain(&support_plan.signature_event_ids)
        .filter(|event_id| !primary_by_id.contains_key(*event_id))
        .cloned()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    #[cfg(any(test, feature = "longitudinal-counting"))]
    {
        let primary = primary_by_id.keys().cloned().collect::<BTreeSet<_>>();
        let removal_only = support_plan
            .removal_event_ids
            .iter()
            .filter(|event_id| !primary.contains(*event_id))
            .cloned()
            .collect::<BTreeSet<_>>();
        let primary_or_removal = primary
            .union(&removal_only)
            .cloned()
            .collect::<BTreeSet<_>>();
        let signature_only = support_plan
            .signature_event_ids
            .iter()
            .filter(|event_id| !primary_or_removal.contains(*event_id))
            .count();
        record_timeline_removal_support_carriers(removal_only.len());
        record_timeline_signature_support_carriers(signature_only);
    }
    let secondary_hydrated = hydrate_validated(service, connection, &secondary_event_ids, as_of)?;
    validate_signature_support(
        connection,
        &support_plan,
        &primary_by_id,
        &secondary_hydrated,
        as_of,
    )?;

    let selected = selected_event_ids
        .iter()
        .map(|event_id| {
            primary_by_id
                .get(event_id)
                .cloned()
                .ok_or_else(|| format!("selected Timeline carrier {event_id} is absent"))
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(ValidatedTimelineCarriers {
        selected,
        selected_expectations,
        primary_by_id,
        primary_sequence_by_id,
        candidate_bindings,
    })
}

fn validate_signature_support(
    connection: &rusqlite::Connection,
    support_plan: &SupportEventPlan,
    primary_by_id: &BTreeMap<String, ShoreEvent>,
    secondary: &[HydratedLocatorRow],
    as_of: TruthCursor,
) -> Result<(), String> {
    let secondary_by_id = secondary
        .iter()
        .map(|hydrated| {
            (
                hydrated.event.event_id.as_str().to_owned(),
                (&hydrated.event, hydrated.row.cursor.sequence),
            )
        })
        .collect::<BTreeMap<_, _>>();
    let valid_targets = primary_by_id
        .keys()
        .chain(support_plan.removal_event_ids.iter())
        .cloned()
        .collect::<BTreeSet<_>>();
    if support_plan.signature_event_ids.is_empty() {
        return Ok(());
    }
    replace_timeline_lookup(connection, support_plan.signature_event_ids.iter())?;
    let mut statement = connection
        .prepare(
            "SELECT locator.event_id, locator.sequence, signature.target_event_id
             FROM product_history_signature AS signature
             JOIN locator_event_text AS locator ON locator.sequence = signature.sequence
             JOIN temp.pointbreak_timeline_lookup AS selected
               ON selected.value = locator.event_id
             WHERE locator.epoch = ?1 AND locator.sequence <= ?2
             ORDER BY locator.event_id",
        )
        .map_err(|error| error.to_string())?;
    let rows = statement
        .query_map(
            [
                to_sql_integer(as_of.epoch)?,
                to_sql_integer(as_of.sequence)?,
            ],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    to_u64_sql(row.get(1)?)?,
                    row.get::<_, String>(2)?,
                ))
            },
        )
        .map_err(|error| error.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())?;
    if rows.len() != support_plan.signature_event_ids.len() {
        return Err("Timeline signature support row is absent".to_owned());
    }
    for (event_id, sequence, target_event_id) in rows {
        if !valid_targets.contains(&target_event_id) {
            return Err(format!(
                "Timeline signature carrier {event_id} targets an unselected event"
            ));
        }
        let (event, hydrated_sequence) = secondary_by_id
            .get(&event_id)
            .copied()
            .or_else(|| {
                primary_by_id.get(&event_id).map(|event| {
                    // A signature event cannot ordinarily be a selected Timeline
                    // row, but a duplicate support identity still validates
                    // through its normalized sequence below.
                    (event, sequence)
                })
            })
            .ok_or_else(|| format!("Timeline signature carrier {event_id} is absent"))?;
        if hydrated_sequence != sequence {
            return Err(format!("Timeline signature carrier {event_id} moved"));
        }
        let fact =
            ProductHistoryFact::from_event(sequence, event).map_err(|error| error.to_string())?;
        if fact.signature_target_event_id.as_deref() != Some(target_event_id.as_str()) {
            return Err(format!(
                "Timeline signature carrier {event_id} has the wrong target"
            ));
        }
    }
    Ok(())
}

fn validate_selected_relations(
    carriers: &ValidatedTimelineCarriers,
    drafts: &[EventHistoryEntryDraftV1],
) -> Result<(), String> {
    if carriers.selected.len() != drafts.len() {
        return Err("Timeline projection omitted a selected product event".to_owned());
    }
    let support_by_sequence = carriers
        .primary_sequence_by_id
        .iter()
        .filter_map(|(event_id, sequence)| {
            carriers
                .primary_by_id
                .get(event_id)
                .map(|event| (*sequence, event))
        })
        .collect::<BTreeMap<_, _>>();

    for draft in drafts {
        let event_id = draft.event().event_id.as_str();
        let expectation = carriers
            .selected_expectations
            .get(event_id)
            .ok_or_else(|| format!("projected Timeline event {event_id} was not selected"))?;
        let fact =
            ProductHistoryFact::from_event(expectation.locator.cursor.sequence, draft.event())
                .map_err(|error| error.to_string())?;
        let timeline = fact.timeline.as_ref().ok_or_else(|| {
            format!("selected Timeline event {event_id} has no Timeline product fact")
        })?;
        if timeline.request_state.map(str::to_owned) != expectation.request_state
            || fact.tag_values.iter().cloned().collect::<BTreeSet<_>>() != expectation.tag_values
        {
            return Err(format!(
                "selected Timeline event {event_id} differs from its normalized query fields"
            ));
        }

        let inherited_claim_id = fact
            .membership_withdrawal_claim_id
            .as_deref()
            .or(fact.relation_withdrawal_claim_id.as_deref());
        let correlation_support = expectation
            .correlations
            .iter()
            .map(|correlation| {
                let support = support_by_sequence
                    .get(&correlation.support_sequence)
                    .copied()
                    .ok_or_else(|| {
                        format!(
                            "selected Timeline event {event_id} lacks Change correlation support {}",
                            correlation.source_id
                        )
                    })?;
                let support_fact =
                    ProductHistoryFact::from_event(correlation.support_sequence, support)
                        .map_err(|error| error.to_string())?;
                let supports_tuple = support_fact.timeline.as_ref().is_some_and(|timeline| {
                    timeline.direct_changes.iter().any(|change| {
                        change.change_id == correlation.change_id
                            && change.source_kind == correlation.source_kind
                            && change.source_id == correlation.source_id
                    })
                });
                if !supports_tuple
                    || (correlation.correlation_role == "direct"
                        && correlation.support_sequence != expectation.locator.cursor.sequence
                        && inherited_claim_id != Some(correlation.source_id.as_str()))
                {
                    return Err(format!(
                        "selected Timeline event {event_id} has invalid Change correlation support {}",
                        correlation.source_id
                    ));
                }
                Ok((correlation, support_fact))
            })
            .collect::<Result<Vec<_>, String>>()?;

        let inherited_timelines = correlation_support
            .iter()
            .filter(|(correlation, _)| {
                inherited_claim_id == Some(correlation.source_id.as_str())
                    && correlation.support_sequence != expectation.locator.cursor.sequence
            })
            .filter_map(|(_, support)| support.timeline.as_ref())
            .collect::<Vec<_>>();
        let effective_revision_references = timeline
            .revision_references
            .iter()
            .chain(
                inherited_timelines
                    .iter()
                    .flat_map(|timeline| timeline.revision_references.iter()),
            )
            .collect::<Vec<_>>();

        let fact_reference_shape = effective_revision_references
            .iter()
            .map(|reference| {
                (
                    reference.source_kind,
                    reference.reference_role,
                    reference.revision_id.as_str(),
                    reference.historical_change_eligible,
                )
            })
            .collect::<BTreeSet<_>>();
        let normalized_reference_shape = expectation
            .revision_references
            .iter()
            .map(|reference| {
                (
                    reference.source_kind.as_str(),
                    reference.reference_role.as_str(),
                    reference.revision_id.as_str(),
                    reference.historical_change_eligible,
                )
            })
            .collect::<BTreeSet<_>>();
        if fact_reference_shape != normalized_reference_shape {
            return Err(format!(
                "selected Timeline event {event_id} has mismatched normalized Revision roles"
            ));
        }
        let mut exact = BTreeSet::new();
        let mut unresolved = BTreeSet::new();
        for reference in &expectation.revision_references {
            let fact_reference = effective_revision_references
                .iter()
                .find(|candidate| {
                    candidate.source_kind == reference.source_kind
                        && candidate.revision_id == reference.revision_id
                })
                .ok_or_else(|| {
                    format!("selected Timeline event {event_id} lost a Revision source")
                })?;
            let expected_binding = if reference.reference_role == "direct" {
                fact_reference
                    .object_artifact_content_hash
                    .as_ref()
                    .map(|hash| {
                        RevisionRefV1::new(
                            RevisionId::new(reference.revision_id.clone()),
                            hash.clone(),
                        )
                        .map_err(|error| error.to_string())
                    })
                    .transpose()?
            } else {
                let bindings = carriers
                    .candidate_bindings
                    .get(&reference.revision_id)
                    .cloned()
                    .unwrap_or_default();
                (bindings.len() == 1)
                    .then(|| bindings.into_iter().next().expect("singleton binding"))
            };
            match expected_binding {
                Some(binding) => {
                    if reference.resolution != "exact"
                        || reference.object_artifact_content_hash.as_deref()
                            != Some(binding.object_artifact_content_hash.as_str())
                    {
                        return Err(format!(
                            "selected Timeline event {event_id} has an unproved exact Revision"
                        ));
                    }
                    exact.insert(binding);
                }
                None => {
                    if reference.resolution != "unresolved"
                        || reference.object_artifact_content_hash.is_some()
                    {
                        return Err(format!(
                            "selected Timeline event {event_id} hides an ambiguous Revision binding"
                        ));
                    }
                    unresolved.insert(RevisionId::new(reference.revision_id.clone()));
                }
            }
        }
        if draft
            .entry()
            .revision_refs
            .iter()
            .cloned()
            .collect::<BTreeSet<_>>()
            != exact
            || draft
                .entry()
                .unresolved_revision_ids
                .iter()
                .cloned()
                .collect::<BTreeSet<_>>()
                != unresolved
        {
            return Err(format!(
                "selected Timeline event {event_id} projection disagrees with normalized Revision resolution"
            ));
        }

        let direct_changes = timeline
            .direct_changes
            .iter()
            .chain(
                inherited_timelines
                    .iter()
                    .flat_map(|timeline| timeline.direct_changes.iter()),
            )
            .map(|change| {
                (
                    change.change_id.as_str(),
                    change.source_kind,
                    change.source_id.as_str(),
                )
            })
            .collect::<BTreeSet<_>>();
        let normalized_direct = expectation
            .correlations
            .iter()
            .filter(|correlation| correlation.correlation_role == "direct")
            .map(|correlation| {
                (
                    correlation.change_id.as_str(),
                    correlation.source_kind.as_str(),
                    correlation.source_id.as_str(),
                )
            })
            .collect::<BTreeSet<_>>();
        if direct_changes != normalized_direct {
            return Err(format!(
                "selected Timeline event {event_id} has mismatched direct Change correlations"
            ));
        }
        let normalized_changes = expectation
            .correlations
            .iter()
            .map(|correlation| correlation.change_id.as_str())
            .collect::<BTreeSet<_>>();
        let projected_changes = draft
            .entry()
            .change_ids
            .iter()
            .map(|change| change.as_str())
            .collect::<BTreeSet<_>>();
        if normalized_changes != projected_changes {
            return Err(format!(
                "selected Timeline event {event_id} projection disagrees with normalized Change correlation"
            ));
        }
    }
    Ok(())
}

fn query_locator_expectations(
    connection: &rusqlite::Connection,
    event_ids: &[String],
    as_of: TruthCursor,
) -> Result<BTreeMap<String, LocatorExpectation>, String> {
    replace_timeline_lookup(connection, event_ids.iter())?;
    let mut statement = connection
        .prepare(
            "SELECT locator.epoch, locator.sequence,
                    receipt.logical_reread_key_hash, locator.replay_key,
                    locator.event_id, locator.normalized_occurred_at,
                    locator.event_type, locator.journal_id, locator.subject_id,
                    locator.track_id, locator.payload_hash,
                    receipt.validation_witness, receipt.epoch
             FROM locator_event_text AS locator
             JOIN cursor_receipt_text AS receipt ON receipt.sequence = locator.sequence
             JOIN temp.pointbreak_timeline_lookup AS selected
               ON selected.value = locator.event_id
             WHERE locator.epoch = ?1 AND locator.sequence <= ?2
             ORDER BY locator.event_id",
        )
        .map_err(|error| error.to_string())?;
    let rows = statement
        .query_map(
            [
                to_sql_integer(as_of.epoch)?,
                to_sql_integer(as_of.sequence)?,
            ],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, String>(5)?,
                    row.get::<_, String>(6)?,
                    row.get::<_, String>(7)?,
                    row.get::<_, Option<String>>(8)?,
                    row.get::<_, Option<String>>(9)?,
                    row.get::<_, String>(10)?,
                    row.get::<_, String>(11)?,
                    row.get::<_, i64>(12)?,
                ))
            },
        )
        .map_err(|error| error.to_string())?;
    let mut expectations = BTreeMap::new();
    for row in rows {
        let (
            epoch,
            sequence,
            logical_reread_key_hash,
            replay_key,
            event_id,
            normalized_occurred_at,
            event_type,
            journal_id,
            subject_id,
            track_id,
            payload_hash,
            validation_witness,
            receipt_epoch,
        ) = row.map_err(|error| error.to_string())?;
        if receipt_epoch != epoch {
            return Err(format!(
                "Timeline carrier {event_id} has a mismatched receipt epoch"
            ));
        }
        expectations.insert(
            event_id.clone(),
            LocatorExpectation {
                cursor: TruthCursor::new(to_u64(epoch)?, to_u64(sequence)?),
                logical_reread_key_hash,
                replay_key,
                event_id,
                normalized_occurred_at,
                event_type,
                journal_id,
                subject_id,
                track_id,
                payload_hash,
                validation_witness,
            },
        );
    }
    if expectations.len() != event_ids.len() {
        return Err("Timeline support has missing or duplicate compact locators".to_owned());
    }
    Ok(expectations)
}

fn to_sql_integer(value: u64) -> Result<i64, String> {
    i64::try_from(value).map_err(|_| "Timeline cursor does not fit SQLite INTEGER".to_owned())
}

fn to_u64(value: i64) -> Result<u64, String> {
    u64::try_from(value).map_err(|_| "negative Timeline cursor".to_owned())
}

fn to_u64_sql(value: i64) -> rusqlite::Result<u64> {
    u64::try_from(value).map_err(|error| {
        rusqlite::Error::FromSqlConversionFailure(
            0,
            rusqlite::types::Type::Integer,
            Box::new(error),
        )
    })
}

#[derive(Clone, Debug)]
struct TimelineGlobalProjection {
    completion: EventHistoryCompletionV1,
    diagnostics: Vec<String>,
}

fn query_global_projection(
    connection: &rusqlite::Connection,
    as_of: TruthCursor,
) -> Result<TimelineGlobalProjection, String> {
    let cte = timeline_cte(as_of);
    let event_types = query_strings(
        connection,
        &format!(
            "{cte}
             SELECT event_type
             FROM {TIMELINE_RELATION}
             GROUP BY event_type
             ORDER BY min(normalized_occurred_at || '|' || event_id)"
        ),
    )?
    .into_iter()
    .map(|wire| {
        serde_json::from_value::<EventType>(serde_json::Value::String(wire))
            .map_err(|error| error.to_string())
    })
    .collect::<Result<Vec<_>, _>>()?;
    let track_ids = query_strings(
        connection,
        &format!(
            "{cte}
             SELECT DISTINCT track_id
             FROM {TIMELINE_RELATION}
             WHERE track_id IS NOT NULL
             ORDER BY track_id"
        ),
    )?
    .into_iter()
    .map(TrackId::new)
    .collect();
    let change_ids = query_strings(
        connection,
        &format!(
            "SELECT DISTINCT correlation.change_id
             FROM product_history_change_correlation AS correlation
             JOIN locator_event_text AS locator ON locator.sequence = correlation.sequence
             WHERE locator.epoch = {} AND locator.sequence <= {}
             ORDER BY correlation.change_id",
            as_of.epoch, as_of.sequence
        ),
    )?
    .into_iter()
    .map(ChangeId::new)
    .collect();
    let mut statement = connection
        .prepare(&format!(
            "SELECT DISTINCT reference.revision_id,
                    reference.object_artifact_content_hash
             FROM product_history_revision_reference AS reference
             JOIN locator_event_text AS locator ON locator.sequence = reference.sequence
             WHERE locator.epoch = {} AND locator.sequence <= {}
               AND reference.resolution = 'exact'
             ORDER BY reference.revision_id,
                      reference.object_artifact_content_hash",
            as_of.epoch, as_of.sequence
        ))
        .map_err(|error| error.to_string())?;
    let revision_refs = statement
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })
        .map_err(|error| error.to_string())?
        .map(|row| {
            let (revision_id, artifact_hash) = row.map_err(|error| error.to_string())?;
            RevisionRefV1::new(RevisionId::new(revision_id), artifact_hash)
                .map_err(|error| error.to_string())
        })
        .collect::<Result<Vec<_>, _>>()?;
    let unresolved_revision_ids = query_strings(
        connection,
        &format!(
            "SELECT DISTINCT reference.revision_id
             FROM product_history_revision_reference AS reference
             JOIN locator_event_text AS locator ON locator.sequence = reference.sequence
             WHERE locator.epoch = {} AND locator.sequence <= {}
               AND reference.resolution = 'unresolved'
             ORDER BY reference.revision_id",
            as_of.epoch, as_of.sequence
        ),
    )?
    .into_iter()
    .map(RevisionId::new)
    .collect();
    let diagnostics = query_strings(
        connection,
        &format!(
            "SELECT diagnostic FROM (
                 SELECT 'event_history_membership_claim_missing:' || withdrawal.claim_id
                            AS diagnostic
                 FROM product_history_membership_withdrawal AS withdrawal
                 JOIN locator_event_text AS locator ON locator.sequence = withdrawal.sequence
                 LEFT JOIN product_history_membership_claim AS claim
                   ON claim.claim_id = withdrawal.claim_id
                 WHERE locator.epoch = {} AND locator.sequence <= {}
                   AND claim.sequence IS NULL
                 UNION
                 SELECT 'event_history_relation_claim_missing:' || withdrawal.claim_id
                            AS diagnostic
                 FROM product_history_relation_withdrawal AS withdrawal
                 JOIN locator_event_text AS locator ON locator.sequence = withdrawal.sequence
                 LEFT JOIN product_history_relation_claim AS claim
                   ON claim.claim_id = withdrawal.claim_id
                 WHERE locator.epoch = {} AND locator.sequence <= {}
                   AND claim.sequence IS NULL
             ) ORDER BY diagnostic",
            as_of.epoch, as_of.sequence, as_of.epoch, as_of.sequence
        ),
    )?;
    Ok(TimelineGlobalProjection {
        completion: EventHistoryCompletionV1 {
            event_types,
            track_ids,
            change_ids,
            revision_refs,
            unresolved_revision_ids,
        },
        diagnostics,
    })
}

fn query_strings(connection: &rusqlite::Connection, sql: &str) -> Result<Vec<String>, String> {
    let mut statement = connection.prepare(sql).map_err(|error| error.to_string())?;
    let rows = statement
        .query_map([], |row| row.get::<_, String>(0))
        .map_err(|error| error.to_string())?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())
}

#[allow(clippy::too_many_arguments)]
pub(super) fn prepare_timeline_page(
    service: &DerivedAccessService,
    connection: &rusqlite::Connection,
    change_projection: &crate::session::ChangeDocumentProjectionV1,
    as_of: TruthCursor,
    authority_cursor: AuthorityCursorV2,
    source_change_projection_stamp: String,
    timeline_projection_stamp: String,
    request: &DerivedTimelinePageRequestV1,
    trust_set: &TrustSet,
    hook: &mut impl FnMut(TimelineReadBoundary),
) -> Result<DerivedTimelinePageV1, TimelinePageError> {
    let global = query_global_projection(connection, as_of)?;
    match classify_query(request) {
        TimelineQueryClass::Bodyless => prepare_bodyless_page(
            service,
            connection,
            change_projection,
            as_of,
            authority_cursor,
            source_change_projection_stamp,
            timeline_projection_stamp,
            request,
            trust_set,
            global,
            hook,
        ),
        TimelineQueryClass::Exhaustive => prepare_exhaustive_page(
            service,
            connection,
            change_projection,
            as_of,
            authority_cursor,
            source_change_projection_stamp,
            timeline_projection_stamp,
            request,
            trust_set,
            global,
            hook,
        ),
    }
}

#[allow(clippy::too_many_arguments)]
fn prepare_bodyless_page(
    service: &DerivedAccessService,
    connection: &rusqlite::Connection,
    change_projection: &crate::session::ChangeDocumentProjectionV1,
    as_of: TruthCursor,
    authority_cursor: AuthorityCursorV2,
    source_change_projection_stamp: String,
    timeline_projection_stamp: String,
    request: &DerivedTimelinePageRequestV1,
    trust_set: &TrustSet,
    global: TimelineGlobalProjection,
    hook: &mut impl FnMut(TimelineReadBoundary),
) -> Result<DerivedTimelinePageV1, TimelinePageError> {
    let selection = select_bodyless_timeline(connection, as_of, request)?;
    #[cfg(any(test, feature = "longitudinal-counting"))]
    {
        record_timeline_sqlite_candidates(selection.match_count);
        record_timeline_sqlite_window_rows(selection.keys.len());
        record_timeline_sqlite_facet_rows(selection.facets.values().sum());
    }
    let event_ids = selection
        .keys
        .iter()
        .map(|key| key.event_id.clone())
        .collect::<Vec<_>>();
    let carriers = validate_timeline_carriers(service, connection, &event_ids, as_of, hook)?;
    let (drafts, selected_diagnostics) =
        project_selected_event_history_without_trust(&carriers.selected, change_projection)
            .map_err(|error| error.to_string())?;
    if !selected_diagnostics
        .iter()
        .all(|diagnostic| global.diagnostics.contains(diagnostic))
    {
        return Err(
            "selected Timeline diagnostics disagree with normalized history"
                .to_owned()
                .into(),
        );
    }
    validate_selected_relations(&carriers, &drafts)?;
    #[cfg(any(test, feature = "longitudinal-counting"))]
    record_timeline_trust_support_carriers(drafts.len());
    let entries =
        bind_selected_event_history_trust(drafts, trust_set).map_err(|error| error.to_string())?;
    hook(TimelineReadBoundary::TrustBindingComplete);
    #[cfg(any(test, feature = "longitudinal-counting"))]
    record_timeline_entries_emitted(entries.len());
    let document = EventHistoryDocumentV1 {
        schema: INSPECT_EVENT_HISTORY_SCHEMA.to_owned(),
        version: 1,
        event_count: authority_cursor.event_count,
        authority_cursor,
        source_change_projection_stamp,
        timeline_projection_stamp,
        order: event_history_order(request.order()),
        match_count: selection.match_count,
        offset: selection.offset,
        match_index: selection.match_index,
        facets: selection.facets,
        completion: global.completion,
        diagnostics: global.diagnostics,
        query_notices: selection.query_notices,
        entries,
        previous: None,
        next: None,
    };
    Ok(DerivedTimelinePageV1::new(document, selection.adjacent))
}

#[allow(clippy::too_many_arguments)]
fn prepare_exhaustive_page(
    service: &DerivedAccessService,
    connection: &rusqlite::Connection,
    change_projection: &crate::session::ChangeDocumentProjectionV1,
    as_of: TruthCursor,
    authority_cursor: AuthorityCursorV2,
    source_change_projection_stamp: String,
    timeline_projection_stamp: String,
    request: &DerivedTimelinePageRequestV1,
    trust_set: &TrustSet,
    global: TimelineGlobalProjection,
    hook: &mut impl FnMut(TimelineReadBoundary),
) -> Result<DerivedTimelinePageV1, TimelinePageError> {
    let cte = timeline_cte(as_of);
    let candidates = timeline_predicate(request, false);
    let candidate_count = query_selected_count(
        connection,
        &cte,
        TIMELINE_RELATION,
        &candidates.sql,
        &candidates.parameters,
    )?;
    let keys = query_selected_window(
        connection,
        &cte,
        TIMELINE_RELATION,
        &candidates.sql,
        &candidates.parameters,
        false,
        candidate_count,
        0,
    )?;
    #[cfg(any(test, feature = "longitudinal-counting"))]
    {
        record_timeline_sqlite_candidates(candidate_count);
        record_timeline_sqlite_window_rows(keys.len());
        record_timeline_sqlite_facet_rows(0);
        record_timeline_exhaustive_candidates(keys.len());
    }
    let event_ids = keys
        .iter()
        .map(|key| key.event_id.clone())
        .collect::<Vec<_>>();
    let carriers = validate_timeline_carriers(service, connection, &event_ids, as_of, hook)?;
    let (drafts, selected_diagnostics) =
        project_selected_event_history_without_trust(&carriers.selected, change_projection)
            .map_err(|error| error.to_string())?;
    if !selected_diagnostics
        .iter()
        .all(|diagnostic| global.diagnostics.contains(diagnostic))
    {
        return Err(
            "exhaustive Timeline diagnostics disagree with normalized history"
                .to_owned()
                .into(),
        );
    }
    validate_selected_relations(&carriers, &drafts)?;
    #[cfg(any(test, feature = "longitudinal-counting"))]
    record_timeline_trust_support_carriers(drafts.len());
    let entries =
        bind_selected_event_history_trust(drafts, trust_set).map_err(|error| error.to_string())?;
    hook(TimelineReadBoundary::TrustBindingComplete);
    let parsed = parse_search_query_for(
        request.query().unwrap_or_default(),
        QuerySurface::ChangeTimeline,
    );
    let facet_base = entries
        .into_iter()
        .filter(|entry| matches_query(&event_history_search_record(entry), &parsed.clauses))
        .collect::<Vec<_>>();
    let facets = facet_base
        .iter()
        .fold(BTreeMap::new(), |mut facets, entry| {
            *facets
                .entry(entry.event_type.as_str().to_owned())
                .or_insert(0) += 1;
            facets
        });
    let mut selected = facet_base
        .into_iter()
        .filter(|entry| {
            request.event_types().is_empty() || request.event_types().contains(&entry.event_type)
        })
        .collect::<Vec<_>>();
    if request.order() == DerivedTimelineOrderV1::Desc {
        selected.reverse();
    }
    let match_count = selected.len();
    let (offset, match_index) = exhaustive_offset(&selected, request)?;
    let end = offset.saturating_add(request.limit()).min(match_count);
    let adjacent = exhaustive_adjacent(&selected, request.limit(), offset, end)?;
    let entries = selected[offset..end].to_vec();
    #[cfg(any(test, feature = "longitudinal-counting"))]
    record_timeline_entries_emitted(entries.len());
    let document = EventHistoryDocumentV1 {
        schema: INSPECT_EVENT_HISTORY_SCHEMA.to_owned(),
        version: 1,
        event_count: authority_cursor.event_count,
        authority_cursor,
        source_change_projection_stamp,
        timeline_projection_stamp,
        order: event_history_order(request.order()),
        match_count,
        offset,
        match_index,
        facets,
        completion: global.completion,
        diagnostics: global.diagnostics,
        query_notices: parsed
            .diagnostics
            .into_iter()
            .map(|diagnostic| diagnostic.message)
            .collect(),
        entries,
        previous: None,
        next: None,
    };
    Ok(DerivedTimelinePageV1::new(document, adjacent))
}

fn event_history_order(order: DerivedTimelineOrderV1) -> EventHistoryOrderV1 {
    match order {
        DerivedTimelineOrderV1::Asc => EventHistoryOrderV1::Asc,
        DerivedTimelineOrderV1::Desc => EventHistoryOrderV1::Desc,
    }
}

#[cfg(test)]
pub(super) fn timeline_window_query_plan_for_test(
    connection: &rusqlite::Connection,
    as_of: TruthCursor,
    request: &DerivedTimelinePageRequestV1,
) -> Result<Vec<String>, String> {
    let cte = timeline_cte(as_of);
    let predicate = timeline_predicate(request, true);
    let direction = if request.order() == DerivedTimelineOrderV1::Desc {
        "DESC"
    } else {
        "ASC"
    };
    let sql = format!(
        "EXPLAIN QUERY PLAN
         {cte}
         SELECT normalized_occurred_at, event_id
         FROM {TIMELINE_RELATION}
         WHERE {}
         ORDER BY normalized_occurred_at {direction}, event_id {direction}
         LIMIT ? OFFSET ?",
        predicate.sql
    );
    let mut parameters = predicate.parameters;
    parameters.extend([
        i64::try_from(request.limit())
            .map_err(|_| "Timeline limit overflow".to_owned())?
            .into(),
        0_i64.into(),
    ]);
    let mut statement = connection
        .prepare(&sql)
        .map_err(|error| error.to_string())?;
    let rows = statement
        .query_map(rusqlite::params_from_iter(parameters.iter()), |row| {
            row.get::<_, String>(3)
        })
        .map_err(|error| error.to_string())?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())
}

fn exhaustive_offset(
    selected: &[EventHistoryEntryV1],
    request: &DerivedTimelinePageRequestV1,
) -> Result<(usize, Option<usize>), TimelinePageError> {
    match request.position() {
        DerivedTimelinePagePositionV1::Initial => Ok((0, None)),
        DerivedTimelinePagePositionV1::At(event_id) => {
            let index = selected
                .iter()
                .position(|entry| &entry.event_id == event_id)
                .ok_or_else(|| {
                    TimelinePageError::RequestInvalid(
                        "Timeline locator does not match this query".to_owned(),
                    )
                })?;
            Ok((index / request.limit() * request.limit(), Some(index)))
        }
        DerivedTimelinePagePositionV1::Continuation {
            boundary: DerivedTimelinePageBoundaryV1::SelectedOrderStart,
            ..
        } => Ok((0, None)),
        DerivedTimelinePagePositionV1::Continuation {
            boundary: DerivedTimelinePageBoundaryV1::Key(boundary),
            ..
        } => {
            let index = selected
                .iter()
                .position(|entry| {
                    entry.event_id == *boundary.event_id()
                        && entry.occurred_at == boundary.occurred_at()
                })
                .ok_or_else(|| {
                    TimelinePageError::RequestInvalid(
                        "continuation boundary is absent from this Timeline".to_owned(),
                    )
                })?;
            let offset = index + 1;
            if offset % request.limit() != 0 {
                return Err(TimelinePageError::RequestInvalid(
                    "continuation boundary is not page-aligned".to_owned(),
                ));
            }
            Ok((offset, None))
        }
    }
}

fn exhaustive_adjacent(
    selected: &[EventHistoryEntryV1],
    limit: usize,
    offset: usize,
    end: usize,
) -> Result<DerivedTimelineAdjacentWindowV1, String> {
    let previous = if offset == 0 {
        None
    } else {
        let previous_start = offset.saturating_sub(limit);
        if previous_start == 0 {
            Some(DerivedTimelinePageBoundaryV1::SelectedOrderStart)
        } else {
            Some(DerivedTimelinePageBoundaryV1::Key(entry_page_key(
                &selected[previous_start - 1],
            )?))
        }
    };
    let next = (end < selected.len())
        .then(|| {
            selected
                .get(end.saturating_sub(1))
                .ok_or_else(|| "Timeline page has no continuation anchor".to_owned())
                .and_then(entry_page_key)
                .map(DerivedTimelinePageBoundaryV1::Key)
        })
        .transpose()?;
    Ok(DerivedTimelineAdjacentWindowV1::new(previous, next))
}

fn entry_page_key(entry: &EventHistoryEntryV1) -> Result<DerivedTimelinePageKeyV1, String> {
    DerivedTimelinePageKeyV1::new(entry.occurred_at.clone(), entry.event_id.clone())
        .map_err(|error| error.to_string())
}

#[derive(Clone, Debug, Eq, PartialEq)]
#[doc(hidden)]
pub struct DerivedTimelineAdjacentWindowV1 {
    previous: Option<DerivedTimelinePageBoundaryV1>,
    next: Option<DerivedTimelinePageBoundaryV1>,
}

impl DerivedTimelineAdjacentWindowV1 {
    pub(super) fn new(
        previous: Option<DerivedTimelinePageBoundaryV1>,
        next: Option<DerivedTimelinePageBoundaryV1>,
    ) -> Self {
        Self { previous, next }
    }

    pub fn previous(&self) -> Option<&DerivedTimelinePageBoundaryV1> {
        self.previous.as_ref()
    }

    pub fn next(&self) -> Option<&DerivedTimelinePageBoundaryV1> {
        self.next.as_ref()
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
#[doc(hidden)]
pub struct DerivedTimelinePageV1 {
    document: EventHistoryDocumentV1,
    adjacent: DerivedTimelineAdjacentWindowV1,
}

impl DerivedTimelinePageV1 {
    pub(super) fn new(
        document: EventHistoryDocumentV1,
        adjacent: DerivedTimelineAdjacentWindowV1,
    ) -> Self {
        Self { document, adjacent }
    }

    pub fn document(&self) -> &EventHistoryDocumentV1 {
        &self.document
    }

    pub fn adjacent(&self) -> &DerivedTimelineAdjacentWindowV1 {
        &self.adjacent
    }

    pub fn into_document(self) -> EventHistoryDocumentV1 {
        self.document
    }
}

#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
#[doc(hidden)]
pub enum DerivedTimelinePageRequestError {
    #[error("Timeline limit must be between 1 and 100")]
    InvalidLimit,
    #[error("Timeline query is empty")]
    EmptyQuery,
    #[error("Timeline query is too long")]
    QueryTooLong,
    #[error("invalid Timeline query: {0}")]
    InvalidQuery(String),
    #[error("Timeline event types contain a duplicate")]
    DuplicateEventType,
    #[error("Timeline track is empty")]
    EmptyTrack,
    #[error("Timeline continuation is missing its projection stamp")]
    MissingProjectionStamp,
    #[error("Timeline continuation boundary is invalid")]
    InvalidBoundary,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(query: Option<&str>) -> DerivedTimelinePageRequestV1 {
        DerivedTimelinePageRequestV1::new(
            10,
            DerivedTimelineOrderV1::Asc,
            query.map(str::to_owned),
            Vec::new(),
            None,
            None,
            None,
            DerivedTimelinePagePositionV1::Initial,
        )
        .unwrap()
    }

    #[test]
    fn classifies_every_supported_field_as_bodyless_and_text_as_exhaustive() {
        for query in [
            "type:observation",
            "track:agent:codex",
            "actor:agent:codex",
            "revision:01234567",
            "change:01234567",
            "snapshot:01234567",
            "check:passed",
            "assessment:accepted",
            "is:open",
            "tag:correctness",
            "before:2026-08-18",
            "after:2026-08-17",
            "-tag:later type:assessment",
        ] {
            assert_eq!(
                classify_query(&request(Some(query))),
                TimelineQueryClass::Bodyless,
                "{query}"
            );
        }
        for query in ["serialized prose", "tag:correctness prose", "-missing"] {
            assert_eq!(
                classify_query(&request(Some(query))),
                TimelineQueryClass::Exhaustive,
                "{query}"
            );
        }
    }

    #[test]
    fn request_rejects_ambiguous_or_unauthenticated_shapes() {
        assert!(matches!(
            DerivedTimelinePageRequestV1::new(
                0,
                DerivedTimelineOrderV1::Asc,
                None,
                Vec::new(),
                None,
                None,
                None,
                DerivedTimelinePagePositionV1::Initial,
            ),
            Err(DerivedTimelinePageRequestError::InvalidLimit)
        ));
        assert!(matches!(
            DerivedTimelinePagePositionV1::continuation(
                DerivedTimelineTraversalV1::Next,
                DerivedTimelinePageBoundaryV1::SelectedOrderStart,
                "sha256:stamp",
            ),
            Err(DerivedTimelinePageRequestError::InvalidBoundary)
        ));
    }

    #[test]
    fn adapter_reuses_shared_selection_hydration_support_and_search_engines() {
        let source = include_str!("timeline.rs");
        let implementation = source
            .split("#[cfg(test)]\nmod tests")
            .next()
            .expect("Timeline implementation prefix");
        for required in [
            "query_selected_count(",
            "query_selected_facets(",
            "query_selected_window(",
            "support_event_plan(",
            ".semantic_ids_hydrated_at(",
            "event_history_search_record(",
        ] {
            assert!(
                implementation.contains(required),
                "missing shared helper {required}"
            );
        }
        for forbidden in [
            "list_events(",
            "list_events_lenient(",
            "QualificationLocalJournal",
            "hydrate_locator_row(",
            "ExhaustiveSearchFallback",
        ] {
            assert!(
                !implementation.contains(forbidden),
                "Timeline adapter must not contain {forbidden}"
            );
        }

        let cli = include_str!("../../cli/inspect/event_history_query.rs");
        assert!(cli.contains("event_history_search_record(entry)"));
        assert!(!cli.contains("fn summary_search_fields("));
        assert!(!cli.contains("fn token_set<"));
    }
}
