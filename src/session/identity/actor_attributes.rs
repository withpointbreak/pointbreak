//! The actor-attributes map: a sibling checked-in file
//! (`.shore/actor-attributes.json`, with an optional locally-excluded
//! `.shore/actor-attributes.local.json` override layered over it by the CLI
//! discovery helper) that records what *kind* of party an actor is and which
//! *roles* it carries. It is human-committed, advisory, reader-relative, and
//! never self-asserted (ADR-0012) — a sibling of `delegates.json` and
//! `allowed-signers.json`, with `git log -p` as the audit trail.
//!
//! File shape (top-level key `actors`; unknown top-level keys — including
//! `schema` — are ignored for forward compatibility):
//!
//! ```json
//! {
//!   "schema": "shore.actor-attributes.v1",
//!   "actors": {
//!     "actor:agent:review-bot":           { "kind": "reviewer-model", "roles": ["reviewer"] },
//!     "actor:git-email:kevin@swiber.dev": { "kind": "human", "roles": ["author", "reviewer"], "comment": "me" }
//!   }
//! }
//! ```
//!
//! Each key is any well-formed *persisted* actor id, validated with the
//! whitespace-permitting `is_valid_principal_actor_id`. Every entry declares
//! exactly one `kind` (a reserved-but-open lowercase-kebab token); `roles` is an
//! open set of lowercase-kebab tokens, deduped and sorted for byte-stable config.
//! An actor *absent* from the map resolves to an explicit unattributed result
//! (`kind: None`, empty `roles`) — never an error.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use serde_json::{Map, Value};

use super::writer::is_valid_principal_actor_id;
use crate::error::{Result, ShoreError};
use crate::model::ActorId;

/// Declared attributes for one actor. A parsed map entry always carries `Some(kind)`
/// (ADR-0012: exactly one kind per actor). An *unattributed* actor — one **absent** from
/// the map — resolves to `ActorAttributes::default()` (`kind: None`, empty `roles`), never
/// an error. So `kind: None` is the unattributed sentinel only, never a stored entry.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ActorAttributes {
    /// Reserved-but-open kind token (lowercase kebab). `None` only for the unattributed
    /// resolve-default; a parsed entry is always `Some`.
    kind: Option<String>,
    /// Open set of role tokens (lowercase kebab), deduped + sorted at parse.
    roles: BTreeSet<String>,
}

impl ActorAttributes {
    /// The declared kind token, if any. Round-trips any stored kind (including an
    /// unrecognized one); use `is_kind` for a reserved-and-exact predicate.
    pub fn kind(&self) -> Option<&str> {
        self.kind.as_deref()
    }
    /// The declared roles (deduped, sorted).
    pub fn roles(&self) -> &BTreeSet<String> {
        &self.roles
    }

    /// True iff this actor's **declared** kind is a **reserved** kind exactly equal to `kind`.
    /// Two reasons this can be false even with a declared kind:
    ///  - the actor is unattributed (`kind == None`) — the hard split; the actor-id scheme is
    ///    NEVER consulted here, and
    ///  - the declared kind is unrecognized (not reserved) — ADR-0012: "an unrecognized kind
    ///    does not satisfy any kind= predicate" (forward-compat; it still round-trips via `kind()`).
    pub fn is_kind(&self, kind: &str) -> bool {
        matches!(self.kind.as_deref(), Some(k) if is_reserved_kind(k) && k == kind)
    }

    /// True iff this actor has the declared `role`. An unattributed actor (empty roles)
    /// satisfies NO `role=` predicate. (`roles` is an open set — no reserved-set filter.)
    pub fn has_role(&self, role: &str) -> bool {
        self.roles.contains(role)
    }
}

/// The reserved well-known kinds (ADR-0012). `kind` is reserved-but-OPEN: the parser
/// stores any lowercase-kebab token (so unknown kinds round-trip via `ActorAttributes::kind`),
/// but only a reserved kind is matchable by the `is_kind` predicate.
pub(crate) const RESERVED_KINDS: &[&str] = &["human", "agent", "service", "reviewer-model"];

