use std::collections::{BTreeMap, BTreeSet};

use pointbreak::session::{
    ChangeLifecycleV1, ChangeListOrderV1, ChangeOrderKey, ChangePageKeyV1, ChangeTopologyV1,
    DerivedChangeAttentionFilterV1, DerivedChangeAvailabilityFilterV1, DerivedChangePageBoundaryV1,
    DerivedChangePageContinuationV1, DerivedChangePageRequestV1, DerivedChangePageSelectionV1,
    DerivedChangePageWindowV1, compare_change_order,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub(super) use super::page_token::PageTokenSigner;

const TOKEN_SCHEMA: &str = "pointbreak.inspect-change-page-token.v1";
const MAX_TOKEN_BYTES: usize = 4096;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum Lens {
    Changes,
    Attention,
}

impl Lens {
    fn default_order(self) -> ChangeListOrderV1 {
        match self {
            Self::Changes => ChangeListOrderV1::default_for_changes(),
            Self::Attention => ChangeListOrderV1::default_for_attention(),
        }
    }

    fn admits(self, order: ChangeListOrderV1) -> bool {
        match self {
            Self::Changes => order.admitted_on_changes_lens(),
            Self::Attention => order.admitted_on_attention_lens(),
        }
    }
}

/// The comparator input for one projected Change row. Both the strict sort
/// and the page slice read the same fields, so a cursor lands between exactly
/// the neighbours the sort produced.
fn page_order_key(change: &Value) -> ChangeOrderKey<'_> {
    let wait = &change["attentionWaitAt"];
    ChangeOrderKey {
        change_id: change["changeId"].as_str().unwrap_or_default(),
        activity_at: change["activityAt"].as_str(),
        tier_rank: wait["tierRank"]
            .as_u64()
            .and_then(|rank| u8::try_from(rank).ok()),
        oldest_observed_at: wait["oldestObservedAt"].as_str(),
    }
}

#[derive(Debug, Eq, PartialEq)]
pub(super) enum PageError {
    Invalid(String),
    Stale,
}

#[derive(Debug)]
pub(super) enum Request {
    Bare,
    Bounded(Box<Query>),
}

impl Request {
    /// Convert the authenticated CLI request into the storage-neutral session
    /// contract before the derived reader is opened.
    pub(super) fn derived_request(&self) -> Result<DerivedChangePageRequestV1, PageError> {
        match self {
            Self::Bare => Ok(DerivedChangePageRequestV1::Bare),
            Self::Bounded(query) => query.derived_request(),
        }
    }

    /// Attach signed opaque capabilities to a page already selected by the
    /// session reader. This adapter never filters, sorts, or repaginates rows.
    pub(super) fn apply_derived_window(
        &self,
        mut document: Value,
        window: Option<&DerivedChangePageWindowV1>,
        signer: &PageTokenSigner,
    ) -> Result<Value, PageError> {
        match self {
            Self::Bare => {
                if window.is_some() {
                    return Err(invalid("bare Change page unexpectedly has a window"));
                }
                Ok(document)
            }
            Self::Bounded(query) => {
                // Re-run the pure binding validation at the response adapter so
                // this function is safe even when called outside the normal API
                // helper. Normal routing performs it before reader access.
                query.derived_request()?;
                let window = window.ok_or_else(|| invalid("bounded Change page has no window"))?;
                let stamp = document["projectionStamp"]
                    .as_str()
                    .ok_or_else(|| invalid("missing projection stamp"))?;
                if stamp != window.projection_stamp {
                    return Err(invalid("Change page window has the wrong projection stamp"));
                }
                if document["order"] != query.order.as_str() {
                    return Err(invalid("Change page document has the wrong order"));
                }
                let identity = query.identity();
                let issue = |boundary: &DerivedChangePageBoundaryV1| {
                    if boundary
                        .last_key()
                        .is_some_and(|key| key.order() != query.order)
                    {
                        return Err(invalid("Change page window has the wrong order"));
                    }
                    encode_token(
                        &Token {
                            schema: TOKEN_SCHEMA.to_owned(),
                            lens: query.lens,
                            projection_stamp: window.projection_stamp.clone(),
                            query: identity.clone(),
                            limit: query.limit,
                            order: query.order,
                            last_key: boundary.last_key().cloned(),
                        },
                        signer,
                    )
                };
                document["previous"] = window
                    .previous
                    .as_ref()
                    .map(issue)
                    .transpose()?
                    .map(Value::String)
                    .unwrap_or(Value::Null);
                document["next"] = window
                    .next
                    .as_ref()
                    .map(issue)
                    .transpose()?
                    .map(Value::String)
                    .unwrap_or(Value::Null);
                document["last"] = window
                    .last
                    .as_ref()
                    .map(issue)
                    .transpose()?
                    .map(Value::String)
                    .unwrap_or(Value::Null);
                Ok(document)
            }
        }
    }
}

