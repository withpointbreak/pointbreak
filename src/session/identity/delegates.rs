//! The delegation map: a sibling checked-in file (`.shore/delegates.json`, with
//! an optional locally-excluded `.shore/delegates.local.json` override layered
//! over it by the CLI discovery helper) that
//! records which human principal an agent actor writes on behalf of, scoped to
//! validity windows. It is deliberately separate from the allowed-signers trust
//! set — keys answer "who signed this", delegation answers "whose
//! responsibility is this write" — so key rotation never touches delegation.
//!
//! File shape (top-level key `delegates`; unknown top-level keys are ignored for
//! forward compatibility):
//!
//! ```json
//! {
//!   "delegates": {
//!     "actor:agent:claude-code": [
//!       {
//!         "principal": "actor:git-email:kevin@swiber.dev",
//!         "validFrom": "2026-06-10T00:00:00Z",
//!         "validUntil": null,
//!         "comment": "claude-code, enrolled by Kevin"
//!       }
//!     ]
//!   }
//! }
//! ```
//!
//! Each key is an `actor:agent:<name>` id mapping to an array of windowed
//! records. A record's `principal` must be a valid **non-agent** actor id in v1
//! (delegation chains have depth 0), `validFrom` is a required RFC 3339 UTC
//! instant, `validUntil` is null (open window) or an RFC 3339 UTC instant, and
//! `comment` is free text for diff readers — never authority.

use std::collections::BTreeMap;
use std::path::Path;

use serde_json::{Map, Value};

use super::instant::{parse_event_instant, parse_rfc3339_utc_millis};
use super::writer::{is_agent_actor_id, is_valid_principal_actor_id};
use crate::error::{Result, ShoreError};
use crate::model::ActorId;

/// A half-open validity window `[from_ms, until_ms)` over epoch milliseconds.
/// Extracted as the reusable mechanism ADR-0009/0010 name for trust-set
/// validity windows; it stays here until a second consumer exists.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct ValidityWindow {
    from_ms: i64,
    until_ms: Option<i64>,
}

impl ValidityWindow {
    /// True when `instant` falls in `[from_ms, until_ms)`: `from` is inclusive,
    /// `until` (when set) is exclusive, an absent `until` is an open window.
    fn contains(&self, instant: i64) -> bool {
        self.from_ms <= instant && self.until_ms.is_none_or(|until| instant < until)
    }
}

/// The outcome of resolving an agent actor's principal at a given `occurredAt`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PrincipalResolution {
    /// Exactly one principal covers the instant (after de-duplication).
    Resolved(ActorId),
    /// No principal could be resolved; the reason names the cheapest fix.
    None(UnresolvedReason),
    /// More than one distinct principal covers the instant. Sorted and deduped;
    /// surfaced, never auto-picked (ADR-0003 advisory posture).
    Ambiguous(Vec<ActorId>),
}

/// Why a principal could not be resolved. `NoDelegationMap` is the *caller's*
/// case — the options layer emits it when no map was supplied at all — so it is
/// absent here: `resolve` always has a map.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UnresolvedReason {
    /// The agent is absent from the map, including deliberately deleted
    /// (disavowed) records.
    NoDelegationRecord,
    /// The agent is enrolled but no window contains `occurredAt`.
    NoCoveringWindow,
    /// The event `occurredAt` is neither `unix-ms:` nor RFC 3339 UTC.
    UnparseableTimestamp,
}

impl UnresolvedReason {
    /// The stable snake_case reason code surfaced in diagnostics.
    pub fn as_str(self) -> &'static str {
        match self {
            UnresolvedReason::NoDelegationRecord => "no_delegation_record",
            UnresolvedReason::NoCoveringWindow => "no_covering_window",
            UnresolvedReason::UnparseableTimestamp => "unparseable_timestamp",
        }
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct DelegationMap {
    delegates: BTreeMap<ActorId, Vec<DelegationRecord>>,
}

/// One parsed delegation record. v1 stores only what projection-time resolution
/// consumes — the principal and the parsed validity window. The source instant
/// strings and the `comment` are validated at parse but not retained: the ADR
/// treats the delegates file's git history as the audit log and `comment` as
/// free text for diff readers, never authority, so no projection reads them.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DelegationRecord {
    principal: ActorId,
    /// Window bounds parsed once at construction so `resolve` is a comparison
    /// loop; a malformed bound is a parse error (never a resolve-time surprise).
    window: ValidityWindow,
}

