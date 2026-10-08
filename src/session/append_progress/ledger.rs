//! Private owned enrollment and append ledger; public routing remains disabled.

pub(super) mod c4;
#[cfg(target_os = "macos")]
mod projection;
use std::cell::{Cell, RefCell};
use std::ffi::{CString, c_void};
use std::marker::PhantomData;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Path, PathBuf};
use std::rc::Rc;

use libsqlite3_sys as ffi;
#[cfg(target_os = "macos")]
pub(in crate::session) use projection::{with_published_projection, with_staging_projection};
#[cfg(test)]
use rusqlite::OpenFlags;
use rusqlite::{Connection, params};
use serde_json::{Value, json};

use super::enrollment::{EvidenceRole, EvidenceStage, RetainedEvidence};
use super::native::{self, NativeRootHandle};
use crate::canonical_hash::{canonical_json_bytes, sha256_bytes_hex, sha256_json_hex};
use crate::error::{Result, ShoreError};
use crate::session::store::authority_lock::StoreAuthorityLock;
use crate::session::store::backend::{
    Journal, JournalChangeCheck, JournalChangeStamp, JournalChangeVerdict, LocalJournal,
};

fn refuse(detail: &str) -> ShoreError {
    ShoreError::Message(format!("initial progress ledger refused: {detail}"))
}
fn sql(error: rusqlite::Error) -> ShoreError {
    refuse(&error.to_string())
}
fn io(error: std::io::Error) -> ShoreError {
    refuse(&error.to_string())
}
fn text(value: &Value) -> Result<String> {
    String::from_utf8(canonical_json_bytes(value)?).map_err(|_| refuse("invalid UTF-8"))
}
fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|v| format!("{v:02x}")).collect()
}
fn hash64(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

// Constructed only within the binary-owned operation, after all N acquisition.
struct LiveSqlPermit(super::owner::SqlPermit);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ScopeProfile {
    Selected,
    OrdinaryRead,
    CreateInit,
    ReopenInit,
    Certify,
    ReadyResume,
    Append,
    Reconcile,
}
impl ScopeProfile {
    fn native(self) -> u32 {
        match self {
            Self::Selected => ffi::SQLITE_PB_PROFILE_SELECTED_H0,
            Self::OrdinaryRead => ffi::SQLITE_PB_PROFILE_ORDINARY_READ_SNAPSHOT,
            Self::CreateInit => ffi::SQLITE_PB_PROFILE_CREATE_INIT,
            Self::ReopenInit => ffi::SQLITE_PB_PROFILE_REOPEN_INIT,
            Self::Certify => ffi::SQLITE_PB_PROFILE_CERTIFY,
            Self::ReadyResume => ffi::SQLITE_PB_PROFILE_READY_RESUME,
            Self::Append => ffi::SQLITE_PB_PROFILE_APPEND,
            Self::Reconcile => ffi::SQLITE_PB_PROFILE_RECONCILE,
        }
    }
    fn readonly(self) -> bool {
        matches!(self, Self::Selected | Self::OrdinaryRead)
    }
    fn capabilities(self) -> u64 {
        ffi::SQLITE_PB_CAP_NATIVE_OWNERSHIP
            | ffi::SQLITE_PB_CAP_EARLY_CONTEXT
            | ffi::SQLITE_PB_CAP_EXPLICIT_FINISH
            | ffi::SQLITE_PB_CAP_NO_RETRY_BINDING
            | if self.readonly() {
                ffi::SQLITE_PB_CAP_READONLY_SHM | ffi::SQLITE_PB_CAP_NO_PERSISTENT_MUTATION
            } else {
                ffi::SQLITE_PB_CAP_WRITER_BARRIER
            }
            | if matches!(self, Self::Append | Self::Reconcile) {
                ffi::SQLITE_PB_CAP_APPEND_PROTOCOL
            } else {
                0
            }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum OwnerPhase {
    Prepared,
    Opening,
    Open,
    Finishing,
    Finished,
}
#[derive(Debug)]
enum OwnerFailure {
    MissingLiveGate,
    Abi,
    State,
    Callback,
    Reentry,
    Panic,
    Native(i32),
    Operation(ShoreError),
}
impl OwnerFailure {
    fn code(&self) -> i32 {
        match self {
            Self::Native(code) if *code > 0 => *code,
            _ => ffi::SQLITE_MISUSE,
        }
    }
    fn into_error(self) -> ShoreError {
        match self {
            Self::Operation(error) => error,
            other => refuse(&format!("SQL owner: {other:?}")),
        }
    }
}
struct OwnerContext<'a> {
    evidence: RetainedEvidence<'a>,
    profile: ScopeProfile,
    phase: Cell<OwnerPhase>,
    in_callback: Cell<bool>,
    first_error: RefCell<Option<OwnerFailure>>,
    anchor: Option<JournalChangeStamp>,
    c4_command: Option<c4::Command>,
    live: super::live::Gate,
    _thread: PhantomData<Rc<()>>,
}
impl OwnerContext<'_> {
    fn latch(&self, failure: OwnerFailure) -> i32 {
        let Ok(mut first) = self.first_error.try_borrow_mut() else {
            private_worker_terminal();
        };
        first.get_or_insert(failure).code()
    }
    fn take_error(&self) -> Option<ShoreError> {
        self.first_error
            .borrow_mut()
            .take()
            .map(OwnerFailure::into_error)
    }
    fn reported_error(&self) -> ShoreError {
        let first = self.first_error.borrow();
        match first.as_ref() {
            Some(OwnerFailure::Operation(error)) => ShoreError::Message(error.to_string()),
            Some(other) => refuse(&format!("SQL owner: {other:?}")),
            None => refuse("SQL owner has no recorded failure"),
        }
    }
    fn callback_result(&self, run: impl FnOnce() -> std::result::Result<(), OwnerFailure>) -> i32 {
        if self.in_callback.replace(true) {
            return self.latch(OwnerFailure::Reentry);
        }
        let result = catch_unwind(AssertUnwindSafe(run));
        self.in_callback.set(false);
        match result {
            Ok(Err(error)) => self.latch(error),
            Ok(Ok(())) => ffi::SQLITE_OK,
            Err(payload) => {
                // A user panic payload may itself panic on Drop; never unwind C.
                std::mem::forget(payload);
                self.latch(OwnerFailure::Panic)
            }
        }
    }
    fn native_record(
        &self,
        record: &ffi::sqlite3_pb_native_v1,
    ) -> std::result::Result<(), OwnerFailure> {
        let kind = if cfg!(windows) {
            ffi::SQLITE_PB_NATIVE_WIN_HANDLE
        } else {
            ffi::SQLITE_PB_NATIVE_FD
        };
        if record.struct_size as usize != std::mem::size_of_val(record)
            || !(ffi::SQLITE_PB_ROLE_MAIN..=ffi::SQLITE_PB_ROLE_MAIN_JOURNAL).contains(&record.role)
            || record.kind != kind
            || !(ffi::SQLITE_PB_PHASE_ADMIT..=ffi::SQLITE_PB_PHASE_RETIRE).contains(&record.phase)
            || record.generation == 0
            || !matches!(
                self.phase.get(),
                OwnerPhase::Opening | OwnerPhase::Open | OwnerPhase::Finishing
            )
        {
            return Err(OwnerFailure::Callback);
        }
        if super::owner::require_owned().is_err() {
            return Err(OwnerFailure::MissingLiveGate);
        }
        let _retirement =
            (record.phase == ffi::SQLITE_PB_PHASE_RETIRE).then(super::audit::retirement);
        let result = self
            .live
            .observe(&self.evidence, record)
            .map_err(OwnerFailure::Operation);
        #[cfg(test)]
        let result = result.and_then(|()| {
            if record.role == ffi::SQLITE_PB_ROLE_MAIN
                && record.phase == ffi::SQLITE_PB_PHASE_RETIRE
            {
                hook("native-retire-main").map_err(OwnerFailure::Operation)?;
            }
            Ok(())
        });
        if result.is_err() && record.phase == ffi::SQLITE_PB_PHASE_RETIRE {
            private_worker_terminal();
        }
        result
    }
}

fn private_worker_terminal() -> ! {
    super::owner::terminal()
}
unsafe extern "C" fn owner_native(
    context: *mut c_void,
    record: *const ffi::sqlite3_pb_native_v1,
) -> i32 {
    if context.is_null() {
        return ffi::SQLITE_MISUSE;
    }
    // SAFETY: options point at the stable Box through confirmed native finish;
    // C calls synchronously on this thread, with a live borrowed record.
    let context = unsafe { &*context.cast::<OwnerContext<'_>>() };
    // SAFETY: the pinned ABI owns a properly aligned record for this call.
    let record = unsafe { record.as_ref() };
    let rc =
        context.callback_result(|| context.native_record(record.ok_or(OwnerFailure::Callback)?));
    if rc != ffi::SQLITE_OK && record.is_some_and(|r| r.phase == ffi::SQLITE_PB_PHASE_RETIRE) {
        // A panic or malformed retirement must not fall through to native close.
        private_worker_terminal();
    }
    rc
}
unsafe extern "C" fn owner_terminal(_context: *mut c_void, _code: i32) {
    // Do not allocate, inspect uncertain pointers, unwind, or release N first.
    private_worker_terminal();
}

fn compatible_owner_abi(abi: &ffi::sqlite3_pb_abi_v1, profile: ScopeProfile) -> bool {
    abi.struct_size as usize == std::mem::size_of_val(abi)
        && abi.abi_version == ffi::SQLITE_PB_ABI_VERSION
        && abi.options_size as usize == std::mem::size_of::<ffi::sqlite3_pb_options_v1>()
        && abi.native_size as usize == std::mem::size_of::<ffi::sqlite3_pb_native_v1>()
        && abi.finish_size as usize == std::mem::size_of::<ffi::sqlite3_pb_finish_v1>()
        && abi.pointer_bits == usize::BITS
        && abi.capabilities & profile.capabilities() == profile.capabilities()
}
fn empty_finish_record() -> ffi::sqlite3_pb_finish_v1 {
    ffi::sqlite3_pb_finish_v1 {
        struct_size: std::mem::size_of::<ffi::sqlite3_pb_finish_v1>() as u32,
        abi_version: ffi::SQLITE_PB_ABI_VERSION,
        detached_roles: 0,
        shared_roles: 0,
        closed_roles: 0,
        first_error: 0,
        generation: 0,
    }
}
fn detached_finish(record: &ffi::sqlite3_pb_finish_v1, rc: i32) -> bool {
    record.struct_size as usize == std::mem::size_of_val(record)
        && record.abi_version == ffi::SQLITE_PB_ABI_VERSION
        && record.detached_roles & !ffi::SQLITE_PB_ROLE_MASK_ALL == 0
        && (record.shared_roles | record.closed_roles) == record.detached_roles
        && (record.shared_roles & record.closed_roles) == 0
        && (record.detached_roles == 0 || record.generation > 0)
        && record.first_error >= 0
        && rc == record.first_error
}

struct NativeSqlOwner<'a> {
    connection: *mut ffi::sqlite3,
    disposition: Option<ffi::sqlite3_pb_finish_v1>,
    // Taken only after confirmed finish, into the closed holder below.
    context: Option<Box<OwnerContext<'a>>>,
}
impl<'a> NativeSqlOwner<'a> {
    fn prepare(evidence: RetainedEvidence<'a>, profile: ScopeProfile) -> Self {
        let wrong_target_profile = evidence
            .required_profile()
            .is_some_and(|required| required != profile.native());
        Self {
            connection: std::ptr::null_mut(),
            disposition: None,
            context: Some(Box::new(OwnerContext {
                evidence,
                profile,
                phase: Cell::new(OwnerPhase::Prepared),
                in_callback: Cell::new(false),
                first_error: RefCell::new(wrong_target_profile.then_some(OwnerFailure::State)),
                anchor: None,
                c4_command: None,
                live: super::live::Gate::new(),
                _thread: PhantomData,
            })),
        }
    }
    fn context(&self) -> &OwnerContext<'a> {
        self.context
            .as_deref()
            .unwrap_or_else(|| private_worker_terminal())
    }
    fn options(&self) -> ffi::sqlite3_pb_options_v1 {
        ffi::sqlite3_pb_options_v1 {
            struct_size: std::mem::size_of::<ffi::sqlite3_pb_options_v1>() as u32,
            abi_version: ffi::SQLITE_PB_ABI_VERSION,
            profile: self.context().profile.native(),
            flags: 0,
            context: (self.context() as *const OwnerContext<'a>)
                .cast_mut()
                .cast(),
            xNative: Some(owner_native),
            xTerminal: Some(owner_terminal),
            reserved: [0; 4],
        }
    }
    fn path(&self) -> Result<CString> {
        let evidence = &self.context().evidence;
        let path = evidence.database()?;
        #[cfg(unix)]
        let bytes = {
            use std::os::unix::ffi::OsStrExt as _;
            path.as_os_str().as_bytes()
        };
        #[cfg(not(unix))]
        let bytes = path
            .to_str()
            .ok_or_else(|| refuse("SQL owner path encoding"))?
            .as_bytes();
        CString::new(bytes).map_err(|_| refuse("SQL owner path contains NUL"))
    }
    fn open_native(&mut self, _gate: &LiveSqlPermit) -> Result<()> {
        if self.context().phase.get() != OwnerPhase::Prepared || !self.connection.is_null() {
            self.context().latch(OwnerFailure::State);
        }
        // SAFETY: the pinned ABI returns an immutable static descriptor.
        let abi = unsafe { ffi::sqlite3_pb_reader_abi_v1().as_ref() };
        if !abi.is_some_and(|abi| compatible_owner_abi(abi, self.context().profile)) {
            self.context().latch(OwnerFailure::Abi);
        }
        if self.context().first_error.borrow().is_some() {
            self.finish_native();
            return Err(self.context().reported_error());
        }
        let path = match self.path() {
            Ok(path) => path,
            Err(error) => {
                self.context().latch(OwnerFailure::Operation(error));
                self.finish_native();
                return Err(self.context().reported_error());
            }
        };
        let options = self.options();
        self.context().phase.set(OwnerPhase::Opening);
        // SAFETY: stable context and N predate open. The owner slot receives
        // every nonnull result directly, including failed/sick connections.
        let rc = unsafe {
            if self.context().profile.readonly() {
                ffi::sqlite3_pb_reader_open_v1(path.as_ptr(), &options, &mut self.connection)
            } else {
                ffi::sqlite3_pb_writer_open_v1(path.as_ptr(), &options, &mut self.connection)
            }
        };
        if rc != ffi::SQLITE_OK {
            self.context().latch(OwnerFailure::Native(rc));
        } else if self.connection.is_null() {
            self.context().latch(OwnerFailure::State);
        }
        if self.context().first_error.borrow().is_some() {
            self.finish_native();
            return Err(self.context().reported_error());
        }
        self.context().phase.set(OwnerPhase::Open);
        Ok(())
    }
    // Internal noncapturing operation; only detached outcome data can escape.
    fn with_connection<T>(
        &mut self,
        _gate: &LiveSqlPermit,
        run: impl for<'db> FnOnce(&'db Connection, &'db OwnerContext<'a>) -> Result<T>,
    ) -> Result<T> {
        if self.context().phase.get() != OwnerPhase::Open
            || self.connection.is_null()
            || self.context().in_callback.get()
        {
            self.context().latch(OwnerFailure::State);
        }
        if self.context().first_error.borrow().is_some() {
            self.finish_native();
            return Err(self.context().reported_error());
        }
        // SAFETY: this non-owning wrapper is lexical and exclusive; native
        // ownership stays in the slot until the wrapper and scoped values drop.
        let wrapper = catch_unwind(AssertUnwindSafe(|| unsafe {
            Connection::from_handle(self.connection)
        }))
        .map_err(|payload| {
            std::mem::forget(payload);
            OwnerFailure::Panic
        })
        .and_then(|result| result.map_err(|error| OwnerFailure::Operation(sql(error))));
        let borrowed = match wrapper {
            Ok(connection) => connection,
            Err(error) => {
                self.context().latch(error);
                self.finish_native();
                return Err(self.context().reported_error());
            }
        };
        let outcome = catch_unwind(AssertUnwindSafe(|| run(&borrowed, self.context())));
        let mut completed = None;
        match outcome {
            Ok(Ok(value)) => completed = Some(value),
            Ok(Err(error)) => {
                self.context().latch(OwnerFailure::Operation(error));
            }
            Err(payload) => {
                std::mem::forget(payload);
                self.context().latch(OwnerFailure::Panic);
            }
        }
        if let Err(payload) = catch_unwind(AssertUnwindSafe(|| drop(borrowed))) {
            std::mem::forget(payload);
            private_worker_terminal();
        }
        if self.context().first_error.borrow().is_some() {
            self.finish_native();
            return Err(self.context().reported_error());
        }
        completed.ok_or_else(|| refuse("operation completed without owned result"))
    }
    fn finish_native(&mut self) {
        if self
            .context
            .as_ref()
            .is_some_and(|context| context.in_callback.get())
        {
            private_worker_terminal();
        }
        if self.connection.is_null() {
            if let Some(context) = &self.context {
                context.phase.set(OwnerPhase::Finished);
            }
            return;
        }
        self.context().phase.set(OwnerPhase::Finishing);
        let mut record = empty_finish_record();
        // SAFETY: no lexical wrapper/statement remains; the pinned finish owns
        // rollback/close and alone clears the slot after confirmed detachment.
        let rc = unsafe {
            if self.context().profile.readonly() {
                ffi::sqlite3_pb_reader_finish_v1(&mut self.connection, &mut record)
            } else {
                ffi::sqlite3_pb_writer_finish_v1(&mut self.connection, &mut record)
            }
        };
        if !self.connection.is_null()
            || !detached_finish(&record, rc)
            || record.shared_roles != 0
            || !self.context().live.is_empty()
        {
            private_worker_terminal();
        }
        self.context().phase.set(OwnerPhase::Finished);
        if rc != ffi::SQLITE_OK {
            self.context().latch(OwnerFailure::Native(rc));
        }
        self.disposition = Some(record);
    }
    fn finish(mut self) -> Result<FinishedScope<'a>> {
        self.finish_native();
        if let Some(error) = self.context().take_error() {
            return Err(error);
        }
        Ok(FinishedScope {
            context: self.context.take().expect("finished owner context"),
            disposition: self.disposition.take(),
        })
    }
}
impl Drop for NativeSqlOwner<'_> {
    fn drop(&mut self) {
        self.finish_native();
        // Only confirmed detachment or an always-empty slot reaches field Drop.
    }
}
pub(super) struct FinishedScope<'a> {
    context: Box<OwnerContext<'a>>,
    // None means never opened, not a native success/validation receipt.
    disposition: Option<ffi::sqlite3_pb_finish_v1>,
}
impl<'a> FinishedScope<'a> {
    pub(super) fn evidence(&self) -> &RetainedEvidence<'a> {
        &self.context.evidence
    }
}
pub(super) struct ReaderScope<'a>(NativeSqlOwner<'a>);
impl<'a> ReaderScope<'a> {
    pub(super) fn prepare(evidence: RetainedEvidence<'a>) -> Result<Self> {
        if !matches!(
            evidence.stage(),
            EvidenceStage::Selected | EvidenceStage::BeforeSelector
        ) {
            return Err(refuse("reader owner requires selected evidence"));
        }
        Ok(Self(NativeSqlOwner::prepare(
            evidence,
            ScopeProfile::Selected,
        )))
    }
    pub(super) fn finish(self) -> Result<FinishedScope<'a>> {
        self.0.finish()
    }
}
#[cfg(test)]
pub(super) fn reader_transaction_control(evidence: RetainedEvidence<'_>, end_transaction: bool) {
    let permit = LiveSqlPermit(super::owner::SqlPermit::enter().unwrap());
    let mut owner = ReaderScope::prepare(evidence).unwrap().0;
    owner.open_native(&permit).unwrap();
    // SAFETY: this test owns the private connection and retains N through finish.
    unsafe {
        let db = owner.connection;
        assert_eq!(ffi::sqlite3_get_autocommit(db), 0);
        let mut statement = std::ptr::null_mut();
        assert_eq!(
            ffi::sqlite3_prepare_v2(
                db,
                c"SELECT state FROM meta WHERE singleton=1".as_ptr(),
                -1,
                &mut statement,
                std::ptr::null_mut(),
            ),
            ffi::SQLITE_OK
        );
        assert!(!statement.is_null());
        assert_eq!(ffi::sqlite3_step(statement), ffi::SQLITE_ROW);
        assert_eq!(ffi::sqlite3_step(statement), ffi::SQLITE_DONE);
        assert_eq!(ffi::sqlite3_reset(statement), ffi::SQLITE_OK);
        if end_transaction {
            assert_eq!(
                ffi::sqlite3_exec(
                    db,
                    c"COMMIT".as_ptr(),
                    None,
                    std::ptr::null_mut(),
                    std::ptr::null_mut()
                ),
                ffi::SQLITE_OK
            );
            assert_eq!(ffi::sqlite3_get_autocommit(db), 1);
            // Already prepared: this reaches the step fence, not authorizer refusal.
            assert_eq!(ffi::sqlite3_step(statement), ffi::SQLITE_MISUSE);
        }
        assert_eq!(ffi::sqlite3_finalize(statement), ffi::SQLITE_OK);
    }
    let finished = owner.finish();
    if end_transaction {
        let error = finished
            .err()
            .expect("native first error survives confirmed finish");
        assert_eq!(
            error.to_string(),
            "initial progress ledger refused: SQL owner: Native(21)"
        );
    } else {
        assert!(finished.is_ok());
    }
    drop(permit);
}

