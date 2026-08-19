//! Strict query and opaque-cursor contract for the Change-aware Timeline.
//!
//! This module has no store access. The reader facade owns filtering and
//! ordering; the server parses a request here, loads that facade exactly once,
//! then checks [`Request::continuation_projection_stamp`] before selecting a
//! window. Keeping the projection stamp check outside token parsing means a
//! syntactically valid cursor can report a typed stale-projection response
//! rather than being mistaken for a malformed query.

use std::collections::{BTreeMap, BTreeSet};

use pointbreak::model::{ChangeId, EventId, RevisionId, RevisionRefV1};
use pointbreak::session::event::EventType;
use pointbreak::session::{
    DerivedTimelineExactRevisionV1, DerivedTimelineOrderV1, DerivedTimelinePageBoundaryV1,
    DerivedTimelinePageKeyV1, DerivedTimelinePagePositionV1, DerivedTimelinePageRequestV1,
    DerivedTimelineTraversalV1, QueryDiagnosticCode, QuerySurface, parse_search_query_for,
};
use serde::{Deserialize, Serialize};

use super::page_token::PageTokenSigner;

const ROUTE: &str = "/api/v2/history";
const TOKEN_SCHEMA: &str = "pointbreak.inspect-event-history-page-token.v1";
pub(super) const DEFAULT_LIMIT: usize = 100;
pub(super) const MAX_LIMIT: usize = 100;

/// The one error class which is safe to return before loading the cached
/// Timeline. A valid continuation's projection stamp is deliberately not
/// compared here; see [`Request::continuation_projection_stamp`].
#[derive(Debug, Eq, PartialEq)]
pub(super) enum PageError {
    Invalid(String),
}

/// The selected display order, after filtering. Its key order is always the
/// stable `(occurredAt, eventId)` tuple; `Desc` reverses that selected list.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum Order {
    Asc,
    #[default]
    Desc,
}

impl Order {
    pub(super) const fn as_str(self) -> &'static str {
        match self {
            Self::Asc => "asc",
            Self::Desc => "desc",
        }
    }
}

/// A key in the reader's already-selected display order.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct TimelineKey {
    pub(super) occurred_at: String,
    pub(super) event_id: String,
}

/// A continuation moves one full page in the selected display order. Both
/// directions travel through the sole `after` request field; direction is
/// authenticated inside the opaque token rather than inferred from a raw URL.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum Traversal {
    Previous,
    Next,
}

/// A signed continuation boundary. `SelectedOrderStart` is necessary for a
/// previous-page request targeting the first page: there is no physical entry
/// before it to bind.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum Boundary {
    SelectedOrderStart,
    Key(TimelineKey),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum Position {
    Initial,
    AtEventId(String),
    Continuation {
        traversal: Traversal,
        boundary: Boundary,
    },
}

/// Query fields that select the Timeline's source rows. Position fields are
/// intentionally excluded from [`Query::identity`] so every neighbor token
/// binds one stable selected set rather than one transient page.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct Query {
    limit: usize,
    order: Order,
    q: Option<String>,
    types: Vec<EventType>,
    track: Option<String>,
    change: Option<String>,
    revision: Option<ExactRevision>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct ExactRevision {
    reference: RevisionRefV1,
}