impl DelegationMap {
    /// Read and parse a delegates file from `path`. Path-agnostic like
    /// `TrustSet::from_allowed_signers_file`; CLI auto-discovery lives in the
    /// CLI layer.
    pub fn from_delegates_file(path: impl AsRef<Path>) -> Result<Self> {
        let bytes =
            std::fs::read(path.as_ref()).map_err(|error| ShoreError::WorkflowInputInvalid {
                reason: format!(
                    "failed to read delegates file {}: {error}",
                    path.as_ref().display()
                ),
            })?;
        delegation_map_from_value(serde_json::from_slice(&bytes)?)
    }

    /// True when no agent has any delegation record.
    pub fn is_empty(&self) -> bool {
        self.delegates.is_empty()
    }

    /// Layer `local` over `self` (the committed map), git-config style: for each
    /// agent present in `local`, its records **fully replace** `self`'s records
    /// for that agent (including replacement with an empty array, which disavows
    /// the agent locally); agents absent from `local` keep `self`'s records
    /// unchanged. Either map may be empty.
    pub fn with_local_override(mut self, local: DelegationMap) -> DelegationMap {
        for (agent, records) in local.delegates {
            self.delegates.insert(agent, records);
        }
        self
    }

    /// The windowed records for `actor`, in file order. Empty when the actor has
    /// no delegation record.
    pub(crate) fn records_for(&self, actor: &ActorId) -> &[DelegationRecord] {
        self.delegates.get(actor).map(Vec::as_slice).unwrap_or(&[])
    }

    /// Number of delegation records held for `actor` (0 when absent). Public so the
    /// possession-based `enroll --local` command can report how many committed
    /// records a local override will shadow (the git-config full-replace caveat).
    pub fn record_count_for(&self, actor: &ActorId) -> usize {
        self.records_for(actor).len()
    }

    /// Resolve the principal an agent actor wrote on behalf of at `occurred_at`.
    /// Projection-time, replay-stable, git-free: it reads only the parsed map and
    /// the event timestamp. Ambiguity (more than one distinct covering principal)
    /// is surfaced, never auto-picked.
    pub fn resolve(&self, actor: &ActorId, occurred_at: &str) -> PrincipalResolution {
        let Some(instant) = parse_event_instant(occurred_at) else {
            return PrincipalResolution::None(UnresolvedReason::UnparseableTimestamp);
        };
        let records = self.records_for(actor);
        if records.is_empty() {
            return PrincipalResolution::None(UnresolvedReason::NoDelegationRecord);
        }
        let mut principals: Vec<ActorId> = records
            .iter()
            .filter(|record| record.window.contains(instant))
            .map(|record| record.principal.clone())
            .collect();
        if principals.is_empty() {
            return PrincipalResolution::None(UnresolvedReason::NoCoveringWindow);
        }
        principals.sort();
        principals.dedup();
        if principals.len() == 1 {
            PrincipalResolution::Resolved(principals.into_iter().next().expect("len checked"))
        } else {
            PrincipalResolution::Ambiguous(principals)
        }
    }
}

/// Parse a `DelegationMap` from an already-decoded JSON value. Public like
/// `event_signature_trust_set` so library callers can supply config from any
/// source. Unknown top-level keys are ignored for forward compatibility.
pub fn delegation_map_from_value(value: Value) -> Result<DelegationMap> {
    let delegates = value
        .get("delegates")
        .and_then(Value::as_object)
        .ok_or_else(|| invalid_delegation_map("missing delegates object"))?;

    let mut parsed = BTreeMap::new();
    for (actor, records) in delegates {
        if !is_agent_actor_id(actor) {
            return Err(invalid_delegation_map(format!(
                "delegate key {actor} must be an actor:agent:<name> id"
            )));
        }
        let records = records.as_array().ok_or_else(|| {
            invalid_delegation_map(format!("delegation records for {actor} must be an array"))
        })?;
        let mut parsed_records = Vec::with_capacity(records.len());
        for record in records {
            parsed_records.push(parse_record(actor, record)?);
        }
        parsed.insert(ActorId::new(actor), parsed_records);
    }

    Ok(DelegationMap { delegates: parsed })
}

/// Repo-relative paths to the delegates config. Mirrors `ALLOWED_SIGNERS_REL_PATH`.
pub const DELEGATES_REL_PATH: &str = ".shore/delegates.json";
pub const DELEGATES_LOCAL_REL_PATH: &str = ".shore/delegates.local.json";

/// Outcome of staging one delegation record: whether it was newly added (`true`)
/// or already present (`false`, a byte-stable no-op).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DelegationStageOutcome {
    pub added: bool,
}

/// A write-oriented delegation record that RETAINS the raw `validFrom`/`validUntil`
/// strings and `comment` for faithful round-trip writing — the read-path
/// `DelegationRecord` keeps only the parsed window, so it cannot serialize them back.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DelegationWriteRecord {
    principal: ActorId,
    valid_from: String,
    valid_until: Option<String>,
    comment: Option<String>,
}

