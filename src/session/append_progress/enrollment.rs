//! Explicit private enrollment only. No ordinary reader, writer or constructor
//! calls this module; a retained plan never substitutes for capability authority.
use std::fs;
use std::io::Write as _;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::ledger::{self, HeldPath, InitialInputs};
use super::native::{self, NativeRootHandle};
use crate::canonical_hash::{canonical_json_bytes, sha256_bytes_hex, sha256_json_hex};
use crate::crypto::{EventSigner, SignerId};
use crate::error::{Result, ShoreError};
use crate::session::event::Writer;
use crate::session::store::authority_lock::{PhysicalFileIdentity, StoreAuthorityLock};
use crate::session::store::backend::{Journal, LocalJournal};
use crate::session::store::capabilities::{self, SupportedReaderProfileV1};

fn refuse(detail: impl std::fmt::Display) -> ShoreError {
    ShoreError::Message(format!("progress-initialization-required: {detail}"))
}
fn io(e: std::io::Error) -> ShoreError {
    refuse(e)
}
fn id(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
fn bytes_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
fn decode_hex(value: &str) -> Result<Vec<u8>> {
    if value.is_empty()
        || !value.len().is_multiple_of(2)
        || !value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err(refuse("retained byte encoding"));
    }
    (0..value.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&value[i..i + 2], 16).map_err(refuse))
        .collect()
}
fn random_id() -> Result<String> {
    let mut bytes = [0; 32];
    getrandom::fill(&mut bytes).map_err(refuse)?;
    Ok(bytes_hex(&bytes))
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct EnrollmentPlanV1 {
    schema: String,
    version: u32,
    request_id: String,
    parameter_sha256: String,
    actor_id: String,
    signer_id: SignerId,
    occurred_at: String,
    root_witness: Value,
    incarnation_id: String,
    progress_epoch: String,
    activation_key: String,
    completion_key: String,
    activation_bytes_hex: String,
    completion_bytes_hex: String,
    selection: Value,
}
fn parameters(request: &str, actor: &str, root: &Value) -> Result<String> {
    sha256_json_hex(
        &json!({"domain":"pointbreak.append-progress-enrollment-parameters.v1", "requestId":request,"actorId":actor,"expectEmpty":true,
        "minimumReaderProfile":"review_change_revision_progress_v1","rootWitness":root}),
    )
}
impl EnrollmentPlanV1 {
    fn validate(&self, request: &str, actor: &str, root: &Value) -> Result<InitialInputs> {
        if self.schema != "pointbreak.append-progress-enrollment"
            || self.version != 1
            || self.request_id != request
            || !id(request)
            || !id(&self.incarnation_id)
            || !id(&self.progress_epoch)
            || self.incarnation_id == self.progress_epoch
            || self.actor_id != actor
            || self.root_witness != *root
            || self.parameter_sha256 != parameters(request, actor, root)?
        {
            return Err(refuse("retained plan parameter/root identity mismatch"));
        }
        let a = decode_hex(&self.activation_bytes_hex)?;
        let c = decode_hex(&self.completion_bytes_hex)?;
        let pair = capabilities::validate_progress_pair(&a, &c)?;
        if pair.actor() != actor
            || pair.signer() != &self.signer_id
            || pair.occurred_at() != self.occurred_at
            || pair.activation.logical_key() != self.activation_key
            || pair.completion.logical_key() != self.completion_key
        {
            return Err(refuse("retained plan signed pair binding"));
        }
        let inputs = InitialInputs::from_signed(
            self.incarnation_id.clone(),
            self.progress_epoch.clone(),
            root.clone(),
            a,
            c,
        )?;
        if inputs.bindings()?.selection != self.selection {
            return Err(refuse("retained plan selection binding"));
        }
        Ok(inputs)
    }
}

/// N membership only. These stages do not attest SQL state or admit any I/O.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum EvidenceStage {
    CreateInit,
    ReopenInit,
    Certify,
    ReadyResume,
    Selected,
    BeforeSelector,
    Append,
    Reconcile,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum EvidenceRole {
    Root,
    Authority,
    Progress,
    Enrollment,
    Plan,
    Incarnations,
    Incarnation,
    Events,
    Activation,
    Completion,
    Selector,
    Artifacts,
    Notes,
    Objects,
}
impl EvidenceRole {
    fn directory(self) -> bool {
        matches!(
            self,
            Self::Root
                | Self::Artifacts
                | Self::Notes
                | Self::Objects
                | Self::Progress
                | Self::Enrollment
                | Self::Incarnations
                | Self::Incarnation
                | Self::Events
        )
    }
    #[cfg(target_os = "macos")]
    fn private_mode(self) -> bool {
        matches!(
            self,
            Self::Progress
                | Self::Enrollment
                | Self::Plan
                | Self::Incarnations
                | Self::Incarnation
                | Self::Selector
        )
    }
}

#[derive(Debug, PartialEq, Eq)]
struct EvidenceObservation {
    identity: PhysicalFileIdentity,
    length: u64,
    modified: Option<std::time::SystemTime>,
    #[cfg(unix)]
    changed: (i64, i64),
    #[cfg(target_os = "macos")]
    mode: u32,
    #[cfg(windows)]
    descriptor: native::private_acl::Descriptor,
}
fn observe_evidence(
    file: &fs::File,
    role: EvidenceRole,
    path: &Path,
) -> Result<EvidenceObservation> {
    #[cfg(target_os = "macos")]
    let observed = {
        use std::os::fd::AsFd as _;
        let observed = native::borrowed_metadata(file.as_fd())?;
        if role.private_mode() {
            observed.require_private_mode(role.directory())?
        } else {
            observed
        }
    };
    #[cfg(windows)]
    let (observed, descriptor) = {
        use std::os::windows::io::AsHandle as _;

        use native::private_acl::Kind;
        let kind = if role == EvidenceRole::Root {
            Kind::Root
        } else if role.directory() {
            Kind::Directory
        } else {
            Kind::File
        };
        native::windows_acl::borrowed_private_descriptor(file.as_handle(), path, kind)?
    };
    #[cfg(any(target_os = "macos", windows))]
    {
        let _ = path;
        if observed.directory != role.directory() {
            return Err(refuse("retained evidence object type"));
        }
        let metadata = file.metadata().map_err(io)?;
        #[cfg(unix)]
        use std::os::unix::fs::MetadataExt as _;
        Ok(EvidenceObservation {
            modified: metadata.modified().ok(),
            #[cfg(unix)]
            changed: (metadata.ctime(), metadata.ctime_nsec()),
            identity: observed.identity,
            length: file.metadata().map_err(io)?.len(),
            #[cfg(target_os = "macos")]
            mode: observed.mode,
            #[cfg(windows)]
            descriptor,
        })
    }
    #[cfg(not(any(target_os = "macos", windows)))]
    {
        let _ = (file, role, path);
        Err(refuse("retained evidence platform unavailable"))
    }
}

enum EvidenceHandle<'a> {
    Borrowed(&'a fs::File),
    Owned(fs::File),
}
impl EvidenceHandle<'_> {
    fn file(&self) -> &fs::File {
        match self {
            Self::Borrowed(file) => file,
            Self::Owned(file) => file,
        }
    }
}
enum EvidencePresence<'a> {
    Absent,
    Present {
        handle: EvidenceHandle<'a>,
        observation: EvidenceObservation,
    },
}
pub(super) struct RetainedMember<'a> {
    role: EvidenceRole,
    path: PathBuf,
    presence: EvidencePresence<'a>,
    // Comparison target only; actual reads can never return these bytes instead.
    expected: Option<Vec<u8>>,
}
impl<'a> RetainedMember<'a> {
    fn present(role: EvidenceRole, path: PathBuf, handle: EvidenceHandle<'a>) -> Result<Self> {
        let observation = observe_evidence(handle.file(), role, &path)?;
        Ok(Self {
            role,
            path,
            presence: EvidencePresence::Present {
                handle,
                observation,
            },
            expected: None,
        })
    }
    fn capture(role: EvidenceRole, path: PathBuf, required: Option<bool>) -> Result<Self> {
        // Only pre-SQL capture may inspect/open the fixed canonical namespace.
        // Some(true) requires presence, Some(false) absence, None permits either.
        if !exists(&path)? {
            if required == Some(true) {
                return Err(refuse("missing required retained evidence"));
            }
            return Ok(Self {
                role,
                path,
                presence: EvidencePresence::Absent,
                expected: None,
            });
        }
        if required == Some(false) {
            return Err(refuse("unexpected retained evidence presence"));
        }
        let mut options = fs::OpenOptions::new();
        options.read(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt as _;
            options.custom_flags(
                libc::O_NOFOLLOW
                    | libc::O_NONBLOCK
                    | if role.directory() {
                        libc::O_DIRECTORY
                    } else {
                        0
                    },
            );
        }
        #[cfg(windows)]
        {
            use std::os::windows::fs::OpenOptionsExt as _;
            options
                .write(matches!(
                    role,
                    EvidenceRole::Progress | EvidenceRole::Incarnations | EvidenceRole::Incarnation
                ))
                .custom_flags(0x0020_0000 | if role.directory() { 0x0200_0000 } else { 0 });
        }
        let file = options.open(&path).map_err(io)?;
        Self::present(role, path, EvidenceHandle::Owned(file))
    }
    fn read_actual(&self) -> Result<Vec<u8>> {
        use std::io::{Read as _, Seek as _};
        let EvidencePresence::Present {
            handle,
            observation,
        } = &self.presence
        else {
            return Err(refuse("absent retained evidence has no bytes"));
        };
        if self.role.directory()
            || observe_evidence(handle.file(), self.role, &self.path)? != *observation
        {
            return Err(refuse("retained evidence changed before read"));
        }
        if super::audit::active() {
            let bytes = super::audit::read(handle.file(), observation.length)?;
            if observe_evidence(handle.file(), self.role, &self.path)? != *observation
                || self
                    .expected
                    .as_ref()
                    .is_some_and(|expected| *expected != bytes)
            {
                return Err(refuse("retained evidence changed during audit read"));
            }
            return Ok(bytes);
        }
        let limit = observation
            .length
            .checked_add(1)
            .ok_or_else(|| refuse("evidence length"))?;
        let mut file = handle.file();
        file.rewind().map_err(io)?;
        let mut bytes = Vec::new();
        file.take(limit).read_to_end(&mut bytes).map_err(io)?;
        if bytes.len() as u64 != observation.length
            || observe_evidence(handle.file(), self.role, &self.path)? != *observation
            || self
                .expected
                .as_ref()
                .is_some_and(|expected| *expected != bytes)
        {
            return Err(refuse("retained evidence changed during read"));
        }
        Ok(bytes)
    }
    pub(super) fn role(&self) -> EvidenceRole {
        self.role
    }
    pub(super) fn path(&self) -> &Path {
        &self.path
    }
    /// Capture-time identity; absence is an observation, never a create permit.
    pub(super) fn identity(&self) -> Option<PhysicalFileIdentity> {
        match &self.presence {
            EvidencePresence::Absent => None,
            EvidencePresence::Present { observation, .. } => Some(observation.identity),
        }
    }
    fn file(&self) -> Option<&fs::File> {
        match &self.presence {
            EvidencePresence::Absent => None,
            EvidencePresence::Present { handle, .. } => Some(handle.file()),
        }
    }
    #[cfg(unix)]
    pub(super) fn borrowed_fd(&self) -> Option<std::os::fd::BorrowedFd<'_>> {
        use std::os::fd::AsFd as _;
        self.file().map(|file| file.as_fd())
    }
    #[cfg(windows)]
    pub(super) fn borrowed_handle(&self) -> Option<std::os::windows::io::BorrowedHandle<'_>> {
        use std::os::windows::io::AsHandle as _;
        self.file().map(|file| file.as_handle())
    }
}

