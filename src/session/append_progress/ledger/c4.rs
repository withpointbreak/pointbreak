//! Closed private append/restart protocol. No product route calls this module.
use super::super::audit as budget;
use super::*;
pub(in crate::session::append_progress) mod audit;
use rusqlite::OptionalExtension;
use serde::{Deserialize, Serialize};

use crate::session::event::ShoreEvent;
use crate::session::store::authority_lock::PhysicalFileIdentity;
use crate::session::store::{EventStore, EventWriteOutcome, PreparedProgressEvent};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Binding {
    selection_sha256: String,
    baseline_sha256: String,
    capability_pair_sha256: String,
    epoch: String,
}
impl Binding {
    fn from_inputs(inputs: &InitialInputs) -> Result<Self> {
        let b = inputs.bindings()?;
        Ok(Self {
            selection_sha256: sha256_json_hex(&b.selection)?,
            baseline_sha256: sha256_json_hex(&b.baseline)?,
            capability_pair_sha256: sha256_json_hex(&b.pair)?,
            epoch: inputs.epoch.clone(),
        })
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Obligation {
    version: u32,
    binding: Binding,
    minimum_reader_profile: String,
    attempt_id: String,
    key: String,
    expected_sha256: String,
    expected_length: i64,
    next_head: i64,
    previous_head: i64,
    previous_prefix: String,
    previous_native: JournalChangeStamp,
}
impl Obligation {
    fn validate(&self, binding: &Binding) -> Result<()> {
        if self.version != 1
            || &self.binding != binding
            || self.minimum_reader_profile != "review_change_revision_progress_v1"
            || !hash64(&self.attempt_id)
            || self.key.is_empty()
            || !hash64(&self.expected_sha256)
            || self.expected_length < 0
            || self.previous_head < 0
            || self.previous_head.checked_add(1) != Some(self.next_head)
            || !hash64(&self.previous_prefix)
        {
            return Err(fail("invalid or foreign obligation"));
        }
        supported_stamp(&self.previous_native)
    }
    fn receipt(&self) -> Receipt {
        Receipt {
            sequence: self.next_head,
            key: self.key.clone(),
            stored_sha256: self.expected_sha256.clone(),
            stored_length: self.expected_length,
            original_attempt: self.attempt_id.clone(),
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum Proof {
    StableAbsence {
        native: JournalChangeStamp,
    },
    LivePublication {
        native: JournalChangeStamp,
        stored_sha256: String,
        stored_length: i64,
    },
    CensusAbsent {
        census: audit::CensusBinding,
    },
    CensusPresent {
        census: audit::CensusBinding,
        stored_sha256: String,
        stored_length: i64,
    },
}
impl Proof {
    fn native(&self) -> &JournalChangeStamp {
        match self {
            Self::StableAbsence { native } | Self::LivePublication { native, .. } => native,
            Self::CensusAbsent { census } | Self::CensusPresent { census, .. } => &census.native,
        }
    }
    fn present(&self) -> bool {
        matches!(
            self,
            Self::LivePublication { .. } | Self::CensusPresent { .. }
        )
    }
    fn census(&self) -> bool {
        matches!(self, Self::CensusAbsent { .. } | Self::CensusPresent { .. })
    }
    fn validate(&self, o: &Obligation) -> Result<()> {
        if let Self::CensusAbsent { census } | Self::CensusPresent { census, .. } = self {
            census.validate(o)?;
        }
        supported_stamp(self.native())?;
        same_native_identity(&o.previous_native, self.native())?;
        if let Self::LivePublication {
            stored_sha256,
            stored_length,
            ..
        }
        | Self::CensusPresent {
            stored_sha256,
            stored_length,
            ..
        } = self
            && (stored_sha256 != &o.expected_sha256 || *stored_length != o.expected_length)
        {
            return Err(fail("presence proof does not bind original stored bytes"));
        }
        Ok(())
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Phase {
    Init,
    Intent,
    ProvedAbsent,
    ProvedPresent,
    SettledAbsent,
    SettledPresent,
}
impl Phase {
    fn text(self) -> &'static str {
        match self {
            Self::Init => "INIT",
            Self::Intent => "INTENT",
            Self::ProvedAbsent => "PROVED_ABSENT",
            Self::ProvedPresent => "PROVED_PRESENT",
            Self::SettledAbsent => "SETTLED_ABSENT",
            Self::SettledPresent => "SETTLED_PRESENT",
        }
    }
    fn parse(s: &str) -> Result<Self> {
        match s {
            "INIT" => Ok(Self::Init),
            "INTENT" => Ok(Self::Intent),
            "PROVED_ABSENT" => Ok(Self::ProvedAbsent),
            "PROVED_PRESENT" => Ok(Self::ProvedPresent),
            "SETTLED_ABSENT" => Ok(Self::SettledAbsent),
            "SETTLED_PRESENT" => Ok(Self::SettledPresent),
            _ => Err(fail("unknown phase")),
        }
    }
    fn completed(self) -> bool {
        matches!(
            self,
            Self::Init | Self::SettledAbsent | Self::SettledPresent
        )
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
struct State {
    binding: Binding,
    head: i64,
    prefix: String,
    native: JournalChangeStamp,
    phase: Phase,
    obligation: Option<Obligation>,
    proof: Option<Proof>,
}
impl State {
    fn obligation(&self) -> Result<&Obligation> {
        self.obligation
            .as_ref()
            .ok_or_else(|| fail("missing obligation"))
    }
    fn validate(&self, initial_prefix: &str) -> Result<()> {
        if self.head < 0 || !hash64(&self.prefix) {
            return Err(fail("invalid head/prefix"));
        }
        supported_stamp(&self.native)?;
        if self.phase == Phase::Init {
            if self.head != 0
                || self.prefix != initial_prefix
                || self.obligation.is_some()
                || self.proof.is_some()
            {
                return Err(fail("invalid INIT state"));
            }
            return Ok(());
        }
        let o = self.obligation()?;
        o.validate(&self.binding)?;
        if self.phase == Phase::Intent {
            if self.proof.is_some() {
                return Err(fail("INTENT contains proof"));
            }
        } else {
            let proof = self
                .proof
                .as_ref()
                .ok_or_else(|| fail("missing phase proof"))?;
            proof.validate(o)?;
            let present = proof.present();
            if present != matches!(self.phase, Phase::ProvedPresent | Phase::SettledPresent) {
                return Err(fail("proof/phase mismatch"));
            }
        }
        let (head, prefix, native) = match self.phase {
            Phase::SettledPresent => (
                o.next_head,
                prefix_for(o)?,
                self.proof.as_ref().unwrap().native(),
            ),
            Phase::SettledAbsent => (
                o.previous_head,
                o.previous_prefix.clone(),
                self.proof.as_ref().unwrap().native(),
            ),
            _ => (
                o.previous_head,
                o.previous_prefix.clone(),
                &o.previous_native,
            ),
        };
        if self.head != head || self.prefix != prefix || &self.native != native {
            return Err(fail("phase metadata does not bind original obligation"));
        }
        Ok(())
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Receipt {
    sequence: i64,
    key: String,
    stored_sha256: String,
    stored_length: i64,
    original_attempt: String,
}
impl Receipt {
    fn validate(&self) -> Result<()> {
        if self.sequence < 1
            || self.key.is_empty()
            || !hash64(&self.stored_sha256)
            || self.stored_length < 0
            || !hash64(&self.original_attempt)
        {
            return Err(fail("invalid receipt"));
        }
        Ok(())
    }
}
#[derive(Clone, Debug)]
pub(super) struct Snapshot {
    state: State,
    tail: Option<Receipt>,
    requested: Option<Receipt>,
    attempt_exists: bool,
    prefix_inventory: Option<audit::ValidatedPrefix>,
}
#[derive(Clone)]
struct Change {
    old: State,
    new: State,
    attempt: Option<Obligation>,
    receipt: Option<Receipt>,
}
#[derive(Clone)]
pub(super) struct Command {
    full_prefix: bool,
    key: Option<String>,
    attempt: Option<String>,
    change: Option<Change>,
    required_native: Option<JournalChangeStamp>,
}

#[cfg(test)]
thread_local! { static WORK: Cell<(u64,u64)> = const { Cell::new((0,0)) }; }
fn row_read(r: &rusqlite::Row<'_>) -> rusqlite::Result<()> {
    budget::row(r)?;
    #[cfg(test)]
    WORK.with(|w| {
        let (rows, bytes) = w.get();
        w.set((rows + 1, bytes));
    });
    Ok(())
}
#[cfg(test)]
fn reset_work() {
    WORK.with(|w| w.set((0, 0)));
}
#[cfg(test)]
fn work() -> (u64, u64) {
    WORK.with(Cell::get)
}

fn fail(message: &str) -> ShoreError {
    ShoreError::Message(format!("private append refused: {message}"))
}
fn prefix_for(o: &Obligation) -> Result<String> {
    let bytes = canonical_json_bytes(
        &json!({"domain":"pointbreak.append-progress-prefix.v1", "previousPrefixSha256":o.previous_prefix,
        "head":o.next_head, "receipt":o.receipt()}),
    )?;
    #[cfg(test)]
    WORK.with(|w| {
        let (rows, count) = w.get();
        w.set((rows, count + bytes.len() as u64));
    });
    Ok(sha256_bytes_hex(&bytes))
}

fn closed<T: serde::de::DeserializeOwned + Serialize>(value: &str) -> Result<T> {
    let result: T = serde_json::from_str(value).map_err(|_| fail("closed phase record"))?;
    if text(&serde_json::to_value(&result)?)? != value {
        return Err(fail("noncanonical phase record"));
    }
    Ok(result)
}
fn same_native_identity(a: &JournalChangeStamp, b: &JournalChangeStamp) -> Result<()> {
    supported_stamp(a)?;
    supported_stamp(b)?;
    match (a, b) {
        (
            JournalChangeStamp::Observed {
                identity_sha256: a, ..
            },
            JournalChangeStamp::Observed {
                identity_sha256: b, ..
            },
        ) if a == b => Ok(()),
        _ => Err(fail("native identity changed")),
    }
}
fn receipt_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<Receipt> {
    row_read(r)?;
    Ok(Receipt {
        sequence: r.get(0)?,
        key: r.get(1)?,
        stored_sha256: r.get(2)?,
        stored_length: r.get(3)?,
        original_attempt: r.get(4)?,
    })
}
fn key_receipt(db: &Connection, key: &str) -> Result<Option<Receipt>> {
    db.query_row("SELECT sequence,key,stored_sha256,stored_length,original_attempt FROM receipt WHERE key=?1",[key],receipt_row).optional().map_err(sql)
}
fn attempt_matches(db: &Connection, o: &Obligation) -> Result<()> {
    let actual: (String, String, i64) = db
        .query_row(
            "SELECT key,expected_sha256,expected_length FROM attempt WHERE attempt_id=?1",
            [&o.attempt_id],
            |r| {
                row_read(r)?;
                Ok((r.get(0)?, r.get(1)?, r.get(2)?))
            },
        )
        .map_err(sql)?;
    if actual != (o.key.clone(), o.expected_sha256.clone(), o.expected_length) {
        return Err(fail("original attempt mismatch"));
    }
    Ok(())
}
fn check_receipt_attempt(db: &Connection, r: &Receipt) -> Result<()> {
    r.validate()?;
    let actual: (String, String, i64) = db
        .query_row(
            "SELECT key,expected_sha256,expected_length FROM attempt WHERE attempt_id=?1",
            [&r.original_attempt],
            |r| {
                row_read(r)?;
                Ok((r.get(0)?, r.get(1)?, r.get(2)?))
            },
        )
        .map_err(sql)?;
    if actual != (r.key.clone(), r.stored_sha256.clone(), r.stored_length) {
        return Err(fail("receipt attempt mismatch"));
    }
    Ok(())
}
fn load(db: &Connection, inputs: &InitialInputs, command: &Command) -> Result<Snapshot> {
    // Constant schema, two-row baseline, singleton, current attempt, tail and
    // requested key only. Never count or enumerate attempts or receipts.
    validate_schema(db)?;
    let binding = Binding::from_inputs(inputs)?;
    let (actual,head,prefix,native,state):(Binding,i64,String,String,String)=db.query_row(
        "SELECT selection_sha256,baseline_sha256,capability_pair_sha256,epoch,head,prefix_sha256,native_stamp,state FROM meta WHERE singleton=1",[],|r|{row_read(r)?; Ok((Binding{selection_sha256:r.get(0)?,baseline_sha256:r.get(1)?,capability_pair_sha256:r.get(2)?,epoch:r.get(3)?},r.get(4)?,r.get(5)?,r.get(6)?,r.get(7)?))}).map_err(sql)?;
    if actual != binding || state != "ready" {
        return Err(fail("not selected ready metadata"));
    }
    let b = inputs.bindings()?;
    let mut query=db.prepare("SELECT key_digest,record_sha256,record_length,kind FROM baseline ORDER BY key_digest LIMIT 3").map_err(sql)?;
    let rows=query.query_map([],|r|{row_read(r)?; Ok(json!({"keyDigest":r.get::<_,String>(0)?,"recordSha256":r.get::<_,String>(1)?,"recordLength":r.get::<_,i64>(2)?,"kind":r.get::<_,String>(3)?}))}).map_err(sql)?.collect::<rusqlite::Result<Vec<_>>>().map_err(sql)?;
    if Some(&rows) != b.baseline["records"].as_array() {
        return Err(fail("baseline changed"));
    }
    drop(query);
    let (phase, obligation, proof): (String, Option<String>, Option<String>) = db
        .query_row(
            "SELECT phase,original_obligation_json,proof_json FROM current WHERE singleton=1",
            [],
            |r| {
                row_read(r)?;
                Ok((r.get(0)?, r.get(1)?, r.get(2)?))
            },
        )
        .map_err(sql)?;
    let state = State {
        binding,
        head,
        prefix,
        native: decode_stamp(&native)?,
        phase: Phase::parse(&phase)?,
        obligation: obligation.as_deref().map(closed).transpose()?,
        proof: proof.as_deref().map(closed).transpose()?,
    };
    state.validate(&sha256_json_hex(&b.prefix)?)?;
    if let Some(o) = &state.obligation {
        attempt_matches(db, o)?;
        let found = key_receipt(db, &o.key)?;
        if state.phase == Phase::SettledPresent {
            if found.as_ref() != Some(&o.receipt()) {
                return Err(fail("terminal receipt mismatch"));
            }
        } else if found.is_some() {
            return Err(fail("unsettled/absent obligation already has receipt"));
        }
    }
    let tail = if head == 0 {
        None
    } else {
        Some(db.query_row("SELECT sequence,key,stored_sha256,stored_length,original_attempt FROM receipt WHERE sequence=?1",[head],receipt_row).map_err(sql)?)
    };
    if let Some(r) = &tail {
        check_receipt_attempt(db, r)?;
    }
    if let Some(next) = head.checked_add(1)
        && db
            .query_row(
                "SELECT sequence FROM receipt WHERE sequence=?1",
                [next],
                |r| {
                    budget::row(r)?;
                    r.get::<_, i64>(0)
                },
            )
            .optional()
            .map_err(sql)?
            .is_some()
    {
        return Err(fail("receipt beyond head"));
    }
    let requested = command
        .key
        .as_deref()
        .map(|key| key_receipt(db, key))
        .transpose()?
        .flatten();
    if let Some(r) = &requested {
        check_receipt_attempt(db, r)?;
        if r.sequence > head {
            return Err(fail("requested receipt beyond head"));
        }
    }
    let attempt_exists = match &command.attempt {
        Some(attempt) => db
            .query_row(
                "SELECT attempt_id FROM attempt WHERE attempt_id=?1",
                [attempt],
                |r| {
                    budget::row(r)?;
                    r.get::<_, String>(0)
                },
            )
            .optional()
            .map_err(sql)?
            .is_some(),
        None => false,
    };
    Ok(Snapshot {
        prefix_inventory: None,
        state,
        tail,
        requested,
        attempt_exists,
    })
}
fn canonical_option<T: Serialize>(v: &Option<T>) -> Result<Option<String>> {
    v.as_ref()
        .map(|v| text(&serde_json::to_value(v)?))
        .transpose()
}
fn execute(db: &Connection, context: &OwnerContext<'_>) -> Result<OperationOutcome> {
    let command = context
        .c4_command
        .as_ref()
        .ok_or_else(|| fail("missing closed command"))?;
    let anchor = context
        .anchor
        .as_ref()
        .ok_or_else(|| fail("missing native anchor"))?;
    configure(db)?;
    context.gate(db, anchor)?;
    let tx = rusqlite::Transaction::new_unchecked(db, rusqlite::TransactionBehavior::Immediate)
        .map_err(sql)?;
    let before = load(db, context.evidence.inputs(), command)?;
    if let Some(change) = &command.change {
        if before.state != change.old {
            return Err(fail("transition old state changed"));
        }
        validate_change(change, context.profile)?;
        if let Some(o) = &change.attempt {
            tx.execute("INSERT INTO attempt(attempt_id,key,expected_sha256,expected_length) VALUES(?1,?2,?3,?4)",params![o.attempt_id,o.key,o.expected_sha256,o.expected_length]).map_err(sql)?;
        }
        if let Some(r) = &change.receipt {
            tx.execute("INSERT INTO receipt(sequence,key,stored_sha256,stored_length,original_attempt) VALUES(?1,?2,?3,?4,?5)",params![r.sequence,r.key,r.stored_sha256,r.stored_length,r.original_attempt]).map_err(sql)?;
        }
        if tx.execute("UPDATE current SET phase=?1,original_obligation_json=?2,proof_json=?3 WHERE singleton=1",params![change.new.phase.text(),canonical_option(&change.new.obligation)?,canonical_option(&change.new.proof)?]).map_err(sql)?!=1 { return Err(fail("current singleton disappeared")); }
        if tx
            .execute(
                "UPDATE meta SET head=?1,prefix_sha256=?2,native_stamp=?3 WHERE singleton=1",
                params![
                    change.new.head,
                    change.new.prefix,
                    text(&serde_json::to_value(&change.new.native)?)?
                ],
            )
            .map_err(sql)?
            != 1
        {
            return Err(fail("metadata singleton disappeared"));
        }
        if load(db, context.evidence.inputs(), command)?.state != change.new {
            return Err(fail("transition new state mismatch"));
        }
        boundary(&format!("{}-before-commit", change.new.phase.text()))?;
    }
    context.gate(db, anchor)?;
    tx.commit().map_err(sql)?;
    if let Some(change) = &command.change {
        boundary(&format!("{}-after-commit", change.new.phase.text()))?;
    }
    context.gate(db, anchor)?;
    let (busy, log, done): (i64, i64, i64) = db
        .query_row("PRAGMA wal_checkpoint(FULL)", [], |r| {
            budget::row(r)?;
            Ok((r.get(0)?, r.get(1)?, r.get(2)?))
        })
        .map_err(sql)?;
    if busy != 0 || log < 0 || done != log {
        return Err(fail("phase checkpoint incomplete"));
    }
    // SAFETY: existing native owner retains every Q descriptor through finish.
    if unsafe { ffi::sqlite3_pb_writer_barrier_v1(db.handle(), 0) } != ffi::SQLITE_OK {
        return Err(fail("phase native barrier failed"));
    }
    context.evidence.sync_directories()?;
    context.gate(db, anchor)?;
    if let Some(change) = &command.change {
        boundary(&format!("{}-after-barrier", change.new.phase.text()))?;
    }
    let mut snapshot = load(db, context.evidence.inputs(), command)?;
    if command.full_prefix {
        if context.profile != ScopeProfile::Reconcile || command.change.is_some() {
            return Err(fail("full prefix requires read-only Reconcile command"));
        }
        snapshot.prefix_inventory = Some(audit::read_prefix(
            db,
            context.evidence.inputs(),
            &snapshot,
            anchor,
        )?);
    }
    Ok(OperationOutcome {
        anchor: anchor.clone(),
        already_ready: false,
        c4: Some(snapshot),
    })
}
fn validate_change(c: &Change, profile: ScopeProfile) -> Result<()> {
    if c.old.binding != c.new.binding {
        return Err(fail("transition binding changed"));
    }
    match (c.old.phase, c.new.phase) {
        (old, Phase::Intent) if old.completed() && profile == ScopeProfile::Append => {
            let o = c.new.obligation()?;
            if c.attempt.as_ref() != Some(o)
                || c.receipt.is_some()
                || c.new.proof.is_some()
                || o.previous_head != c.old.head
                || o.previous_prefix != c.old.prefix
                || o.previous_native != c.old.native
            {
                return Err(fail("invalid intent transition"));
            }
        }
        (Phase::Intent, Phase::ProvedAbsent | Phase::ProvedPresent) => {
            if c.old.obligation != c.new.obligation || c.attempt.is_some() || c.receipt.is_some() {
                return Err(fail("proof replaced original obligation"));
            }
        }
        (
            Phase::ProvedAbsent | Phase::ProvedPresent,
            Phase::ProvedAbsent | Phase::ProvedPresent,
        ) if profile == ScopeProfile::Reconcile
            && c.new.proof.as_ref().is_some_and(Proof::census) =>
        {
            if c.old.obligation != c.new.obligation
                || c.attempt.is_some()
                || c.receipt.is_some()
                || c.old.head != c.new.head
                || c.old.prefix != c.new.prefix
                || c.old.native != c.new.native
                || (c.old.phase == Phase::ProvedPresent && c.new.phase != Phase::ProvedPresent)
            {
                return Err(fail("invalid explicit census reproof"));
            }
        }
        (Phase::ProvedAbsent, Phase::SettledAbsent)
        | (Phase::ProvedPresent, Phase::SettledPresent) => {
            if c.old.obligation != c.new.obligation
                || c.old.proof != c.new.proof
                || c.attempt.is_some()
            {
                return Err(fail("settlement replaced proof/obligation"));
            }
            let expected = if c.new.phase == Phase::SettledPresent {
                Some(c.new.obligation()?.receipt())
            } else {
                None
            };
            if c.receipt != expected {
                return Err(fail("settlement receipt mismatch"));
            }
        }
        _ => return Err(fail("illegal phase transition")),
    }
    Ok(())
}
fn boundary(at: &str) -> Result<()> {
    #[cfg(target_os = "macos")]
    super::super::consumer::boundary(at)?;
    #[cfg(test)]
    {
        super::hook(at)?;
        if std::env::var("POINTBREAK_C4_CUT").as_deref() == Ok(at) {
            assert_eq!(std::env::var("POINTBREAK_C4_CHILD").as_deref(), Ok("1"));
            // SAFETY: only the explicitly isolated test child has this cfg(test) path.
            unsafe { libc::_exit(79) };
        }
    }
    let _ = at;
    Ok(())
}

struct Session<'a> {
    root: &'a NativeRootHandle,
    guard: &'a StoreAuthorityLock,
    request: &'a str,
    actor: &'a str,
    canonical: PathBuf,
    identities: Vec<(EvidenceRole, Option<PhysicalFileIdentity>)>,
    publication_used: Cell<bool>,
}
impl<'a> Session<'a> {
    fn new(
        root: &'a NativeRootHandle,
        guard: &'a StoreAuthorityLock,
        request: &'a str,
        actor: &'a str,
    ) -> Result<Self> {
        super::super::owner::require_quiescent()?;
        let evidence = RetainedEvidence::capture_before_sql(
            root,
            guard,
            EvidenceStage::Reconcile,
            request,
            actor,
        )?;
        evidence.validate_live()?;
        let identities = evidence
            .members()
            .iter()
            .map(|m| (m.role(), m.identity()))
            .collect();
        Ok(Self {
            root,
            guard,
            request,
            actor,
            canonical: native::root_snapshot(root)?.canonical,
            identities,
            publication_used: Cell::new(false),
        })
    }
    fn capture(&self, stage: EvidenceStage) -> Result<RetainedEvidence<'a>> {
        let evidence = RetainedEvidence::capture_before_sql(
            self.root,
            self.guard,
            stage,
            self.request,
            self.actor,
        )?;
        if evidence
            .members()
            .iter()
            .map(|m| (m.role(), m.identity()))
            .collect::<Vec<_>>()
            != self.identities
        {
            return Err(fail("retained namespace identity changed between scopes"));
        }
        evidence.validate_live()?;
        Ok(evidence)
    }
    fn step(&self, profile: ScopeProfile, command: Command) -> Result<Snapshot> {
        if command.full_prefix && (profile != ScopeProfile::Reconcile || command.change.is_some()) {
            return Err(fail("audit read requires closed Reconcile entry"));
        }
        let stage = if profile == ScopeProfile::Append {
            EvidenceStage::Append
        } else {
            EvidenceStage::Reconcile
        };
        let evidence = self.capture(stage)?;
        if let Some(required) = &command.required_native {
            accept_check(required, &evidence.journal_check(required)?)?;
        }
        let anchor = evidence.journal_stamp()?;
        let reserve = budget::reserve_retirement(evidence.retirement_read_bytes()?)?;
        let permit = LiveSqlPermit(super::super::owner::SqlPermit::enter()?);
        let mut owner = NativeSqlOwner::prepare(evidence, profile);
        let context = owner.context.as_mut().expect("prepared context");
        context.anchor = Some(anchor);
        context.c4_command = Some(command);
        let result = owner
            .open_native(&permit)
            .and_then(|()| owner.with_connection(&permit, execute));
        let finished = owner.finish();
        drop(permit);
        drop(reserve);
        let finished = finished?;
        finished.evidence().validate_live()?;
        let outcome = result?;
        accept_check(
            &outcome.anchor,
            &finished.evidence().journal_check(&outcome.anchor)?,
        )?;
        outcome
            .c4
            .ok_or_else(|| fail("missing detached append snapshot"))
    }
    fn read(&self, key: Option<&str>, attempt: Option<&str>) -> Result<Snapshot> {
        self.step(
            ScopeProfile::Reconcile,
            Command {
                full_prefix: false,
                key: key.map(str::to_owned),
                attempt: attempt.map(str::to_owned),
                change: None,
                required_native: None,
            },
        )
    }
    fn transition(
        &self,
        change: Change,
        key: Option<&str>,
        attempt: Option<&str>,
    ) -> Result<Snapshot> {
        let profile = if change.new.phase == Phase::Intent {
            ScopeProfile::Append
        } else {
            ScopeProfile::Reconcile
        };
        let required = change
            .new
            .proof
            .as_ref()
            .map(Proof::native)
            .unwrap_or(&change.new.native)
            .clone();
        let command = Command {
            full_prefix: false,
            key: key.map(str::to_owned),
            attempt: attempt.map(str::to_owned),
            change: Some(change.clone()),
            required_native: Some(required.clone()),
        };
        match self.step(profile, command) {
            Ok(value) => Ok(value),
            Err(first) => {
                // All SQL owners finished before this fresh-handle uncertainty read.
                // Re-establish durability; no third state is guessed into success.
                let observed = self.step(
                    ScopeProfile::Reconcile,
                    Command {
                        full_prefix: false,
                        key: key.map(str::to_owned),
                        attempt: attempt.map(str::to_owned),
                        change: None,
                        required_native: Some(required),
                    },
                )?;
                if observed.state == change.new {
                    Ok(observed)
                } else if observed.state == change.old {
                    Err(first)
                } else {
                    Err(fail(
                        "uncertain commit reopened to neither legal old nor new state",
                    ))
                }
            }
        }
    }
    fn check_carrier(
        &self,
        key: &str,
        witness: Option<(&str, i64)>,
        anchor: &JournalChangeStamp,
        durable: bool,
    ) -> Result<JournalChangeStamp> {
        let evidence = self.capture(EvidenceStage::Reconcile)?;
        accept_check(anchor, &evidence.journal_check(anchor)?)?;
        let bytes = if budget::active() {
            let path = self
                .canonical
                .join("events")
                .join(format!("{}.json", sha256_bytes_hex(key.as_bytes())));
            match std::fs::symlink_metadata(&path) {
                Ok(_) => Some(HeldPath::open(&path, false)?.read()?),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
                Err(e) => return Err(io(e)),
            }
        } else {
            LocalJournal::new(&self.canonical).read_event_bytes(key)?
        };
        match (witness, bytes) {
            (None, None) => {}
            (Some((hash, length)), Some(bytes))
                if bytes.len() as u128 == length as u128 && sha256_bytes_hex(&bytes) == hash =>
            {
                if durable {
                    native::sync_exact_carrier(self.root, key, hash, length as u64)?;
                }
            }
            _ => {
                return Err(fail(
                    "carrier mismatch; explicit audit required, obligation preserved",
                ));
            }
        }
        evidence.validate_live()?;
        let check = evidence.journal_check(anchor)?;
        accept_check(anchor, &check)?;
        Ok(check.after)
    }
    fn completed(&self, snapshot: &Snapshot) -> Result<()> {
        let evidence = self.capture(EvidenceStage::Reconcile)?;
        accept_check(
            &snapshot.state.native,
            &evidence.journal_check(&snapshot.state.native)?,
        )?;
        if let Some(tail) = &snapshot.tail {
            self.check_carrier(
                &tail.key,
                Some((&tail.stored_sha256, tail.stored_length)),
                &snapshot.state.native,
                false,
            )?;
        }
        if snapshot.state.phase == Phase::SettledAbsent {
            self.check_carrier(
                &snapshot.state.obligation()?.key,
                None,
                &snapshot.state.native,
                false,
            )?;
        }
        Ok(())
    }
    fn reconcile(
        &self,
        mut snapshot: Snapshot,
        key: Option<&str>,
        attempt: Option<&str>,
    ) -> Result<Snapshot> {
        if snapshot.state.phase.completed() {
            self.completed(&snapshot)?;
            return Ok(snapshot);
        }
        if snapshot.state.phase == Phase::Intent {
            let o = snapshot.state.obligation()?;
            let native = self.check_carrier(&o.key, None, &o.previous_native, false)?;
            let mut next = snapshot.state.clone();
            next.phase = Phase::ProvedAbsent;
            next.proof = Some(Proof::StableAbsence { native });
            snapshot = self.transition(
                Change {
                    old: snapshot.state,
                    new: next,
                    attempt: None,
                    receipt: None,
                },
                key,
                attempt,
            )?;
        }
        let o = snapshot.state.obligation()?.clone();
        let proof = snapshot
            .state
            .proof
            .as_ref()
            .ok_or_else(|| fail("unresolved phase has no proof"))?
            .clone();
        let present = proof.present();
        self.check_carrier(
            &o.key,
            if present {
                Some((&o.expected_sha256, o.expected_length))
            } else {
                None
            },
            proof.native(),
            present,
        )?;
        let mut next = snapshot.state.clone();
        next.phase = if present {
            Phase::SettledPresent
        } else {
            Phase::SettledAbsent
        };
        next.native = proof.native().clone();
        if present {
            next.head = o.next_head;
            next.prefix = prefix_for(&o)?;
        }
        let snapshot = self.transition(
            Change {
                old: snapshot.state,
                new: next,
                attempt: None,
                receipt: present.then(|| o.receipt()),
            },
            key,
            attempt,
        )?;
        self.completed(&snapshot)?;
        Ok(snapshot)
    }
}

/// The sole construction sites are below, after durable intent or a validated
/// existing receipt. Consumed once; no safe caller can invent publication rights.
pub(crate) struct PublicationPermit<'s, 'a> {
    session: &'s Session<'a>,
    snapshot: &'s Snapshot,
    prepared_sha256: String,
    prepared_length: usize,
}
impl<'s, 'a> PublicationPermit<'s, 'a> {
    pub(crate) fn authorize(self, prepared: &PreparedProgressEvent) -> Result<&'s Path> {
        if self.session.publication_used.replace(true) {
            return Err(fail("publication already attempted"));
        }
        super::super::owner::require_quiescent()?;
        self.session.root.validate_live()?;
        self.session
            .guard
            .validate_physical_root(&self.session.canonical)?;
        if sha256_bytes_hex(prepared.bytes()) != self.prepared_sha256
            || prepared.bytes().len() != self.prepared_length
        {
            return Err(fail("prepared publication bytes changed"));
        }
        let state = &self.snapshot.state;
        if let Some(receipt) = &self.snapshot.requested {
            if !state.phase.completed() || receipt.key != prepared.event().idempotency_key {
                return Err(fail("invalid duplicate publication phase"));
            }
            self.session.check_carrier(
                &receipt.key,
                Some((&receipt.stored_sha256, receipt.stored_length)),
                &state.native,
                false,
            )?;
        } else {
            let o = state.obligation()?;
            if state.phase != Phase::Intent
                || o.key != prepared.event().idempotency_key
                || o.expected_sha256 != self.prepared_sha256
                || o.expected_length as u128 != self.prepared_length as u128
            {
                return Err(fail("publication does not match durable INTENT"));
            }
            self.session
                .check_carrier(&o.key, None, &o.previous_native, false)?;
        }
        Ok(&self.session.canonical)
    }
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppendExit {
    pub outcome: Option<EventWriteOutcome>,
    pub finalized: bool,
    pub head: Option<u64>,
    pub prefix_sha256: Option<String>,
    pub detail: Option<String>,
}
impl AppendExit {
    fn finalized(snapshot: &Snapshot, outcome: Option<EventWriteOutcome>) -> Self {
        Self {
            outcome,
            finalized: true,
            head: Some(snapshot.state.head as u64),
            prefix_sha256: Some(snapshot.state.prefix.clone()),
            detail: None,
        }
    }
    fn unsettled(outcome: EventWriteOutcome, error: ShoreError) -> Self {
        Self {
            outcome: Some(outcome),
            finalized: false,
            head: None,
            prefix_sha256: None,
            detail: Some(error.to_string()),
        }
    }
}
pub(in crate::session::append_progress) fn prepare(
    event: ShoreEvent,
) -> Result<PreparedProgressEvent> {
    verify_signature(&event)?;
    EventStore::prepare_owned_progress_event(event)
}
fn verify_signature(event: &ShoreEvent) -> Result<()> {
    // This private entry accepts exact signed events; trust authorization remains
    // a caller/product concern. Strict cryptographic validation precedes intent.
    let tbs = crate::session::event::event_to_be_signed(event)
        .map_err(|_| fail("signed event required"))?;
    let signature = event
        .signature
        .as_ref()
        .ok_or_else(|| fail("signed event required"))?;
    if signature.alg != "ed25519" || signature.sig_version != 1 {
        return Err(fail("signature format"));
    }
    let message = crate::session::event::event_signature_pre_authentication_encoding(&tbs)?;
    if crate::crypto::verify_ed25519_strict(&tbs.signer, &message, signature.sig.as_str())?
        != crate::crypto::EventVerificationStatus::Valid
    {
        return Err(fail("invalid event signature"));
    }
    Ok(())
}
/// Validate enrolled operational state without a history census or settlement.
/// The bounded read accepts nonzero heads and preserves any original obligation
/// until the caller has validated its operation inputs and enters append.
pub(in crate::session::append_progress) fn admit(
    root: &NativeRootHandle,
    guard: &StoreAuthorityLock,
    request: &str,
    actor: &str,
) -> Result<()> {
    Session::new(root, guard, request, actor)?.read(None, None)?;
    Ok(())
}

pub(in crate::session::append_progress) fn reconcile(
    root: &NativeRootHandle,
    guard: &StoreAuthorityLock,
    request: &str,
    actor: &str,
) -> Result<AppendExit> {
    let session = Session::new(root, guard, request, actor)?;
    let snapshot = session.reconcile(session.read(None, None)?, None, None)?;
    Ok(AppendExit::finalized(&snapshot, None))
}
pub(in crate::session::append_progress) fn append(
    root: &NativeRootHandle,
    guard: &StoreAuthorityLock,
    request: &str,
    actor: &str,
    attempt: &str,
    prepared: PreparedProgressEvent,
) -> Result<AppendExit> {
    if !hash64(attempt) {
        return Err(fail("invalid attempt identifier"));
    }
    let session = Session::new(root, guard, request, actor)?;
    let key = prepared.event().idempotency_key.as_str();
    let mut snapshot = session.reconcile(
        session.read(Some(key), Some(attempt))?,
        Some(key),
        Some(attempt),
    )?;
    let hash = sha256_bytes_hex(prepared.bytes());
    let length = i64::try_from(prepared.bytes().len()).map_err(|_| fail("event byte length"))?;
    let duplicate = snapshot.requested.is_some();
    if !duplicate {
        if snapshot.attempt_exists {
            return Err(fail("attempt already used; fresh attempt required"));
        }
        session.check_carrier(key, None, &snapshot.state.native, false)?;
        let o = Obligation {
            version: 1,
            binding: snapshot.state.binding.clone(),
            minimum_reader_profile: "review_change_revision_progress_v1".into(),
            attempt_id: attempt.into(),
            key: key.into(),
            expected_sha256: hash.clone(),
            expected_length: length,
            next_head: snapshot
                .state
                .head
                .checked_add(1)
                .ok_or_else(|| fail("head exhausted"))?,
            previous_head: snapshot.state.head,
            previous_prefix: snapshot.state.prefix.clone(),
            previous_native: snapshot.state.native.clone(),
        };
        let mut next = snapshot.state.clone();
        next.phase = Phase::Intent;
        next.obligation = Some(o.clone());
        next.proof = None;
        snapshot = session.transition(
            Change {
                old: snapshot.state,
                new: next,
                attempt: Some(o),
                receipt: None,
            },
            Some(key),
            Some(attempt),
        )?;
    }
    boundary("before-publication")?;
    let mut evidence = session.capture(EvidenceStage::Append)?;
    let permit = PublicationPermit {
        session: &session,
        snapshot: &snapshot,
        prepared_sha256: hash.clone(),
        prepared_length: prepared.bytes().len(),
    };
    // Consumes the sole publication permit; errors preserve the durable obligation.
    let outcome = EventStore::publish_owned_progress_event(permit, &prepared).map_err(|error| {
        if duplicate {error} else {fail(&format!("publication outcome unknown; original obligation retained for the same logical operation: {error}"))}
    })?;
    // A publisher return is attributable to this intended root only after the
    // retained physical/namespace sandwich and exact carrier check. Detected
    // movement here is an unknown outcome, not metadata-finalization failure.
    boundary("after-publication")?;
    let proof = if duplicate {
        if outcome == EventWriteOutcome::Created {
            return Err(fail("receipted carrier was unexpectedly recreated"));
        }
        evidence.validate_live()?;
        let r = snapshot.requested.as_ref().unwrap();
        session.check_carrier(
            key,
            Some((&r.stored_sha256, r.stored_length)),
            &snapshot.state.native,
            false,
        )?;
        None
    } else {
        // Check held identities before following any post-publication path.
        // Only Events timestamps may advance with the actual publisher result.
        evidence.after_live_publication()?;
        if LocalJournal::new(&session.canonical)
            .read_event_bytes(key)?
            .as_deref()
            != Some(prepared.bytes())
        {
            return Err(fail(
                "publication binding unknown; original obligation retained",
            ));
        }
        native::sync_exact_carrier(root, key, &hash, length as u64)?;
        let check = evidence.journal_check(&snapshot.state.native)?;
        if check.verdict == JournalChangeVerdict::Indeterminate {
            return Err(fail("native publication observation indeterminate"));
        }
        same_native_identity(&snapshot.state.native, &check.after)?;
        Some(Proof::LivePublication {
            native: check.after,
            stored_sha256: hash,
            stored_length: length,
        })
    };
    let finalized = (|| -> Result<Snapshot> {
        let Some(proof) = proof else {
            return Ok(snapshot);
        };
        boundary("before-presence-proof")?;
        let mut next = snapshot.state.clone();
        next.phase = Phase::ProvedPresent;
        next.proof = Some(proof);
        snapshot = session.transition(
            Change {
                old: snapshot.state,
                new: next,
                attempt: None,
                receipt: None,
            },
            Some(key),
            Some(attempt),
        )?;
        boundary("after-presence-owner-finish")?;
        session.reconcile(snapshot, Some(key), Some(attempt))
    })();
    // Retain the original evidence through SQL finalization and native finish.
    // A later detected root/namespace change cannot be exposed as a confirmed
    // intended-store outcome, even when the metadata result is an error.
    evidence.validate_live()?;
    Ok(match finalized {
        Ok(snapshot) => AppendExit::finalized(&snapshot, Some(outcome)),
        Err(error) => AppendExit::unsettled(outcome, error),
    })
}

#[cfg(test)]
mod tests;
