//! Closed full-prefix and native census. Ordinary C4 never enters this module.
use std::collections::{BTreeMap, BTreeSet};

use super::*;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct CensusBinding {
    version: u32,
    audit_request: String,
    obligation_sha256: String,
    previous_head: i64,
    previous_prefix: String,
    pub(super) native: JournalChangeStamp,
    inventory_sha256: String,
    totals: budget::Totals,
}
impl CensusBinding {
    pub(super) fn validate(&self, o: &Obligation) -> Result<()> {
        let maximum = budget::AuditLimits::default();
        if self.version != 1
            || !hash64(&self.audit_request)
            || self.obligation_sha256 != sha256_json_hex(&serde_json::to_value(o)?)?
            || self.previous_head != o.previous_head
            || self.previous_prefix != o.previous_prefix
            || !hash64(&self.inventory_sha256)
            || self.totals.visits == 0
            || self.totals.bytes == 0
            || self.totals.visits > maximum.visits
            || self.totals.bytes > maximum.bytes
        {
            return Err(fail("invalid census proof binding"));
        }
        same_native_identity(&o.previous_native, &self.native)
    }
}
#[derive(Clone, Debug)]
pub(super) struct ValidatedPrefix {
    // Native filenames are digest addresses; values bind exact stored bytes.
    expected: BTreeMap<String, (String, i64)>,
    native: JournalChangeStamp,
}
fn chain(previous: &str, receipt: &Receipt) -> Result<String> {
    sha256_json_hex(&json!({"domain":"pointbreak.append-progress-prefix.v1",
        "previousPrefixSha256":previous,"head":receipt.sequence,"receipt":receipt}))
}
pub(super) fn read_prefix(
    db: &Connection,
    inputs: &InitialInputs,
    snapshot: &Snapshot,
    native: &JournalChangeStamp,
) -> Result<ValidatedPrefix> {
    if snapshot.state.phase.completed() {
        return Err(fail("completed state needs no audit"));
    }
    read_prefix_inner(db, inputs, snapshot, native, false).map(|(prefix, _)| prefix)
}