/// Pre-SQL retained N input, not a namespace/currentness or Q/N-disjointness proof.
/// The authority borrow also keeps this collection !Send/!Sync. No SQL caller
/// consumes it yet. Capture must precede every overlapping SQL ownership scope.
pub(super) struct RetainedEvidence<'a> {
    _root: &'a NativeRootHandle,
    _authority: &'a StoreAuthorityLock,
    stage: EvidenceStage,
    inputs: InitialInputs,
    members: Vec<RetainedMember<'a>>,
    #[cfg(windows)]
    journal: std::cell::RefCell<crate::session::store::backend::ntfs_journal::RetainedJournal>,
}
impl<'a> RetainedEvidence<'a> {
    pub(super) fn retirement_read_bytes(&self) -> Result<u64> {
        self.members.iter().try_fold(0u64, |total, member| {
            let bytes = member
                .expected
                .as_ref()
                .map_or(0, |bytes| bytes.len() as u64);
            total
                .checked_add(
                    bytes
                        .checked_mul(10)
                        .ok_or_else(|| refuse("retirement reserve overflow"))?,
                )
                .ok_or_else(|| refuse("retirement reserve overflow"))
        })
    }
    pub(super) fn capture_before_sql(
        root: &'a NativeRootHandle,
        authority: &'a StoreAuthorityLock,
        stage: EvidenceStage,
        request: &str,
        actor: &str,
    ) -> Result<Self> {
        super::owner::require_quiescent()?;
        if !id(request) {
            return Err(refuse("retained evidence request grammar"));
        }
        let snapshot = native::root_snapshot(root)?;
        let canonical = &snapshot.canonical;
        authority.validate_physical_root(canonical)?;
        let witness = ledger::root_witness(root)?;
        let mut members = vec![
            RetainedMember::present(
                EvidenceRole::Root,
                canonical.clone(),
                EvidenceHandle::Borrowed(native::retained_root_file(root)?),
            )?,
            RetainedMember::present(
                EvidenceRole::Authority,
                canonical.join("authority.writer.lock"),
                EvidenceHandle::Borrowed(authority.retained_file()),
            )?,
        ];
        let progress = canonical.join("append-progress");
        let enrollment = progress.join("enrollment");
        for (role, path) in [
            (EvidenceRole::Progress, progress.clone()),
            (EvidenceRole::Enrollment, enrollment.clone()),
        ] {
            members.push(RetainedMember::capture(role, path, Some(true))?);
        }
        let mut retained_plan = RetainedMember::capture(
            EvidenceRole::Plan,
            enrollment.join(format!("{request}.json")),
            Some(true),
        )?;
        let bytes = retained_plan.read_actual()?;
        let plan: EnrollmentPlanV1 = serde_json::from_slice(&bytes).map_err(refuse)?;
        if canonical_json_bytes(&serde_json::to_value(&plan)?)? != bytes {
            return Err(refuse("noncanonical retained plan"));
        }
        let inputs = plan.validate(request, actor, &witness)?;
        retained_plan.expected = Some(bytes);
        members.push(retained_plan);
        let incarnations = progress.join("incarnations");
        let incarnation = incarnations.join(&plan.incarnation_id);
        for (role, path) in [
            (EvidenceRole::Incarnations, incarnations),
            (EvidenceRole::Incarnation, incarnation),
        ] {
            let required = if stage == EvidenceStage::CreateInit {
                None
            } else {
                Some(true)
            };
            members.push(RetainedMember::capture(role, path, required)?);
        }
        let events = canonical.join("events");
        members.push(RetainedMember::capture(
            EvidenceRole::Events,
            events.clone(),
            Some(true),
        )?);
        let artifacts = canonical.join("artifacts");
        let present_artifacts = exists(&artifacts)?;
        members.push(RetainedMember::capture(
            EvidenceRole::Artifacts,
            artifacts.clone(),
            None,
        )?);
        if present_artifacts {
            for (role, leaf) in [
                (EvidenceRole::Notes, "notes"),
                (EvidenceRole::Objects, "objects"),
            ] {
                members.push(RetainedMember::capture(role, artifacts.join(leaf), None)?);
            }
        }
        let complete = matches!(
            stage,
            EvidenceStage::Certify
                | EvidenceStage::ReadyResume
                | EvidenceStage::Selected
                | EvidenceStage::BeforeSelector
                | EvidenceStage::Append
                | EvidenceStage::Reconcile
        );
        for ((key, bytes), role) in inputs
            .carriers()
            .into_iter()
            .zip([EvidenceRole::Activation, EvidenceRole::Completion])
        {
            let required = role == EvidenceRole::Activation || complete;
            let path = events.join(format!("{}.json", sha256_bytes_hex(key.as_bytes())));
            let mut member = RetainedMember::capture(role, path, Some(required))?;
            member.expected = Some(bytes.to_vec());
            members.push(member);
        }
        let required = match stage {
            EvidenceStage::CreateInit | EvidenceStage::ReopenInit => Some(false),
            EvidenceStage::Certify | EvidenceStage::ReadyResume => None,
            EvidenceStage::Selected | EvidenceStage::Append | EvidenceStage::Reconcile => {
                Some(true)
            }
            EvidenceStage::BeforeSelector => Some(false),
        };
        let mut selector = RetainedMember::capture(
            EvidenceRole::Selector,
            progress.join("active.json"),
            required,
        )?;
        selector.expected = Some(canonical_json_bytes(&plan.selection)?);
        members.push(selector);
        for member in &members {
            if member.expected.is_some() && member.file().is_some() {
                member.read_actual()?;
            }
        }
        if native::root_snapshot(root)? != snapshot {
            return Err(refuse("retained evidence root changed"));
        }
        authority.validate_physical_root(canonical)?;
        #[cfg(windows)]
        let journal = {
            let events = members
                .iter()
                .find(|m| m.role == EvidenceRole::Events)
                .expect("events retained");
            std::cell::RefCell::new(
                crate::session::store::backend::ntfs_journal::RetainedJournal::capture(
                    events.path(),
                    events.file().expect("events present"),
                )?,
            )
        };
        Ok(Self {
            #[cfg(windows)]
            journal,
            _root: root,
            _authority: authority,
            stage,
            inputs,
            members,
        })
    }
    #[cfg(any(target_os = "macos", windows))]
    pub(super) fn validate_live(&self) -> Result<()> {
        self.validate_members(false)
    }
    #[cfg(any(target_os = "macos", windows))]
    fn validate_members(&self, published: bool) -> Result<()> {
        use super::native::namespace::{Role, observe_private_worker};
        self._root.validate_live()?;
        for member in &self.members {
            if member.role != EvidenceRole::Root {
                let parent = self
                    .members
                    .iter()
                    .find(|value| Some(value.path()) == member.path.parent())
                    .and_then(RetainedMember::file)
                    .ok_or_else(|| refuse("unretained evidence parent"))?;
                let role = match member.role {
                    EvidenceRole::Authority => Role::Authority,
                    EvidenceRole::Progress => Role::Progress,
                    EvidenceRole::Enrollment => Role::Enrollment,
                    EvidenceRole::Plan => Role::Request,
                    EvidenceRole::Incarnations => Role::Incarnations,
                    EvidenceRole::Incarnation => Role::Incarnation,
                    EvidenceRole::Events => Role::Events,
                    EvidenceRole::Artifacts => Role::Artifacts,
                    EvidenceRole::Notes => Role::Notes,
                    EvidenceRole::Objects => Role::Objects,
                    EvidenceRole::Activation | EvidenceRole::Completion => Role::Carrier,
                    EvidenceRole::Selector => Role::Active,
                    EvidenceRole::Root => unreachable!(),
                };
                let leaf = member
                    .path
                    .file_name()
                    .ok_or_else(|| refuse("evidence leaf"))?;
                // SAFETY: retained parents/N and binary-owned permit cover the query.
                let named = unsafe { observe_private_worker(parent, leaf, role) }?;
                if named.map(|value| value.identity) != member.identity() {
                    return Err(refuse("retained evidence namespace changed"));
                }
            }
            if let EvidencePresence::Present {
                handle,
                observation,
            } = &member.presence
            {
                let actual = observe_evidence(handle.file(), member.role, &member.path)?;
                if actual.identity != observation.identity
                    || (!member.role.directory() && actual.length != observation.length)
                {
                    return Err(refuse("retained evidence changed"));
                }
                if matches!(
                    member.role,
                    EvidenceRole::Root
                        | EvidenceRole::Artifacts
                        | EvidenceRole::Notes
                        | EvidenceRole::Objects
                        | EvidenceRole::Events
                ) {
                    if !(published && member.role == EvidenceRole::Events)
                        && actual.modified != observation.modified
                    {
                        return Err(refuse("authority directory changed"));
                    }
                    #[cfg(unix)]
                    if !(published && member.role == EvidenceRole::Events)
                        && actual.changed != observation.changed
                    {
                        return Err(refuse("authority directory changed"));
                    }
                }
                #[cfg(target_os = "macos")]
                if actual.mode != observation.mode {
                    return Err(refuse("retained evidence mode changed"));
                }
                #[cfg(windows)]
                if actual.descriptor != observation.descriptor {
                    return Err(refuse("retained evidence ACL changed"));
                }
                if member.expected.is_some() {
                    member.read_actual()?;
                }
            }
        }
        Ok(())
    }
    #[cfg(not(any(target_os = "macos", windows)))]
    pub(super) fn validate_live(&self) -> Result<()> {
        Err(refuse("native enrollment unavailable on this platform"))
    }
    /// Only the private C4 live publisher may call this, after confirmed SQL
    /// finish. All retained identities and immutable bytes remain unchanged;
    /// the successful event publication alone permits Events timestamps to move.
    #[cfg(any(target_os = "macos", windows))]
    pub(super) fn after_live_publication(&mut self) -> Result<()> {
        super::owner::require_quiescent()?;
        if self.stage != EvidenceStage::Append {
            return Err(refuse("publication requires append evidence"));
        }
        self.validate_members(true)?;
        for member in &mut self.members {
            if member.role == EvidenceRole::Events
                && let EvidencePresence::Present {
                    handle,
                    observation,
                } = &mut member.presence
            {
                *observation = observe_evidence(handle.file(), member.role, &member.path)?;
            }
        }
        self.validate_live()
    }
    #[cfg(not(any(target_os = "macos", windows)))]
    pub(super) fn after_live_publication(&mut self) -> Result<()> {
        Err(refuse("native append unavailable on this platform"))
    }
    pub(super) fn journal_stamp(
        &self,
    ) -> Result<crate::session::store::backend::JournalChangeStamp> {
        self.validate_live()?;
        #[cfg(target_os = "macos")]
        {
            let events = self
                .members
                .iter()
                .find(|m| m.role == EvidenceRole::Events)
                .ok_or_else(|| refuse("events evidence"))?;
            native::retained_journal_stamp(
                events.file().ok_or_else(|| refuse("events absent"))?,
                events.path(),
            )
        }
        #[cfg(windows)]
        {
            Ok(self.journal.borrow().stamp())
        }
        #[cfg(not(any(target_os = "macos", windows)))]
        {
            Err(refuse("unsupported journal observation"))
        }
    }
    pub(super) fn journal_check(
        &self,
        before: &crate::session::store::backend::JournalChangeStamp,
    ) -> Result<crate::session::store::backend::JournalChangeCheck> {
        self.validate_live()?;
        #[cfg(windows)]
        {
            let events = self
                .members
                .iter()
                .find(|m| m.role == EvidenceRole::Events)
                .ok_or_else(|| refuse("events evidence"))?;
            self.journal.borrow_mut().changes_since(
                events.file().ok_or_else(|| refuse("events absent"))?,
                before,
            )
        }
        #[cfg(not(windows))]
        {
            Ok(
                crate::session::store::backend::JournalChangeStamp::compared(
                    before,
                    self.journal_stamp()?,
                ),
            )
        }
    }
    pub(super) fn conflicts_with_journal(&self, id: PhysicalFileIdentity) -> Result<bool> {
        #[cfg(windows)]
        {
            Ok(self.journal.borrow().volume_identity()? == id)
        }
        #[cfg(not(windows))]
        {
            let _ = id;
            Ok(false)
        }
    }
    pub(super) fn root(&self) -> &NativeRootHandle {
        self._root
    }
    pub(super) fn native_parent(&self) -> Result<&fs::File> {
        self.members
            .iter()
            .find(|m| m.role == EvidenceRole::Incarnation)
            .and_then(RetainedMember::file)
            .ok_or_else(|| refuse("missing retained incarnation"))
    }
    pub(super) fn sync_directories(&self) -> Result<()> {
        self.validate_live()?;
        for member in self.members.iter().rev().filter(|m| {
            matches!(
                m.role,
                EvidenceRole::Progress | EvidenceRole::Incarnations | EvidenceRole::Incarnation
            )
        }) {
            if let Some(file) = member.file() {
                file.sync_all().map_err(io)?;
            }
        }
        self.validate_live()
    }
    pub(super) fn members(&self) -> &[RetainedMember<'a>] {
        &self.members
    }
    pub(super) fn inputs(&self) -> &InitialInputs {
        &self.inputs
    }
    pub(super) fn stage(&self) -> EvidenceStage {
        self.stage
    }
    /// Reads the actual retained object, never a remembered carrier. This does
    /// not validate its current pathname and must not be used as a live SQL gate.
    pub(super) fn read_before_sql(&self, role: EvidenceRole) -> Result<Vec<u8>> {
        let member = self
            .members
            .iter()
            .find(|member| member.role == role)
            .ok_or_else(|| refuse("unknown retained evidence role"))?;
        if member.expected.is_none() {
            return Err(refuse("retained role has no canonical byte contract"));
        }
        member.read_actual()
    }
    /// After the existing creation phase, retain only the missing fixed dirs.
    /// Creates nothing, replaces no existing owner and confers no SQL admission.
    pub(super) fn retain_created_directories_before_sql(&mut self) -> Result<()> {
        if self.stage != EvidenceStage::CreateInit {
            return Err(refuse("directory retention outside CreateInit"));
        }
        for member in &mut self.members {
            if matches!(
                member.role,
                EvidenceRole::Incarnations | EvidenceRole::Incarnation
            ) && member.file().is_none()
            {
                *member = RetainedMember::capture(member.role, member.path.clone(), Some(true))?;
            }
        }
        Ok(())
    }
}