pub(super) struct WriterScope<'a>(NativeSqlOwner<'a>);
impl<'a> WriterScope<'a> {
    pub(super) fn prepare(evidence: RetainedEvidence<'a>) -> Result<Self> {
        let profile = match evidence.stage() {
            EvidenceStage::CreateInit => ScopeProfile::CreateInit,
            EvidenceStage::ReopenInit => ScopeProfile::ReopenInit,
            EvidenceStage::Certify => ScopeProfile::Certify,
            EvidenceStage::ReadyResume => ScopeProfile::ReadyResume,
            EvidenceStage::Append => ScopeProfile::Append,
            EvidenceStage::Reconcile => ScopeProfile::Reconcile,
            EvidenceStage::Selected | EvidenceStage::BeforeSelector => {
                return Err(refuse("writer owner requires writer evidence"));
            }
        };
        Ok(Self(NativeSqlOwner::prepare(evidence, profile)))
    }
    pub(super) fn finish(self) -> Result<FinishedScope<'a>> {
        self.0.finish()
    }
}

#[cfg(test)]
pub(super) fn owner_foundation_control(evidence: RetainedEvidence<'_>) {
    let stage = evidence.stage();
    let owner = match stage {
        EvidenceStage::Selected | EvidenceStage::BeforeSelector => {
            ReaderScope::prepare(evidence).unwrap().0
        }
        _ => WriterScope::prepare(evidence).unwrap().0,
    };
    assert!(owner.connection.is_null());
    assert_eq!(owner.context().phase.get(), OwnerPhase::Prepared);
    let address = owner.options().context;
    let moved = std::hint::black_box(Some(owner));
    let owner = moved.unwrap();
    assert_eq!(owner.options().context, address);
    let options = owner.options();
    assert_eq!(options.flags, 0);
    assert_eq!(options.reserved, [0; 4]);
    assert_eq!(options.profile, owner.context().profile.native());
    assert!(options.xNative.is_some() && options.xTerminal.is_some());

    let finished = owner.finish().unwrap();
    assert!(finished.disposition.is_none());
    assert_eq!(finished.context.phase.get(), OwnerPhase::Finished);
    assert_eq!(finished.evidence().stage(), stage);
    let context = *finished.context;
    let profile = context.profile;
    let mut owner = NativeSqlOwner::prepare(context.evidence, profile);

    // Pure predicates only: no ABI query, native open, finish, or SQL runs.
    let mut abi = ffi::sqlite3_pb_abi_v1 {
        struct_size: std::mem::size_of::<ffi::sqlite3_pb_abi_v1>() as u32,
        abi_version: ffi::SQLITE_PB_ABI_VERSION,
        options_size: std::mem::size_of::<ffi::sqlite3_pb_options_v1>() as u32,
        native_size: std::mem::size_of::<ffi::sqlite3_pb_native_v1>() as u32,
        finish_size: std::mem::size_of::<ffi::sqlite3_pb_finish_v1>() as u32,
        pointer_bits: usize::BITS,
        capabilities: 0,
    };
    assert!(!compatible_owner_abi(&abi, profile));
    abi.capabilities = profile.capabilities();
    assert!(compatible_owner_abi(&abi, profile));
    abi.pointer_bits += 1;
    assert!(!compatible_owner_abi(&abi, profile));
    abi.pointer_bits = usize::BITS;
    abi.finish_size += 1;
    assert!(!compatible_owner_abi(&abi, profile));
    assert_ne!(
        ScopeProfile::Selected.capabilities(),
        ScopeProfile::CreateInit.capabilities()
    );
    let mut record = empty_finish_record();
    assert!(detached_finish(&record, ffi::SQLITE_OK));
    record.detached_roles = 1 << ffi::SQLITE_PB_ROLE_MAIN;
    assert!(!detached_finish(&record, ffi::SQLITE_OK));
    record.closed_roles = record.detached_roles;
    assert!(!detached_finish(&record, ffi::SQLITE_OK));
    record.generation = 1;
    assert!(detached_finish(&record, ffi::SQLITE_OK));
    record.shared_roles = record.detached_roles;
    assert!(!detached_finish(&record, ffi::SQLITE_OK));
    record.closed_roles = 0;
    assert!(detached_finish(&record, ffi::SQLITE_OK));
    record.first_error = ffi::SQLITE_IOERR;
    assert!(!detached_finish(&record, ffi::SQLITE_OK));
    assert!(detached_finish(&record, ffi::SQLITE_IOERR));
    record.detached_roles |= 1 << 20;
    assert!(!detached_finish(&record, ffi::SQLITE_IOERR));

    let options = owner.options();
    let mut native = ffi::sqlite3_pb_native_v1 {
        struct_size: 0, // malformed before any synthetic handle interpretation
        role: ffi::SQLITE_PB_ROLE_MAIN,
        kind: if cfg!(windows) {
            ffi::SQLITE_PB_NATIVE_WIN_HANDLE
        } else {
            ffi::SQLITE_PB_NATIVE_FD
        },
        phase: ffi::SQLITE_PB_PHASE_ADMIT,
        handle: 0,
        generation: 1,
    };
    // The synthetic envelope is refused without interpreting its handle.
    owner.context().phase.set(OwnerPhase::Opening);
    let callback = options.xNative.unwrap();
    // SAFETY: the Box and stack record live across each synchronous callback;
    // the closed missing-gate path never dereferences the synthetic handle.
    assert_eq!(
        unsafe { callback(options.context, &native) },
        ffi::SQLITE_MISUSE
    );
    assert!(matches!(
        *owner.context().first_error.borrow(),
        Some(OwnerFailure::Callback)
    ));
    let first = owner.context().reported_error().to_string();
    assert_eq!(owner.context().take_error().unwrap().to_string(), first);
    native.struct_size = 0;
    assert_eq!(
        unsafe { callback(options.context, &native) },
        ffi::SQLITE_MISUSE
    );
    assert!(matches!(
        *owner.context().first_error.borrow(),
        Some(OwnerFailure::Callback)
    ));
    owner.context().take_error().unwrap();
    assert_eq!(
        unsafe { callback(options.context, std::ptr::null()) },
        ffi::SQLITE_MISUSE
    );
    owner.context().take_error().unwrap();
    owner.context().in_callback.set(true);
    assert_eq!(
        unsafe { callback(options.context, &native) },
        ffi::SQLITE_MISUSE
    );
    assert!(owner.context().in_callback.get());
    assert!(matches!(
        *owner.context().first_error.borrow(),
        Some(OwnerFailure::Reentry)
    ));
    owner.context().in_callback.set(false);
    owner.context().take_error().unwrap();
    assert_eq!(
        owner
            .context()
            .callback_result(|| panic!("owner callback fault")),
        ffi::SQLITE_MISUSE
    );
    assert!(!owner.context().in_callback.get());
    assert!(matches!(
        *owner.context().first_error.borrow(),
        Some(OwnerFailure::Panic)
    ));
    owner.context().take_error().unwrap();
    assert_eq!(owner.context().callback_result(|| Ok(())), ffi::SQLITE_OK);
    assert!(owner.context().take_error().is_none());

    owner
        .context()
        .latch(OwnerFailure::Operation(refuse("first operation failure")));
    let first = owner.context().reported_error().to_string();
    owner
        .context()
        .latch(OwnerFailure::Native(ffi::SQLITE_IOERR));
    owner.finish_native();
    assert_eq!(owner.context().reported_error().to_string(), first);
    assert!(owner.connection.is_null() && owner.disposition.is_none());
    assert_eq!(owner.finish().err().unwrap().to_string(), first);
}