#[derive(Clone, Debug)]
pub(super) struct Query {
    lens: Lens,
    limit: usize,
    order: ChangeListOrderV1,
    after: Option<Token>,
    raw_q: Option<String>,
    q: Option<String>,
    topology: Option<String>,
    lifecycle: Option<String>,
    attention: Option<String>,
    availability: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Token {
    schema: String,
    lens: Lens,
    projection_stamp: String,
    query: String,
    limit: usize,
    order: ChangeListOrderV1,
    /// The order-tagged key of the Change immediately before the target page,
    /// or `None` for page one. This boundary is always server-issued and
    /// covered by the token signature.
    last_key: Option<ChangePageKeyV1>,
}

pub(super) fn parse_signed(
    lens: Lens,
    raw: Option<&str>,
    signer: &PageTokenSigner,
) -> Result<Request, PageError> {
    let Some(raw) = raw.filter(|raw| !raw.is_empty()) else {
        return Ok(Request::Bare);
    };
    let mut fields = BTreeMap::new();
    for pair in raw.split('&') {
        let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
        let key = decode(key)?;
        if !matches!(
            key.as_str(),
            "limit"
                | "after"
                | "q"
                | "topology"
                | "lifecycle"
                | "attention"
                | "availability"
                | "order"
        ) {
            return Err(invalid("unknown query field"));
        }
        if fields.insert(key, decode(value)?).is_some() {
            return Err(invalid("duplicate query field"));
        }
    }
    let nonempty = |name: &str| -> Result<Option<String>, PageError> {
        fields
            .get(name)
            .map(|v| {
                if v.is_empty() {
                    Err(invalid("empty query field"))
                } else {
                    Ok(v.clone())
                }
            })
            .transpose()
    };
    let limit = match nonempty("limit")? {
        Some(v) if v.bytes().all(|byte| byte.is_ascii_digit()) => v
            .parse()
            .ok()
            .filter(|v: &usize| (1..=100).contains(v))
            .ok_or_else(|| invalid("invalid limit"))?,
        Some(_) => return Err(invalid("invalid limit")),
        None => 50,
    };
    let order = match nonempty("order")? {
        Some(raw) => ChangeListOrderV1::parse(&raw)
            .filter(|order| lens.admits(*order))
            .ok_or_else(|| invalid("invalid order"))?,
        None => lens.default_order(),
    };
    let q = nonempty("q")?
        .map(|v| v.trim().to_owned())
        .filter(|v| !v.is_empty());
    if fields.contains_key("q") && q.is_none() {
        return Err(invalid("empty query field"));
    }
    if q.as_ref().is_some_and(|v| v.len() > 256) {
        return Err(invalid("query is too long"));
    }
    let raw_q = q;
    let q = raw_q.as_ref().map(|value| value.to_lowercase());
    let topology = domain(
        nonempty("topology")?,
        &[
            "initial",
            "replacement",
            "replacement_divergent",
            "consolidation",
            "parallel_current",
            "mixed",
            "incomplete",
            "cycle_conflicted",
        ],
    )?;
    let lifecycle = domain(
        nonempty("lifecycle")?,
        &["incomplete", "conflicted", "in_progress", "accepted"],
    )?;
    let attention = domain(
        nonempty("attention")?,
        &["clear", "in_progress", "incomplete", "conflicted"],
    )?;
    let availability = domain(nonempty("availability")?, &["available", "incomplete"])?;
    let after = nonempty("after")?
        .map(|v| decode_token(&v, signer))
        .transpose()?;
    Ok(Request::Bounded(Box::new(Query {
        lens,
        limit,
        order,
        after,
        raw_q,
        q,
        topology,
        lifecycle,
        attention,
        availability,
    })))
}

pub(super) fn apply_signed(
    mut document: Value,
    query: Query,
    signer: &PageTokenSigner,
) -> Result<Value, PageError> {
    let stamp = document["projectionStamp"]
        .as_str()
        .ok_or_else(|| invalid("missing projection stamp"))?
        .to_owned();
    let identity = query.identity();
    let order = query.order;
    let after = if let Some(token) = &query.after {
        if token.lens != query.lens
            || token.query != identity
            || token.limit != query.limit
            || token.order != order
        {
            return Err(invalid("continuation does not match request"));
        }
        if token.projection_stamp != stamp {
            return Err(PageError::Stale);
        }
        token.last_key.as_ref()
    } else {
        None
    };
    let mut changes = document["changes"]
        .as_array()
        .cloned()
        .ok_or_else(|| invalid("missing changes"))?;
    if query.lens == Lens::Attention {
        changes.retain(|c| c["lifecycle"] != "accepted");
    }
    changes.retain(|c| query.matches(c, &document["presentations"]));
    changes.sort_by(|a, b| compare_change_order(order, page_order_key(a), page_order_key(b)));
    let page_start = after
        .map(|last| {
            let last = last.order_key();
            changes.partition_point(|change| {
                compare_change_order(order, page_order_key(change), last).is_le()
            })
        })
        .unwrap_or(0);
    let page_end = changes.len().min(page_start.saturating_add(query.limit));
    let last_page_start = changes
        .len()
        .checked_sub(1)
        .map(|last_index| (last_index / query.limit) * query.limit);
    let boundary_before = |start: usize| {
        start
            .checked_sub(1)
            .map(|index| ChangePageKeyV1::from_order_key(order, page_order_key(&changes[index])))
    };
    let issue = |last_key: Option<ChangePageKeyV1>| {
        encode_token(
            &Token {
                schema: TOKEN_SCHEMA.into(),
                lens: query.lens,
                projection_stamp: stamp.clone(),
                query: identity.clone(),
                limit: query.limit,
                order,
                last_key,
            },
            signer,
        )
    };
    let previous = (page_start > 0)
        .then(|| {
            let previous_start = page_start.saturating_sub(query.limit);
            issue(boundary_before(previous_start))
        })
        .transpose()?;
    let next = (page_end < changes.len())
        .then(|| issue(boundary_before(page_end)))
        .transpose()?;
    let last = last_page_start
        .filter(|last_start| *last_start != page_start)
        .map(|last_start| issue(boundary_before(last_start)))
        .transpose()?;
    changes = changes
        .into_iter()
        .skip(page_start)
        .take(query.limit)
        .collect();
    let emitted: BTreeSet<_> = changes
        .iter()
        .filter_map(|c| c["changeId"].as_str())
        .collect();
    let presentations_empty = document
        .get_mut("presentations")
        .and_then(Value::as_object_mut)
        .is_some_and(|map| {
            map.retain(|id, _| emitted.contains(id.as_str()));
            map.is_empty()
        });
    if presentations_empty {
        document
            .as_object_mut()
            .expect("validated Change page document is an object")
            .remove("presentations");
    }
    // The client treats every page capability as opaque. Signing makes that
    // boundary enforceable: callers cannot alter a target boundary to skip or
    // revisit rows, and capabilities die with the Inspector process that issued them.
    document["order"] = Value::String(order.as_str().to_owned());
    document["changes"] = Value::Array(changes);
    document["previous"] = previous.map(Value::String).unwrap_or(Value::Null);
    document["next"] = next.map(Value::String).unwrap_or(Value::Null);
    document["last"] = last.map(Value::String).unwrap_or(Value::Null);
    Ok(document)
}

impl Query {
    fn derived_request(&self) -> Result<DerivedChangePageRequestV1, PageError> {
        let after = self
            .after
            .as_ref()
            .map(|token| {
                if token.lens != self.lens
                    || token.query != self.identity()
                    || token.limit != self.limit
                    || token.order != self.order
                {
                    return Err(invalid("continuation does not match request"));
                }
                let boundary = token.last_key.clone().map_or_else(
                    DerivedChangePageBoundaryV1::page_one,
                    DerivedChangePageBoundaryV1::after,
                );
                DerivedChangePageContinuationV1::new(token.projection_stamp.clone(), boundary)
                    .map_err(|error| PageError::Invalid(error.to_string()))
            })
            .transpose()?;
        let topology = self.topology.as_deref().map(|topology| match topology {
            "initial" => ChangeTopologyV1::Initial,
            "replacement" => ChangeTopologyV1::Replacement,
            "replacement_divergent" => ChangeTopologyV1::ReplacementDivergent,
            "consolidation" => ChangeTopologyV1::Consolidation,
            "parallel_current" => ChangeTopologyV1::ParallelCurrent,
            "mixed" => ChangeTopologyV1::Mixed,
            "incomplete" => ChangeTopologyV1::Incomplete,
            "cycle_conflicted" => ChangeTopologyV1::CycleConflicted,
            _ => unreachable!("query parser admits only frozen Change topologies"),
        });
        let lifecycle = self.lifecycle.as_deref().map(|lifecycle| match lifecycle {
            "incomplete" => ChangeLifecycleV1::Incomplete,
            "conflicted" => ChangeLifecycleV1::Conflicted,
            "in_progress" => ChangeLifecycleV1::InProgress,
            "accepted" => ChangeLifecycleV1::Accepted,
            _ => unreachable!("query parser admits only frozen Change lifecycles"),
        });
        let attention = self.attention.as_deref().map(|attention| match attention {
            "clear" => DerivedChangeAttentionFilterV1::Clear,
            "in_progress" => DerivedChangeAttentionFilterV1::InProgress,
            "incomplete" => DerivedChangeAttentionFilterV1::Incomplete,
            "conflicted" => DerivedChangeAttentionFilterV1::Conflicted,
            _ => unreachable!("query parser admits only frozen Attention filters"),
        });
        let availability = self
            .availability
            .as_deref()
            .map(|availability| match availability {
                "available" => DerivedChangeAvailabilityFilterV1::Available,
                "incomplete" => DerivedChangeAvailabilityFilterV1::Incomplete,
                _ => unreachable!("query parser admits only frozen availability filters"),
            });
        DerivedChangePageSelectionV1::new(
            self.limit,
            after,
            self.raw_q.clone(),
            topology,
            lifecycle,
            attention,
            availability,
        )
        .map(|selection| selection.with_order(self.order))
        .map(DerivedChangePageRequestV1::Bounded)
        .map_err(|error| PageError::Invalid(error.to_string()))
    }