impl DelegationWriteRecord {
    /// `principal` is the responsible non-agent actor; `valid_from` is RFC 3339 UTC
    /// (the CLI defaults it to `now_rfc3339_utc()`). Open window + no comment by default.
    pub fn new(principal: ActorId, valid_from: String) -> Self {
        Self {
            principal,
            valid_from,
            valid_until: None,
            comment: None,
        }
    }

    pub fn with_valid_until(mut self, until: Option<String>) -> Self {
        self.valid_until = until;
        self
    }

    pub fn with_comment(mut self, comment: Option<String>) -> Self {
        self.comment = comment;
        self
    }
}

/// Read-or-init `path`, append `record` to `agent`'s delegation array, and write it
/// back byte-stably. Pure (no git, no clock). Validates to the SAME rules
/// `DelegationMap::from_delegates_file` enforces, so a staged file always re-reads
/// (INV-B). An identical record already present is a no-op (`added: false`).
pub fn stage_delegation(
    path: &Path,
    agent: &ActorId,
    record: &DelegationWriteRecord,
) -> Result<DelegationStageOutcome> {
    // Validate exactly as parse_record does at load. The targeted checks here give
    // precise errors on the NEW input; the whole-document re-validation below is the
    // round-trip guarantee.
    if !is_agent_actor_id(agent.as_str()) {
        return Err(invalid_delegation_map(format!(
            "delegate key {} must be an actor:agent:<name> id",
            agent.as_str()
        )));
    }
    if !is_valid_principal_actor_id(record.principal.as_str()) {
        return Err(invalid_delegation_map(format!(
            "principal {} is not a valid actor id",
            record.principal.as_str()
        )));
    }
    if is_agent_actor_id(record.principal.as_str()) {
        return Err(invalid_delegation_map(format!(
            "principal {} must be a non-agent actor id in v1",
            record.principal.as_str()
        )));
    }
    if parse_rfc3339_utc_millis(&record.valid_from).is_none() {
        return Err(invalid_delegation_map(format!(
            "validFrom {} is not an RFC 3339 UTC instant",
            record.valid_from
        )));
    }
    if let Some(until) = &record.valid_until
        && parse_rfc3339_utc_millis(until).is_none()
    {
        return Err(invalid_delegation_map(format!(
            "validUntil {until} is not an RFC 3339 UTC instant"
        )));
    }

    // Read-or-init at the Value level so existing records (and any unknown fields)
    // are preserved; serde_json's Map is BTreeMap-backed → sorted keys → byte-stable.
    let mut root: Value = if path.exists() {
        serde_json::from_slice(
            &std::fs::read(path)
                .map_err(|e| ShoreError::Message(format!("read {}: {e}", path.display())))?,
        )?
    } else {
        Value::Object(Map::new())
    };
    let root_obj = root
        .as_object_mut()
        .ok_or_else(|| invalid_delegation_map("delegates file is not an object"))?;
    let delegates = root_obj
        .entry("delegates".to_owned())
        .or_insert_with(|| Value::Object(Map::new()))
        .as_object_mut()
        .ok_or_else(|| invalid_delegation_map("delegates is not an object"))?;
    let array = delegates
        .entry(agent.as_str().to_owned())
        .or_insert_with(|| Value::Array(Vec::new()))
        .as_array_mut()
        .ok_or_else(|| invalid_delegation_map("agent records must be an array"))?;

    // Dedup compares whole record `Value`s: identity is the FULL record object, so a
    // second window with a different `validFrom` is correctly a distinct (appended)
    // record, while a byte-identical re-stage is a no-op.
    let new_record = delegation_record_value(record);
    let added = !array.iter().any(|existing| existing == &new_record);
    if added {
        array.push(new_record);
    }

    // INV-B: re-validate the ENTIRE post-mutation document with the reader's own
    // parser before writing. This (a) guarantees the staged file always re-reads,
    // and (b) refuses to write when a PRE-EXISTING sibling record is malformed
    // (a structurally-valid-JSON-but-schema-invalid record the raw Value parse above
    // does not catch) rather than producing a file `from_delegates_file` would reject.
    delegation_map_from_value(root.clone())?;

    // Already-present still rewrites canonically (sorted keys, trailing newline): a
    // hand-edited non-canonical file converges to canonical bytes on the first stage,
    // then stays byte-stable — the same posture as `stage_enrollment`.
    write_delegates(path, &root)?;
    Ok(DelegationStageOutcome { added })
}

fn delegation_record_value(record: &DelegationWriteRecord) -> Value {
    let mut obj = Map::new();
    obj.insert(
        "principal".to_owned(),
        Value::String(record.principal.as_str().to_owned()),
    );
    obj.insert(
        "validFrom".to_owned(),
        Value::String(record.valid_from.clone()),
    );
    obj.insert(
        "validUntil".to_owned(),
        match &record.valid_until {
            Some(s) => Value::String(s.clone()),
            None => Value::Null,
        },
    );
    if let Some(comment) = &record.comment {
        obj.insert("comment".to_owned(), Value::String(comment.clone()));
    }
    Value::Object(obj)
}