pub(super) fn root_witness(root: &NativeRootHandle) -> Result<Value> {
    let snapshot = native::root_snapshot(root)?;
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt as _;
        let inode = u64::try_from(snapshot.file).map_err(|_| refuse("inode range"))?;
        Ok(
            json!({"platform":"unix","device":snapshot.volume.to_string(),"inode":inode.to_string(),
            "canonicalPath":{"encoding":"unix-bytes-hex","value":hex(snapshot.canonical.as_os_str().as_bytes())}}),
        )
    }
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt as _;
        let bytes: Vec<u8> = snapshot
            .canonical
            .as_os_str()
            .encode_wide()
            .flat_map(u16::to_le_bytes)
            .collect();
        Ok(
            json!({"platform":"windows","volumeSerial":snapshot.volume.to_string(),"fileId":format!("{:032x}",snapshot.file),
            "canonicalPath":{"encoding":"windows-utf16le-hex","value":hex(&bytes)}}),
        )
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = snapshot;
        Err(refuse("unsupported root"))
    }
}

// Fields stay private; production construction validates retained signed bytes.
pub(super) struct InitialInputs {
    incarnation: String,
    epoch: String,
    root: Value,
    activation_id: String,
    completion_id: String,
    activation_key: String,
    completion_key: String,
    activation_bytes: Vec<u8>,
    completion_bytes: Vec<u8>,
    manifest_hash: String,
}
#[derive(Clone)]
pub(super) struct InitialBindings {
    pub(super) selection: Value,
    pub(super) baseline: Value,
    pub(super) pair: Value,
    pub(super) prefix: Value,
}
impl InitialInputs {
    pub(super) fn from_signed(
        incarnation: String,
        epoch: String,
        root: Value,
        activation_bytes: Vec<u8>,
        completion_bytes: Vec<u8>,
    ) -> Result<Self> {
        let pair = crate::session::store::capabilities::validate_progress_pair(
            &activation_bytes,
            &completion_bytes,
        )?;
        let value = Self {
            incarnation,
            epoch,
            root,
            activation_id: pair.activation.activation_id().to_owned(),
            completion_id: pair.completion.completion_id().to_owned(),
            activation_key: pair.activation.logical_key(),
            completion_key: pair.completion.logical_key(),
            manifest_hash: pair.activation.manifest_hash().to_owned(),
            activation_bytes,
            completion_bytes,
        };
        value.bindings()?;
        Ok(value)
    }
    pub(super) fn database(&self, root: &Path) -> Result<PathBuf> {
        final_path(root, &self.incarnation)
    }
    pub(super) fn carriers(&self) -> [(&str, &[u8]); 2] {
        [
            (&self.activation_key, &self.activation_bytes),
            (&self.completion_key, &self.completion_bytes),
        ]
    }
    fn verify_carriers(&self, root: &Path) -> Result<()> {
        crate::session::store::capabilities::validate_progress_pair(
            &self.activation_bytes,
            &self.completion_bytes,
        )?;
        let journal = LocalJournal::new(root);
        for (key, bytes) in self.carriers() {
            if journal.read_event_bytes(key)?.as_deref() != Some(bytes) {
                return Err(refuse("retained capability carrier changed"));
            }
        }
        Ok(())
    }
    pub(super) fn bindings(&self) -> Result<InitialBindings> {
        if !hash64(&self.incarnation)
            || !hash64(&self.epoch)
            || self.activation_bytes.is_empty()
            || self.completion_bytes.is_empty()
            || self.activation_bytes.len() as u128 > i64::MAX as u128
            || self.completion_bytes.len() as u128 > i64::MAX as u128
            || self.activation_key != "store_capability_activation:review_change_revision_v1:root"
            || self.completion_key != format!("bulk_adoption_completion:{}", self.completion_id)
            || !self
                .activation_id
                .strip_prefix("capability-activation:sha256:")
                .is_some_and(hash64)
            || !self
                .completion_id
                .strip_prefix("bulk-adoption-completion:sha256:")
                .is_some_and(hash64)
            || !self
                .manifest_hash
                .strip_prefix("sha256:")
                .is_some_and(hash64)
        {
            return Err(refuse("invalid prepared input"));
        }
        let ak = sha256_bytes_hex(self.activation_key.as_bytes());
        let ck = sha256_bytes_hex(self.completion_key.as_bytes());
        if ak == ck {
            return Err(refuse("duplicate baseline key"));
        }
        let ah = format!("sha256:{}", sha256_bytes_hex(&self.activation_bytes));
        let ch = format!("sha256:{}", sha256_bytes_hex(&self.completion_bytes));
        let mut rows = vec![
            json!({"keyDigest":ak,"recordSha256":ah,"recordLength":self.activation_bytes.len(),"kind":"activation"}),
            json!({"keyDigest":ck,"recordSha256":ch,"recordLength":self.completion_bytes.len(),"kind":"completion"}),
        ];
        rows.sort_by(|a, b| a["keyDigest"].as_str().cmp(&b["keyDigest"].as_str()));
        let baseline = json!({"domain":"pointbreak.append-progress-baseline.v1","records":rows});
        let pair = json!({"domain":"pointbreak.append-progress-capability-pair.v1",
            "activationId":self.activation_id,"activationKeyDigest":ak,"activationRecordSha256":ah,"activationRecordLength":self.activation_bytes.len(),
            "completionId":self.completion_id,"completionKeyDigest":ck,"completionRecordSha256":ch,"completionRecordLength":self.completion_bytes.len(),
            "manifestHash":self.manifest_hash,"minimumReaderProfile":"review_change_revision_progress_v1"});
        let baseline_hash = sha256_json_hex(&baseline)?;
        let selection = json!({"schema":"pointbreak.append-progress-selection","version":1,"incarnationId":self.incarnation,"progressEpoch":self.epoch,
            "rootWitness":self.root,"activationId":self.activation_id,"activationRecordSha256":ah,"completionId":self.completion_id,
            "completionRecordSha256":ch,"baselineSha256":baseline_hash,"ledgerFormat":1});
        let prefix = json!({"domain":"pointbreak.append-progress-prefix.v1","head":0,"selectionSha256":sha256_json_hex(&selection)?,
            "baselineSha256":baseline_hash,"capabilityPairSha256":sha256_json_hex(&pair)?});
        Ok(InitialBindings {
            selection,
            baseline,
            pair,
            prefix,
        })
    }
}