/// Enumerate actual names/bytes, including foreign files normally hidden by a
/// logical Journal listing. Empty standard content directories alone are allowed.
pub(super) fn census(root: &Path, expected: &[(&str, &[u8])]) -> Result<()> {
    let names: std::collections::BTreeMap<_, _> = expected
        .iter()
        .map(|(key, bytes)| (format!("{}.json", sha256_bytes_hex(key.as_bytes())), *bytes))
        .collect();
    let mut found = std::collections::BTreeSet::new();
    for entry in fs::read_dir(root).map_err(io)? {
        let entry = entry.map_err(io)?;
        let path = entry.path();
        let name = entry.file_name();
        match name.to_str() {
            Some("authority.writer.lock") => {
                HeldPath::open(&path, false)?.validate()?;
            }
            Some("append-progress") => {
                HeldPath::open(&path, true)?.validate()?;
            }
            Some("events") => {
                let directory = HeldPath::open(&path, true)?;
                for event in fs::read_dir(&path).map_err(io)? {
                    let event = event.map_err(io)?;
                    let name = event
                        .file_name()
                        .into_string()
                        .map_err(|_| refuse("non-UTF8 authoritative carrier"))?;
                    let wanted = names
                        .get(&name)
                        .ok_or_else(|| refuse("foreign authoritative carrier"))?;
                    if HeldPath::open(&event.path(), false)?.read()?.as_slice() != *wanted {
                        return Err(refuse("authority census byte mismatch"));
                    }
                    found.insert(name);
                }
                directory.validate()?;
            }
            Some("artifacts") => {
                let directory = HeldPath::open(&path, true)?;
                for child in fs::read_dir(&path).map_err(io)? {
                    let child = child.map_err(io)?;
                    if !matches!(child.file_name().to_str(), Some("notes" | "objects")) {
                        return Err(refuse("unknown content namespace"));
                    }
                    let held = HeldPath::open(&child.path(), true)?;
                    if fs::read_dir(child.path()).map_err(io)?.next().is_some() {
                        return Err(refuse("nonempty authoritative content"));
                    }
                    held.validate()?;
                }
                directory.validate()?;
            }
            _ => return Err(refuse("legacy or unknown store authority")),
        }
    }
    if found.len() != names.len() {
        return Err(refuse("incomplete authority census"));
    }
    Ok(())
}