fn read_prefix_inner(
    db: &Connection,
    inputs: &InitialInputs,
    snapshot: &Snapshot,
    native: &JournalChangeStamp,
    completed_rebuild: bool,
) -> Result<(ValidatedPrefix, Vec<Receipt>)> {
    if snapshot.state.phase.completed() != completed_rebuild {
        return Err(fail("full-prefix operation phase mismatch"));
    }
    // Inspect cardinality without a count query or allocating arbitrary fields.
    for table in ["meta", "current"] {
        let mut query = db
            .prepare(&format!("SELECT singleton FROM {table} LIMIT 2"))
            .map_err(sql)?;
        let mut rows = query.query([]).map_err(sql)?;
        let row = rows
            .next()
            .map_err(sql)?
            .ok_or_else(|| fail("missing singleton"))?;
        budget::row(row).map_err(sql)?;
        if row.get::<_, i64>(0).map_err(sql)? != 1 {
            return Err(fail("invalid singleton"));
        }
        if let Some(extra) = rows.next().map_err(sql)? {
            budget::row(extra).map_err(sql)?;
            return Err(fail("extra singleton"));
        }
    }
    let mut attempts = BTreeMap::new();
    let mut query = db.prepare("SELECT attempt_id,key,expected_sha256,expected_length FROM attempt ORDER BY attempt_id").map_err(sql)?;
    let mut rows = query.query([]).map_err(sql)?;
    while let Some(row) = rows.next().map_err(sql)? {
        budget::row(row).map_err(sql)?;
        let id: String = row.get(0).map_err(sql)?;
        let key: String = row.get(1).map_err(sql)?;
        let hash: String = row.get(2).map_err(sql)?;
        let length: i64 = row.get(3).map_err(sql)?;
        if !hash64(&id)
            || key.is_empty()
            || !hash64(&hash)
            || length < 0
            || attempts.insert(id, (key, hash, length)).is_some()
        {
            return Err(fail("invalid full-prefix attempt"));
        }
    }
    drop(rows);
    drop(query);
    let bindings = inputs.bindings()?;
    let mut prefix = sha256_json_hex(&bindings.prefix)?;
    let mut expected = BTreeMap::new();
    for (key, bytes) in inputs.carriers() {
        expected.insert(
            sha256_bytes_hex(key.as_bytes()),
            (sha256_bytes_hex(bytes), bytes.len() as i64),
        );
    }
    if expected.len() != 2 {
        return Err(fail("invalid baseline cardinality"));
    }
    let mut head = 0i64;
    let mut used_attempts = BTreeSet::new();
    let mut receipts = Vec::new();
    let mut query = db.prepare("SELECT sequence,key,stored_sha256,stored_length,original_attempt FROM receipt ORDER BY sequence").map_err(sql)?;
    let mut rows = query.query([]).map_err(sql)?;
    while let Some(row) = rows.next().map_err(sql)? {
        let receipt = receipt_row(row).map_err(sql)?;
        receipt.validate()?;
        head = head
            .checked_add(1)
            .ok_or_else(|| fail("prefix head overflow"))?;
        if receipt.sequence != head
            || head > snapshot.state.head
            || attempts.get(&receipt.original_attempt)
                != Some(&(
                    receipt.key.clone(),
                    receipt.stored_sha256.clone(),
                    receipt.stored_length,
                ))
            || !used_attempts.insert(receipt.original_attempt.clone())
            || expected
                .insert(
                    sha256_bytes_hex(receipt.key.as_bytes()),
                    (receipt.stored_sha256.clone(), receipt.stored_length),
                )
                .is_some()
        {
            return Err(fail("prefix gap, excess, duplicate or attempt mismatch"));
        }
        prefix = chain(&prefix, &receipt)?;
        if completed_rebuild {
            receipts.push(receipt);
        }
        budget::check()?;
    }
    if head != snapshot.state.head || prefix != snapshot.state.prefix {
        return Err(fail("full prefix does not match metadata"));
    }
    if !completed_rebuild {
        let o = snapshot.state.obligation()?;
        if attempts.get(&o.attempt_id)
            != Some(&(o.key.clone(), o.expected_sha256.clone(), o.expected_length))
            || used_attempts.contains(&o.attempt_id)
            || expected.contains_key(&sha256_bytes_hex(o.key.as_bytes()))
        {
            return Err(fail("current obligation conflicts with complete prefix"));
        }
    }
    Ok((
        ValidatedPrefix {
            expected,
            native: native.clone(),
        },
        receipts,
    ))
}

/// No constructor is exposed outside this closed audit implementation. A value
/// is minted only after selected evidence and the full unresolved prefix pass.
pub(crate) struct JournalAdmission(());
impl JournalAdmission {
    pub(crate) fn checkpoint(&self) -> Result<()> {
        budget::check()
    }
}

#[cfg(target_os = "macos")]
pub(in crate::session) struct RebuildEvent {
    pub(in crate::session) event: ShoreEvent,
    pub(in crate::session) validation_witness: String,
    pub(in crate::session) carrier_bytes: u64,
}

#[cfg(target_os = "macos")]
pub(in crate::session) struct CompletedRebuildInputs {
    pub(in crate::session) fence: crate::session::append_progress::ProgressReadFence,
    pub(in crate::session) events: Vec<RebuildEvent>,
    pub(in crate::session) inspection: crate::session::store::capabilities::JournalInspection,
}