fn is_reserved_kind(kind: &str) -> bool {
    RESERVED_KINDS.contains(&kind)
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ActorAttributesMap {
    actors: BTreeMap<ActorId, ActorAttributes>,
}

impl ActorAttributesMap {
    /// Read and parse an actor-attributes file. Path-agnostic like
    /// `DelegationMap::from_delegates_file`; CLI auto-discovery lives in the CLI layer.
    pub fn from_attributes_file(path: impl AsRef<Path>) -> Result<Self> {
        let bytes =
            std::fs::read(path.as_ref()).map_err(|error| ShoreError::WorkflowInputInvalid {
                reason: format!(
                    "failed to read actor-attributes file {}: {error}",
                    path.as_ref().display()
                ),
            })?;
        actor_attributes_from_value(serde_json::from_slice(&bytes)?)
    }

    /// True when no actor has any declared attributes.
    pub fn is_empty(&self) -> bool {
        self.actors.is_empty()
    }

    /// Layer `local` over `self` (committed), git-config style: each actor present in
    /// `local` fully replaces `self`'s entry for that actor; others are untouched.
    pub fn with_local_override(mut self, local: ActorAttributesMap) -> ActorAttributesMap {
        for (actor, attrs) in local.actors {
            self.actors.insert(actor, attrs);
        }
        self
    }

    /// Resolve an actor's attributes against the reader's current config. Absent =
    /// explicit unattributed (`ActorAttributes::default()`), never an error. v1 reads
    /// no validity window and does not consult `occurredAt`.
    pub fn resolve(&self, actor: &ActorId) -> ActorAttributes {
        self.actors.get(actor).cloned().unwrap_or_default()
    }
}

/// Parse an `ActorAttributesMap` from a decoded JSON value (mirrors
/// `delegation_map_from_value`). Validates keys with the whitespace-permitting
/// `is_valid_principal_actor_id`; unknown top-level keys (including `schema`) are ignored
/// for forward compatibility.
pub fn actor_attributes_from_value(value: Value) -> Result<ActorAttributesMap> {
    let actors = value
        .get("actors")
        .and_then(Value::as_object)
        .ok_or_else(|| invalid("missing actors object"))?;

    let mut parsed = BTreeMap::new();
    for (actor, attrs) in actors {
        if !is_valid_principal_actor_id(actor) {
            return Err(invalid(format!(
                "actor key {actor} is not a valid actor id"
            )));
        }
        parsed.insert(ActorId::new(actor), parse_attributes(actor, attrs)?);
    }
    Ok(ActorAttributesMap { actors: parsed })
}

fn parse_attributes(actor: &str, value: &Value) -> Result<ActorAttributes> {
    let obj = value
        .as_object()
        .ok_or_else(|| invalid(format!("attributes for {actor} must be an object")))?;

    // ADR-0012: exactly one kind per actor — a map entry MUST declare a (string) kind.
    // `kind: None` is reserved for the unattributed resolve-default (absent actor) only.
    let kind = match obj.get("kind") {
        Some(Value::String(k)) => Some(normalize_token(actor, "kind", k)?),
        None | Some(Value::Null) => {
            return Err(invalid(format!(
                "attributes for {actor} must declare exactly one kind"
            )));
        }
        Some(_) => return Err(invalid(format!("kind for {actor} must be a string"))),
    };

    let mut roles = BTreeSet::new();
    if let Some(value) = obj.get("roles") {
        let array = value
            .as_array()
            .ok_or_else(|| invalid(format!("roles for {actor} must be an array")))?;
        for role in array {
            let role = role
                .as_str()
                .ok_or_else(|| invalid(format!("role for {actor} must be a string")))?;
            roles.insert(normalize_token(actor, "role", role)?); // BTreeSet dedupes + sorts
        }
    }
    Ok(ActorAttributes { kind, roles })
}

/// Lowercase-normalize and validate a token against the grammar `[a-z0-9-]+`.
fn normalize_token(actor: &str, field: &str, token: &str) -> Result<String> {
    let lowered = token.to_ascii_lowercase();
    if lowered.is_empty()
        || !lowered
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
    {
        return Err(invalid(format!(
            "{field} {token:?} for {actor} must be a lowercase kebab token ([a-z0-9-]+)"
        )));
    }
    Ok(lowered)
}

fn invalid(reason: impl Into<String>) -> ShoreError {
    ShoreError::WorkflowInputInvalid {
        reason: format!("invalid actor attributes: {}", reason.into()),
    }
}

/// Repo-relative paths to the actor-attributes config. Mirrors `DELEGATES_REL_PATH`.
pub const ACTOR_ATTRIBUTES_REL_PATH: &str = ".shore/actor-attributes.json";
pub const ACTOR_ATTRIBUTES_LOCAL_REL_PATH: &str = ".shore/actor-attributes.local.json";

/// The schema tag the writer emits. The reader ignores top-level `schema` (unknown
/// top-level keys are forward-compatible) but the canonical file declares it.
const ACTOR_ATTRIBUTES_SCHEMA: &str = "shore.actor-attributes.v1";

/// Outcome of staging one actor's attributes: whether the actor's entry changed
/// (`true`) or was byte-identical already (`false`, a no-op).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ActorAttributesStageOutcome {
    pub changed: bool,
}