fn paths(root: &Path, request: &str, incarnation: Option<&str>) -> Vec<(PathBuf, bool)> {
    let base = root.join("append-progress");
    let mut result = vec![
        (base.clone(), true),
        (base.join("enrollment"), true),
        (
            base.join("enrollment").join(format!("{request}.json")),
            false,
        ),
        (base.join("incarnations"), true),
        (base.join("active.json"), false),
    ];
    if let Some(incarnation) = incarnation {
        let dir = base.join("incarnations").join(incarnation);
        result.extend([
            (dir.clone(), true),
            (dir.join("ledger.sqlite"), false),
            (dir.join("ledger.sqlite-wal"), false),
            (dir.join("ledger.sqlite-shm"), false),
        ]);
    }
    result
}
fn layout(root: &Path, expected: &[(PathBuf, bool)]) -> Result<()> {
    for (path, directory) in expected.iter().filter(|(_, dir)| *dir) {
        if !exists(path)? {
            continue;
        }
        let held = HeldPath::open(path, *directory)?;
        for entry in fs::read_dir(path).map_err(io)? {
            let entry = entry.map_err(io)?;
            if !expected.iter().any(|(allowed, _)| *allowed == entry.path()) {
                return Err(refuse("foreign progress path"));
            }
        }
        held.validate()?;
    }
    let _ = root;
    Ok(())
}
fn exists(path: &Path) -> Result<bool> {
    match fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(io(e)),
    }
}
fn create_dir(
    root: &NativeRootHandle,
    guard: &StoreAuthorityLock,
    path: &Path,
    objects: &[(PathBuf, bool)],
) -> Result<()> {
    ledger::access(root, guard, objects)?.finish()?;
    if !exists(path)? {
        let mut builder = fs::DirBuilder::new();
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt as _;
            builder.mode(0o700);
        }
        builder.create(path).map_err(io)?;
    }
    ledger::access(root, guard, objects)?.finish()
}
fn create_file(
    root: &NativeRootHandle,
    guard: &StoreAuthorityLock,
    path: &Path,
    bytes: &[u8],
    objects: &[(PathBuf, bool)],
) -> Result<()> {
    ledger::access(root, guard, objects)?.finish()?;
    if exists(path)? {
        if HeldPath::open(path, false)?.read()? != bytes {
            return Err(refuse("partial or different retained file; preserved"));
        }
    } else {
        let mut options = fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt as _;
            options.mode(0o600);
        }
        let mut file = options.open(path).map_err(io)?;
        let retained = HeldPath::open(path, false)?;
        retained.same(&file)?;
        // E6 in particular inspects the real EMPTY created selector before S.
        let check = ledger::access(root, guard, objects)?;
        #[cfg(target_os = "macos")]
        super::consumer::validate_publication(&native::root_snapshot(root)?.canonical)?;
        file.write_all(bytes).map_err(io)?;
        file.sync_all().map_err(io)?;
        retained.same(&file)?;
        check.finish()?;
    }
    let check = ledger::access(root, guard, objects)?;
    native::sync_carrier(root, path, bytes)?;
    check.finish()
}
fn publish(
    root: &NativeRootHandle,
    guard: &StoreAuthorityLock,
    canonical: &Path,
    key: &str,
    bytes: &[u8],
    objects: &[(PathBuf, bool)],
) -> Result<()> {
    let check = ledger::access(root, guard, objects)?;
    #[cfg(target_os = "macos")]
    super::consumer::validate_publication(canonical)?;
    let journal = LocalJournal::new(canonical);
    journal.create_record_once(key, bytes)?;
    if journal.read_event_bytes(key)?.as_deref() != Some(bytes) {
        return Err(refuse("different capability publication"));
    }
    native::sync_exact_carrier(root, key, &sha256_bytes_hex(bytes), bytes.len() as u64)?;
    check.finish()
}