/// The explicit rebuild's O(H) read uses profile7 and never settles an obligation.
/// Native authority is read independently, then matched to the complete prefix.
#[cfg(target_os = "macos")]
pub(in crate::session) fn completed_rebuild_inputs(
    context: &crate::session::append_progress::ReadContext,
    layout: &crate::session::derived_access::generation::GenerationLayout,
    authority: &StoreAuthorityLock,
    rebuild: &crate::session::derived_access::generation::RebuildLease,
) -> Result<CompletedRebuildInputs> {
    if !budget::active() {
        return Err(fail("completed rebuild requires invocation budget"));
    }
    let evidence = RetainedEvidence::capture_rebuild_read(context, layout, authority, rebuild)?;
    let (fence, (prefix, receipts)) = completed_scope(evidence, |db, snapshot, inputs| {
        read_prefix_inner(db, inputs, snapshot, &snapshot.state.native, true)
    })?;
    // All SQL is explicitly finished before opening native event/content N.
    let mut inventory = Inventory::scan_inner(context.store_dir(), true)?;
    if inventory.stored != prefix.expected {
        return Err(fail(
            "completed progress prefix differs from native authority",
        ));
    }
    let admission = JournalAdmission(());
    let inspection = crate::session::store::capabilities::route_audited_progress_entries(
        std::mem::take(&mut inventory.journal),
        &admission,
    )?;
    if !matches!(
        inspection.status,
        crate::session::store::capabilities::StoreCapabilityStatus::Ready { .. }
    ) || inspection.minimum_reader_profile.as_deref()
        != Some("review_change_revision_progress_v1")
        || inspection.cursor.event_count != fence.head()
    {
        return Err(fail("completed rebuild capability/cursor binding"));
    }
    content::validate_events(&inventory, inspection.event_entries.clone())?;
    let mut entries = inspection
        .event_entries
        .iter()
        .map(|entry| (entry.key_digest.clone(), entry))
        .collect::<BTreeMap<_, _>>();
    let mut events = Vec::with_capacity(receipts.len());
    for receipt in receipts {
        let key = sha256_bytes_hex(receipt.key.as_bytes());
        let entry = entries
            .remove(&key)
            .ok_or_else(|| fail("progress receipt has no native event"))?;
        if sha256_bytes_hex(&entry.bytes) != receipt.stored_sha256
            || i64::try_from(entry.bytes.len()).ok() != Some(receipt.stored_length)
        {
            return Err(fail("progress receipt/native byte mismatch"));
        }
        events.push(RebuildEvent {
            event: EventStore::decode_qualification_entry(
                entry.key_digest.clone(),
                entry.bytes.clone(),
            )?,
            validation_witness: receipt.stored_sha256,
            carrier_bytes: receipt.stored_length as u64,
        });
    }
    if !entries.is_empty() {
        return Err(fail("unreceipted native domain event"));
    }
    inventory.validate()?;
    drop(entries);
    drop(inventory);
    authority.validate_physical_root(context.store_dir())?;
    let after = observe_completed(RetainedEvidence::capture_rebuild_read(
        context, layout, authority, rebuild,
    )?)?;
    if after != fence {
        return Err(fail("authority changed during completed rebuild census"));
    }
    Ok(CompletedRebuildInputs {
        fence,
        events,
        inspection,
    })
}

