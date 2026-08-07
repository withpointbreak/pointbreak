use std::collections::{BTreeMap, BTreeSet};

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use ed25519_dalek::{Signature, Signer as _, SigningKey, Verifier as _};
use serde::{Deserialize, Serialize};
use serde_json::Value;

const TOKEN_SCHEMA: &str = "pointbreak.inspect-change-page-token.v1";
const ORDER: &str = "change_id_asc";

pub(super) struct PageTokenSigner(SigningKey);

impl PageTokenSigner {
    pub(super) fn generate() -> Result<Self, getrandom::Error> {
        let mut seed = [0_u8; 32];
        getrandom::fill(&mut seed)?;
        Ok(Self(SigningKey::from_bytes(&seed)))
    }

    #[cfg(test)]
    pub(super) fn from_seed(seed: [u8; 32]) -> Self {
        Self(SigningKey::from_bytes(&seed))
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum Lens {
    Changes,
    Attention,
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

#[derive(Clone, Debug)]
pub(super) struct Query {
    lens: Lens,
    limit: usize,
    after: Option<Token>,
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
    order: String,
    last_change_id: String,
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
    if let Some(order) = nonempty("order")?
        && order != ORDER
    {
        return Err(invalid("invalid order"));
    }
    let q = nonempty("q")?
        .map(|v| v.trim().to_owned())
        .filter(|v| !v.is_empty());
    if fields.contains_key("q") && q.is_none() {
        return Err(invalid("empty query field"));
    }
    if q.as_ref().is_some_and(|v| v.len() > 256) {
        return Err(invalid("query is too long"));
    }
    let q = q.map(|v| v.to_lowercase());
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
        after,
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
    let after = if let Some(token) = &query.after {
        if token.lens != query.lens
            || token.query != identity
            || token.limit != query.limit
            || token.order != ORDER
        {
            return Err(invalid("continuation does not match request"));
        }
        if token.projection_stamp != stamp {
            return Err(PageError::Stale);
        }
        Some(token.last_change_id.as_str())
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
    changes.sort_by(|a, b| a["changeId"].as_str().cmp(&b["changeId"].as_str()));
    if let Some(last) = after {
        changes.retain(|c| c["changeId"].as_str().is_some_and(|id| id > last));
    }
    let has_more = changes.len() > query.limit;
    changes.truncate(query.limit);
    let emitted: BTreeSet<_> = changes
        .iter()
        .filter_map(|c| c["changeId"].as_str())
        .collect();
    if let Some(map) = document
        .get_mut("presentations")
        .and_then(Value::as_object_mut)
    {
        map.retain(|id, _| emitted.contains(id.as_str()));
    }
    // The client treats the continuation as opaque. Signing makes that boundary
    // enforceable: a caller cannot alter the last emitted ChangeId and silently
    // skip eligible rows, and a token dies with the Inspector process that issued it.
    let next = has_more
        .then(|| Token {
            schema: TOKEN_SCHEMA.into(),
            lens: query.lens,
            projection_stamp: stamp,
            query: identity,
            limit: query.limit,
            order: ORDER.into(),
            last_change_id: changes.last().unwrap()["changeId"].as_str().unwrap().into(),
        })
        .map(|t| encode_token(&t, signer));
    document["changes"] = Value::Array(changes);
    document["next"] = next.map(Value::String).unwrap_or(Value::Null);
    Ok(document)
}

impl Query {
    fn identity(&self) -> String {
        format!(
            "limit={}&q={:?}&topology={:?}&lifecycle={:?}&attention={:?}&availability={:?}&order={ORDER}&lens={}",
            self.limit,
            self.q,
            self.topology,
            self.lifecycle,
            self.attention,
            self.availability,
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

fn encode_token(token: &Token, signer: &PageTokenSigner) -> String {
    let payload = URL_SAFE_NO_PAD.encode(serde_json::to_vec(token).unwrap());
    let signature = signer.0.sign(payload.as_bytes());
    format!("{payload}.{}", URL_SAFE_NO_PAD.encode(signature.to_bytes()))
}

fn decode_token(raw: &str, signer: &PageTokenSigner) -> Result<Token, PageError> {
    if raw.len() > 4096 {
        return Err(invalid("continuation is too long"));
    }
    let (payload, encoded_signature) = raw
        .split_once('.')
        .filter(|(_, signature)| !signature.contains('.'))
        .ok_or_else(|| invalid("malformed continuation"))?;
    let signature_bytes = URL_SAFE_NO_PAD
        .decode(encoded_signature)
        .map_err(|_| invalid("malformed continuation"))?;
    let signature =
        Signature::from_slice(&signature_bytes).map_err(|_| invalid("malformed continuation"))?;
    signer
        .0
        .verifying_key()
        .verify(payload.as_bytes(), &signature)
        .map_err(|_| invalid("malformed continuation"))?;
    let bytes = URL_SAFE_NO_PAD
        .decode(payload)
        .map_err(|_| invalid("malformed continuation"))?;
    let t: Token = serde_json::from_slice(&bytes).map_err(|_| invalid("malformed continuation"))?;
    if t.schema != TOKEN_SCHEMA
        || t.order != ORDER
        || t.projection_stamp.is_empty()
        || t.last_change_id.is_empty()
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
    fn strict_grammar_rejects_unknown_duplicate_empty_and_order() {
        for q in ["wat=x", "order=activity_desc", "q=%FF", "q=%2G", "q=%"] {
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
                ORDER
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
    fn grammar_accepts_every_frozen_enum_and_only_change_id_order() {
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
            ("order", &["change_id_asc"][..]),
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
    }
    #[test]
    fn equivalent_defaults_share_identity() {
        let Request::Bounded(a) = parse(Lens::Changes, Some("limit=50")).unwrap() else {
            panic!()
        };
        let Request::Bounded(b) =
            parse(Lens::Changes, Some("order=change_id_asc&limit=50")).unwrap()
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
        encode_token(&token, &signer())
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
            mutate_token(token, |v| v["order"] = "other".into()),
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
}