#[allow(
    clippy::too_many_arguments,
    reason = "keep the established enrollment request, writer, signer and selection inputs explicit"
)]
pub(super) fn enroll(
    root: &NativeRootHandle,
    guard: &StoreAuthorityLock,
    request: &str,
    expect_empty: bool,
    acknowledgment: &str,
    signer: &(impl EventSigner + ?Sized),
    writer: Writer,
    occurred_at: &str,
) -> Result<()> {
    // This private entry already receives a retained root/guard; reject invalid
    // request parameters before any additional path access or mutation.
    validate_request(request, expect_empty, acknowledgment)?;
    let canonical = native::root_snapshot(root)?.canonical;
    guard.validate_physical_root(&canonical)?;
    let witness = ledger::root_witness(root)?;
    let preliminary = paths(&canonical, request, None);
    ledger::access(root, guard, &preliminary)?.finish()?;
    let plan_path = canonical
        .join("append-progress/enrollment")
        .join(format!("{request}.json"));
    let plan: EnrollmentPlanV1 = if exists(&plan_path)? {
        let raw = HeldPath::open(&plan_path, false)?.read()?;
        let plan: EnrollmentPlanV1 = serde_json::from_slice(&raw).map_err(refuse)?;
        if canonical_json_bytes(&serde_json::to_value(&plan)?)? != raw {
            return Err(refuse("noncanonical retained plan"));
        }
        plan
    } else {
        if exists(&canonical.join("append-progress"))? {
            return Err(refuse("missing original plan; no regeneration"));
        }
        census(&canonical, &[])?;
        layout(&canonical, &preliminary)?;
        if exists(&canonical.join("append-progress/active.json"))? {
            return Err(refuse("selector without original plan"));
        }
        let incarnation_id = random_id()?;
        let progress_epoch = random_id()?;
        let (a, c) = capabilities::build_progress_pair(
            signer,
            writer.clone(),
            occurred_at.to_owned(),
            random_id()?,
        )?;
        let pair = capabilities::validate_progress_pair(&a, &c)?;
        let inputs = InitialInputs::from_signed(
            incarnation_id.clone(),
            progress_epoch.clone(),
            witness.clone(),
            a.clone(),
            c.clone(),
        )?;
        EnrollmentPlanV1 {
            schema: "pointbreak.append-progress-enrollment".to_owned(),
            version: 1,
            request_id: request.to_owned(),
            parameter_sha256: parameters(request, writer.actor_id.as_str(), &witness)?,
            actor_id: writer.actor_id.as_str().to_owned(),
            signer_id: pair.signer().clone(),
            occurred_at: occurred_at.to_owned(),
            root_witness: witness.clone(),
            incarnation_id,
            progress_epoch,
            activation_key: pair.activation.logical_key(),
            completion_key: pair.completion.logical_key(),
            activation_bytes_hex: bytes_hex(&a),
            completion_bytes_hex: bytes_hex(&c),
            selection: inputs.bindings()?.selection,
        }
    };
    let inputs = plan.validate(request, writer.actor_id.as_str(), &witness)?;
    let objects = paths(&canonical, request, Some(&plan.incarnation_id));
    layout(&canonical, &objects)?;
    let base = canonical.join("append-progress");
    create_dir(root, guard, &base, &objects)?;
    create_dir(root, guard, &base.join("enrollment"), &objects)?;
    create_file(
        root,
        guard,
        &plan_path,
        &canonical_json_bytes(&serde_json::to_value(&plan)?)?,
        &objects,
    )?;
    let retained_plan = HeldPath::open(&plan_path, false)?;
    let plan_bytes = canonical_json_bytes(&serde_json::to_value(&plan)?)?;
    boundary("E1")?;
    let journal = LocalJournal::new(&canonical);
    let carriers = inputs.carriers();
    let has_a = journal.record_exists(carriers[0].0)?;
    let has_c = journal.record_exists(carriers[1].0)?;
    let expected = if has_c {
        &carriers[..]
    } else if has_a {
        &carriers[..1]
    } else {
        &carriers[..0]
    };
    let gate = ledger::access(root, guard, &objects)?;
    census(&canonical, expected)?;
    if retained_plan.read()? != plan_bytes {
        return Err(refuse("retained plan changed"));
    }
    gate.finish()?;
    if has_c && !has_a {
        return Err(refuse("completion without activation"));
    }
    publish(
        root,
        guard,
        &canonical,
        carriers[0].0,
        carriers[0].1,
        &objects,
    )?;
    boundary("E2")?;
    let database = inputs.database(&canonical)?;
    if !has_c {
        if exists(&base.join("active.json"))? {
            return Err(refuse("selector before completion"));
        }
        let stage = if exists(&database)? {
            EvidenceStage::ReopenInit
        } else {
            EvidenceStage::CreateInit
        };
        ledger::run_owned(root, guard, request, writer.actor_id.as_str(), stage)?;
    } else if !exists(&database)? {
        return Err(refuse("completion without E3"));
    }
    if retained_plan.read()? != plan_bytes {
        return Err(refuse("retained plan changed"));
    }
    boundary("E3")?;
    publish(
        root,
        guard,
        &canonical,
        carriers[1].0,
        carriers[1].1,
        &objects,
    )?;
    boundary("E4")?;
    ledger::run_owned(
        root,
        guard,
        request,
        writer.actor_id.as_str(),
        EvidenceStage::Certify,
    )?;
    if retained_plan.read()? != plan_bytes {
        return Err(refuse("retained plan changed"));
    }
    boundary("E5")?;
    if exists(&base.join("active.json"))? {
        return selected(root, guard, request, writer.actor_id.as_str());
    }
    ledger::run_owned(
        root,
        guard,
        request,
        writer.actor_id.as_str(),
        EvidenceStage::BeforeSelector,
    )?;
    create_file(
        root,
        guard,
        &base.join("active.json"),
        &canonical_json_bytes(&plan.selection)?,
        &objects,
    )?;
    boundary("E6")?;
    selected(root, guard, request, writer.actor_id.as_str())
}