struct Witness {
    held: HeldPath,
    length: u64,
    modified: std::time::SystemTime,
    #[cfg(unix)]
    changed: (i64, i64),
}
impl Witness {
    fn open(path: &Path, directory: bool) -> Result<Self> {
        let held = HeldPath::open(path, directory)?;
        let metadata = held.file.metadata().map_err(io)?;
        #[cfg(unix)]
        use std::os::unix::fs::MetadataExt as _;
        Ok(Self {
            held,
            length: metadata.len(),
            modified: metadata.modified().map_err(io)?,
            #[cfg(unix)]
            changed: (metadata.ctime(), metadata.ctime_nsec()),
        })
    }
    fn validate(&self) -> Result<()> {
        budget::check()?;
        self.held.validate()?;
        let metadata = self.held.file.metadata().map_err(io)?;
        if metadata.len() != self.length || metadata.modified().map_err(io)? != self.modified {
            return Err(fail("census object changed"));
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt as _;
            if (metadata.ctime(), metadata.ctime_nsec()) != self.changed {
                return Err(fail("census object changed"));
            }
        }
        Ok(())
    }
}
struct Inventory {
    witnesses: Vec<Witness>,
    journal: Vec<crate::session::store::backend::JournalEntry>,
    stored: BTreeMap<String, (String, i64)>,
    content: BTreeMap<String, Vec<u8>>,
    identities: BTreeMap<String, (String, u64)>,
}
impl Inventory {
    fn new() -> Self {
        Self {
            witnesses: vec![],
            journal: vec![],
            stored: BTreeMap::new(),
            content: BTreeMap::new(),
            identities: BTreeMap::new(),
        }
    }
    fn hold(&mut self, path: &Path, directory: bool) -> Result<()> {
        self.witnesses.push(Witness::open(path, directory)?);
        Ok(())
    }
    fn read(&mut self, root: &Path, path: &Path) -> Result<Vec<u8>> {
        let witness = Witness::open(path, false)?;
        let bytes = witness.held.read()?;
        witness.validate()?;
        let relative = path
            .strip_prefix(root)
            .map_err(|_| fail("escaped census root"))?
            .to_str()
            .ok_or_else(|| fail("non-UTF8 census path"))?
            .replace('\\', "/");
        self.identities
            .insert(relative, (sha256_bytes_hex(&bytes), bytes.len() as u64));
        self.witnesses.push(witness);
        Ok(bytes)
    }
    fn validate(&self) -> Result<()> {
        for witness in &self.witnesses {
            witness.validate()?;
        }
        Ok(())
    }
    fn scan(root: &Path) -> Result<Self> {
        Self::scan_inner(root, false)
    }
    fn scan_inner(root: &Path, completed_rebuild: bool) -> Result<Self> {
        let mut result = Self::new();
        result.hold(root, true)?;
        for entry in std::fs::read_dir(root).map_err(io)? {
            budget::visit()?;
            let entry = entry.map_err(io)?;
            let path = entry.path();
            let name = entry
                .file_name()
                .into_string()
                .map_err(|_| fail("non-UTF8 root entry"))?;
            match name.as_str() {
                "events" => result.scan_events(root, &path)?,
                "artifacts" => result.scan_content(root, &path)?,
                "authority.writer.lock" | "derived.writer.lock" | "derived.rebuild.lock" => {
                    result.hold(&path, false)?
                }
                "derived" => result.hold(&path, true)?,
                // Real Change commands retain plans/checkpoints outside Journal
                // authority. Admit only the closed existing filenames and keep
                // their physical witnesses; never replay them as events.
                "operations" if completed_rebuild => {
                    result.hold(&path, true)?;
                    for child in std::fs::read_dir(&path).map_err(io)? {
                        budget::visit()?;
                        let child = child.map_err(io)?;
                        let name = child.file_name();
                        let name = name.to_str().ok_or_else(|| fail("operation filename"))?;
                        let stem = name
                            .strip_suffix(".capture.json")
                            .or_else(|| name.strip_suffix(".json"));
                        if !stem.is_some_and(hash64) {
                            return Err(fail("unknown operation metadata"));
                        }
                        result.hold(&child.path(), false)?;
                    }
                }
                name if completed_rebuild
                    && name
                        .strip_prefix("derived.generation-lease-")
                        .and_then(|n| n.strip_suffix(".lock"))
                        .is_some_and(|n| {
                            !n.is_empty()
                                && n.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
                        }) =>
                {
                    result.hold(&path, false)?
                }
                // SQL and operational metadata have their own exact native and
                // schema gates. Their contents never become Journal authority.
                "append-progress" => result.scan_operational(&path, 0)?,
                _ => return Err(fail("unknown store namespace")),
            }
        }
        result.validate()?;
        Ok(result)
    }
    fn scan_operational(&mut self, path: &Path, level: u8) -> Result<()> {
        // Retain operational identity, but not timestamps changed by our own
        // later proof transactions. Do not follow any arbitrary directory.
        let held = HeldPath::open(path, true)?;
        for entry in std::fs::read_dir(path).map_err(io)? {
            budget::visit()?;
            let entry = entry.map_err(io)?;
            let name = entry
                .file_name()
                .into_string()
                .map_err(|_| fail("operational name"))?;
            let p = entry.path();
            match (level, name.as_str()) {
                (0, "active.json") => {
                    HeldPath::open(&p, false)?.validate()?;
                }
                (0, "enrollment") => self.scan_operational(&p, 1)?,
                (0, "incarnations") => self.scan_operational(&p, 2)?,
                (1, name) if json_stem(name).is_some() => {
                    HeldPath::open(&p, false)?.validate()?;
                }
                (2, name) if hash64(name) => self.scan_operational(&p, 3)?,
                (3, "ledger.sqlite" | "ledger.sqlite-wal" | "ledger.sqlite-shm") => {
                    HeldPath::open(&p, false)?.validate()?;
                }
                (_, _) if crate::storage::is_temp_file_path(&p) => {
                    HeldPath::open(&p, false)?.validate()?;
                }
                _ => return Err(fail("unknown progress metadata")),
            }
        }
        held.validate()
    }
    fn scan_events(&mut self, root: &Path, path: &Path) -> Result<()> {
        self.hold(path, true)?;
        for entry in std::fs::read_dir(path).map_err(io)? {
            budget::visit()?;
            let entry = entry.map_err(io)?;
            let p = entry.path();
            if crate::storage::is_temp_file_path(&p) {
                self.hold(&p, false)?;
                continue;
            }
            let name = entry
                .file_name()
                .into_string()
                .map_err(|_| fail("non-UTF8 carrier"))?;
            let digest = json_stem(&name)
                .ok_or_else(|| fail("unknown carrier address"))?
                .to_owned();
            let bytes = self.read(root, &p)?;
            if self
                .stored
                .insert(
                    digest.clone(),
                    (sha256_bytes_hex(&bytes), bytes.len() as i64),
                )
                .is_some()
            {
                return Err(fail("duplicate carrier address"));
            }
            boundary("audit-inventory-entry")?;
            self.journal
                .push(crate::session::store::backend::JournalEntry {
                    key_digest: digest,
                    bytes,
                });
        }
        Ok(())
    }
    fn scan_content(&mut self, root: &Path, path: &Path) -> Result<()> {
        self.hold(path, true)?;
        for entry in std::fs::read_dir(path).map_err(io)? {
            budget::visit()?;
            let entry = entry.map_err(io)?;
            let family = entry
                .file_name()
                .into_string()
                .map_err(|_| fail("content family name"))?;
            if !matches!(family.as_str(), "objects" | "notes" | "proofs") {
                return Err(fail("unknown content family"));
            }
            self.hold(&entry.path(), true)?;
            for resource in std::fs::read_dir(entry.path()).map_err(io)? {
                budget::visit()?;
                let resource = resource.map_err(io)?;
                let p = resource.path();
                if crate::storage::is_temp_file_path(&p) {
                    self.hold(&p, false)?;
                    continue;
                }
                let name = resource
                    .file_name()
                    .into_string()
                    .map_err(|_| fail("content name"))?;
                let stem = json_stem(&name).ok_or_else(|| fail("unknown content address"))?;
                let relative = format!("artifacts/{family}/{name}");
                boundary("audit-before-content-read")?;
                let bytes = self.read(root, &p)?;
                content::validate_resource(&family, stem, &relative, &bytes)?;
                if self.content.insert(relative, bytes).is_some() {
                    return Err(fail("duplicate content address"));
                }
            }
        }
        Ok(())
    }
}
fn json_stem(name: &str) -> Option<&str> {
    name.strip_suffix(".json").filter(|stem| hash64(stem))
}
mod content;