/// Parsed request data. The token itself remains private; adapters can inspect
/// only the verified position and, after their one cached reader load, its
/// projection stamp.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct Request {
    query: Query,
    position: Position,
    continuation_projection_stamp: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Token {
    schema: String,
    route: String,
    timeline_projection_stamp: String,
    query_identity: String,
    limit: usize,
    order: Order,
    traversal: Traversal,
    boundary: Boundary,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct QueryIdentity<'a> {
    limit: usize,
    order: &'a str,
    q: &'a Option<String>,
    types: Vec<&'static str>,
    track: &'a Option<String>,
    change: &'a Option<String>,
    revision: Option<RevisionIdentity<'a>>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct RevisionIdentity<'a> {
    revision_id: &'a str,
    artifact_hash: &'a str,
}

impl Request {
    pub(super) fn query(&self) -> &Query {
        &self.query
    }

    pub(super) fn position(&self) -> &Position {
        &self.position
    }

    /// Compare this value with the one atomic reader generation's Timeline
    /// stamp only after that generation has been loaded. `None` is an initial
    /// or `at` request and therefore has no stale-cursor condition.
    pub(super) fn continuation_projection_stamp(&self) -> Option<&str> {
        self.continuation_projection_stamp.as_deref()
    }

    /// Convert the authenticated public request into the neutral session
    /// request. Token bytes and signatures remain private to this module.
    pub(super) fn derived_request(&self) -> Result<DerivedTimelinePageRequestV1, PageError> {
        let position = match &self.position {
            Position::Initial => DerivedTimelinePagePositionV1::Initial,
            Position::AtEventId(event_id) => {
                DerivedTimelinePagePositionV1::At(EventId::new(event_id.clone()))
            }
            Position::Continuation {
                traversal,
                boundary,
            } => DerivedTimelinePagePositionV1::continuation(
                match traversal {
                    Traversal::Previous => DerivedTimelineTraversalV1::Previous,
                    Traversal::Next => DerivedTimelineTraversalV1::Next,
                },
                derived_boundary(boundary)?,
                self.continuation_projection_stamp
                    .clone()
                    .ok_or_else(|| invalid("continuation is missing its projection stamp"))?,
            )
            .map_err(|error| invalid(&error.to_string()))?,
        };
        DerivedTimelinePageRequestV1::new(
            self.query.limit,
            match self.query.order {
                Order::Asc => DerivedTimelineOrderV1::Asc,
                Order::Desc => DerivedTimelineOrderV1::Desc,
            },
            self.query.q.clone(),
            self.query.types.clone(),
            self.query.track.clone(),
            self.query.change.clone().map(ChangeId::new),
            self.query
                .revision
                .as_ref()
                .map(|revision| DerivedTimelineExactRevisionV1::new(revision.reference.clone())),
            position,
        )
        .map_err(|error| invalid(&error.to_string()))
    }
}

fn derived_boundary(boundary: &Boundary) -> Result<DerivedTimelinePageBoundaryV1, PageError> {
    match boundary {
        Boundary::SelectedOrderStart => Ok(DerivedTimelinePageBoundaryV1::SelectedOrderStart),
        Boundary::Key(key) => DerivedTimelinePageKeyV1::new(
            key.occurred_at.clone(),
            EventId::new(key.event_id.clone()),
        )
        .map(DerivedTimelinePageBoundaryV1::Key)
        .map_err(|error| invalid(&error.to_string())),
    }
}

impl Query {
    pub(super) const fn limit(&self) -> usize {
        self.limit
    }

    pub(super) const fn order(&self) -> Order {
        self.order
    }

    pub(super) fn q(&self) -> Option<&str> {
        self.q.as_deref()
    }

    pub(super) fn types(&self) -> &[EventType] {
        &self.types
    }

    pub(super) fn track(&self) -> Option<&str> {
        self.track.as_deref()
    }

    pub(super) fn change(&self) -> Option<&str> {
        self.change.as_deref()
    }

    pub(super) fn revision(&self) -> Option<&ExactRevision> {
        self.revision.as_ref()
    }

    fn identity(&self) -> String {
        let revision = self.revision.as_ref().map(|revision| RevisionIdentity {
            revision_id: revision.revision_id(),
            artifact_hash: revision.artifact_hash(),
        });
        serde_json::to_string(&QueryIdentity {
            limit: self.limit,
            order: self.order.as_str(),
            q: &self.q,
            types: self
                .types
                .iter()
                .map(|event_type| event_type.as_str())
                .collect(),
            track: &self.track,
            change: &self.change,
            revision,
        })
        .expect("Timeline query identity must serialize")
    }
}

impl ExactRevision {
    pub(super) fn revision_id(&self) -> &str {
        self.reference.revision_id.as_str()
    }

    pub(super) fn artifact_hash(&self) -> &str {
        &self.reference.object_artifact_content_hash
    }
}

/// Parse the strict public query grammar and authenticate an optional
/// continuation. This performs neither reader I/O nor projection-stamp
/// comparison.
pub(super) fn parse_signed(
    raw: Option<&str>,
    signer: &PageTokenSigner,
) -> Result<Request, PageError> {
    let fields = parse_fields(raw)?;
    let limit = parse_limit(nonempty(&fields, "limit")?)?;
    let order = parse_order(nonempty(&fields, "order")?)?;
    let q = parse_q(nonempty(&fields, "q")?)?;
    let types = parse_types(nonempty(&fields, "type")?)?;
    let track = nonempty(&fields, "track")?;
    let change = nonempty(&fields, "change")?;
    if change
        .as_deref()
        .is_some_and(|change| !change.starts_with("change:"))
    {
        return Err(invalid("invalid change"));
    }
    let revision_id = nonempty(&fields, "revision")?;
    let artifact_hash = nonempty(&fields, "artifactHash")?;
    let revision = match (revision_id, artifact_hash) {
        (Some(revision_id), Some(artifact_hash)) => Some(ExactRevision {
            reference: RevisionRefV1::new(RevisionId::new(revision_id), artifact_hash)
                .map_err(|_| invalid("invalid exact revision"))?,
        }),
        (None, None) => None,
        _ => return Err(invalid("revision requires artifactHash")),
    };
    let at = nonempty(&fields, "at")?;
    if at
        .as_deref()
        .is_some_and(|event_id| !event_id.starts_with("evt:"))
    {
        return Err(invalid("invalid event id"));
    }
    let after = nonempty(&fields, "after")?;
    if at.is_some() && after.is_some() {
        return Err(invalid("at and after are mutually exclusive"));
    }
    let query = Query {
        limit,
        order,
        q,
        types,
        track,
        change,
        revision,
    };
    let (position, continuation_projection_stamp) = match (at, after) {
        (Some(event_id), None) => (Position::AtEventId(event_id), None),
        (None, Some(raw_token)) => {
            let token = decode_token(&raw_token, signer)?;
            if token.query_identity != query.identity()
                || token.limit != query.limit
                || token.order != query.order
            {
                return Err(invalid("continuation does not match request"));
            }
            (
                Position::Continuation {
                    traversal: token.traversal,
                    boundary: token.boundary,
                },
                Some(token.timeline_projection_stamp),
            )
        }
        (None, None) => (Position::Initial, None),
        (Some(_), Some(_)) => unreachable!("mutual exclusion checked above"),
    };
    Ok(Request {
        query,
        position,
        continuation_projection_stamp,
    })
}

/// Issue an opaque token for one neighbor of the emitted page. The caller must
/// pass the single Timeline projection stamp and selected-order boundary from
/// the same cached reader generation used to build that page.
pub(super) fn issue_continuation(
    query: &Query,
    timeline_projection_stamp: &str,
    traversal: Traversal,
    boundary: Boundary,
    signer: &PageTokenSigner,
) -> Result<String, PageError> {
    if timeline_projection_stamp.is_empty() {
        return Err(invalid("missing timeline projection stamp"));
    }
    if matches!(&boundary, Boundary::Key(key) if key.occurred_at.is_empty() || key.event_id.is_empty())
    {
        return Err(invalid("invalid continuation boundary"));
    }
    Ok(signer.encode(&Token {
        schema: TOKEN_SCHEMA.into(),
        route: ROUTE.into(),
        timeline_projection_stamp: timeline_projection_stamp.into(),
        query_identity: query.identity(),
        limit: query.limit,
        order: query.order,
        traversal,
        boundary,
    }))
}

/// Align an `at` event's selected-order index to the page containing it.
pub(super) const fn page_start_for_index(index: usize, limit: usize) -> usize {
    index / limit * limit
}

fn parse_fields(raw: Option<&str>) -> Result<BTreeMap<String, String>, PageError> {
    let Some(raw) = raw.filter(|raw| !raw.is_empty()) else {
        return Ok(BTreeMap::new());
    };
    let mut fields = BTreeMap::new();
    for pair in raw.split('&') {
        let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
        let key = decode(key)?;
        if !matches!(
            key.as_str(),
            "limit"
                | "after"
                | "at"
                | "q"
                | "type"
                | "track"
                | "change"
                | "revision"
                | "artifactHash"
                | "order"
        ) {
            return Err(invalid("unknown query field"));
        }
        if fields.insert(key, decode(value)?).is_some() {
            return Err(invalid("duplicate query field"));
        }
    }
    Ok(fields)
}

fn nonempty(fields: &BTreeMap<String, String>, name: &str) -> Result<Option<String>, PageError> {
    fields
        .get(name)
        .map(|value| {
            if value.is_empty() {
                Err(invalid("empty query field"))
            } else {
                Ok(value.clone())
            }
        })
        .transpose()
}

fn parse_limit(raw: Option<String>) -> Result<usize, PageError> {
    match raw {
        Some(value) if value.bytes().all(|byte| byte.is_ascii_digit()) => value
            .parse()
            .ok()
            .filter(|value: &usize| (1..=MAX_LIMIT).contains(value))
            .ok_or_else(|| invalid("invalid limit")),
        Some(_) => Err(invalid("invalid limit")),
        None => Ok(DEFAULT_LIMIT),
    }
}

fn parse_order(raw: Option<String>) -> Result<Order, PageError> {
    match raw.as_deref() {
        None | Some("desc") => Ok(Order::Desc),
        Some("asc") => Ok(Order::Asc),
        Some(_) => Err(invalid("invalid order")),
    }
}

fn parse_q(raw: Option<String>) -> Result<Option<String>, PageError> {
    let value = raw.as_ref().map(|value| value.trim().to_owned());
    if raw.is_some() && value.as_deref().is_none_or(str::is_empty) {
        return Err(invalid("empty query field"));
    }
    if value.as_ref().is_some_and(|value| value.len() > 256) {
        return Err(invalid("query is too long"));
    }
    let value = value.map(|value| value.to_lowercase());
    if let Some(value) = &value {
        let parsed = parse_search_query_for(value, QuerySurface::ChangeTimeline);
        if let Some(fatal) = parsed.diagnostics.iter().find(|diagnostic| {
            matches!(
                diagnostic.code,
                QueryDiagnosticCode::UnsupportedQualifier | QueryDiagnosticCode::UnsupportedValue
            )
        }) {
            return Err(invalid(&fatal.message));
        }
    }
    Ok(value)
}

fn parse_types(raw: Option<String>) -> Result<Vec<EventType>, PageError> {
    let Some(raw) = raw else {
        return Ok(Vec::new());
    };
    let mut types = BTreeSet::new();
    for wire in raw.split(',') {
        if wire.is_empty() {
            return Err(invalid("empty event type"));
        }
        let event_type = event_type_from_wire(wire).ok_or_else(|| invalid("invalid event type"))?;
        if !types.insert(event_type.as_str()) {
            return Err(invalid("duplicate event type"));
        }
    }
    Ok(types
        .into_iter()
        .map(|wire| event_type_from_wire(wire).expect("wire came from EventType"))
        .collect())
}

fn event_type_from_wire(wire: &str) -> Option<EventType> {
    let event_type = match wire {
        "review_initialized" => EventType::ReviewInitialized,
        "work_object_proposed" => EventType::WorkObjectProposed,
        "review_observation_recorded" => EventType::ReviewObservationRecorded,
        "review_assessment_recorded" => EventType::ReviewAssessmentRecorded,
        "input_request_opened" => EventType::InputRequestOpened,
        "input_request_responded" => EventType::InputRequestResponded,
        "review_note_imported" => EventType::ReviewNoteImported,
        "revision_ref_associated" => EventType::RevisionRefAssociated,
        "revision_ref_withdrawn" => EventType::RevisionRefWithdrawn,
        "revision_commit_associated" => EventType::RevisionCommitAssociated,
        "revision_commit_withdrawn" => EventType::RevisionCommitWithdrawn,
        "validation_check_recorded" => EventType::ValidationCheckRecorded,
        "task_checkpoint_captured" => EventType::TaskCheckpointCaptured,
        "task_observation_recorded" => EventType::TaskObservationRecorded,
        "event_signature_recorded" => EventType::EventSignatureRecorded,
        "artifact_removed" => EventType::ArtifactRemoved,
        "change_declared" => EventType::ChangeDeclared,
        "change_membership_asserted" => EventType::ChangeMembershipAsserted,
        "change_membership_withdrawn" => EventType::ChangeMembershipWithdrawn,
        "change_link_asserted" => EventType::ChangeLinkAsserted,
        "change_revision_relation_asserted" => EventType::ChangeRevisionRelationAsserted,
        "change_revision_relation_withdrawn" => EventType::ChangeRevisionRelationWithdrawn,
        "revision_relation_attested" => EventType::RevisionRelationAttested,
        "review_fact_ported" => EventType::ReviewFactPorted,
        _ => return None,
    };
    (!matches!(
        event_type,
        EventType::TaskCheckpointCaptured
            | EventType::TaskObservationRecorded
            | EventType::EventSignatureRecorded
            | EventType::ArtifactRemoved
    ))
    .then_some(event_type)
}

fn decode_token(raw: &str, signer: &PageTokenSigner) -> Result<Token, PageError> {
    if raw.len() > 4096 {
        return Err(invalid("continuation is too long"));
    }
    let token: Token = signer
        .decode(raw)
        .map_err(|()| invalid("malformed continuation"))?;
    if token.schema != TOKEN_SCHEMA
        || token.route != ROUTE
        || token.timeline_projection_stamp.is_empty()
        || !(1..=MAX_LIMIT).contains(&token.limit)
        || matches!(&token.boundary, Boundary::Key(key) if key.occurred_at.is_empty() || key.event_id.is_empty())
    {
        return Err(invalid("malformed continuation"));
    }
    Ok(token)
}

fn decode(raw: &str) -> Result<String, PageError> {
    let mut out = Vec::new();
    let bytes = raw.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'%' => {
                if index + 2 >= bytes.len() {
                    return Err(invalid("invalid percent encoding"));
                }
                let high = (bytes[index + 1] as char)
                    .to_digit(16)
                    .ok_or_else(|| invalid("invalid percent encoding"))?;
                let low = (bytes[index + 2] as char)
                    .to_digit(16)
                    .ok_or_else(|| invalid("invalid percent encoding"))?;
                out.push((high * 16 + low) as u8);
                index += 3;
            }
            b'+' => {
                out.push(b' ');
                index += 1;
            }
            byte => {
                out.push(byte);
                index += 1;
            }
        }
    }
    String::from_utf8(out).map_err(|_| invalid("invalid utf-8"))
}