// Closed grammar: schema validation compares normalized SQL tokens, preserving
// quoted literals. This accepts whitespace/case variation without permitting a
// missing CHECK, extra trigger or an alternate constraint expression.
fn schema() -> Vec<(&'static str, String)> {
    fn t(name: &str) -> String {
        format!("{name} TEXT NOT NULL CHECK(length({name})=64 AND {name} NOT GLOB '*[^0-9a-f]*')")
    }
    fn ph(name: &str) -> String {
        format!(
            "{name} TEXT NOT NULL CHECK(length({name})=71 AND substr({name},1,7)='sha256:' AND substr({name},8) NOT GLOB '*[^0-9a-f]*')"
        )
    }
    fn n(name: &str, low: u8) -> String {
        format!("{name} INTEGER NOT NULL CHECK({name}>={low} AND {name}<=9223372036854775807)")
    }
    fn k(name: &str) -> String {
        format!("{name} TEXT NOT NULL CHECK(length(CAST({name} AS BLOB))>0)")
    }
    vec![
        (
            "meta",
            format!(
                "CREATE TABLE meta(singleton INTEGER NOT NULL PRIMARY KEY CHECK(singleton=1),format INTEGER NOT NULL CHECK(format=1),{},{},{},{},{},{},{},state TEXT NOT NULL CHECK(state IN ('initializing','ready','fenced'))) STRICT",
                t("selection_sha256"),
                t("baseline_sha256"),
                t("capability_pair_sha256"),
                t("epoch"),
                n("head", 0),
                t("prefix_sha256"),
                k("native_stamp")
            ),
        ),
        (
            "baseline",
            format!(
                "CREATE TABLE baseline({} PRIMARY KEY,{},{},kind TEXT NOT NULL CHECK(kind IN ('activation','completion'))) STRICT",
                t("key_digest"),
                ph("record_sha256"),
                n("record_length", 0)
            ),
        ),
        (
            "attempt",
            format!(
                "CREATE TABLE attempt({} PRIMARY KEY,{},{},{}) STRICT",
                k("attempt_id"),
                k("key"),
                t("expected_sha256"),
                n("expected_length", 0)
            ),
        ),
        (
            "receipt",
            format!(
                "CREATE TABLE receipt({} PRIMARY KEY,{} UNIQUE,{},{},{} REFERENCES attempt(attempt_id) ON UPDATE NO ACTION ON DELETE NO ACTION) STRICT",
                n("sequence", 1),
                k("key"),
                t("stored_sha256"),
                n("stored_length", 0),
                k("original_attempt")
            ),
        ),
        (
            "current",
            String::from(
                "CREATE TABLE current(singleton INTEGER NOT NULL PRIMARY KEY CHECK(singleton=1),phase TEXT NOT NULL CHECK(phase IN ('INIT','INTENT','PROVED_ABSENT','PROVED_PRESENT','SETTLED_ABSENT','SETTLED_PRESENT')),original_obligation_json TEXT,proof_json TEXT,CHECK((phase='INIT' AND original_obligation_json IS NULL AND proof_json IS NULL) OR (phase<>'INIT' AND original_obligation_json IS NOT NULL))) STRICT",
            ),
        ),
    ]
}
fn tokens(input: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut chars = input.chars().peekable();
    while let Some(c) = chars.next() {
        if c.is_ascii_whitespace() {
            continue;
        }
        let mut token = c.to_string();
        if c == '\'' {
            while let Some(next) = chars.next() {
                token.push(next);
                if next == '\'' {
                    if chars.peek() == Some(&'\'') {
                        token.push(chars.next().unwrap());
                    } else {
                        break;
                    }
                }
            }
        } else if c.is_ascii_alphanumeric() || c == '_' {
            while chars
                .peek()
                .is_some_and(|c| c.is_ascii_alphanumeric() || *c == '_')
            {
                token.push(chars.next().unwrap());
            }
            token.make_ascii_lowercase();
        }
        out.push(token);
    }
    out
}
fn validate_schema(db: &Connection) -> Result<()> {
    for (pragma, expected) in [("user_version", 1), ("application_id", 0)] {
        if db
            .pragma_query_value(None, pragma, |r| {
                super::audit::row(r)?;
                r.get::<_, i64>(0)
            })
            .map_err(sql)?
            != expected
        {
            return Err(refuse("schema version"));
        }
    }
    let mut query = db
        .prepare("SELECT type,name,tbl_name,sql FROM sqlite_schema ORDER BY name")
        .map_err(sql)?;
    let entries = query
        .query_map([], |r| {
            super::audit::row(r)?;
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, Option<String>>(3)?,
            ))
        })
        .map_err(sql)?;
    let expected = schema();
    let mut tables = 0;
    let mut indexes = Vec::new();
    for entry in entries {
        let (kind, name, table, body) = entry.map_err(sql)?;
        if kind == "index" && body.is_none() {
            indexes.push((name, table));
            continue;
        }
        let Some((_, wanted)) = expected.iter().find(|(n, _)| *n == name) else {
            return Err(refuse("unexpected schema object"));
        };
        if kind != "table" || table != name || body.as_deref().map(tokens) != Some(tokens(wanted)) {
            return Err(refuse("schema constraints"));
        }
        tables += 1;
    }
    let expected_indexes = vec![
        (
            "sqlite_autoindex_attempt_1".to_owned(),
            "attempt".to_owned(),
        ),
        (
            "sqlite_autoindex_baseline_1".to_owned(),
            "baseline".to_owned(),
        ),
        (
            "sqlite_autoindex_receipt_1".to_owned(),
            "receipt".to_owned(),
        ),
    ];
    if tables != 5 || indexes != expected_indexes {
        return Err(refuse("schema table/index set"));
    }
    Ok(())
}
fn configure(db: &Connection) -> Result<()> {
    db.busy_timeout(std::time::Duration::ZERO).map_err(sql)?;
    let mut statement = db.prepare("PRAGMA journal_mode=WAL").map_err(sql)?;
    let configured: Result<()> = (|| {
        let mut rows = statement.query([]).map_err(sql)?;
        let row = rows
            .next()
            .map_err(sql)?
            .ok_or_else(|| refuse("missing journal mode"))?;
        super::audit::row(row).map_err(sql)?;
        let mode: String = row.get(0).map_err(sql)?;
        if mode != "wal" {
            return Err(refuse("connection configuration"));
        }
        // The result row precedes completion of the mode-change transaction.
        if rows.next().map_err(sql)?.is_some() {
            return Err(refuse("multiple journal modes"));
        }
        Ok(())
    })();
    // Always finalize the acquired statement; preserve the first failure.
    let finalized = statement.finalize().map_err(sql);
    configured?;
    finalized?;
    db.execute_batch("PRAGMA synchronous=FULL; PRAGMA foreign_keys=ON;")
        .map_err(sql)?;
    if configuration_scalar(db, "synchronous")? != 2
        || configuration_scalar(db, "foreign_keys")? != 1
    {
        return Err(refuse("connection configuration"));
    }
    Ok(())
}
pub(super) fn configuration_scalar(db: &Connection, name: &str) -> Result<i64> {
    db.pragma_query_value(None, name, |row| {
        super::audit::row(row)?;
        row.get(0)
    })
    .map_err(sql)
}
fn supported_stamp(value: &JournalChangeStamp) -> Result<()> {
    let JournalChangeStamp::Observed {
        identity_sha256,
        change_sha256,
        entry_count,
        native_cursor,
    } = value
    else {
        return Err(refuse("activation observation absent"));
    };
    if !hash64(identity_sha256) || !hash64(change_sha256) {
        return Err(refuse("malformed observation hash"));
    }
    #[cfg(windows)]
    if native_cursor.is_none() || entry_count.is_some() {
        return Err(refuse("unsupported Windows observation"));
    }
    #[cfg(target_os = "macos")]
    if native_cursor.is_some() || entry_count.is_none() {
        return Err(refuse("unsupported macOS observation"));
    }
    #[cfg(not(any(target_os = "macos", windows)))]
    {
        let _ = (entry_count, native_cursor);
        return Err(refuse("unsupported observation platform"));
    }
    #[cfg(any(target_os = "macos", windows))]
    Ok(())
}
fn decode_stamp(encoded: &str) -> Result<JournalChangeStamp> {
    // Deserialize directly: duplicate/unknown fields are rejected before any
    // Value map could erase them. Exact reserialization also rejects omissions.
    let value: JournalChangeStamp =
        serde_json::from_str(encoded).map_err(|_| refuse("closed native stamp"))?;
    if text(&serde_json::to_value(&value).map_err(|_| refuse("stamp serialization"))?)? != encoded {
        return Err(refuse("noncanonical native stamp"));
    }
    supported_stamp(&value)?;
    Ok(value)
}
fn accept_check(anchor: &JournalChangeStamp, check: &JournalChangeCheck) -> Result<()> {
    supported_stamp(anchor)?;
    supported_stamp(&check.after)?;
    let (
        JournalChangeStamp::Observed {
            identity_sha256: a, ..
        },
        JournalChangeStamp::Observed {
            identity_sha256: b, ..
        },
    ) = (anchor, &check.after)
    else {
        return Err(refuse("observation absent"));
    };
    if check.verdict != JournalChangeVerdict::Stable || a != b {
        return Err(refuse("native continuation not stable"));
    }
    Ok(())
}
fn stamp(root: &Path) -> Result<JournalChangeStamp> {
    let value = LocalJournal::new(root).change_stamp()?;
    supported_stamp(&value)?;
    Ok(value)
}
fn sidecar(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path.as_os_str().to_os_string();
    name.push(suffix);
    PathBuf::from(name)
}
// Retain named file/parent identities across the SQLite lifetime. This reuses
// the existing file_identity primitive; it is not a new identity authority.
pub(super) struct HeldPath {
    path: PathBuf,
    file: std::fs::File,
    directory: bool,
}