pub(super) fn validate_request(
    request: &str,
    expect_empty: bool,
    acknowledgment: &str,
) -> Result<()> {
    if !id(request)
        || !expect_empty
        || acknowledgment
            != SupportedReaderProfileV1::ChangeRevisionProgress.minimum_reader_profile()
    {
        return Err(refuse(
            "invalid request or missing empty/new-profile acknowledgment",
        ));
    }
    Ok(())
}

pub(super) fn selected(
    root: &NativeRootHandle,
    guard: &StoreAuthorityLock,
    request: &str,
    actor: &str,
) -> Result<()> {
    ledger::run_owned(root, guard, request, actor, EvidenceStage::Selected)
}

pub(super) fn boundary(stage: &str) -> Result<()> {
    #[cfg(test)]
    {
        if std::env::var("POINTBREAK_ENROLLMENT_CUT").as_deref() == Ok(stage) {
            assert_eq!(
                std::env::var("POINTBREAK_ENROLLMENT_CHILD").as_deref(),
                Ok("cut")
            );
            std::process::exit(79);
        }
        BOUNDARY.with(|slot| match slot.borrow_mut().as_mut() {
            Some(hook) => hook(stage),
            None => Ok(()),
        })?;
    }
    let _ = stage;
    Ok(())
}
#[cfg(test)]
type BoundaryHook = Box<dyn FnMut(&str) -> Result<()>>;
#[cfg(test)]
thread_local! { static BOUNDARY: std::cell::RefCell<Option<BoundaryHook>> = const { std::cell::RefCell::new(None) }; }
#[cfg(test)]
pub(super) fn with_boundary<T>(
    hook: impl FnMut(&str) -> Result<()> + 'static,
    run: impl FnOnce() -> T,
) -> T {
    struct Reset;
    impl Drop for Reset {
        fn drop(&mut self) {
            BOUNDARY.with(|slot| *slot.borrow_mut() = None);
        }
    }
    BOUNDARY.with(|slot| {
        assert!(slot.borrow().is_none());
        *slot.borrow_mut() = Some(Box::new(hook));
    });
    let _reset = Reset;
    run()
}