fn write_delegates(path: &Path, root: &Value) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| ShoreError::Message(format!("create {}: {e}", parent.display())))?;
    }
    let mut bytes = serde_json::to_vec_pretty(root)?;
    bytes.push(b'\n');
    std::fs::write(path, &bytes)
        .map_err(|e| ShoreError::Message(format!("write {}: {e}", path.display())))
}

/// Validate and build one delegation record. Errors name the offending agent
/// id, mirroring `event_signature_trust_set`'s style.
fn parse_record(actor: &str, record: &Value) -> Result<DelegationRecord> {
    let record = record.as_object().ok_or_else(|| {
        invalid_delegation_map(format!("delegation record for {actor} must be an object"))
    })?;

    let principal = record
        .get("principal")
        .and_then(Value::as_str)
        .ok_or_else(|| {
            invalid_delegation_map(format!(
                "delegation record for {actor} is missing principal"
            ))
        })?;
    if !is_valid_principal_actor_id(principal) {
        return Err(invalid_delegation_map(format!(
            "principal {principal} for {actor} is not a valid actor id"
        )));
    }
    // v1: delegation chains have depth 0 — a principal is never itself an agent.
    if is_agent_actor_id(principal) {
        return Err(invalid_delegation_map(format!(
            "principal {principal} for {actor} must be a non-agent actor id in v1"
        )));
    }

    let from_ms = required_instant(actor, record, "validFrom")?;
    let until_ms = optional_instant(actor, record, "validUntil")?;
    // `comment` is validated as an optional string but not retained — it is
    // audit text for diff readers, never consumed by a projection.
    match record.get("comment") {
        None | Some(Value::Null) | Some(Value::String(_)) => {}
        Some(_) => {
            return Err(invalid_delegation_map(format!(
                "comment for {actor} must be a string"
            )));
        }
    }

    Ok(DelegationRecord {
        principal: ActorId::new(principal),
        window: ValidityWindow { from_ms, until_ms },
    })
}

/// A required RFC 3339 UTC field; returns its epoch milliseconds. Errors name
/// the field and the agent id.
fn required_instant(
    actor: &str,
    record: &serde_json::Map<String, Value>,
    field: &str,
) -> Result<i64> {
    let value = record.get(field).and_then(Value::as_str).ok_or_else(|| {
        invalid_delegation_map(format!("delegation record for {actor} is missing {field}"))
    })?;
    parse_rfc3339_utc_millis(value).ok_or_else(|| {
        invalid_delegation_map(format!(
            "{field} {value} for {actor} is not an RFC 3339 UTC instant"
        ))
    })
}

/// An optional RFC 3339 UTC field: absent or `null` yields `None`; any present
/// string must parse to its epoch milliseconds.
fn optional_instant(
    actor: &str,
    record: &serde_json::Map<String, Value>,
    field: &str,
) -> Result<Option<i64>> {
    match record.get(field) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(value)) => parse_rfc3339_utc_millis(value).map(Some).ok_or_else(|| {
            invalid_delegation_map(format!(
                "{field} {value} for {actor} is not an RFC 3339 UTC instant"
            ))
        }),
        Some(_) => Err(invalid_delegation_map(format!(
            "{field} for {actor} must be a string or null"
        ))),
    }
}