/// Stage-local read-only access proof. Missing paths must remain absent; every
/// present handle is retained and rebound at the other endpoint, including WAL/SHM.
pub(super) struct Access<'a> {
    root: &'a NativeRootHandle,
    guard: &'a StoreAuthorityLock,
    held: Vec<HeldPath>,
    absent: Vec<PathBuf>,
    #[cfg(windows)]
    acl: native::windows_acl::Gate<'a>,
}
pub(super) fn access<'a>(
    root: &'a NativeRootHandle,
    guard: &'a StoreAuthorityLock,
    paths: &[(PathBuf, bool)],
) -> Result<Access<'a>> {
    let canonical = native::root_snapshot(root)?.canonical;
    guard.validate_physical_root(&canonical)?;
    let mut held = vec![
        HeldPath::open(&canonical, true)?,
        HeldPath::open(&canonical.join("authority.writer.lock"), false)?,
    ];
    let mut absent = Vec::new();
    for (path, directory) in paths {
        if path == &canonical {
            if !directory {
                return Err(refuse("root is not a file"));
            }
            continue;
        }
        if !path.starts_with(&canonical)
            || path
                .components()
                .any(|c| matches!(c, std::path::Component::ParentDir))
        {
            return Err(refuse("access path escaped root"));
        }
        match std::fs::symlink_metadata(path) {
            Ok(_) => held.push(HeldPath::open(path, *directory)?),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => absent.push(path.clone()),
            Err(e) => return Err(io(e)),
        }
    }
    #[cfg(unix)]
    for object in held.iter().skip(2) {
        use std::os::unix::fs::PermissionsExt as _;
        if object.file.metadata().map_err(io)?.permissions().mode() & 0o777
            != if object.directory { 0o700 } else { 0o600 }
        {
            return Err(refuse("private progress mode"));
        }
    }
    #[cfg(windows)]
    let acl = {
        use native::private_acl::Kind;
        let objects: Vec<_> = held
            .iter()
            .skip(1)
            .map(|h| {
                (
                    h.path.as_path(),
                    &h.file,
                    if h.directory {
                        Kind::Directory
                    } else {
                        Kind::File
                    },
                )
            })
            .collect();
        native::windows_acl::begin(root, guard, &objects)?
    };
    Ok(Access {
        root,
        guard,
        held,
        absent,
        #[cfg(windows)]
        acl,
    })
}
impl Access<'_> {
    // Only the caller-declared creation stage may consume absence. Retained
    // objects still must keep identity/descriptor; every other absence stays
    // fenced. The caller immediately reacquires the complete object proof.
    fn finish_creation(mut self, created: &[PathBuf]) -> Result<()> {
        self.absent.retain(|path| !created.contains(path));
        self.finish()
    }
    pub(super) fn finish(self) -> Result<()> {
        self.guard
            .validate_physical_root(&native::root_snapshot(self.root)?.canonical)?;
        for held in &self.held {
            held.validate()?;
        }
        #[cfg(unix)]
        for held in self.held.iter().skip(2) {
            use std::os::unix::fs::PermissionsExt as _;
            if held.file.metadata().map_err(io)?.permissions().mode() & 0o777
                != if held.directory { 0o700 } else { 0o600 }
            {
                return Err(refuse("private progress mode changed"));
            }
        }
        for absent in &self.absent {
            match std::fs::symlink_metadata(absent) {
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                _ => return Err(refuse("absent progress object moved during gate")),
            }
        }
        #[cfg(windows)]
        self.acl.finish()?;
        self.guard
            .validate_physical_root(&native::root_snapshot(self.root)?.canonical)
    }
}
impl HeldPath {
    pub(super) fn open(path: &Path, directory: bool) -> Result<Self> {
        let mut options = std::fs::OpenOptions::new();
        options.read(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt as _;
            options.custom_flags(
                libc::O_NOFOLLOW | libc::O_NONBLOCK | if directory { libc::O_DIRECTORY } else { 0 },
            );
        }
        #[cfg(windows)]
        {
            use std::os::windows::fs::OpenOptionsExt as _;
            options.custom_flags(0x0020_0000 | if directory { 0x0200_0000 } else { 0 });
        }
        let file = options.open(path).map_err(io)?;
        let m = file.metadata().map_err(io)?;
        if m.is_dir() != directory || (!directory && !m.is_file()) {
            return Err(refuse("ledger path kind"));
        }
        #[cfg(windows)]
        {
            use std::os::windows::fs::MetadataExt as _;
            if m.file_attributes() & 0x400 != 0 {
                return Err(refuse("ledger path reparse"));
            }
        }
        Ok(Self {
            path: path.to_path_buf(),
            file,
            directory,
        })
    }
    pub(super) fn validate(&self) -> Result<()> {
        let fresh = Self::open(&self.path, self.directory)?;
        if crate::session::store::authority_lock::file_identity(&self.file)?
            != crate::session::store::authority_lock::file_identity(&fresh.file)?
        {
            return Err(refuse("ledger path rebound"));
        }
        Ok(())
    }
    pub(super) fn read(&self) -> Result<Vec<u8>> {
        use std::io::{Read as _, Seek as _};
        self.validate()?;
        let mut file = &self.file;
        file.rewind().map_err(io)?;
        let mut bytes = Vec::new();
        if super::audit::active() {
            bytes = super::audit::read(&self.file, self.file.metadata().map_err(io)?.len())?;
        } else {
            file.read_to_end(&mut bytes).map_err(io)?;
        }
        self.validate()?;
        Ok(bytes)
    }
    pub(super) fn same(&self, file: &std::fs::File) -> Result<()> {
        if crate::session::store::authority_lock::file_identity(&self.file)?
            != crate::session::store::authority_lock::file_identity(file)?
        {
            return Err(refuse("new file was rebound"));
        }
        self.validate()
    }
}
fn paths(root: &Path, path: &Path) -> Result<Vec<HeldPath>> {
    let mut held = vec![HeldPath::open(path, false)?];
    let mut parent = path.parent();
    while let Some(p) = parent {
        if p == root {
            return Ok(held);
        }
        if !p.starts_with(root) {
            break;
        }
        held.push(HeldPath::open(p, true)?);
        parent = p.parent();
    }
    Err(refuse("ledger escaped root"))
}
fn check_sidecars(path: &Path) -> Result<()> {
    for suffix in ["-wal", "-shm"] {
        let p = sidecar(path, suffix);
        match std::fs::symlink_metadata(&p) {
            Ok(_) => {
                HeldPath::open(&p, false)?;
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(io(e)),
        }
    }
    Ok(())
}
fn physical(
    root: &NativeRootHandle,
    guard: &StoreAuthorityLock,
    canonical: &Path,
    expected: &Value,
) -> Result<()> {
    guard.validate_physical_root(canonical)?;
    if root_witness(root)? != *expected {
        return Err(refuse("planned root mismatch"));
    }
    Ok(())
}
fn continuation(
    root: &NativeRootHandle,
    guard: &StoreAuthorityLock,
    canonical: &Path,
    expected: &Value,
    anchor: &JournalChangeStamp,
    at: &str,
) -> Result<()> {
    physical(root, guard, canonical, expected)?;
    #[cfg(test)]
    hook(at)?;
    #[cfg(not(test))]
    let _ = at;
    let check = LocalJournal::new(canonical).changes_since(anchor)?;
    accept_check(anchor, &check)?;
    physical(root, guard, canonical, expected)
}
fn final_path(root: &Path, incarnation: &str) -> Result<PathBuf> {
    if !hash64(incarnation) {
        return Err(refuse("incarnation grammar"));
    }
    let path = root
        .join("append-progress")
        .join("incarnations")
        .join(incarnation)
        .join("ledger.sqlite");
    sqlite_paths(&path)?;
    Ok(path)
}
fn sqlite_paths(database: &Path) -> Result<()> {
    for path in [
        database.to_path_buf(),
        sidecar(database, "-wal"),
        sidecar(database, "-shm"),
    ] {
        #[cfg(windows)]
        {
            let text = path
                .to_str()
                .ok_or_else(|| refuse("unsupported Windows SQLite path"))?;
            if text.contains('\0') {
                return Err(refuse("unsupported Windows SQLite path"));
            }
        }
        #[cfg(unix)]
        {
            use std::os::unix::ffi::OsStrExt as _;
            if path.as_os_str().as_bytes().contains(&0) {
                return Err(refuse("SQLite path contains NUL"));
            }
        }
        #[cfg(not(any(unix, windows)))]
        {
            let _ = path;
            return Err(refuse("unsupported SQLite path platform"));
        }
    }
    Ok(())
}
#[cfg(test)]
pub(super) fn fixture_sqlite_paths(path: &Path) -> Result<()> {
    sqlite_paths(path)
}
fn path_chain(root: &Path, incarnation: &str, create: bool) -> Result<()> {
    path_chain_checked(root, incarnation, create, None)
}
type CreationProof<'a> = (
    &'a NativeRootHandle,
    &'a StoreAuthorityLock,
    &'a [(PathBuf, bool)],
);
fn proof(check: Option<CreationProof<'_>>) -> Result<Option<Access<'_>>> {
    check
        .map(|(root, guard, objects)| access(root, guard, objects))
        .transpose()
}
fn finish_proof(check: Option<Access<'_>>) -> Result<()> {
    match check {
        Some(check) => check.finish(),
        None => Ok(()),
    }
}
fn finish_creation(check: Option<Access<'_>>, created: &[PathBuf]) -> Result<()> {
    match check {
        Some(check) => check.finish_creation(created),
        None => Ok(()),
    }
}
fn path_chain_checked(
    root: &Path,
    incarnation: &str,
    create: bool,
    check: Option<CreationProof<'_>>,
) -> Result<()> {
    let mut path = root.to_path_buf();
    for component in ["append-progress", "incarnations", incarnation] {
        path.push(component);
        if create && (component == incarnation || !path.try_exists().map_err(io)?) {
            let before = proof(check)?;
            let mut builder = std::fs::DirBuilder::new();
            #[cfg(unix)]
            {
                use std::os::unix::fs::DirBuilderExt as _;
                builder.mode(0o700);
            }
            builder.create(&path).map_err(io)?;
            finish_creation(before, std::slice::from_ref(&path))?;
            finish_proof(proof(check)?)?;
        }
        let m = std::fs::symlink_metadata(&path).map_err(io)?;
        if !m.is_dir() || m.file_type().is_symlink() {
            return Err(refuse("invalid ledger directory"));
        }
        #[cfg(windows)]
        {
            use std::os::windows::fs::MetadataExt as _;
            if m.file_attributes() & 0x400 != 0 {
                return Err(refuse("reparse directory"));
            }
        }
    }
    Ok(())
}
fn regular(path: &Path) -> Result<()> {
    let m = std::fs::symlink_metadata(path).map_err(io)?;
    if !m.is_file() || m.file_type().is_symlink() {
        return Err(refuse("invalid ledger file"));
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt as _;
        if m.file_attributes() & 0x400 != 0 {
            return Err(refuse("reparse ledger"));
        }
    }
    Ok(())
}

fn validate_initial_state(
    db: &Connection,
    inputs: &InitialInputs,
    observation: &JournalChangeStamp,
    state: &str,
) -> Result<()> {
    supported_stamp(observation)?;
    validate_schema(db)?;
    for (table, count) in [
        ("meta", 1),
        ("baseline", 2),
        ("attempt", 0),
        ("receipt", 0),
        ("current", 1),
    ] {
        if db
            .query_row(&format!("SELECT count(*) FROM {table}"), [], |r| {
                r.get::<_, i64>(0)
            })
            .map_err(sql)?
            != count
        {
            return Err(refuse("initial row count"));
        }
    }
    let b = inputs.bindings()?;
    let expected = vec![
        rusqlite::types::Value::Integer(1),
        1.into(),
        sha256_json_hex(&b.selection)?.into(),
        sha256_json_hex(&b.baseline)?.into(),
        sha256_json_hex(&b.pair)?.into(),
        inputs.epoch.clone().into(),
        0.into(),
        sha256_json_hex(&b.prefix)?.into(),
        text(&serde_json::to_value(observation)?)?.into(),
        state.to_owned().into(),
    ];
    let actual = db
        .query_row("SELECT * FROM meta", [], |r| {
            (0..10)
                .map(|i| r.get::<_, rusqlite::types::Value>(i))
                .collect::<rusqlite::Result<Vec<_>>>()
        })
        .map_err(sql)?;
    if actual != expected {
        return Err(refuse("initial metadata binding"));
    }
    let current = db
        .query_row("SELECT * FROM current", [], |r| {
            (0..4)
                .map(|i| r.get::<_, rusqlite::types::Value>(i))
                .collect::<rusqlite::Result<Vec<_>>>()
        })
        .map_err(sql)?;
    if current
        != vec![
            1.into(),
            "INIT".to_owned().into(),
            rusqlite::types::Value::Null,
            rusqlite::types::Value::Null,
        ]
    {
        return Err(refuse("initial current row"));
    }
    let mut query = db
        .prepare(
            "SELECT key_digest,record_sha256,record_length,kind FROM baseline ORDER BY key_digest",
        )
        .map_err(sql)?;
    let rows=query.query_map([],|r|Ok(json!({"keyDigest":r.get::<_,String>(0)?,"recordSha256":r.get::<_,String>(1)?,"recordLength":r.get::<_,i64>(2)?,"kind":r.get::<_,String>(3)?}))).map_err(sql)?.collect::<rusqlite::Result<Vec<_>>>().map_err(sql)?;
    if rows
        != *b.baseline["records"]
            .as_array()
            .ok_or_else(|| refuse("baseline"))?
    {
        return Err(refuse("baseline binding"));
    }
    Ok(())
}

#[cfg(test)]
pub(super) struct InitialLedger<'a> {
    db: Connection,
    guard: &'a StoreAuthorityLock,
    root: &'a NativeRootHandle,
    inputs: &'a InitialInputs,
    canonical: PathBuf,
    path: PathBuf,
    held_paths: Vec<HeldPath>,
    observation: JournalChangeStamp,
}
#[cfg(test)]
impl<'a> InitialLedger<'a> {
    pub(super) fn create(
        root: &'a NativeRootHandle,
        guard: &'a StoreAuthorityLock,
        inputs: &'a InitialInputs,
    ) -> Result<Self> {
        Self::create_checked(root, guard, inputs, None)
    }
    pub(super) fn create_enrolled(
        root: &'a NativeRootHandle,
        guard: &'a StoreAuthorityLock,
        inputs: &'a InitialInputs,
        objects: &[(PathBuf, bool)],
    ) -> Result<Self> {
        Self::create_checked(root, guard, inputs, Some((root, guard, objects)))
    }
    fn create_checked(
        root: &'a NativeRootHandle,
        guard: &'a StoreAuthorityLock,
        inputs: &'a InitialInputs,
        check: Option<CreationProof<'_>>,
    ) -> Result<Self> {
        let bindings = inputs.bindings()?;
        let canonical = native::root_snapshot(root)?.canonical;
        physical(root, guard, &canonical, &inputs.root)?;
        let path = final_path(&canonical, &inputs.incarnation)?;
        let observation = stamp(&canonical)?;
        continuation(
            root,
            guard,
            &canonical,
            &inputs.root,
            &observation,
            "create-before-path",
        )?;
        path_chain_checked(&canonical, &inputs.incarnation, true, check)?;
        let empty_before = proof(check)?;
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt as _;
            options.mode(0o600);
        }
        let file = options.open(&path).map_err(io)?;
        let created = HeldPath::open(&path, false)?;
        created.same(&file)?;
        finish_creation(empty_before, std::slice::from_ref(&path))?;
        finish_proof(proof(check)?)?;
        created.validate()?;
        drop(file);
        let held_paths = paths(&canonical, &path)?;
        check_sidecars(&path)?;
        let opening = proof(check)?;
        let mut db = Connection::open_with_flags(
            &path,
            OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )
        .map_err(sql)?;
        configure(&db)?;
        let sidecars = [sidecar(&path, "-wal"), sidecar(&path, "-shm")];
        finish_creation(opening, &sidecars)?;
        finish_proof(proof(check)?)?;
        #[cfg(test)]
        cut("before-transaction");
        let before_transaction = proof(check)?;
        let behavior = if check.is_some() {
            rusqlite::TransactionBehavior::Immediate
        } else {
            rusqlite::TransactionBehavior::Deferred
        };
        let tx = db.transaction_with_behavior(behavior).map_err(sql)?;
        finish_creation(before_transaction, &sidecars)?;
        if check.is_some() {
            for sidecar in &sidecars {
                regular(sidecar)?;
            }
        }
        // BEGIN IMMEDIATE is the declared SQLite sidecar-creation stage.
        // No schema or retained row is written before this complete gate.
        let mutation_access = proof(check)?;
        #[cfg(test)]
        if check.is_some() {
            hook("create-before-schema")?;
        }
        for (_, ddl) in schema() {
            tx.execute_batch(&ddl).map_err(sql)?;
        }
        tx.execute_batch("PRAGMA user_version=1;").map_err(sql)?;
        tx.execute(
            "INSERT INTO meta VALUES(1,1,?1,?2,?3,?4,0,?5,?6,'initializing')",
            params![
                sha256_json_hex(&bindings.selection)?,
                sha256_json_hex(&bindings.baseline)?,
                sha256_json_hex(&bindings.pair)?,
                inputs.epoch,
                sha256_json_hex(&bindings.prefix)?,
                text(&serde_json::to_value(&observation)?)?
            ],
        )
        .map_err(sql)?;
        for row in bindings.baseline["records"]
            .as_array()
            .ok_or_else(|| refuse("baseline rows"))?
        {
            tx.execute(
                "INSERT INTO baseline VALUES(?1,?2,?3,?4)",
                params![
                    row["keyDigest"].as_str(),
                    row["recordSha256"].as_str(),
                    row["recordLength"].as_i64(),
                    row["kind"].as_str()
                ],
            )
            .map_err(sql)?;
        }
        tx.execute("INSERT INTO current VALUES(1,'INIT',NULL,NULL)", [])
            .map_err(sql)?;
        #[cfg(test)]
        cut("during-transaction");
        for held in &held_paths {
            held.validate()?;
        }
        check_sidecars(&path)?;
        continuation(
            root,
            guard,
            &canonical,
            &inputs.root,
            &observation,
            "create-before-commit",
        )?;
        tx.commit().map_err(sql)?;
        finish_proof(mutation_access)?;
        #[cfg(test)]
        cut("after-commit");
        let ledger = Self {
            db,
            guard,
            root,
            inputs,
            canonical,
            path,
            held_paths,
            observation,
        };
        let flush_access = proof(check)?;
        ledger.flush()?;
        finish_proof(flush_access)?;
        Ok(ledger)
    }
    pub(super) fn reopen(
        root: &'a NativeRootHandle,
        guard: &'a StoreAuthorityLock,
        inputs: &'a InitialInputs,
    ) -> Result<Self> {
        Self::reopen_checked(root, guard, inputs, None)
    }
    pub(super) fn reopen_enrolled(
        root: &'a NativeRootHandle,
        guard: &'a StoreAuthorityLock,
        inputs: &'a InitialInputs,
        objects: &[(PathBuf, bool)],
    ) -> Result<Self> {
        Self::reopen_checked(root, guard, inputs, Some((root, guard, objects)))
    }
    fn reopen_checked(
        root: &'a NativeRootHandle,
        guard: &'a StoreAuthorityLock,
        inputs: &'a InitialInputs,
        check: Option<CreationProof<'_>>,
    ) -> Result<Self> {
        inputs.bindings()?;
        let canonical = native::root_snapshot(root)?.canonical;
        physical(root, guard, &canonical, &inputs.root)?;
        let path = final_path(&canonical, &inputs.incarnation)?;
        path_chain(&canonical, &inputs.incarnation, false)?;
        regular(&path)?;
        let held_paths = paths(&canonical, &path)?;
        check_sidecars(&path)?;
        let opening = proof(check)?;
        let db = Connection::open_with_flags(
            &path,
            OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )
        .map_err(sql)?;
        db.busy_timeout(std::time::Duration::ZERO).map_err(sql)?;
        // Force SQLite's read admission before acquiring the stable proof.
        db.query_row("PRAGMA schema_version", [], |r| r.get::<_, i64>(0))
            .map_err(sql)?;
        finish_creation(opening, &[sidecar(&path, "-wal"), sidecar(&path, "-shm")])?;
        let stable = proof(check)?;
        validate_schema(&db)?;
        let encoded: String = db
            .query_row("SELECT native_stamp FROM meta WHERE singleton=1", [], |r| {
                r.get(0)
            })
            .map_err(sql)?;
        let observation = decode_stamp(&encoded)?;
        let ledger = Self {
            db,
            guard,
            root,
            inputs,
            canonical,
            path,
            held_paths,
            observation,
        };
        ledger.validate(inputs)?;
        ledger.gate("reopen-before-configure")?;
        configure(&ledger.db)?;
        ledger.flush()?;
        finish_proof(stable)?;
        Ok(ledger)
    }
    fn physical(&self) -> Result<()> {
        physical(self.root, self.guard, &self.canonical, &self.inputs.root)?;
        for held in &self.held_paths {
            held.validate()?;
        }
        check_sidecars(&self.path)
    }
    fn gate(&self, at: &str) -> Result<()> {
        self.physical()?;
        continuation(
            self.root,
            self.guard,
            &self.canonical,
            &self.inputs.root,
            &self.observation,
            at,
        )?;
        self.physical()
    }
    fn validate(&self, inputs: &InitialInputs) -> Result<()> {
        self.validate_state(inputs, "initializing")
    }
    fn validate_state(&self, inputs: &InitialInputs, state: &str) -> Result<()> {
        self.physical()?;
        validate_initial_state(&self.db, inputs, &self.observation, state)
    }
    pub(super) fn flush(&self) -> Result<()> {
        self.validate(self.inputs)?;
        self.gate("before-checkpoint")?;
        #[cfg(test)]
        hook("checkpoint")?;
        let (busy, log, done): (i64, i64, i64) = self
            .db
            .query_row("PRAGMA wal_checkpoint(FULL)", [], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?))
            })
            .map_err(sql)?;
        if busy != 0 || log < 0 || done != log {
            return Err(refuse("busy or incomplete checkpoint"));
        }
        self.gate("after-checkpoint")?;
        #[cfg(test)]
        cut("after-checkpoint");
        native::sync_progress_files(self.root, &self.path)?;
        self.gate("after-barriers")?;
        self.validate(self.inputs)?;
        #[cfg(test)]
        cut("after-barriers");
        Ok(())
    }
    #[cfg(test)]
    pub(super) fn fixture_db(&self) -> &Connection {
        &self.db
    }
    #[cfg(test)]
    pub(super) fn fixture_path(&self) -> &Path {
        &self.path
    }
}
/// Separate E5 operation: it never invokes INIT continuation after E4, and never
/// changes a ready anchor. No connection or arbitrary state setter escapes.
#[cfg(test)]
pub(super) fn certify(
    root: &NativeRootHandle,
    guard: &StoreAuthorityLock,
    inputs: &InitialInputs,
    objects: &[(PathBuf, bool)],
) -> Result<()> {
    let canonical = native::root_snapshot(root)?.canonical;
    physical(root, guard, &canonical, &inputs.root)?;
    let path = inputs.database(&canonical)?;
    path_chain(&canonical, &inputs.incarnation, false)?;
    let entry = access(root, guard, objects)?;
    let held_paths = paths(&canonical, &path)?;
    let db = Connection::open_with_flags(
        &path,
        OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(sql)?;
    db.busy_timeout(std::time::Duration::ZERO).map_err(sql)?;
    db.query_row("PRAGMA schema_version", [], |r| r.get::<_, i64>(0))
        .map_err(sql)?;
    entry.finish_creation(&[sidecar(&path, "-wal"), sidecar(&path, "-shm")])?;
    let entry = access(root, guard, objects)?;
    validate_schema(&db)?;
    let (state, encoded): (String, String) = db
        .query_row(
            "SELECT state,native_stamp FROM meta WHERE singleton=1",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .map_err(sql)?;
    let mut ledger = InitialLedger {
        db,
        guard,
        root,
        inputs,
        canonical,
        path,
        held_paths,
        observation: decode_stamp(&encoded)?,
    };
    if state != "initializing" && state != "ready" {
        return Err(refuse("E5 invalid old state"));
    }
    ledger.validate_state(inputs, &state)?;
    inputs.verify_carriers(&ledger.canonical)?;
    entry.finish()?;
    if state == "ready" {
        ledger.gate("ready-resume-admission")?;
        configure(&ledger.db)?;
        return ready_flush(&ledger, objects);
    }
    configure(&ledger.db)?;
    let census_access = access(root, guard, objects)?;
    let observation = stamp(&ledger.canonical)?;
    super::enrollment::census(&ledger.canonical, &inputs.carriers())?;
    inputs.verify_carriers(&ledger.canonical)?;
    continuation(
        root,
        guard,
        &ledger.canonical,
        &inputs.root,
        &observation,
        "certify-after-census",
    )?;
    census_access.finish()?;
    let before_transaction = access(root, guard, objects)?;
    let tx =
        rusqlite::Transaction::new_unchecked(&ledger.db, rusqlite::TransactionBehavior::Immediate)
            .map_err(sql)?;
    before_transaction
        .finish_creation(&[sidecar(&ledger.path, "-wal"), sidecar(&ledger.path, "-shm")])?;
    regular(&sidecar(&ledger.path, "-wal"))?;
    regular(&sidecar(&ledger.path, "-shm"))?;
    let commit_access = access(root, guard, objects)?;
    // Validate the exact immutable old A and H0 inside this transaction.
    ledger.validate_state(inputs, "initializing")?;
    #[cfg(test)]
    super::enrollment::boundary("E5-before-update")?;
    let changed = tx.execute("UPDATE meta SET state='ready',native_stamp=?1 WHERE singleton=1 AND state='initializing' AND native_stamp=?2", params![text(&serde_json::to_value(&observation)?)?, encoded]).map_err(sql)?;
    if changed != 1 {
        return Err(refuse("E5 old state changed"));
    }
    #[cfg(test)]
    super::enrollment::boundary("E5-before-commit")?;
    continuation(
        root,
        guard,
        &ledger.canonical,
        &inputs.root,
        &observation,
        "certify-before-commit",
    )?;
    tx.commit().map_err(sql)?;
    ledger.observation = observation;
    #[cfg(test)]
    super::enrollment::boundary("E5-after-commit")?;
    ledger.gate("certify-after-commit")?;
    commit_access.finish()?;
    access(root, guard, objects)?.finish()?;
    // The E5 five G(B) checks are census, precommit, postcommit,
    // postcheckpoint, postbarrier. Ready resume uses admission plus three.
    ready_flush_inner(&ledger, objects, false)
}
#[cfg(test)]
fn ready_flush(ledger: &InitialLedger<'_>, objects: &[(PathBuf, bool)]) -> Result<()> {
    ready_flush_inner(ledger, objects, true)
}
#[cfg(test)]
fn ready_flush_inner(
    ledger: &InitialLedger<'_>,
    objects: &[(PathBuf, bool)],
    before: bool,
) -> Result<()> {
    let check = access(ledger.root, ledger.guard, objects)?;
    ledger.validate_state(ledger.inputs, "ready")?;
    if before {
        ledger.gate("ready-before-checkpoint")?;
    }
    #[cfg(test)]
    hook("ready-checkpoint")?;
    let (busy, log, done): (i64, i64, i64) = ledger
        .db
        .query_row("PRAGMA wal_checkpoint(FULL)", [], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?))
        })
        .map_err(sql)?;
    if busy != 0 || log < 0 || done != log {
        return Err(refuse("ready checkpoint uncertain or busy"));
    }
    ledger.gate("ready-after-checkpoint")?;
    native::sync_progress_files(ledger.root, &ledger.path)?;
    ledger.gate("ready-after-barriers")?;
    ledger.validate_state(ledger.inputs, "ready")?;
    check.finish()
}

