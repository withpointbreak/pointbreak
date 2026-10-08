//! Synchronous actual-native-handle admission; this module never owns a Q handle.
use std::cell::{Cell, RefCell};

use libsqlite3_sys as ffi;

use super::enrollment::RetainedEvidence;
use super::{native, owner};
use crate::error::{Result, ShoreError};

fn refused() -> ShoreError {
    ShoreError::Message("actual native handle/evidence binding refused".into())
}
#[cfg(any(target_os = "macos", windows))]
struct Object {
    handle: u64,
    observation: native::BorrowedMetadata,
    #[cfg(windows)]
    descriptor: native::private_acl::Descriptor,
}
pub(super) struct Gate {
    generation: Cell<u64>,
    #[cfg(any(target_os = "macos", windows))]
    objects: RefCell<[Option<Object>; 6]>,
}
impl Gate {
    pub(super) fn new() -> Self {
        Self {
            generation: Cell::new(0),
            #[cfg(any(target_os = "macos", windows))]
            objects: RefCell::new(std::array::from_fn(|_| None)),
        }
    }
    pub(super) fn is_empty(&self) -> bool {
        self.objects
            .try_borrow()
            .is_ok_and(|objects| objects.iter().all(Option::is_none))
    }
    pub(super) fn observe(
        &self,
        evidence: &RetainedEvidence<'_>,
        record: &ffi::sqlite3_pb_native_v1,
    ) -> Result<()> {
        owner::require_owned()?;
        #[cfg(any(target_os = "macos", windows))]
        {
            use native::namespace::{Role, observe_private_worker};
            let (role, leaf) = match (evidence.projection_target(), record.role) {
                (true, ffi::SQLITE_PB_ROLE_MAIN) => (Role::ProjectionMain, "cursor.sqlite3"),
                (true, ffi::SQLITE_PB_ROLE_WAL) => (Role::ProjectionWal, "cursor.sqlite3-wal"),
                (true, ffi::SQLITE_PB_ROLE_SHM_MAPPING | ffi::SQLITE_PB_ROLE_SHM_LOCK) => {
                    (Role::ProjectionShm, "cursor.sqlite3-shm")
                }
                (true, ffi::SQLITE_PB_ROLE_MAIN_JOURNAL) => {
                    (Role::ProjectionJournal, "cursor.sqlite3-journal")
                }
                (false, ffi::SQLITE_PB_ROLE_MAIN) => (Role::Main, "ledger.sqlite"),
                (false, ffi::SQLITE_PB_ROLE_WAL) => (Role::Wal, "ledger.sqlite-wal"),
                (false, ffi::SQLITE_PB_ROLE_SHM_MAPPING | ffi::SQLITE_PB_ROLE_SHM_LOCK) => {
                    (Role::Shm, "ledger.sqlite-shm")
                }
                (false, ffi::SQLITE_PB_ROLE_MAIN_JOURNAL) => {
                    (Role::Journal, "ledger.sqlite-journal")
                }
                _ => return Err(refused()),
            };
            evidence.validate_live()?;
            #[cfg(target_os = "macos")]
            let observation = {
                let fd = i32::try_from(record.handle).map_err(|_| refused())?;
                // SAFETY: pinned SQLite calls synchronously with its owned live fd.
                // The borrow is lexical; never File::from_raw_fd, dup, or close.
                native::borrowed_private_mode(
                    unsafe { std::os::fd::BorrowedFd::borrow_raw(fd) },
                    false,
                )?
            };
            #[cfg(windows)]
            let (observation, descriptor) = {
                let raw = usize::try_from(record.handle).map_err(|_| refused())?;
                if raw == 0 || raw == usize::MAX {
                    return Err(refused());
                }
                let path = evidence.database()?.with_file_name(leaf);
                // SAFETY: pinned native callback retains this exact HANDLE.
                native::windows_acl::borrowed_private_descriptor(
                    unsafe { std::os::windows::io::BorrowedHandle::borrow_raw(raw as *mut _) },
                    &path,
                    native::private_acl::Kind::File,
                )?
            };
            if record.role == ffi::SQLITE_PB_ROLE_MAIN {
                evidence.validate_main_identity(observation.identity)?;
            }
            if evidence
                .members()
                .iter()
                .any(|m| m.identity() == Some(observation.identity))
                || evidence
                    .root()
                    .conflicts_with_ancestor(observation.identity)?
                || evidence.conflicts_with_journal(observation.identity)?
            {
                return Err(refused());
            }
            // SAFETY: the private owner and retained incarnation cover this query.
            let named =
                unsafe { observe_private_worker(evidence.native_parent()?, leaf.as_ref(), role) }?;
            if named != Some(observation) {
                return Err(refused());
            }
            let mut objects = self.objects.try_borrow_mut().map_err(|_| refused())?;
            for (index, other) in objects.iter().enumerate() {
                let Some(other) = other else {
                    continue;
                };
                if index == record.role as usize {
                    continue;
                }
                let shm_pair = matches!(index, 3 | 4) && matches!(record.role, 3 | 4);
                if (other.observation.identity == observation.identity
                    || other.handle == record.handle)
                    && !shm_pair
                {
                    return Err(refused());
                }
                if shm_pair && other.observation.identity != observation.identity {
                    return Err(refused());
                }
            }
            let slot = &mut objects[record.role as usize];
            match record.phase {
                ffi::SQLITE_PB_PHASE_ADMIT => {
                    if slot.is_some()
                        || self.generation.get().checked_add(1) != Some(record.generation)
                    {
                        return Err(refused());
                    }
                    self.generation.set(record.generation);
                    *slot = Some(Object {
                        handle: record.handle,
                        observation,
                        #[cfg(windows)]
                        descriptor,
                    });
                }
                ffi::SQLITE_PB_PHASE_VISIT | ffi::SQLITE_PB_PHASE_RETIRE => {
                    let held = slot.as_ref().ok_or_else(refused)?;
                    if record.phase == ffi::SQLITE_PB_PHASE_VISIT
                        && record.role == ffi::SQLITE_PB_ROLE_MAIN
                        && self.generation.get().checked_add(1) == Some(record.generation)
                    {
                        self.generation.set(record.generation);
                    }
                    if self.generation.get() != record.generation
                        || held.handle != record.handle
                        || held.observation != observation
                    {
                        return Err(refused());
                    }
                    #[cfg(windows)]
                    if held.descriptor != descriptor {
                        return Err(refused());
                    }
                    if record.phase == ffi::SQLITE_PB_PHASE_RETIRE {
                        *slot = None;
                    }
                }
                _ => return Err(refused()),
            }
            drop(objects);
            evidence.validate_live()
        }
        #[cfg(not(any(target_os = "macos", windows)))]
        {
            let _ = (evidence, record);
            Err(refused())
        }
    }
}