fn invalid_delegation_map(reason: impl Into<String>) -> ShoreError {
    ShoreError::WorkflowInputInvalid {
        reason: format!("invalid delegation map: {}", reason.into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn actor(id: &str) -> ActorId {
        ActorId::new(id)
    }

    #[test]
    fn parses_delegates_file_shape() {
        let map = delegation_map_from_value(serde_json::json!({
            "delegates": {
                "actor:agent:claude-code": [{
                    "principal": "actor:git-email:kevin@swiber.dev",
                    "validFrom": "2026-06-10T00:00:00Z",
                    "validUntil": null,
                    "comment": "claude-code, enrolled by Kevin"
                }]
            },
            "futureTopLevelKey": {"ignored": true}
        }))
        .unwrap();

        let agent = actor("actor:agent:claude-code");
        let records = map.records_for(&agent);
        assert_eq!(records.len(), 1);
        assert_eq!(
            records[0].principal,
            actor("actor:git-email:kevin@swiber.dev")
        );
        assert!(!map.is_empty());
        // The window parsed correctly: an instant inside the open window resolves.
        assert_eq!(
            map.resolve(&agent, "2026-06-11T00:00:00Z"),
            PrincipalResolution::Resolved(actor("actor:git-email:kevin@swiber.dev"))
        );
    }

    #[test]
    fn rejects_missing_delegates_key() {
        let err = delegation_map_from_value(serde_json::json!({ "notDelegates": {} }))
            .expect_err("missing delegates key must be rejected");
        let message = err.to_string();
        assert!(
            message.contains("delegates"),
            "error names the missing key; got: {message}"
        );
    }

    #[test]
    fn rejects_agent_scheme_principal_in_v1() {
        let err = delegation_map_from_value(serde_json::json!({
            "delegates": {
                "actor:agent:claude-code": [{
                    "principal": "actor:agent:subagent",
                    "validFrom": "2026-06-10T00:00:00Z",
                    "validUntil": null
                }]
            }
        }))
        .expect_err("agent-scheme principal must be rejected in v1");
        let message = err.to_string();
        assert!(
            message.contains("actor:agent:claude-code"),
            "error names the offending agent id; got: {message}"
        );
    }

    #[test]
    fn rejects_invalid_principal_actor_id() {
        let err = delegation_map_from_value(serde_json::json!({
            "delegates": {
                "actor:agent:claude-code": [{
                    "principal": "not-an-actor-id",
                    "validFrom": "2026-06-10T00:00:00Z",
                    "validUntil": null
                }]
            }
        }))
        .expect_err("malformed principal must be rejected");
        assert!(err.to_string().contains("actor:agent:claude-code"));
    }

    #[test]
    fn rejects_missing_valid_from() {
        let err = delegation_map_from_value(serde_json::json!({
            "delegates": {
                "actor:agent:claude-code": [{
                    "principal": "actor:git-email:kevin@swiber.dev",
                    "validUntil": null
                }]
            }
        }))
        .expect_err("missing validFrom must be rejected");
        assert!(err.to_string().contains("validFrom"));
    }

    #[test]
    fn rejects_non_rfc3339_valid_from() {
        let err = delegation_map_from_value(serde_json::json!({
            "delegates": {
                "actor:agent:claude-code": [{
                    "principal": "actor:git-email:kevin@swiber.dev",
                    "validFrom": "yesterday",
                    "validUntil": null
                }]
            }
        }))
        .expect_err("non-RFC-3339 validFrom must be rejected");
        assert!(err.to_string().contains("validFrom"));
    }

    #[test]
    fn rejects_non_rfc3339_valid_until() {
        let err = delegation_map_from_value(serde_json::json!({
            "delegates": {
                "actor:agent:claude-code": [{
                    "principal": "actor:git-email:kevin@swiber.dev",
                    "validFrom": "2026-06-10T00:00:00Z",
                    "validUntil": "later"
                }]
            }
        }))
        .expect_err("non-RFC-3339 validUntil must be rejected");
        assert!(err.to_string().contains("validUntil"));
    }

    #[test]
    fn accepts_open_window_and_optional_comment() {
        // No validUntil and no comment parse cleanly; the open window resolves
        // arbitrarily far into the future.
        let map = delegation_map_from_value(serde_json::json!({
            "delegates": {
                "actor:agent:claude-code": [{
                    "principal": "actor:git-email:kevin@swiber.dev",
                    "validFrom": "2026-06-10T00:00:00Z",
                    "validUntil": null
                }]
            }
        }))
        .unwrap();
        assert_eq!(
            map.resolve(&actor("actor:agent:claude-code"), "2099-01-01T00:00:00Z"),
            PrincipalResolution::Resolved(actor("actor:git-email:kevin@swiber.dev"))
        );
    }

    #[test]
    fn accepts_git_name_principal_with_internal_space() {
        // The system mints `actor:git-name:<name>` with spaces; a principal must
        // be able to name such a human even though the env-override validator
        // forbids whitespace.
        let map = map_with(serde_json::json!([{
            "principal": "actor:git-name:Kevin Swiber",
            "validFrom": "2026-06-10T00:00:00Z",
            "validUntil": null
        }]));
        let records = map.records_for(&actor(AGENT));
        assert_eq!(records[0].principal, actor("actor:git-name:Kevin Swiber"));
    }

    #[test]
    fn rejects_non_agent_delegate_key() {
        let err = delegation_map_from_value(serde_json::json!({
            "delegates": {
                "actor:git-email:kevin@swiber.dev": [{
                    "principal": "actor:git-email:kevin@swiber.dev",
                    "validFrom": "2026-06-10T00:00:00Z",
                    "validUntil": null
                }]
            }
        }))
        .expect_err("a non-agent delegate key must be rejected");
        assert!(err.to_string().contains("actor:git-email:kevin@swiber.dev"));
    }

    fn map_with(records: Value) -> DelegationMap {
        delegation_map_from_value(serde_json::json!({
            "delegates": { "actor:agent:claude-code": records }
        }))
        .unwrap()
    }

    const AGENT: &str = "actor:agent:claude-code";
    const KEVIN: &str = "actor:git-email:kevin@swiber.dev";
    const ALICE: &str = "actor:git-email:alice@example.com";

    #[test]
    fn resolves_principal_inside_open_window() {
        let map = map_with(serde_json::json!([{
            "principal": KEVIN, "validFrom": "2026-06-10T00:00:00Z", "validUntil": null
        }]));
        assert_eq!(
            map.resolve(&actor(AGENT), "2026-06-11T12:00:00Z"),
            PrincipalResolution::Resolved(actor(KEVIN))
        );
    }

    #[test]
    fn unix_ms_event_timestamp_resolves_against_rfc3339_window() {
        let map = map_with(serde_json::json!([{
            "principal": KEVIN, "validFrom": "2026-06-10T00:00:00Z", "validUntil": null
        }]));
        // unix-ms:1781136000000 == 2026-06-11T00:00:00Z, inside the window.
        assert_eq!(
            map.resolve(&actor(AGENT), "unix-ms:1781136000000"),
            PrincipalResolution::Resolved(actor(KEVIN))
        );
    }

    #[test]
    fn window_boundaries_are_half_open() {
        let map = map_with(serde_json::json!([{
            "principal": KEVIN,
            "validFrom": "2026-06-10T00:00:00Z",
            "validUntil": "2026-06-20T00:00:00Z"
        }]));
        // validFrom is inclusive.
        assert_eq!(
            map.resolve(&actor(AGENT), "2026-06-10T00:00:00Z"),
            PrincipalResolution::Resolved(actor(KEVIN))
        );
        // validUntil is exclusive.
        assert_eq!(
            map.resolve(&actor(AGENT), "2026-06-20T00:00:00Z"),
            PrincipalResolution::None(UnresolvedReason::NoCoveringWindow)
        );
    }

    #[test]
    fn closed_window_keeps_resolving_history_and_rejects_later_events() {
        let map = map_with(serde_json::json!([{
            "principal": KEVIN,
            "validFrom": "2026-06-10T00:00:00Z",
            "validUntil": "2026-06-20T00:00:00Z"
        }]));
        // An event inside the now-closed window still resolves (history-stable).
        assert_eq!(
            map.resolve(&actor(AGENT), "2026-06-15T00:00:00Z"),
            PrincipalResolution::Resolved(actor(KEVIN))
        );
        // An event after revocation no longer resolves.
        assert_eq!(
            map.resolve(&actor(AGENT), "2026-06-25T00:00:00Z"),
            PrincipalResolution::None(UnresolvedReason::NoCoveringWindow)
        );
    }

    #[test]
    fn unknown_agent_resolves_none_no_delegation_record() {
        let map = map_with(serde_json::json!([{
            "principal": KEVIN, "validFrom": "2026-06-10T00:00:00Z", "validUntil": null
        }]));
        assert_eq!(
            map.resolve(&actor("actor:agent:other"), "2026-06-11T00:00:00Z"),
            PrincipalResolution::None(UnresolvedReason::NoDelegationRecord)
        );
    }

    #[test]
    fn overlapping_windows_with_distinct_principals_are_ambiguous() {
        let map = map_with(serde_json::json!([
            { "principal": KEVIN, "validFrom": "2026-06-10T00:00:00Z", "validUntil": null },
            { "principal": ALICE, "validFrom": "2026-06-15T00:00:00Z", "validUntil": null }
        ]));
        // Both windows cover 2026-06-16; ambiguity is surfaced sorted, never auto-picked.
        assert_eq!(
            map.resolve(&actor(AGENT), "2026-06-16T00:00:00Z"),
            PrincipalResolution::Ambiguous(vec![actor(ALICE), actor(KEVIN)])
        );
    }

    #[test]
    fn overlapping_windows_with_same_principal_resolve() {
        let map = map_with(serde_json::json!([
            { "principal": KEVIN, "validFrom": "2026-06-10T00:00:00Z", "validUntil": null },
            { "principal": KEVIN, "validFrom": "2026-06-15T00:00:00Z", "validUntil": null }
        ]));
        assert_eq!(
            map.resolve(&actor(AGENT), "2026-06-16T00:00:00Z"),
            PrincipalResolution::Resolved(actor(KEVIN))
        );
    }

    #[test]
    fn unparseable_event_timestamp_resolves_none_with_reason() {
        let map = map_with(serde_json::json!([{
            "principal": KEVIN, "validFrom": "2026-06-10T00:00:00Z", "validUntil": null
        }]));
        assert_eq!(
            map.resolve(&actor(AGENT), "garbage"),
            PrincipalResolution::None(UnresolvedReason::UnparseableTimestamp)
        );
    }

    #[test]
    fn rejects_non_array_records() {
        let err = delegation_map_from_value(serde_json::json!({
            "delegates": {
                "actor:agent:claude-code": {
                    "principal": "actor:git-email:kevin@swiber.dev",
                    "validFrom": "2026-06-10T00:00:00Z"
                }
            }
        }))
        .expect_err("records for an agent must be an array");
        assert!(err.to_string().contains("actor:agent:claude-code"));
    }

    #[test]
    fn local_records_fully_replace_committed_for_same_agent() {
        // committed: AGENT -> KEVIN
        let committed = map_with(serde_json::json!([
            { "principal": KEVIN, "validFrom": "2026-06-10T00:00:00Z", "validUntil": null }
        ]));
        // local: AGENT -> ALICE  (same agent key, different principal)
        let local = map_with(serde_json::json!([
            { "principal": ALICE, "validFrom": "2026-06-10T00:00:00Z", "validUntil": null }
        ]));

        let merged = committed.with_local_override(local);

        // Local fully replaces committed for AGENT.
        assert_eq!(
            merged.resolve(&actor(AGENT), "2026-06-12T00:00:00Z"),
            PrincipalResolution::Resolved(actor(ALICE))
        );
    }

    #[test]
    fn agent_absent_from_local_inherits_committed() {
        let committed = delegation_map_from_value(serde_json::json!({
            "delegates": {
                "actor:agent:claude-code": [
                    { "principal": KEVIN, "validFrom": "2026-06-10T00:00:00Z", "validUntil": null }],
                "actor:agent:other": [
                    { "principal": ALICE, "validFrom": "2026-06-10T00:00:00Z", "validUntil": null }]
            }
        }))
        .unwrap();
        // local only overrides claude-code.
        let local = map_with(serde_json::json!([
            { "principal": ALICE, "validFrom": "2026-06-10T00:00:00Z", "validUntil": null }
        ]));

        let merged = committed.with_local_override(local);

        // other inherits committed (KEVIN -> ALICE swap only for claude-code).
        assert_eq!(
            merged.resolve(&actor("actor:agent:other"), "2026-06-12T00:00:00Z"),
            PrincipalResolution::Resolved(actor(ALICE))
        );
        assert_eq!(
            merged.resolve(&actor(AGENT), "2026-06-12T00:00:00Z"),
            PrincipalResolution::Resolved(actor(ALICE))
        );
    }

    #[test]
    fn either_map_alone_round_trips_through_merge() {
        let committed = map_with(serde_json::json!([
            { "principal": KEVIN, "validFrom": "2026-06-10T00:00:00Z", "validUntil": null }
        ]));
        assert_eq!(
            committed
                .clone()
                .with_local_override(DelegationMap::default()),
            committed
        );
        assert_eq!(
            DelegationMap::default().with_local_override(committed.clone()),
            committed
        );
    }

    #[test]
    fn local_empty_record_array_disavows_the_agent() {
        // git-config "set to empty" — a local AGENT -> [] FULLY replaces committed,
        // so AGENT resolves NoDelegationRecord (deliberate disavowal via override).
        let committed = map_with(serde_json::json!([
            { "principal": KEVIN, "validFrom": "2026-06-10T00:00:00Z", "validUntil": null }
        ]));
        let local = delegation_map_from_value(serde_json::json!({
            "delegates": { "actor:agent:claude-code": [] }
        }))
        .unwrap();

        let merged = committed.with_local_override(local);

        assert_eq!(
            merged.resolve(&actor(AGENT), "2026-06-12T00:00:00Z"),
            PrincipalResolution::None(UnresolvedReason::NoDelegationRecord)
        );
    }

    #[test]
    fn stage_delegation_round_trips_through_the_reader() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(".shore/delegates.json");
        let agent = ActorId::new("actor:agent:claude-code");
        let record = DelegationWriteRecord::new(
            ActorId::new("actor:git-email:kevin@swiber.dev"),
            "2026-06-10T00:00:00Z".to_owned(),
        )
        .with_comment(Some("enrolled by Kevin".to_owned()));

        let outcome = stage_delegation(&path, &agent, &record).unwrap();
        assert!(outcome.added);

        // The landed reader loads the staged file and resolves the principal in-window.
        let map = DelegationMap::from_delegates_file(&path).unwrap();
        assert_eq!(
            map.resolve(&agent, "2026-06-11T00:00:00Z"),
            PrincipalResolution::Resolved(ActorId::new("actor:git-email:kevin@swiber.dev"))
        );
    }

    #[test]
    fn stage_delegation_is_byte_stable_on_identical_restage() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(".shore/delegates.json");
        let agent = ActorId::new("actor:agent:claude-code");
        let record = DelegationWriteRecord::new(
            ActorId::new("actor:git-email:kevin@swiber.dev"),
            "2026-06-10T00:00:00Z".to_owned(),
        );

        let first = stage_delegation(&path, &agent, &record).unwrap();
        let before = std::fs::read(&path).unwrap();
        let second = stage_delegation(&path, &agent, &record).unwrap();
        let after = std::fs::read(&path).unwrap();

        assert!(
            first.added && !second.added,
            "identical re-stage is a no-op"
        );
        assert_eq!(before, after, "re-stage leaves the file byte-identical");
        assert!(
            before.ends_with(b"\n"),
            "trailing newline like stage_enrollment"
        );
    }

    #[test]
    fn stage_delegation_appends_a_second_record_for_the_same_agent() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(".shore/delegates.json");
        let agent = ActorId::new("actor:agent:claude-code");
        stage_delegation(
            &path,
            &agent,
            &DelegationWriteRecord::new(
                ActorId::new("actor:git-email:kevin@swiber.dev"),
                "2026-06-10T00:00:00Z".to_owned(),
            ),
        )
        .unwrap();
        let second = stage_delegation(
            &path,
            &agent,
            &DelegationWriteRecord::new(
                ActorId::new("actor:git-email:alice@example.com"),
                "2026-06-12T00:00:00Z".to_owned(),
            ),
        )
        .unwrap();
        assert!(second.added);
        let map = DelegationMap::from_delegates_file(&path).unwrap();
        // Both windows cover 2026-06-13 → ambiguous (two distinct principals), proving both records persisted.
        assert!(matches!(
            map.resolve(&agent, "2026-06-13T00:00:00Z"),
            PrincipalResolution::Ambiguous(_)
        ));
    }

    #[test]
    fn stage_delegation_rejects_agent_principal_depth0_rule() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(".shore/delegates.json");
        let agent = ActorId::new("actor:agent:claude-code");
        // A principal that is itself an agent violates the v1 depth-0 rule.
        let bad = DelegationWriteRecord::new(
            ActorId::new("actor:agent:subagent"),
            "2026-06-10T00:00:00Z".to_owned(),
        );
        assert!(stage_delegation(&path, &agent, &bad).is_err());
        assert!(!path.exists(), "a rejected record writes nothing");
    }

    #[test]
    fn stage_delegation_refuses_when_an_existing_sibling_is_malformed() {
        // INV-B: a pre-existing malformed sibling (here, an agent-scheme principal — valid
        // JSON, invalid schema) must make the stage FAIL, not write a file the reader rejects.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(".shore/delegates.json");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, br#"{"delegates":{"actor:agent:claude-code":[{"principal":"actor:agent:bad","validFrom":"2026-06-10T00:00:00Z","validUntil":null}]}}"#).unwrap();
        let before = std::fs::read(&path).unwrap();

        let result = stage_delegation(
            &path,
            &ActorId::new("actor:agent:claude-code"),
            &DelegationWriteRecord::new(
                ActorId::new("actor:git-email:kevin@swiber.dev"),
                "2026-06-10T00:00:00Z".to_owned(),
            ),
        );
        assert!(
            result.is_err(),
            "a malformed existing sibling must make the stage fail"
        );
        assert_eq!(
            std::fs::read(&path).unwrap(),
            before,
            "a failed stage writes nothing"
        );
    }

    #[test]
    fn stage_delegation_rejects_non_agent_key_and_non_rfc3339_instants() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(".shore/delegates.json");
        let principal = ActorId::new("actor:git-email:kevin@swiber.dev");
        // Non-agent map key.
        assert!(
            stage_delegation(
                &path,
                &ActorId::new("actor:git-email:not-an-agent"),
                &DelegationWriteRecord::new(principal.clone(), "2026-06-10T00:00:00Z".to_owned())
            )
            .is_err()
        );
        // Non-RFC-3339 validFrom.
        assert!(
            stage_delegation(
                &path,
                &ActorId::new("actor:agent:claude-code"),
                &DelegationWriteRecord::new(principal, "yesterday".to_owned())
            )
            .is_err()
        );
    }

    #[test]
    fn record_count_for_returns_array_length() {
        let agent = actor(AGENT);
        assert_eq!(DelegationMap::default().record_count_for(&agent), 0);
        let map = map_with(serde_json::json!([
            { "principal": KEVIN, "validFrom": "2026-06-10T00:00:00Z", "validUntil": null },
            { "principal": ALICE, "validFrom": "2026-06-15T00:00:00Z", "validUntil": null }
        ]));
        assert_eq!(map.record_count_for(&agent), 2);
        assert_eq!(map.record_count_for(&actor("actor:agent:absent")), 0);
    }
}