/// A write-oriented actor-attributes entry. `kind` is required (ADR-0012: exactly
/// one kind per actor); `roles` is an open set; `comment` is optional audit text the
/// reader does not interpret but the writer preserves.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ActorAttributesWriteRecord {
    kind: String,
    roles: Vec<String>,
    comment: Option<String>,
}

impl ActorAttributesWriteRecord {
    pub fn new(kind: String) -> Self {
        Self {
            kind,
            roles: Vec::new(),
            comment: None,
        }
    }

    pub fn with_roles(mut self, roles: Vec<String>) -> Self {
        self.roles = roles;
        self
    }

    pub fn with_comment(mut self, comment: Option<String>) -> Self {
        self.comment = comment;
        self
    }
}

/// Read-or-init `path`, set/replace `actor`'s attributes entry, write it back
/// byte-stably. Pure (no git, no clock). Normalizes + validates tokens to the SAME
/// grammar `parse_attributes` enforces (lowercase-kebab `[a-z0-9-]+`; roles are
/// deduped and sorted), so a staged file always re-reads (INV-B). A byte-identical
/// entry is a no-op (`changed: false`). Per-actor REPLACE: an actor has one entry.
pub fn stage_actor_attributes(
    path: &Path,
    actor: &ActorId,
    attrs: &ActorAttributesWriteRecord,
) -> Result<ActorAttributesStageOutcome> {
    if !is_valid_principal_actor_id(actor.as_str()) {
        return Err(invalid(format!(
            "actor key {} is not a valid actor id",
            actor.as_str()
        )));
    }
    // Normalize + validate via the reader's own grammar: `normalize_token` is private
    // to this module, so the writer reuses it directly — one grammar, reader and writer
    // can never drift.
    let kind = normalize_token(actor.as_str(), "kind", &attrs.kind)?;
    let mut roles_set = BTreeSet::new();
    for role in &attrs.roles {
        roles_set.insert(normalize_token(actor.as_str(), "role", role)?);
    }

    let mut root: Value = if path.exists() {
        serde_json::from_slice(
            &std::fs::read(path)
                .map_err(|e| ShoreError::Message(format!("read {}: {e}", path.display())))?,
        )?
    } else {
        let mut init = Map::new();
        init.insert(
            "schema".to_owned(),
            Value::String(ACTOR_ATTRIBUTES_SCHEMA.to_owned()),
        );
        init.insert("actors".to_owned(), Value::Object(Map::new()));
        Value::Object(init)
    };
    let root_obj = root
        .as_object_mut()
        .ok_or_else(|| invalid("attributes file is not an object"))?;
    // Keep/refresh the schema tag.
    root_obj.insert(
        "schema".to_owned(),
        Value::String(ACTOR_ATTRIBUTES_SCHEMA.to_owned()),
    );
    let actors = root_obj
        .entry("actors".to_owned())
        .or_insert_with(|| Value::Object(Map::new()))
        .as_object_mut()
        .ok_or_else(|| invalid("actors is not an object"))?;

    // `changed` compares the NORMALIZED entry against what is on disk, so re-attesting
    // with differently-cased-but-equivalent tokens is correctly a no-op.
    let new_entry = actor_attributes_entry_value(&kind, &roles_set, attrs.comment.as_deref());
    let changed = actors.get(actor.as_str()) != Some(&new_entry);
    actors.insert(actor.as_str().to_owned(), new_entry);

    // INV-B: re-validate the ENTIRE post-mutation document with the reader's own
    // parser before writing — guarantees the staged file always re-reads, and refuses
    // to write when a PRE-EXISTING sibling entry is malformed (e.g. a missing/null
    // kind the raw Value parse does not catch) rather than producing a file
    // `from_attributes_file` would reject.
    actor_attributes_from_value(root.clone())?;

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| ShoreError::Message(format!("create {}: {e}", parent.display())))?;
    }
    let mut bytes = serde_json::to_vec_pretty(&root)?;
    bytes.push(b'\n');
    std::fs::write(path, &bytes)
        .map_err(|e| ShoreError::Message(format!("write {}: {e}", path.display())))?;
    Ok(ActorAttributesStageOutcome { changed })
}