/// Existing-only H0 snapshot; no configure, checkpoint, census or repair.
#[cfg(test)]
pub(super) fn validate_ready(
    root: &NativeRootHandle,
    guard: &StoreAuthorityLock,
    inputs: &InitialInputs,
    objects: &[(PathBuf, bool)],
) -> Result<()> {
    let canonical = native::root_snapshot(root)?.canonical;
    physical(root, guard, &canonical, &inputs.root)?;
    let check = access(root, guard, objects)?;
    let path = inputs.database(&canonical)?;
    path_chain(&canonical, &inputs.incarnation, false)?;
    let held_paths = paths(&canonical, &path)?;
    let db = Connection::open_with_flags(
        &path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(sql)?;
    db.busy_timeout(std::time::Duration::ZERO).map_err(sql)?;
    validate_schema(&db)?;
    let encoded: String = db
        .query_row("SELECT native_stamp FROM meta WHERE singleton=1", [], |r| {
            r.get(0)
        })
        .map_err(sql)?;
    let observation = decode_stamp(&encoded)?;
    continuation(
        root,
        guard,
        &canonical,
        &inputs.root,
        &observation,
        "selected-before",
    )?;
    // Validation shares only the closed structural H0 grammar, never INIT's
    // continuation admission or a mutable ready-state constructor.
    let ledger = InitialLedger {
        db,
        guard,
        root,
        inputs,
        canonical,
        path,
        held_paths,
        observation,
    };
    let snapshot = ledger.db.unchecked_transaction().map_err(sql)?;
    ledger.validate_state(inputs, "ready")?;
    inputs.verify_carriers(&ledger.canonical)?;
    ledger.gate("selected-after")?;
    drop(snapshot);
    check.finish()
}

#[cfg(test)]
type BoundaryHook = Box<dyn FnMut(&str) -> Result<()>>;
#[cfg(test)]
thread_local! { static HOOK: std::cell::RefCell<Option<BoundaryHook>> = const { std::cell::RefCell::new(None) }; }
#[cfg(test)]
fn hook(at: &str) -> Result<()> {
    HOOK.with(|h| match h.borrow_mut().as_mut() {
        Some(f) => f(at),
        None => Ok(()),
    })
}
#[cfg(test)]
pub(super) fn with_hook<T>(
    f: impl FnMut(&str) -> Result<()> + 'static,
    run: impl FnOnce() -> T,
) -> T {
    struct Reset;
    impl Drop for Reset {
        fn drop(&mut self) {
            HOOK.with(|h| *h.borrow_mut() = None);
        }
    }
    HOOK.with(|h| {
        assert!(h.borrow().is_none());
        *h.borrow_mut() = Some(Box::new(f));
    });
    let _reset = Reset;
    run()
}
#[cfg(test)]
thread_local! { static INITIAL_SCHEMA: std::cell::Cell<Option<Option<i64>>> = const { std::cell::Cell::new(None) }; }
#[cfg(test)]
pub(super) fn observe_initial_schema<T>(run: impl FnOnce() -> T) -> (T, Option<i64>) {
    struct Reset;
    impl Drop for Reset {
        fn drop(&mut self) {
            INITIAL_SCHEMA.with(|slot| slot.set(None));
        }
    }
    INITIAL_SCHEMA.with(|slot| {
        assert!(slot.get().is_none());
        slot.set(Some(None));
    });
    let _reset = Reset;
    let result = run();
    (result, INITIAL_SCHEMA.with(|slot| slot.get().flatten()))
}
#[cfg(test)]
pub(super) fn fixture_decode_stamp(encoded: &str) -> Result<JournalChangeStamp> {
    decode_stamp(encoded)
}
#[cfg(test)]
pub(super) fn fixture_check(anchor: &JournalChangeStamp, check: &JournalChangeCheck) -> Result<()> {
    accept_check(anchor, check)
}

#[cfg(test)]
fn cut(at: &str) {
    if std::env::var("POINTBREAK_INITIAL_LEDGER_CUT").as_deref() == Ok(at) {
        let root = std::path::PathBuf::from(
            std::env::var_os("POINTBREAK_INITIAL_LEDGER_ROOT").expect("owned child root"),
        );
        assert_eq!(
            std::fs::read(root.join("initial-ledger-owned")).unwrap(),
            b"fixture child only"
        );
        std::process::exit(86);
    }
}
#[cfg(test)]
pub(super) fn fixture_inputs(root: Value) -> InitialInputs {
    let completion_id = format!("bulk-adoption-completion:sha256:{}", "b".repeat(64));
    InitialInputs {
        incarnation: "1".repeat(64),
        epoch: "2".repeat(64),
        root,
        activation_id: format!("capability-activation:sha256:{}", "a".repeat(64)),
        completion_key: format!("bulk_adoption_completion:{completion_id}"),
        completion_id,
        activation_key: "store_capability_activation:review_change_revision_v1:root".into(),
        activation_bytes: b"synthetic activation\n".to_vec(),
        completion_bytes: b"synthetic completion\n".to_vec(),
        manifest_hash: format!("sha256:{}", sha256_bytes_hex(b"{\"reservedRecords\":[]}")),
    }
}

struct OperationOutcome {
    c4: Option<c4::Snapshot>,
    anchor: JournalChangeStamp,
    already_ready: bool,
}
impl OwnerContext<'_> {
    fn gate(&self, db: &Connection, anchor: &JournalChangeStamp) -> Result<()> {
        self.evidence.validate_live()?;
        accept_check(anchor, &self.evidence.journal_check(anchor)?)?;
        // SAFETY: the lexical non-owning wrapper cannot outlive NativeSqlOwner.
        let rc = unsafe {
            if self.profile.readonly() {
                ffi::sqlite3_pb_reader_visit_v1(db.handle(), 1 << ffi::SQLITE_PB_ROLE_MAIN)
            } else {
                ffi::sqlite3_pb_writer_visit_v1(db.handle(), 1 << ffi::SQLITE_PB_ROLE_MAIN)
            }
        };
        if rc != ffi::SQLITE_OK {
            return Err(refuse("native role inventory failed"));
        }
        self.evidence.validate_live()
    }
}
fn execute_owned_sql(db: &Connection, context: &OwnerContext<'_>) -> Result<OperationOutcome> {
    let inputs = context.evidence.inputs();
    let planned = context
        .anchor
        .as_ref()
        .ok_or_else(|| refuse("missing pre-census anchor"))?;
    let mut anchor = planned.clone();
    let mut already_ready = false;
    if context.profile == ScopeProfile::CreateInit {
        configure(db)?;
        let tx = rusqlite::Transaction::new_unchecked(db, rusqlite::TransactionBehavior::Immediate)
            .map_err(sql)?;
        context.gate(db, &anchor)?;
        let bindings = inputs.bindings()?;
        #[cfg(test)]
        INITIAL_SCHEMA.with(|slot| -> Result<()> {
            if slot.get().is_some() {
                assert_eq!(slot.get(), Some(None), "one initial schema observation");
                let count = db
                    .query_row("SELECT count(*) FROM sqlite_schema", [], |r| r.get(0))
                    .map_err(sql)?;
                slot.set(Some(Some(count)));
            }
            Ok(())
        })?;
        for (_, ddl) in schema() {
            tx.execute_batch(&ddl).map_err(sql)?;
        }
        tx.execute_batch("PRAGMA user_version=1;").map_err(sql)?;
        tx.execute(
            "INSERT INTO meta VALUES(1,1,?1,?2,?3,?4,0,?5,?6,'initializing')",
            params![
                sha256_json_hex(&bindings.selection)?,
                sha256_json_hex(&bindings.baseline)?,
                sha256_json_hex(&bindings.pair)?,
                inputs.epoch,
                sha256_json_hex(&bindings.prefix)?,
                text(&serde_json::to_value(&anchor)?)?,
            ],
        )
        .map_err(sql)?;
        for row in bindings.baseline["records"]
            .as_array()
            .ok_or_else(|| refuse("baseline rows"))?
        {
            tx.execute(
                "INSERT INTO baseline VALUES(?1,?2,?3,?4)",
                params![
                    row["keyDigest"].as_str(),
                    row["recordSha256"].as_str(),
                    row["recordLength"].as_i64(),
                    row["kind"].as_str(),
                ],
            )
            .map_err(sql)?;
        }
        tx.execute("INSERT INTO current VALUES(1,'INIT',NULL,NULL)", [])
            .map_err(sql)?;
        validate_initial_state(db, inputs, &anchor, "initializing")?;
        context.gate(db, &anchor)?;
        #[cfg(test)]
        hook("owned-create-before-commit")?;
        tx.commit().map_err(sql)?;
        context.gate(db, &anchor)?;
    } else {
        validate_schema(db)?;
        let (state, encoded): (String, String) = db
            .query_row(
                "SELECT state,native_stamp FROM meta WHERE singleton=1",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .map_err(sql)?;
        let stored = decode_stamp(&encoded)?;
        let expected = match context.profile {
            ScopeProfile::ReopenInit => "initializing",
            ScopeProfile::Certify if state == "initializing" => "initializing",
            _ => "ready",
        };
        if state != expected {
            return Err(refuse("operation state mismatch"));
        }
        validate_initial_state(db, inputs, &stored, expected)?;
        if context.profile == ScopeProfile::Certify && state == "initializing" {
            configure(db)?;
            let tx =
                rusqlite::Transaction::new_unchecked(db, rusqlite::TransactionBehavior::Immediate)
                    .map_err(sql)?;
            validate_initial_state(db, inputs, &stored, "initializing")?;
            context.gate(db, &anchor)?;
            #[cfg(test)]
            super::enrollment::boundary("E5-before-update")?;
            let changed = tx.execute("UPDATE meta SET state='ready',native_stamp=?1 WHERE singleton=1 AND state='initializing' AND native_stamp=?2",
                params![text(&serde_json::to_value(&anchor)?)?, encoded]).map_err(sql)?;
            if changed != 1 {
                return Err(refuse("certification old state changed"));
            }
            #[cfg(test)]
            super::enrollment::boundary("E5-before-commit")?;
            context.gate(db, &anchor)?;
            tx.commit().map_err(sql)?;
            #[cfg(test)]
            super::enrollment::boundary("E5-after-commit")?;
            context.gate(db, &anchor)?;
        } else {
            anchor = stored;
            context.gate(db, &anchor)?;
            already_ready = context.profile == ScopeProfile::Certify;
            if context.profile != ScopeProfile::Selected && !already_ready {
                configure(db)?;
            }
        }
    }
    if context.profile != ScopeProfile::Selected && !already_ready {
        context.gate(db, &anchor)?;
        #[cfg(test)]
        hook("ready-checkpoint")?;
        let (busy, log, done): (i64, i64, i64) = db
            .query_row("PRAGMA wal_checkpoint(FULL)", [], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?))
            })
            .map_err(sql)?;
        if busy != 0 || log < 0 || done != log {
            return Err(refuse("checkpoint incomplete"));
        }
        context.gate(db, &anchor)?;
        #[cfg(test)]
        let test_path = inputs.database(context.evidence.members()[0].path())?;
        #[cfg(test)]
        native::run_hook(&test_path, native::Barrier::File)?;
        // SAFETY: native owns all Q; the barrier visits and flushes borrowed Q only.
        if unsafe { ffi::sqlite3_pb_writer_barrier_v1(db.handle(), 0) } != ffi::SQLITE_OK {
            return Err(refuse("native writer barrier failed"));
        }
        #[cfg(test)]
        native::run_hook(&test_path, native::Barrier::Parent)?;
        context.evidence.sync_directories()?;
        context.gate(db, &anchor)?;
    }
    let state = if matches!(
        context.profile,
        ScopeProfile::CreateInit | ScopeProfile::ReopenInit
    ) {
        "initializing"
    } else {
        "ready"
    };
    validate_initial_state(db, inputs, &anchor, state)?;
    context.gate(db, &anchor)?;
    Ok(OperationOutcome {
        c4: None,
        anchor,
        already_ready,
    })
}