fn invalid(message: &str) -> PageError {
    PageError::Invalid(message.into())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn signer() -> PageTokenSigner {
        PageTokenSigner::from_seed([42_u8; 32])
    }

    fn parse(raw: Option<&str>) -> Result<Request, PageError> {
        parse_signed(raw, &signer())
    }

    fn query(raw: &str) -> Query {
        parse(Some(raw)).unwrap().query
    }

    #[test]
    fn strict_grammar_rejects_unknown_duplicate_empty_and_bad_encoding() {
        for raw in ["wat=x", "q=%FF", "q=%2G", "q=%", "limit=1&limit=2"] {
            assert!(
                matches!(parse(Some(raw)), Err(PageError::Invalid(_))),
                "{raw}"
            );
        }
        for field in [
            "limit",
            "after",
            "at",
            "q",
            "type",
            "track",
            "change",
            "revision",
            "artifactHash",
            "order",
        ] {
            assert!(matches!(
                parse(Some(&format!("{field}="))),
                Err(PageError::Invalid(_))
            ));
        }
    }

    #[test]
    fn defaults_and_orders_are_normalized_into_query_identity() {
        let default = query("");
        assert_eq!(default.limit(), 100);
        assert_eq!(default.order(), Order::Desc);
        let explicit = query("order=desc&limit=100");
        assert_eq!(default.identity(), explicit.identity());
        assert_eq!(query("order=asc").order(), Order::Asc);
        for raw in [
            "limit=0",
            "limit=101",
            "limit=-1",
            "limit=1.0",
            "order=sideways",
        ] {
            assert!(
                matches!(parse(Some(raw)), Err(PageError::Invalid(_))),
                "{raw}"
            );
        }
    }

    #[test]
    fn normalizes_query_and_rejects_empty_after_trimming() {
        let parsed = query("q=%C2%A0%C4%B0STANBUL%C2%A0");
        assert_eq!(parsed.q(), Some("i\u{307}stanbul"));
        assert!(matches!(
            parse(Some("q=+%C2%A0+")),
            Err(PageError::Invalid(_))
        ));
        assert!(matches!(
            parse(Some(&format!("q={}", "x".repeat(257)))),
            Err(PageError::Invalid(_))
        ));
        for raw in ["q=is%3Anot-a-state", "q=attention%3Aopen-request"] {
            assert!(
                matches!(parse(Some(raw)), Err(PageError::Invalid(_))),
                "{raw}"
            );
        }
    }

    #[test]
    fn type_csv_is_known_stable_and_query_bound() {
        let parsed = query("type=validation_check_recorded,review_initialized");
        assert_eq!(
            parsed
                .types()
                .iter()
                .map(|kind| kind.as_str())
                .collect::<Vec<_>>(),
            vec!["review_initialized", "validation_check_recorded"]
        );
        for raw in [
            "type=unknown",
            "type=review_initialized,",
            "type=,review_initialized",
            "type=validation_check_recorded,review_initialized,validation_check_recorded",
        ] {
            assert!(
                matches!(parse(Some(raw)), Err(PageError::Invalid(_))),
                "{raw}"
            );
        }
    }

    #[test]
    fn every_admitted_event_type_wire_name_is_accepted_and_excluded_families_refuse() {
        let wires = [
            "review_initialized",
            "work_object_proposed",
            "review_observation_recorded",
            "review_assessment_recorded",
            "input_request_opened",
            "input_request_responded",
            "review_note_imported",
            "revision_ref_associated",
            "revision_ref_withdrawn",
            "revision_commit_associated",
            "revision_commit_withdrawn",
            "validation_check_recorded",
            "change_declared",
            "change_membership_asserted",
            "change_membership_withdrawn",
            "change_link_asserted",
            "change_revision_relation_asserted",
            "change_revision_relation_withdrawn",
            "revision_relation_attested",
            "review_fact_ported",
        ];
        let csv = wires.join(",");
        let parsed = query(&format!("type={csv}"));
        assert_eq!(parsed.types().len(), wires.len());
        for excluded in [
            "task_checkpoint_captured",
            "task_observation_recorded",
            "event_signature_recorded",
            "artifact_removed",
        ] {
            assert!(matches!(
                parse(Some(&format!("type={excluded}"))),
                Err(PageError::Invalid(_))
            ));
        }
    }

    #[test]
    fn exact_revision_filter_requires_the_pair() {
        let artifact_hash = format!("sha256:{}", "a".repeat(64));
        let parsed = query(&format!(
            "revision=rev%3Asha256%3Aone&artifactHash={artifact_hash}"
        ));
        let revision = parsed.revision().unwrap();
        assert_eq!(revision.revision_id(), "rev:sha256:one");
        assert_eq!(revision.artifact_hash(), artifact_hash);
        for raw in ["revision=rev%3Aone", "artifactHash=sha256%3Atwo"] {
            assert!(
                matches!(parse(Some(raw)), Err(PageError::Invalid(_))),
                "{raw}"
            );
        }
        assert!(matches!(
            parse(Some("revision=not-rev&artifactHash=sha256%3Atwo")),
            Err(PageError::Invalid(_))
        ));
    }

    #[test]
    fn structured_identity_query_stays_separate_from_exact_filters() {
        let request = query("q=revision%3A01234567+rev%3A01234567+change%3Afedcba98");
        assert_eq!(
            request.q(),
            Some("revision:01234567 rev:01234567 change:fedcba98")
        );
        assert!(request.change().is_none());
        assert!(request.revision().is_none());
    }

    #[test]
    fn opaque_filters_still_enforce_their_kind_prefixes() {
        assert!(matches!(
            parse(Some("change=not-a-change")),
            Err(PageError::Invalid(_))
        ));
        assert!(matches!(
            parse(Some("at=not-an-event")),
            Err(PageError::Invalid(_))
        ));
        let request = parse(Some("change=change%3Asha256%3Aone&at=evt%3Asha256%3Atwo")).unwrap();
        assert_eq!(request.query().change(), Some("change:sha256:one"));
        assert_eq!(
            request.position(),
            &Position::AtEventId("evt:sha256:two".into())
        );
    }

    #[test]
    fn authenticated_public_request_converts_to_the_neutral_timeline_contract() {
        let artifact_hash = format!("sha256:{}", "a".repeat(64));
        let request = parse(Some(&format!(
            "limit=2&order=asc&q=tag%3Acorrectness&type=review_initialized&\
             track=agent%3Aone&change=change%3Asha256%3Aone&\
             revision=rev%3Asha256%3Aone&artifactHash={artifact_hash}&\
             at=evt%3Asha256%3Atarget"
        )))
        .unwrap();
        let derived = request.derived_request().unwrap();

        assert_eq!(derived.limit(), 2);
        assert_eq!(derived.order(), DerivedTimelineOrderV1::Asc);
        assert_eq!(derived.query(), Some("tag:correctness"));
        assert_eq!(derived.event_types(), &[EventType::ReviewInitialized]);
        assert_eq!(derived.track(), Some("agent:one"));
        assert_eq!(derived.change().unwrap().as_str(), "change:sha256:one");
        assert_eq!(
            derived.revision().unwrap().reference(),
            &RevisionRefV1::new(RevisionId::new("rev:sha256:one"), artifact_hash).unwrap()
        );
        assert!(matches!(
            derived.position(),
            DerivedTimelinePagePositionV1::At(event_id)
                if event_id.as_str() == "evt:sha256:target"
        ));
    }

    #[test]
    fn at_and_after_are_mutually_exclusive() {
        assert!(matches!(
            parse(Some("at=evt%3Aone&after=opaque")),
            Err(PageError::Invalid(_))
        ));
    }

    #[test]
    fn tokens_round_trip_in_both_directions_and_keep_the_stamp_for_the_reader() {
        let query = query("order=asc&limit=2&type=review_initialized&track=agent%3Aone");
        for (traversal, boundary) in [
            (
                Traversal::Next,
                Boundary::Key(TimelineKey {
                    occurred_at: "2026-08-07T00:00:00Z".into(),
                    event_id: "evt:sha256:last".into(),
                }),
            ),
            (Traversal::Previous, Boundary::SelectedOrderStart),
        ] {
            let token = issue_continuation(
                &query,
                "timeline-stamp",
                traversal,
                boundary.clone(),
                &signer(),
            )
            .unwrap();
            let request = parse(Some(&format!(
                "order=asc&limit=2&type=review_initialized&track=agent%3Aone&after={token}"
            )))
            .unwrap();
            assert_eq!(
                request.continuation_projection_stamp(),
                Some("timeline-stamp")
            );
            assert_eq!(request.query().track(), Some("agent:one"));
            assert_eq!(
                request.position(),
                &Position::Continuation {
                    traversal,
                    boundary: boundary.clone()
                }
            );
            let derived = request.derived_request().unwrap();
            assert_eq!(
                derived.position().expected_projection_stamp(),
                Some("timeline-stamp")
            );
            match (derived.position(), traversal, boundary) {
                (
                    DerivedTimelinePagePositionV1::Continuation {
                        traversal: DerivedTimelineTraversalV1::Next,
                        boundary: DerivedTimelinePageBoundaryV1::Key(key),
                        ..
                    },
                    Traversal::Next,
                    Boundary::Key(expected),
                ) => {
                    assert_eq!(key.occurred_at(), expected.occurred_at);
                    assert_eq!(key.event_id().as_str(), expected.event_id);
                }
                (
                    DerivedTimelinePagePositionV1::Continuation {
                        traversal: DerivedTimelineTraversalV1::Previous,
                        boundary: DerivedTimelinePageBoundaryV1::SelectedOrderStart,
                        ..
                    },
                    Traversal::Previous,
                    Boundary::SelectedOrderStart,
                ) => {}
                other => panic!("public continuation drifted during neutral conversion: {other:?}"),
            }
        }
    }

    #[test]
    fn token_authentication_and_query_binding_fail_closed_before_reader_load() {
        let query = query("limit=2&order=desc&q=alpha");
        let token = issue_continuation(
            &query,
            "timeline-stamp",
            Traversal::Next,
            Boundary::Key(TimelineKey {
                occurred_at: "2026-08-07T00:00:00Z".into(),
                event_id: "evt:sha256:last".into(),
            }),
            &signer(),
        )
        .unwrap();
        let mut tampered = token.clone();
        tampered.replace_range(0..1, "x");
        assert!(matches!(
            parse(Some(&format!(
                "limit=2&order=desc&q=alpha&after={tampered}"
            ))),
            Err(PageError::Invalid(_))
        ));
        assert!(matches!(
            parse(Some(&format!("limit=2&order=asc&q=alpha&after={token}"))),
            Err(PageError::Invalid(_))
        ));
        assert!(matches!(
            parse(Some(&format!("limit=2&order=desc&q=beta&after={token}"))),
            Err(PageError::Invalid(_))
        ));

        let wrong_route = signer().encode(&Token {
            schema: TOKEN_SCHEMA.into(),
            route: "/api/v2/changes".into(),
            timeline_projection_stamp: "timeline-stamp".into(),
            query_identity: query.identity(),
            limit: query.limit(),
            order: query.order(),
            traversal: Traversal::Next,
            boundary: Boundary::SelectedOrderStart,
        });
        assert!(matches!(
            parse(Some(&format!(
                "limit=2&order=desc&q=alpha&after={wrong_route}"
            ))),
            Err(PageError::Invalid(_))
        ));
    }

    #[test]
    fn at_positions_align_to_the_containing_page() {
        assert_eq!(page_start_for_index(0, 100), 0);
        assert_eq!(page_start_for_index(99, 100), 0);
        assert_eq!(page_start_for_index(100, 100), 100);
        assert_eq!(page_start_for_index(201, 100), 200);
        let request = parse(Some("at=evt%3Asha256%3Atarget&order=asc")).unwrap();
        assert_eq!(
            request.position(),
            &Position::AtEventId("evt:sha256:target".into())
        );
    }
}