fn actor_attributes_entry_value(
    kind: &str,
    roles: &BTreeSet<String>,
    comment: Option<&str>,
) -> Value {
    let mut obj = Map::new();
    obj.insert("kind".to_owned(), Value::String(kind.to_owned()));
    obj.insert(
        "roles".to_owned(),
        Value::Array(roles.iter().map(|r| Value::String(r.clone())).collect()),
    );
    if let Some(comment) = comment {
        obj.insert("comment".to_owned(), Value::String(comment.to_owned()));
    }
    Value::Object(obj)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::ActorId;

    const MAP: &str = r#"{
      "schema": "shore.actor-attributes.v1",
      "actors": {
        "actor:agent:review-bot": { "kind": "reviewer-model", "roles": ["reviewer"] },
        "actor:git-email:kevin@swiber.dev": { "kind": "human", "roles": ["reviewer", "author"], "comment": "me" }
      }
    }"#;

    #[test]
    fn resolves_declared_attributes() {
        let map = actor_attributes_from_value(serde_json::from_str(MAP).unwrap()).unwrap();
        let kevin = map.resolve(&ActorId::new("actor:git-email:kevin@swiber.dev"));
        assert_eq!(kevin.kind(), Some("human"));
        // roles are deduped + sorted for byte-stable config.
        assert_eq!(
            kevin.roles().iter().cloned().collect::<Vec<_>>(),
            vec!["author", "reviewer"]
        );
    }

    #[test]
    fn absent_actor_resolves_unattributed_never_errors() {
        // "Unattributed" is the ABSENT-from-map case only (kind None via the resolve
        // default). A map ENTRY must always declare a kind (see rejects_missing_or_null_kind).
        let map = actor_attributes_from_value(serde_json::from_str(MAP).unwrap()).unwrap();
        let unknown = map.resolve(&ActorId::new("actor:agent:nobody"));
        assert_eq!(unknown.kind(), None);
        assert!(unknown.roles().is_empty());
    }

    #[test]
    fn local_override_replaces_per_actor() {
        let committed = actor_attributes_from_value(serde_json::from_str(MAP).unwrap()).unwrap();
        let local = actor_attributes_from_value(serde_json::json!({
            "schema": "shore.actor-attributes.v1",
            "actors": { "actor:agent:review-bot": { "kind": "agent", "roles": [] } }
        }))
        .unwrap();
        let merged = committed.with_local_override(local);
        assert_eq!(
            merged
                .resolve(&ActorId::new("actor:agent:review-bot"))
                .kind(),
            Some("agent")
        );
        // An actor absent from local keeps its committed entry.
        assert_eq!(
            merged
                .resolve(&ActorId::new("actor:git-email:kevin@swiber.dev"))
                .kind(),
            Some("human")
        );
    }

    #[test]
    fn rejects_invalid_actor_key() {
        let bad = serde_json::json!({
            "schema": "shore.actor-attributes.v1",
            "actors": { "not-an-actor": { "kind": "human" } }
        });
        assert!(actor_attributes_from_value(bad).is_err());
    }

    #[test]
    fn rejects_non_kebab_kind_or_role() {
        for value in [
            serde_json::json!({"schema":"shore.actor-attributes.v1","actors":{"actor:agent:x":{"kind":"Reviewer_Model"}}}),
            // Role-grammar case keeps a valid kind so it fails ONLY on the bad role token.
            serde_json::json!({"schema":"shore.actor-attributes.v1","actors":{"actor:agent:x":{"kind":"agent","roles":["Has Space"]}}}),
        ] {
            assert!(actor_attributes_from_value(value).is_err());
        }
    }

    #[test]
    fn rejects_missing_or_null_kind() {
        // ADR-0012: "exactly one kind per actor" — a map ENTRY must declare a kind. An entry
        // with no/null kind is NOT a "declared-but-unattributed" actor; it is malformed.
        for value in [
            serde_json::json!({"schema":"shore.actor-attributes.v1","actors":{"actor:agent:x":{"roles":["reviewer"]}}}),
            serde_json::json!({"schema":"shore.actor-attributes.v1","actors":{"actor:agent:x":{"kind":null}}}),
            serde_json::json!({"schema":"shore.actor-attributes.v1","actors":{"actor:agent:x":{}}}),
        ] {
            assert!(
                actor_attributes_from_value(value.clone()).is_err(),
                "missing/null kind must be rejected: {value}"
            );
        }
    }

    #[test]
    fn git_name_actor_with_whitespace_is_a_valid_key() {
        // is_valid_principal_actor_id permits internal whitespace (git-name ids).
        let value = serde_json::json!({
            "schema": "shore.actor-attributes.v1",
            "actors": { "actor:git-name:Kevin Swiber": { "kind": "human" } }
        });
        let map = actor_attributes_from_value(value).unwrap();
        assert_eq!(
            map.resolve(&ActorId::new("actor:git-name:Kevin Swiber"))
                .kind(),
            Some("human")
        );
    }

    #[test]
    fn declared_predicates_match_exactly() {
        let map = actor_attributes_from_value(serde_json::json!({
            "schema": "shore.actor-attributes.v1",
            "actors": { "actor:git-email:kevin@swiber.dev": { "kind": "human", "roles": ["reviewer"] } }
        }))
        .unwrap();
        let kevin = map.resolve(&ActorId::new("actor:git-email:kevin@swiber.dev"));
        assert!(kevin.is_kind("human"));
        assert!(!kevin.is_kind("agent"));
        assert!(kevin.has_role("reviewer"));
        assert!(!kevin.has_role("author"));
    }

    #[test]
    fn hard_split_absent_agent_scheme_satisfies_no_kind_or_role_predicate() {
        // An actor:agent:* id ABSENT from the map is unattributed. The scheme must NOT leak
        // into any kind/role predicate (INV-5) — not even kind=agent.
        let map = ActorAttributesMap::default();
        let agent = map.resolve(&ActorId::new("actor:agent:claude-code"));
        assert_eq!(agent.kind(), None);
        assert!(!agent.is_kind("agent"));
        assert!(!agent.is_kind("human"));
        assert!(!agent.has_role("reviewer"));
        assert!(agent.roles().is_empty());
    }

    #[test]
    fn hard_split_unrecognized_kind_round_trips_but_satisfies_no_predicate() {
        let map = actor_attributes_from_value(serde_json::json!({
            "schema": "shore.actor-attributes.v1",
            "actors": { "actor:agent:future": { "kind": "quorum-service" } }
        }))
        .unwrap();
        let future = map.resolve(&ActorId::new("actor:agent:future"));
        // It round-trips via kind() (reserved-but-OPEN: an unknown kind is still stored/displayed)...
        assert_eq!(future.kind(), Some("quorum-service"));
        // ...but ADR-0012: "an unrecognized kind does not satisfy any kind= predicate" — including a
        // query for its own value. `is_kind` matches only RESERVED kinds.
        assert!(
            !future.is_kind("quorum-service"),
            "unrecognized kind satisfies no kind= predicate"
        );
        assert!(!future.is_kind("human"));
        assert!(!future.is_kind("service"));
    }

    #[test]
    fn stage_actor_attributes_round_trips_through_the_reader() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(".shore/actor-attributes.json");
        let actor = ActorId::new("actor:git-email:kevin@swiber.dev");
        let attrs = ActorAttributesWriteRecord::new("human".to_owned())
            .with_roles(vec!["reviewer".to_owned(), "author".to_owned()])
            .with_comment(Some("me".to_owned()));

        let outcome = stage_actor_attributes(&path, &actor, &attrs).unwrap();
        assert!(outcome.changed);

        let map = ActorAttributesMap::from_attributes_file(&path).unwrap();
        let resolved = map.resolve(&actor);
        assert_eq!(resolved.kind(), Some("human"));
        // roles deduped + sorted (BTreeSet semantics, matching the reader).
        assert_eq!(
            resolved.roles().iter().cloned().collect::<Vec<_>>(),
            vec!["author", "reviewer"]
        );
    }

    #[test]
    fn stage_actor_attributes_normalizes_tokens_lowercase_kebab() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(".shore/actor-attributes.json");
        let actor = ActorId::new("actor:agent:review-bot");
        // Mixed case + duplicate roles → normalized, deduped, sorted.
        let attrs = ActorAttributesWriteRecord::new("Reviewer-Model".to_owned()).with_roles(vec![
            "Reviewer".to_owned(),
            "reviewer".to_owned(),
            "CI".to_owned(),
        ]);
        stage_actor_attributes(&path, &actor, &attrs).unwrap();
        let map = ActorAttributesMap::from_attributes_file(&path).unwrap();
        let r = map.resolve(&actor);
        assert_eq!(r.kind(), Some("reviewer-model"));
        assert_eq!(
            r.roles().iter().cloned().collect::<Vec<_>>(),
            vec!["ci", "reviewer"]
        );
    }

    #[test]
    fn stage_actor_attributes_replaces_per_actor_and_is_byte_stable() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(".shore/actor-attributes.json");
        let actor = ActorId::new("actor:git-email:kevin@swiber.dev");
        let attrs = ActorAttributesWriteRecord::new("human".to_owned())
            .with_roles(vec!["author".to_owned()]);

        let first = stage_actor_attributes(&path, &actor, &attrs).unwrap();
        let before = std::fs::read(&path).unwrap();
        let second = stage_actor_attributes(&path, &actor, &attrs).unwrap();
        let after = std::fs::read(&path).unwrap();
        assert!(
            first.changed && !second.changed,
            "identical re-attest is a no-op"
        );
        assert_eq!(before, after, "re-attest leaves the file byte-identical");
        assert!(before.ends_with(b"\n"));

        // Re-attest with a different kind REPLACES the actor's entry (set-per-actor).
        let changed = stage_actor_attributes(
            &path,
            &actor,
            &ActorAttributesWriteRecord::new("service".to_owned()),
        )
        .unwrap();
        assert!(changed.changed);
        assert_eq!(
            ActorAttributesMap::from_attributes_file(&path)
                .unwrap()
                .resolve(&actor)
                .kind(),
            Some("service")
        );
    }

    #[test]
    fn stage_actor_attributes_preserves_other_actors() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(".shore/actor-attributes.json");
        stage_actor_attributes(
            &path,
            &ActorId::new("actor:git-email:kevin@swiber.dev"),
            &ActorAttributesWriteRecord::new("human".to_owned()),
        )
        .unwrap();
        stage_actor_attributes(
            &path,
            &ActorId::new("actor:agent:review-bot"),
            &ActorAttributesWriteRecord::new("reviewer-model".to_owned()),
        )
        .unwrap();
        let map = ActorAttributesMap::from_attributes_file(&path).unwrap();
        assert_eq!(
            map.resolve(&ActorId::new("actor:git-email:kevin@swiber.dev"))
                .kind(),
            Some("human")
        );
        assert_eq!(
            map.resolve(&ActorId::new("actor:agent:review-bot")).kind(),
            Some("reviewer-model")
        );
    }

    #[test]
    fn stage_actor_attributes_refuses_when_an_existing_sibling_is_malformed() {
        // INV-B: a pre-existing malformed sibling (here, an entry with no kind — valid JSON,
        // invalid schema) must make the stage FAIL, not write a file the reader rejects.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(".shore/actor-attributes.json");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, br#"{"schema":"shore.actor-attributes.v1","actors":{"actor:agent:bad":{"roles":["reviewer"]}}}"#).unwrap();
        let before = std::fs::read(&path).unwrap();

        let result = stage_actor_attributes(
            &path,
            &ActorId::new("actor:git-email:kevin@swiber.dev"),
            &ActorAttributesWriteRecord::new("human".to_owned()),
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
    fn stage_actor_attributes_rejects_bad_key_and_bad_tokens() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(".shore/actor-attributes.json");
        // Invalid actor key.
        assert!(
            stage_actor_attributes(
                &path,
                &ActorId::new("not-an-actor"),
                &ActorAttributesWriteRecord::new("human".to_owned())
            )
            .is_err()
        );
        // Non-kebab role token (contains a space) — must be rejected by the same grammar the reader uses.
        assert!(
            stage_actor_attributes(
                &path,
                &ActorId::new("actor:agent:x"),
                &ActorAttributesWriteRecord::new("agent".to_owned())
                    .with_roles(vec!["Has Space".to_owned()])
            )
            .is_err()
        );
        assert!(!path.exists(), "a rejected attest writes nothing");
    }
}