pub(super) fn run_owned(
    root: &NativeRootHandle,
    guard: &StoreAuthorityLock,
    request: &str,
    actor: &str,
    stage: EvidenceStage,
) -> Result<()> {
    super::owner::require_quiescent()?;
    let canonical = native::root_snapshot(root)?.canonical;
    let mut evidence = RetainedEvidence::capture_before_sql(root, guard, stage, request, actor)?;
    let path = evidence.inputs().database(&canonical)?;
    if stage == EvidenceStage::CreateInit {
        path_chain(&canonical, &evidence.inputs().incarnation, true)?;
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt as _;
            options.mode(0o600).custom_flags(libc::O_NOFOLLOW);
        }
        let creation = options.open(&path).map_err(io)?;
        creation.sync_all().map_err(io)?;
        drop(creation); // Last application Q descriptor closes before native open.
        evidence.retain_created_directories_before_sql()?;
    }
    let anchor = evidence.journal_stamp()?;
    let carriers = evidence.inputs().carriers();
    let expected = if matches!(stage, EvidenceStage::CreateInit | EvidenceStage::ReopenInit) {
        &carriers[..1]
    } else {
        &carriers[..]
    };
    super::enrollment::census(&canonical, expected)?;
    accept_check(&anchor, &evidence.journal_check(&anchor)?)?;
    let permit = LiveSqlPermit(super::owner::SqlPermit::enter()?);
    let mut owner = match stage {
        EvidenceStage::Selected | EvidenceStage::BeforeSelector => {
            ReaderScope::prepare(evidence)?.0
        }
        _ => WriterScope::prepare(evidence)?.0,
    };
    owner.context.as_mut().expect("prepared context").anchor = Some(anchor);
    // Explicit finish precedes permit release and every post-SQL path operation,
    // including failures. Unknown ownership is terminal in the binary policy.
    #[cfg(test)]
    hook("before-owned-open")?;
    let result = owner
        .open_native(&permit)
        .and_then(|()| owner.with_connection(&permit, execute_owned_sql));
    let finished = owner.finish();
    drop(permit);
    let finished = finished?;
    if finished
        .disposition
        .as_ref()
        .is_some_and(|record| record.shared_roles != 0)
    {
        private_worker_terminal();
    }
    finished.evidence().validate_live()?;
    let outcome = result?;
    accept_check(
        &outcome.anchor,
        &finished.evidence().journal_check(&outcome.anchor)?,
    )?;
    let carriers = finished.evidence().inputs().carriers();
    let expected = if matches!(stage, EvidenceStage::CreateInit | EvidenceStage::ReopenInit) {
        &carriers[..1]
    } else {
        &carriers[..]
    };
    super::enrollment::census(&canonical, expected)?;
    finished.evidence().validate_live()?;
    drop(finished);
    if outcome.already_ready {
        run_owned(root, guard, request, actor, EvidenceStage::ReadyResume)?;
    }
    Ok(())
}