struct CompletedCensus {
    proof: Proof,
    inventory: Inventory,
}
fn census(
    session: &Session<'_>,
    snapshot: &Snapshot,
    request: &str,
    scope: &budget::Scope,
) -> Result<CompletedCensus> {
    let prefix = snapshot
        .prefix_inventory
        .as_ref()
        .ok_or_else(|| fail("missing validated complete prefix"))?;
    let evidence = session.capture(EvidenceStage::Reconcile)?;
    accept_check(&prefix.native, &evidence.journal_check(&prefix.native)?)?;
    boundary("audit-before-inventory")?;
    let mut inventory = Inventory::scan(&session.canonical)?;
    let admission = JournalAdmission(());
    let routed = crate::session::store::capabilities::route_audited_progress_entries(
        std::mem::take(&mut inventory.journal),
        &admission,
    )?;
    if !matches!(
        routed.status,
        crate::session::store::capabilities::StoreCapabilityStatus::Ready { .. }
    ) {
        return Err(fail("audit requires completed capability pair"));
    }
    content::validate_events(&inventory, routed.event_entries)?;
    let o = snapshot.state.obligation()?;
    let present = exact_set(prefix, &snapshot.state, &inventory.stored)?;
    if present {
        native::sync_exact_carrier(
            session.root,
            &o.key,
            &o.expected_sha256,
            o.expected_length as u64,
        )?;
    }
    inventory.validate()?;
    evidence.validate_live()?;
    let check = evidence.journal_check(&prefix.native)?;
    accept_check(&prefix.native, &check)?;
    let binding = CensusBinding {
        version: 1,
        audit_request: request.into(),
        obligation_sha256: sha256_json_hex(&serde_json::to_value(o)?)?,
        previous_head: o.previous_head,
        previous_prefix: o.previous_prefix.clone(),
        native: check.after,
        inventory_sha256: sha256_json_hex(&serde_json::to_value(&inventory.identities)?)?,
        totals: scope.totals(),
    };
    binding.validate(o)?;
    let proof = if present {
        Proof::CensusPresent {
            census: binding,
            stored_sha256: o.expected_sha256.clone(),
            stored_length: o.expected_length,
        }
    } else {
        Proof::CensusAbsent { census: binding }
    };
    budget::check()?;
    Ok(CompletedCensus { proof, inventory })
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AuditExit {
    pub audit_request: String,
    pub finalized: bool,
    pub head: Option<u64>,
    pub prefix_sha256: Option<String>,
    pub visits: u64,
    pub bytes: u64,
    pub detail: Option<String>,
}
pub(in crate::session::append_progress) fn run(
    root: &NativeRootHandle,
    guard: &StoreAuthorityLock,
    enrollment_request: &str,
    actor: &str,
    request: &str,
    scope: &budget::Scope,
) -> Result<AuditExit> {
    if !hash64(request) || request == enrollment_request {
        return Err(fail("distinct audit request required"));
    }
    budget::check()?;
    let session = Session::new(root, guard, enrollment_request, actor)?;
    let snapshot = session.step(
        ScopeProfile::Reconcile,
        Command {
            full_prefix: true,
            key: None,
            attempt: None,
            change: None,
            required_native: None,
        },
    )?;
    // The checked SQL owner has finished before any filesystem enumeration.
    let completed = census(&session, &snapshot, request, scope)?;
    completed.inventory.validate()?;
    budget::check()?;
    boundary("audit-before-proof")?;
    budget::check()?;
    let mut next = snapshot.state.clone();
    next.phase = if completed.proof.present() {
        Phase::ProvedPresent
    } else {
        Phase::ProvedAbsent
    };
    next.proof = Some(completed.proof);
    // Past dispatch, failures cannot claim that durable proof/settlement rolled
    // back. C4 reopens under a fresh checked scope and recognizes only old/new.
    let result = (|| -> Result<Snapshot> {
        let proved = session.transition(
            Change {
                old: snapshot.state,
                new: next,
                attempt: None,
                receipt: None,
            },
            None,
            None,
        )?;
        boundary("audit-durable-proof")?;
        completed.inventory.validate()?;
        let settled = session.reconcile(proved, None, None)?;
        completed.inventory.validate()?;
        budget::check()?;
        boundary("audit-before-result")?;
        Ok(settled)
    })();
    let totals = scope.totals();
    Ok(match result {
        Ok(settled) => AuditExit {
            audit_request: request.into(),
            finalized: true,
            head: Some(settled.state.head as u64),
            prefix_sha256: Some(settled.state.prefix),
            visits: totals.visits,
            bytes: totals.bytes,
            detail: None,
        },
        Err(error) => AuditExit {
            audit_request: request.into(),
            finalized: false,
            head: None,
            prefix_sha256: None,
            visits: totals.visits,
            bytes: totals.bytes,
            detail: Some(format!(
                "proof/settlement may be durable; original obligation retained: {error}"
            )),
        },
    })
}

fn exact_set(
    prefix: &ValidatedPrefix,
    state: &State,
    actual: &BTreeMap<String, (String, i64)>,
) -> Result<bool> {
    let o = state.obligation()?;
    let digest = sha256_bytes_hex(o.key.as_bytes());
    let present = actual.contains_key(&digest);
    let mut expected = prefix.expected.clone();
    if present {
        expected.insert(digest, (o.expected_sha256.clone(), o.expected_length));
    }
    if &expected != actual {
        return Err(fail("census is neither exact absence nor exact presence"));
    }
    if state.phase == Phase::ProvedPresent && !present {
        return Err(fail("prior presence cannot become absence"));
    }
    Ok(present)
}
#[cfg(test)]
mod tests;