    fn identity(&self) -> String {
        format!(
            "limit={}&q={:?}&topology={:?}&lifecycle={:?}&attention={:?}&availability={:?}&order={}&lens={}",
            self.limit,
            self.q,
            self.topology,
            self.lifecycle,
            self.attention,
            self.availability,
            self.order.as_str(),
            match self.lens {
                Lens::Changes => "changes",
                Lens::Attention => "attention",
            }
        )
    }
    fn matches(&self, c: &Value, presentations: &Value) -> bool {
        if self.topology.as_deref().is_some_and(|v| c["topology"] != v)
            || self
                .lifecycle
                .as_deref()
                .is_some_and(|v| c["lifecycle"] != v)
            || self
                .attention
                .as_deref()
                .is_some_and(|v| c["attentionSummary"] != v)
            || self
                .availability
                .as_deref()
                .is_some_and(|v| c["availabilitySummary"] != v)
        {
            return false;
        }
        let Some(q) = &self.q else { return true };
        let id = c["changeId"].as_str().unwrap_or("");
        if id.to_lowercase().contains(q) {
            return true;
        }
        c["currentRevisionRefs"]
            .as_array()
            .into_iter()
            .flatten()
            .any(|r| {
                r["revisionId"]
                    .as_str()
                    .is_some_and(|id| id.to_lowercase().contains(q))
            })
            || presentations
                .get(id)
                .and_then(|p| p["currentRevisions"].as_array())
                .into_iter()
                .flatten()
                .any(|r| {
                    r["revisionProposalSummary"]
                        .as_str()
                        .is_some_and(|s| s.to_lowercase().contains(q))
                })
    }
}

fn encode_token(token: &Token, signer: &PageTokenSigner) -> Result<String, PageError> {
    let encoded = signer.encode(token);
    if encoded.len() > MAX_TOKEN_BYTES {
        Err(invalid("continuation is too long"))
    } else {
        Ok(encoded)
    }
}

fn decode_token(raw: &str, signer: &PageTokenSigner) -> Result<Token, PageError> {
    if raw.len() > MAX_TOKEN_BYTES {
        return Err(invalid("continuation is too long"));
    }
    let t: Token = signer
        .decode(raw)
        .map_err(|()| invalid("malformed continuation"))?;
    if t.schema != TOKEN_SCHEMA
        || t.projection_stamp.is_empty()
        || t.last_key
            .as_ref()
            .is_some_and(|key| key.order() != t.order || key.change_id().is_empty())
    {
        Err(invalid("malformed continuation"))
    } else {
        Ok(t)
    }
}
fn domain(value: Option<String>, allowed: &[&str]) -> Result<Option<String>, PageError> {
    if value
        .as_ref()
        .is_some_and(|v| !allowed.contains(&v.as_str()))
    {
        Err(invalid("invalid query value"))
    } else {
        Ok(value)
    }
}
fn decode(raw: &str) -> Result<String, PageError> {
    let mut out = Vec::new();
    let b = raw.as_bytes();
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'%' => {
                if i + 2 >= b.len() {
                    return Err(invalid("invalid percent encoding"));
                }
                let h = (b[i + 1] as char)
                    .to_digit(16)
                    .ok_or_else(|| invalid("invalid percent encoding"))?;
                let l = (b[i + 2] as char)
                    .to_digit(16)
                    .ok_or_else(|| invalid("invalid percent encoding"))?;
                out.push((h * 16 + l) as u8);
                i += 3
            }
            b'+' => {
                out.push(b' ');
                i += 1
            }
            x => {
                out.push(x);
                i += 1
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
    use base64::Engine as _;
    use base64::engine::general_purpose::URL_SAFE_NO_PAD;
    use pointbreak::model::RevisionId;
    use pointbreak::session::{
        ChangeLifecycleV1, ChangeTopologyV1, DerivedChangeAttentionFilterV1,
        DerivedChangeAvailabilityFilterV1, DerivedChangePageBoundaryV1, DerivedChangePageRequestV1,
        DerivedChangePageWindowV1,
    };

    use super::*;
    fn signer() -> PageTokenSigner {
        PageTokenSigner::from_seed([7_u8; 32])
    }
    fn parse(lens: Lens, raw: Option<&str>) -> Result<Request, PageError> {
        parse_signed(lens, raw, &signer())
    }
    fn apply(document: Value, query: Query) -> Result<Value, PageError> {
        apply_signed(document, query, &signer())
    }
    fn doc() -> Value {
        serde_json::json!({"schema":"pointbreak.inspect-changes-page","version":1,"projectionStamp":"stamp-1","changes":[
   {"changeId":"change:01","topology":"initial","lifecycle":"accepted","attentionSummary":"clear","availabilitySummary":"available","currentRevisionRefs":[{"revisionId":"rev:one"}]},
   {"changeId":"change:02","topology":"parallel_current","lifecycle":"in_progress","attentionSummary":"in_progress","availabilitySummary":"available","currentRevisionRefs":[{"revisionId":"rev:two"}]},
   {"changeId":"change:03","topology":"incomplete","lifecycle":"incomplete","attentionSummary":"incomplete","availabilitySummary":"incomplete","currentRevisionRefs":[{"revisionId":"rev:three"}]}
 ],"presentations":{"change:01":{"currentRevisions":[]},"change:02":{"currentRevisions":[{"revisionProposalSummary":"Need Unicode STRASSE"}]},"change:03":{"currentRevisions":[]}}})
    }

    #[test]
    fn derived_request_mapping_authenticates_every_binding_before_reader_access() {
        let parsed = parse(
            Lens::Attention,
            Some(
                "limit=7&q=%C2%A0%C3%84PFEL%E3%80%80&topology=parallel_current&lifecycle=in_progress&attention=conflicted&availability=incomplete&order=change_id_asc",
            ),
        )
        .expect("frozen Change query parses");
        let derived = parsed
            .derived_request()
            .expect("authenticated query maps to the neutral reader request");
        let DerivedChangePageRequestV1::Bounded(selection) = derived else {
            panic!("query must remain bounded")
        };
        assert_eq!(selection.limit(), 7);
        assert_eq!(selection.summary_query(), Some("äpfel"));
        assert_eq!(
            selection.topology(),
            Some(ChangeTopologyV1::ParallelCurrent)
        );
        assert_eq!(selection.lifecycle(), Some(ChangeLifecycleV1::InProgress));
        assert_eq!(
            selection.attention_filter(),
            Some(DerivedChangeAttentionFilterV1::Conflicted)
        );
        assert_eq!(
            selection.availability_filter(),
            Some(DerivedChangeAvailabilityFilterV1::Incomplete)
        );

        let mismatched = encode_token(
            &Token {
                schema: TOKEN_SCHEMA.to_owned(),
                lens: Lens::Changes,
                projection_stamp: "stamp-1".to_owned(),
                query: "different-query".to_owned(),
                limit: 7,
                order: ChangeListOrderV1::ChangeIdAsc,
                last_key: Some(ChangePageKeyV1::ChangeIdAsc {
                    change_id: "change:01".to_owned(),
                }),
            },
            &signer(),
        )
        .unwrap();
        let parsed = parse(
            Lens::Attention,
            Some(&format!("limit=7&order=change_id_asc&after={mismatched}")),
        )
        .expect("token bytes authenticate before binding validation");
        assert!(matches!(
            parsed.derived_request(),
            Err(PageError::Invalid(message)) if message == "continuation does not match request"
        ));
    }

    #[test]
    fn derived_window_signing_does_not_refilter_the_reader_page() {
        let parsed = parse(Lens::Changes, Some("limit=1&q=needle&order=change_id_asc")).unwrap();
        let document = serde_json::json!({
            "schema": "pointbreak.inspect-changes-page",
            "version": 1,
            "order": "change_id_asc",
            "projectionStamp": "stamp-2",
            "changes": [{"changeId": "change:02", "currentRevisionRefs": [{
                "revisionId": RevisionId::new("revision:02"),
                "objectArtifactContentHash": "sha256:02"
            }]}],
            "presentations": {"change:02": {"currentRevisions": []}}
        });
        let window = DerivedChangePageWindowV1 {
            projection_stamp: "stamp-2".to_owned(),
            previous: Some(DerivedChangePageBoundaryV1::page_one()),
            next: Some(DerivedChangePageBoundaryV1::after(
                ChangePageKeyV1::ChangeIdAsc {
                    change_id: "change:02".to_owned(),
                },
            )),
            last: Some(DerivedChangePageBoundaryV1::after(
                ChangePageKeyV1::ChangeIdAsc {
                    change_id: "change:09".to_owned(),
                },
            )),
        };
        let rendered = parsed
            .apply_derived_window(document.clone(), Some(&window), &signer())
            .expect("reader-selected page receives signed opaque boundaries");
        assert_eq!(rendered["changes"], document["changes"]);
        assert_eq!(rendered["presentations"], document["presentations"]);
        for member in ["previous", "next", "last"] {
            let encoded = rendered[member].as_str().expect("signed capability");
            let token = decode_token(encoded, &signer()).expect("decode issued capability");
            assert_eq!(token.projection_stamp, "stamp-2");
            assert_eq!(
                token.query,
                match &parsed {
                    Request::Bounded(query) => query.identity(),
                    Request::Bare => unreachable!(),
                }
            );
        }
    }
    #[test]
    fn strict_grammar_rejects_unknown_duplicate_empty_and_order() {
        for q in [
            "wat=x",
            "order=activity_asc",
            "order=activity-desc",
            "q=%FF",
            "q=%2G",
            "q=%",
        ] {
            assert!(
                matches!(parse(Lens::Changes, Some(q)), Err(PageError::Invalid(_))),
                "{q}"
            );
        }
        for field in [
            "limit",
            "after",
            "q",
            "topology",
            "lifecycle",
            "attention",
            "availability",
            "order",
        ] {
            assert!(
                matches!(
                    parse(Lens::Changes, Some(&format!("{field}="))),
                    Err(PageError::Invalid(_))
                ),
                "empty {field}"
            );
            let value = if field == "limit" {
                "1"
            } else if field == "order" {
                "activity_desc"
            } else {
                "x"
            };
            assert!(
                matches!(
                    parse(
                        Lens::Changes,
                        Some(&format!("{field}={value}&{field}={value}"))
                    ),
                    Err(PageError::Invalid(_))
                ),
                "duplicate {field}"
            );
        }
    }
    #[test]
    fn grammar_enforces_ascii_limit_and_after_size_bounds() {
        for valid in ["1", "50", "100", "001"] {
            assert!(parse(Lens::Changes, Some(&format!("limit={valid}"))).is_ok());
        }
        for invalid in ["0", "101", "-1", "+1", "1.0", "１２"] {
            assert!(matches!(
                parse(Lens::Changes, Some(&format!("limit={invalid}"))),
                Err(PageError::Invalid(_))
            ));
        }
        assert!(matches!(
            parse(Lens::Changes, Some(&format!("after={}", "a".repeat(4097)))),
            Err(PageError::Invalid(_))
        ));
    }
    #[test]
    fn grammar_accepts_every_frozen_enum_and_the_admitted_orders() {
        for (field, values) in [
            (
                "topology",
                &[
                    "initial",
                    "replacement",
                    "replacement_divergent",
                    "consolidation",
                    "parallel_current",
                    "mixed",
                    "incomplete",
                    "cycle_conflicted",
                ][..],
            ),
            (
                "lifecycle",
                &["incomplete", "conflicted", "in_progress", "accepted"][..],
            ),
            (
                "attention",
                &["clear", "in_progress", "incomplete", "conflicted"][..],
            ),
            ("availability", &["available", "incomplete"][..]),
            ("order", &["change_id_asc", "activity_desc"][..]),
        ] {
            for value in values {
                assert!(
                    parse(Lens::Changes, Some(&format!("{field}={value}"))).is_ok(),
                    "{field}={value}"
                );
            }
        }
        for field in [
            "topology",
            "lifecycle",
            "attention",
            "availability",
            "order",
        ] {
            assert!(matches!(
                parse(Lens::Changes, Some(&format!("{field}=other"))),
                Err(PageError::Invalid(_))
            ));
        }
    }
    #[test]
    fn unicode_trim_and_lowercase_vectors_are_stable() {
        for (raw, expected) in [
            ("q=%C2%A0%C4%B0STANBUL%C2%A0", "i\u{307}stanbul"),
            ("q=%E3%80%80%C3%84PFEL%E3%80%80", "äpfel"),
            ("q=%C2%85MIXED%C2%85", "mixed"),
        ] {
            let Request::Bounded(query) = parse(Lens::Changes, Some(raw)).unwrap() else {
                panic!()
            };
            assert_eq!(query.q.as_deref(), Some(expected));
        }

        let boundary = format!("q={}", "%C4%B0".repeat(128));
        let Request::Bounded(query) = parse(Lens::Changes, Some(&boundary)).unwrap() else {
            panic!()
        };
        assert_eq!(query.raw_q.as_ref().unwrap().len(), 256);
        assert!(query.q.as_ref().unwrap().len() > 256);
        let DerivedChangePageRequestV1::Bounded(selection) = query
            .derived_request()
            .expect("the neutral reader checks the raw query before lowercase expansion")
        else {
            panic!()
        };
        assert_eq!(selection.summary_query(), query.q.as_deref());
    }
    #[test]
    fn equivalent_defaults_share_identity() {
        let Request::Bounded(a) = parse(Lens::Changes, Some("limit=50")).unwrap() else {
            panic!()
        };
        let Request::Bounded(b) =
            parse(Lens::Changes, Some("order=activity_desc&limit=50")).unwrap()
        else {
            panic!()
        };
        assert_eq!(a.identity(), b.identity());
    }
    #[test]
    fn pages_strictly_after_last_id_and_filters_presentations() {
        let Request::Bounded(q) = parse(Lens::Changes, Some("limit=1")).unwrap() else {
            panic!()
        };
        let first = apply(doc(), *q).unwrap();
        assert_eq!(first["changes"][0]["changeId"], "change:01");
        assert_eq!(first["presentations"].as_object().unwrap().len(), 1);
        let token = first["next"].as_str().unwrap();
        assert!(token.len() <= 4096);
        let Request::Bounded(q) =
            parse(Lens::Changes, Some(&format!("limit=1&after={token}"))).unwrap()
        else {
            panic!()
        };
        let second = apply(doc(), *q).unwrap();
        assert_eq!(second["changes"][0]["changeId"], "change:02");
        assert_eq!(
            second["presentations"]
                .as_object()
                .unwrap()
                .keys()
                .collect::<Vec<_>>(),
            vec!["change:02"]
        );
        let token = second["next"].as_str().unwrap();
        let Request::Bounded(q) =
            parse(Lens::Changes, Some(&format!("limit=1&after={token}"))).unwrap()
        else {
            panic!()
        };
        let third = apply(doc(), *q).unwrap();
        assert_eq!(third["changes"][0]["changeId"], "change:03");
        assert!(third["next"].is_null());
    }

    #[test]
    fn strict_filter_omits_an_empty_presentation_map() {
        let Request::Bounded(query) =
            parse(Lens::Changes, Some("limit=100&topology=consolidation")).unwrap()
        else {
            panic!()
        };

        let page = apply(doc(), *query).unwrap();

        assert_eq!(page["changes"], serde_json::json!([]));
        assert!(!page.as_object().unwrap().contains_key("presentations"));
    }

    #[test]
    fn first_page_declares_no_previous_and_issues_an_opaque_last_capability() {
        let Request::Bounded(q) = parse(Lens::Changes, Some("limit=1")).unwrap() else {
            panic!()
        };
        let first = apply(doc(), *q).unwrap();

        assert!(
            first.as_object().unwrap().contains_key("previous"),
            "a bounded first page must explicitly declare previous: null"
        );
        assert!(first["previous"].is_null());
        let last = first["last"]
            .as_str()
            .expect("the server must issue an opaque last-page capability");
        assert!(!last.is_empty());
        assert!(last.len() <= 4096);
    }

    #[test]
    fn capability_issuance_refuses_an_oversized_projected_boundary() {
        let mut document = doc();
        document["changes"][1]["changeId"] = format!("change:02{}", "x".repeat(4096)).into();
        let Request::Bounded(q) = parse(Lens::Changes, Some("limit=1")).unwrap() else {
            panic!()
        };

        assert_eq!(
            apply(document, *q),
            Err(PageError::Invalid("continuation is too long".into()))
        );
    }

    #[test]
    fn continuation_page_issues_previous_that_returns_to_the_first_page() {
        let Request::Bounded(q) = parse(Lens::Changes, Some("limit=1")).unwrap() else {
            panic!()
        };
        let first = apply(doc(), *q).unwrap();
        let next = first["next"].as_str().unwrap();
        let Request::Bounded(q) =
            parse(Lens::Changes, Some(&format!("limit=1&after={next}"))).unwrap()
        else {
            panic!()
        };
        let second = apply(doc(), *q).unwrap();
        assert_eq!(second["changes"][0]["changeId"], "change:02");

        let previous = second["previous"]
            .as_str()
            .expect("a continuation page must receive a server-issued previous capability");
        let Request::Bounded(q) =
            parse(Lens::Changes, Some(&format!("limit=1&after={previous}"))).unwrap()
        else {
            panic!()
        };
        let returned = apply(doc(), *q).unwrap();
        assert_eq!(returned["changes"][0]["changeId"], "change:01");
        assert!(returned["previous"].is_null());
    }

    #[test]
    fn last_capability_reaches_the_tail_without_a_client_derived_predecessor() {
        let Request::Bounded(q) = parse(Lens::Changes, Some("limit=1")).unwrap() else {
            panic!()
        };
        let first = apply(doc(), *q).unwrap();
        let last = first["last"]
            .as_str()
            .expect("the server must issue an opaque last-page capability");
        let Request::Bounded(q) =
            parse(Lens::Changes, Some(&format!("limit=1&after={last}"))).unwrap()
        else {
            panic!()
        };
        let tail = apply(doc(), *q).unwrap();

        assert_eq!(tail["changes"][0]["changeId"], "change:03");
        assert!(tail["next"].is_null());
    }
    #[test]
    fn tokens_are_lens_query_and_projection_bound() {
        let Request::Bounded(q) = parse(Lens::Changes, Some("limit=1")).unwrap() else {
            panic!()
        };
        let first = apply(doc(), *q).unwrap();
        let token = first["next"].as_str().unwrap();
        let Request::Bounded(q) =
            parse(Lens::Attention, Some(&format!("limit=1&after={token}"))).unwrap()
        else {
            panic!()
        };
        assert!(matches!(apply(doc(), *q), Err(PageError::Invalid(_))));
        let mut stale = doc();
        stale["projectionStamp"] = "stamp-2".into();
        let Request::Bounded(q) =
            parse(Lens::Changes, Some(&format!("limit=1&after={token}"))).unwrap()
        else {
            panic!()
        };
        assert_eq!(apply(stale, *q), Err(PageError::Stale));
    }
    #[test]
    fn attention_prefilters_accepted_before_shared_page() {
        let Request::Bounded(q) = parse(Lens::Attention, Some("limit=100")).unwrap() else {
            panic!()
        };
        let page = apply(doc(), *q).unwrap();
        assert_eq!(page["changes"][0]["changeId"], "change:02");
        let ids = page["changes"]
            .as_array()
            .unwrap()
            .iter()
            .map(|c| c["changeId"].as_str().unwrap())
            .collect::<Vec<_>>();
        assert_eq!(ids, vec!["change:02", "change:03"]);
    }

    #[test]
    fn search_is_limited_to_ids_and_sourced_proposal_summaries() {
        for (query, expected) in [
            ("q=change%3A02", "change:02"),
            ("q=02", "change:02"),
            ("q=rev%3Atwo", "change:02"),
            ("q=two", "change:02"),
            ("q=unicode+strasse", "change:02"),
        ] {
            let Request::Bounded(q) = parse(Lens::Changes, Some(query)).unwrap() else {
                panic!()
            };
            let page = apply(doc(), *q).unwrap();
            assert_eq!(page["changes"][0]["changeId"], expected);
        }
        let mut source = doc();
        source["changes"][0]["diagnostics"] = serde_json::json!(["secret body phrase"]);
        let Request::Bounded(q) = parse(Lens::Changes, Some("q=secret+body")).unwrap() else {
            panic!()
        };
        assert!(
            apply(source, *q).unwrap()["changes"]
                .as_array()
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn every_status_filter_reads_projected_fields() {
        for (query, expected) in [
            ("topology=parallel_current", "change:02"),
            ("lifecycle=accepted", "change:01"),
            ("attention=incomplete", "change:03"),
            ("availability=incomplete", "change:03"),
        ] {
            let Request::Bounded(q) = parse(Lens::Changes, Some(query)).unwrap() else {
                panic!()
            };
            let page = apply(doc(), *q).unwrap();
            assert_eq!(page["changes"].as_array().unwrap().len(), 1);
            assert_eq!(page["changes"][0]["changeId"], expected);
        }
    }

    fn mutate_token(token: &str, mutate: impl FnOnce(&mut Value)) -> String {
        let payload = token.split_once('.').unwrap().0;
        let bytes = URL_SAFE_NO_PAD.decode(payload).unwrap();
        let mut value: Value = serde_json::from_slice(&bytes).unwrap();
        mutate(&mut value);
        let token: Token = serde_json::from_value(value).unwrap();
        encode_token(&token, &signer()).unwrap()
    }

    fn tamper_payload_keep_signature(token: &str) -> String {
        let (payload, signature) = token.split_once('.').unwrap();
        let mut bytes = URL_SAFE_NO_PAD.decode(payload).unwrap();
        let index = bytes.iter().position(|byte| *byte == b'0').unwrap();
        bytes[index] = b'9';
        format!("{}.{}", URL_SAFE_NO_PAD.encode(bytes), signature)
    }

    #[test]
    fn malformed_and_every_token_binding_mismatch_are_invalid_except_stale_stamp() {
        let Request::Bounded(q) = parse(Lens::Changes, Some("limit=1")).unwrap() else {
            panic!()
        };
        let first = apply(doc(), *q).unwrap();
        let token = first["next"].as_str().unwrap();
        assert!(matches!(
            parse_signed(
                Lens::Changes,
                Some(&format!("limit=1&after={token}")),
                &PageTokenSigner::from_seed([8_u8; 32])
            ),
            Err(PageError::Invalid(_))
        ));
        for bad in [
            "not-base64".to_owned(),
            tamper_payload_keep_signature(token),
            // A key issued under one order presented as another order.
            mutate_token(token, |v| v["order"] = "change_id_asc".into()),
        ] {
            assert!(matches!(
                parse(Lens::Changes, Some(&format!("limit=1&after={bad}"))),
                Err(PageError::Invalid(_))
            ));
        }
        for query in [
            format!("limit=2&after={token}"),
            format!("limit=1&q=x&after={token}"),
        ] {
            let Request::Bounded(q) = parse(Lens::Changes, Some(&query)).unwrap() else {
                panic!()
            };
            assert!(matches!(apply(doc(), *q), Err(PageError::Invalid(_))));
        }
        let Request::Bounded(q) =
            parse(Lens::Attention, Some(&format!("limit=1&after={token}"))).unwrap()
        else {
            panic!()
        };
        assert!(matches!(apply(doc(), *q), Err(PageError::Invalid(_))));
        let stale = mutate_token(token, |v| v["projectionStamp"] = "old".into());
        let Request::Bounded(q) =
            parse(Lens::Changes, Some(&format!("limit=1&after={stale}"))).unwrap()
        else {
            panic!()
        };
        assert_eq!(apply(doc(), *q), Err(PageError::Stale));
    }

    #[test]
    fn previous_and_last_keep_typed_lens_query_tamper_and_stale_refusals() {
        let Request::Bounded(q) = parse(Lens::Changes, Some("limit=1")).unwrap() else {
            panic!()
        };
        let first = apply(doc(), *q).unwrap();
        let next = first["next"].as_str().unwrap();
        let last = first["last"]
            .as_str()
            .expect("the server must issue an opaque last-page capability")
            .to_owned();
        let Request::Bounded(q) =
            parse(Lens::Changes, Some(&format!("limit=1&after={next}"))).unwrap()
        else {
            panic!()
        };
        let second = apply(doc(), *q).unwrap();
        let previous = second["previous"]
            .as_str()
            .expect("a continuation page must receive a server-issued previous capability")
            .to_owned();

        for (name, token) in [("previous", previous), ("last", last)] {
            let Request::Bounded(q) =
                parse(Lens::Attention, Some(&format!("limit=1&after={token}"))).unwrap()
            else {
                panic!()
            };
            assert!(
                matches!(apply(doc(), *q), Err(PageError::Invalid(_))),
                "{name} must remain bound to its issuing lens"
            );

            for query in [
                format!("limit=2&after={token}"),
                format!("limit=1&q=other&after={token}"),
            ] {
                let Request::Bounded(q) = parse(Lens::Changes, Some(&query)).unwrap() else {
                    panic!()
                };
                assert!(
                    matches!(apply(doc(), *q), Err(PageError::Invalid(_))),
                    "{name} must remain bound to its issuing query"
                );
            }

            assert!(matches!(
                parse(Lens::Changes, Some(&format!("limit=1&after={token}x"))),
                Err(PageError::Invalid(_))
            ));

            let mut stale = doc();
            stale["projectionStamp"] = "stamp-2".into();
            let Request::Bounded(q) =
                parse(Lens::Changes, Some(&format!("limit=1&after={token}"))).unwrap()
            else {
                panic!()
            };
            assert_eq!(
                apply(stale, *q),
                Err(PageError::Stale),
                "{name} must retain typed stale-projection refusal"
            );
        }
    }

    // ---- Change list order (activity_desc default, order-tagged keys) ----

    fn ordered_doc(changes: Value) -> Value {
        let ids: Vec<String> = changes
            .as_array()
            .unwrap()
            .iter()
            .map(|c| c["changeId"].as_str().unwrap().to_owned())
            .collect();
        let mut presentations = serde_json::Map::new();
        for id in ids {
            presentations.insert(id, serde_json::json!({"currentRevisions": []}));
        }
        serde_json::json!({
            "schema": "pointbreak.inspect-changes-page",
            "version": 1,
            "projectionStamp": "stamp-1",
            "changes": changes,
            "presentations": presentations
        })
    }

    fn ordered_change(id: &str, activity_at: Option<&str>) -> Value {
        let mut change = serde_json::json!({
            "changeId": id,
            "topology": "initial",
            "lifecycle": "in_progress",
            "attentionSummary": "in_progress",
            "availabilitySummary": "available",
            "currentRevisionRefs": []
        });
        if let Some(at) = activity_at {
            change["activityAt"] = at.into();
        }
        change
    }

    fn document_with_activity() -> Value {
        ordered_doc(serde_json::json!([
            ordered_change("change:sha256:0a1f", Some("2026-09-12T18:04:00.000Z")),
            ordered_change("change:sha256:3b77", Some("2026-09-12T19:20:00.000Z")),
            ordered_change("change:sha256:91cd", Some("unix-ms:1789000000000")),
        ]))
    }

    fn tied_activity_document() -> Value {
        ordered_doc(serde_json::json!([
            ordered_change("change:sha256:91cd", Some("2026-09-12T18:04:00.000Z")),
            ordered_change("change:sha256:0a1f", Some("2026-09-12T18:04:00.000Z")),
        ]))
    }

    fn mixed_activity_document() -> Value {
        ordered_doc(serde_json::json!([
            ordered_change("change:sha256:7e02", Some("unix-ms:not-a-number")),
            ordered_change("change:sha256:3b77", Some("2026-09-12T19:20:00.000Z")),
            ordered_change("change:sha256:0a1f", Some("2026-09-12T18:04:00.000Z")),
        ]))
    }

    fn page(document: Value, lens: Lens, raw: &str) -> Result<Value, PageError> {
        match parse(lens, Some(raw))? {
            Request::Bounded(query) => apply(document, *query),
            Request::Bare => panic!("ordered page tests always bound the request"),
        }
    }

    fn change_ids(page: &Value) -> Vec<String> {
        page["changes"]
            .as_array()
            .unwrap()
            .iter()
            .map(|c| c["changeId"].as_str().unwrap().to_owned())
            .collect()
    }

    fn next_token(page: &Value) -> String {
        page["next"]
            .as_str()
            .expect("a next page exists")
            .to_owned()
    }

    #[test]
    fn the_default_order_is_activity_descending() {
        let page = page(document_with_activity(), Lens::Changes, "limit=50").unwrap();

        assert_eq!(page["order"], "activity_desc");
        assert_eq!(
            change_ids(&page),
            [
                "change:sha256:3b77",
                "change:sha256:0a1f",
                "change:sha256:91cd"
            ]
        );
    }

    #[test]
    fn change_id_asc_remains_available_as_an_explicit_value() {
        let page = page(
            document_with_activity(),
            Lens::Changes,
            "limit=50&order=change_id_asc",
        )
        .unwrap();

        assert_eq!(page["order"], "change_id_asc");
        assert_eq!(
            change_ids(&page),
            [
                "change:sha256:0a1f",
                "change:sha256:3b77",
                "change:sha256:91cd"
            ]
        );
    }

    #[test]
    fn a_tie_on_activity_breaks_on_change_id_ascending() {
        let page = page(tied_activity_document(), Lens::Changes, "limit=50").unwrap();

        assert_eq!(
            change_ids(&page),
            ["change:sha256:0a1f", "change:sha256:91cd"]
        );
    }

    #[test]
    fn an_unreadable_activity_instant_sorts_last_under_activity_desc() {
        let page = page(mixed_activity_document(), Lens::Changes, "limit=50").unwrap();

        assert_eq!(
            change_ids(&page),
            [
                "change:sha256:3b77",
                "change:sha256:0a1f",
                "change:sha256:7e02"
            ]
        );
    }

    #[test]
    fn continuations_are_adjacent_without_overlap_under_activity_desc() {
        let first = page(document_with_activity(), Lens::Changes, "limit=2").unwrap();
        let token = next_token(&first);
        let second = page(
            document_with_activity(),
            Lens::Changes,
            &format!("limit=2&after={token}"),
        )
        .unwrap();

        let mut seen = change_ids(&first);
        seen.extend(change_ids(&second));
        assert_eq!(
            seen,
            [
                "change:sha256:3b77",
                "change:sha256:0a1f",
                "change:sha256:91cd"
            ]
        );
        assert_eq!(seen.len(), seen.iter().collect::<BTreeSet<_>>().len());
        assert!(second["next"].is_null());
        let previous = second["previous"].as_str().unwrap();
        let returned = page(
            document_with_activity(),
            Lens::Changes,
            &format!("limit=2&after={previous}"),
        )
        .unwrap();
        assert_eq!(change_ids(&returned), change_ids(&first));
    }

    #[test]
    fn a_continuation_issued_under_a_different_order_is_rejected() {
        let token = next_token(
            &page(
                document_with_activity(),
                Lens::Changes,
                "limit=2&order=change_id_asc",
            )
            .unwrap(),
        );

        let error = page(
            document_with_activity(),
            Lens::Changes,
            &format!("limit=2&order=activity_desc&after={token}"),
        )
        .unwrap_err();

        assert!(matches!(error, PageError::Invalid(_)));
    }

    #[test]
    fn a_continuation_is_stale_when_the_projection_stamp_moves() {
        let token = next_token(&page(document_with_activity(), Lens::Changes, "limit=2").unwrap());
        let mut moved = document_with_activity();
        moved["projectionStamp"] = "stamp-2".into();

        let error = page(moved, Lens::Changes, &format!("limit=2&after={token}")).unwrap_err();

        assert_eq!(error, PageError::Stale);
    }

    #[test]
    fn attention_wait_is_admitted_only_on_the_attention_lens() {
        assert!(matches!(
            parse(Lens::Changes, Some("limit=50&order=attention_wait")),
            Err(PageError::Invalid(_))
        ));
        let mut waiting = document_with_activity();
        waiting["changes"][0]["attentionWaitAt"] =
            serde_json::json!({"tierRank": 1, "oldestObservedAt": "2026-08-01T00:00:00.000Z"});
        waiting["changes"][2]["attentionWaitAt"] =
            serde_json::json!({"tierRank": 0, "oldestObservedAt": "2026-09-01T00:00:00.000Z"});
        let page = page(waiting, Lens::Attention, "limit=50").unwrap();

        assert_eq!(page["order"], "attention_wait");
        // Primary tier first, then the secondary, then the Change with no
        // anchored attention item last.
        assert_eq!(
            change_ids(&page),
            [
                "change:sha256:91cd",
                "change:sha256:0a1f",
                "change:sha256:3b77"
            ]
        );
    }

    #[test]
    fn order_specific_keys_are_carried_by_every_issued_capability() {
        let first = page(document_with_activity(), Lens::Changes, "limit=1").unwrap();
        let next = decode_token(first["next"].as_str().unwrap(), &signer()).unwrap();
        assert_eq!(next.order, ChangeListOrderV1::ActivityDesc);
        assert_eq!(
            next.last_key,
            Some(ChangePageKeyV1::ActivityDesc {
                activity_at: Some("2026-09-12T19:20:00.000Z".to_owned()),
                change_id: "change:sha256:3b77".to_owned(),
            })
        );
        let last = decode_token(first["last"].as_str().unwrap(), &signer()).unwrap();
        assert_eq!(
            last.last_key.as_ref().map(ChangePageKeyV1::change_id),
            Some("change:sha256:0a1f")
        );
        let derived = match parse(
            Lens::Changes,
            Some(&format!(
                "limit=1&after={}",
                first["next"].as_str().unwrap()
            )),
        )
        .unwrap()
        {
            Request::Bounded(query) => query.derived_request().unwrap(),
            Request::Bare => unreachable!(),
        };
        let DerivedChangePageRequestV1::Bounded(selection) = derived else {
            panic!("bounded")
        };
        assert_eq!(selection.order(), Some(ChangeListOrderV1::ActivityDesc));
        assert_eq!(
            selection.after().unwrap().boundary().last_key(),
            next.last_key.as_ref()
        );
    }
}
