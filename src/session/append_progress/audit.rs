//! Invocation-local cooperative work accounting for the private explicit audit.
//! The scope is deliberately !Send: native ownership and every metered helper
//! execute on the same disposable-owner thread. It grants no routing authority.
use std::cell::{Cell, RefCell};
use std::fs::File;
use std::io::{Read, Seek};
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::error::{Result, ShoreError};

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AuditLimits {
    pub visits: u64,
    pub bytes: u64,
    pub milliseconds: u64,
}
impl Default for AuditLimits {
    fn default() -> Self {
        Self {
            visits: 100_000,
            bytes: 268_435_456,
            milliseconds: 60_000,
        }
    }
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Totals {
    pub visits: u64,
    pub bytes: u64,
}
struct Budget {
    limits: AuditLimits,
    deadline: Cell<Instant>,
    cancel: Arc<AtomicBool>,
    totals: Cell<Totals>,
    retirement_bytes: Cell<u64>,
    retiring: Cell<bool>,
}
thread_local! { static ACTIVE: RefCell<Option<Rc<Budget>>> = const { RefCell::new(None) }; }
pub(super) struct Scope(Rc<Budget>);
fn refused(reason: &str) -> ShoreError {
    ShoreError::Message(format!("private audit refused: {reason}"))
}
impl Scope {
    #[cfg(test)]
    pub(super) fn enter(limits: AuditLimits, cancel: Arc<AtomicBool>) -> Result<Self> {
        Self::enter_at(limits, cancel, Instant::now())
    }
    pub(super) fn enter_at(
        limits: AuditLimits,
        cancel: Arc<AtomicBool>,
        started_at: Instant,
    ) -> Result<Self> {
        let maximum = AuditLimits::default();
        if limits.visits == 0
            || limits.visits > maximum.visits
            || limits.bytes == 0
            || limits.bytes > maximum.bytes
            || limits.milliseconds == 0
            || limits.milliseconds > maximum.milliseconds
        {
            return Err(refused("invalid limits"));
        }
        let budget = Rc::new(Budget {
            limits,
            cancel,
            totals: Cell::new(Totals::default()),
            retirement_bytes: Cell::new(0),
            retiring: Cell::new(false),
            deadline: Cell::new(started_at + Duration::from_millis(limits.milliseconds)),
        });
        budget.check()?;
        ACTIVE.with(|active| {
            let mut active = active.borrow_mut();
            if active.is_some() {
                return Err(refused("overlapping audit budget"));
            }
            *active = Some(budget.clone());
            Ok(Self(budget))
        })
    }
    pub(super) fn deadline(&self) -> Instant {
        self.0.deadline.get()
    }
    pub(super) fn totals(&self) -> Totals {
        self.0.totals.get()
    }
}
impl Drop for Scope {
    fn drop(&mut self) {
        ACTIVE.with(|active| {
            active.borrow_mut().take();
        });
    }
}
fn with_budget<T>(f: impl FnOnce(Option<&Budget>) -> Result<T>) -> Result<T> {
    ACTIVE.with(|active| f(active.borrow().as_deref()))
}
impl Budget {
    fn check(&self) -> Result<()> {
        // Mandatory native retirement still proves actual identities/bytes.
        // Cancellation cannot revoke the already-reserved work needed to close.
        if self.retiring.get() {
            return Ok(());
        }
        if self.cancel.load(Ordering::Acquire) {
            return Err(refused("cancelled"));
        }
        if Instant::now() >= self.deadline.get() {
            return Err(refused("deadline exhausted"));
        }
        Ok(())
    }
    fn charge(&self, visits: u64, bytes: u64) -> Result<()> {
        self.check()?;
        let old = self.totals.get();
        let next = Totals {
            visits: old
                .visits
                .checked_add(visits)
                .ok_or_else(|| refused("visit overflow"))?,
            bytes: old
                .bytes
                .checked_add(bytes)
                .ok_or_else(|| refused("byte overflow"))?,
        };
        let byte_ceiling = if self.retiring.get() {
            self.limits.bytes
        } else {
            self.limits
                .bytes
                .saturating_sub(self.retirement_bytes.get())
        };
        if next.visits > self.limits.visits || next.bytes > byte_ceiling {
            return Err(refused("work limit exhausted"));
        }
        self.totals.set(next);
        if self.retiring.get() {
            self.retirement_bytes
                .set(self.retirement_bytes.get().saturating_sub(bytes));
        }
        Ok(())
    }
}
pub(super) fn check() -> Result<()> {
    with_budget(|b| b.map_or(Ok(()), Budget::check))
}
pub(super) fn visit() -> Result<()> {
    with_budget(|b| b.map_or(Ok(()), |b| b.charge(1, 0)))
}
pub(super) fn bytes(count: u64) -> Result<()> {
    with_budget(|b| b.map_or(Ok(()), |b| b.charge(0, count)))
}
pub(super) fn active() -> bool {
    ACTIVE.with(|b| b.borrow().is_some())
}
/// Charge borrowed SQLite values before any owned Rust field allocation.
/// Numeric fields have an explicit eight-byte logical representation; NULL zero.
pub(super) fn row(row: &rusqlite::Row<'_>) -> rusqlite::Result<()> {
    if !active() {
        return Ok(());
    }
    let charge = (|| -> Result<()> {
        visit()?;
        let mut length = 0u64;
        for index in 0..row.as_ref().column_count() {
            let value = row.get_ref(index).map_err(|e| refused(&e.to_string()))?;
            let n = match value {
                rusqlite::types::ValueRef::Null => 0,
                rusqlite::types::ValueRef::Integer(_) | rusqlite::types::ValueRef::Real(_) => 8,
                rusqlite::types::ValueRef::Text(v) | rusqlite::types::ValueRef::Blob(v) => {
                    v.len() as u64
                }
            };
            length = length
                .checked_add(n)
                .ok_or_else(|| refused("SQL byte overflow"))?;
        }
        bytes(length)
    })();
    charge.map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))
}
/// Read only the captured length, in bounded chunks. Metadata checks prove EOF
/// without consuming a hidden extra byte. Caller retains/rechecks name identity.
pub(super) fn read(file: &File, length: u64) -> Result<Vec<u8>> {
    let io = |e: std::io::Error| refused(&e.to_string());
    let before = file.metadata().map_err(io)?;
    if !before.is_file() || before.len() != length {
        return Err(refused("read length changed"));
    }
    let mut file = file;
    file.rewind().map_err(io)?;
    let mut result = Vec::new();
    let mut remaining = length;
    let mut chunk = [0u8; 16_384];
    while remaining != 0 {
        check()?;
        let available = with_budget(|b| {
            Ok(b.map_or(u64::MAX, |b| {
                b.limits
                    .bytes
                    .saturating_sub(b.totals.get().bytes)
                    .saturating_sub(if b.retiring.get() {
                        0
                    } else {
                        b.retirement_bytes.get()
                    })
            }))
        })?;
        if available == 0 {
            return Err(refused("byte limit exhausted during read"));
        }
        let want = remaining.min(available).min(chunk.len() as u64) as usize;
        let n = file.read(&mut chunk[..want]).map_err(io)?;
        bytes(n as u64)?;
        if n == 0 {
            return Err(refused("short read"));
        }
        result.extend_from_slice(&chunk[..n]);
        remaining -= n as u64;
    }
    let after = file.metadata().map_err(io)?;
    if after.len() != length || before.modified().map_err(io)? != after.modified().map_err(io)? {
        return Err(refused("file changed during read"));
    }
    check()?;
    Ok(result)
}

#[cfg(test)]
pub(super) fn expire_for_test() {
    ACTIVE.with(|b| {
        b.borrow()
            .as_ref()
            .expect("active audit")
            .deadline
            .set(Instant::now())
    });
}
#[cfg(test)]
pub(super) fn totals_for_test() -> Totals {
    ACTIVE.with(|b| b.borrow().as_ref().expect("active audit").totals.get())
}

/// Five closed native roles, each retired at most once in the WAL-only C4
/// scope, each checking retained bytes at both observation endpoints. Reserve
/// before opening SQL; normal audit work cannot spend these bytes. Actual reads
/// still charge the invocation, and unused reservation is released at finish.
pub(super) struct RetirementReserve(Option<Rc<Budget>>);
pub(super) fn reserve_retirement(bytes: u64) -> Result<RetirementReserve> {
    ACTIVE.with(|active| {
        let budget = active.borrow().clone();
        if let Some(b) = &budget {
            b.check()?;
            if b.retirement_bytes.get() != 0
                || bytes > b.limits.bytes.saturating_sub(b.totals.get().bytes)
            {
                return Err(refused("insufficient bytes for checked native finish"));
            }
            b.retirement_bytes.set(bytes);
        }
        Ok(RetirementReserve(budget))
    })
}
impl Drop for RetirementReserve {
    fn drop(&mut self) {
        if let Some(b) = &self.0 {
            b.retirement_bytes.set(0);
        }
    }
}
pub(super) struct Retirement(Option<Rc<Budget>>);
pub(super) fn retirement() -> Retirement {
    ACTIVE.with(|active| {
        let budget = active.borrow().clone();
        if let Some(b) = &budget {
            b.retiring.set(true);
        }
        Retirement(budget)
    })
}
impl Drop for Retirement {
    fn drop(&mut self) {
        if let Some(b) = &self.0 {
            b.retiring.set(false);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn scope(visits: u64, bytes: u64) -> Scope {
        Scope::enter(
            AuditLimits {
                visits,
                bytes,
                milliseconds: 60_000,
            },
            Arc::new(AtomicBool::new(false)),
        )
        .unwrap()
    }
    #[test]
    fn audit_arithmetic_overflow_refuses_without_counter_wrap() {
        let scope = scope(100_000, 268_435_456);
        scope.0.totals.set(Totals {
            visits: u64::MAX,
            bytes: 0,
        });
        assert!(visit().unwrap_err().to_string().contains("overflow"));
        assert_eq!(scope.totals().visits, u64::MAX);
        scope.0.totals.set(Totals {
            visits: 0,
            bytes: u64::MAX,
        });
        assert!(bytes(1).unwrap_err().to_string().contains("overflow"));
    }
    #[test]
    fn audit_record_cap_charges_repeated_visits() {
        let scope = scope(2, 100);
        visit().unwrap();
        visit().unwrap();
        assert_eq!(scope.totals().visits, 2);
        assert!(visit().is_err());
        assert_eq!(scope.totals().visits, 2);
    }
    #[test]
    fn audit_byte_cap_charges_repeated_sql_values_before_allocation() {
        let db = rusqlite::Connection::open_in_memory().unwrap();
        let scope = scope(10, 6);
        let read = || {
            db.query_row("SELECT 'abc'", [], |r| {
                row(r)?;
                r.get::<_, String>(0)
            })
        };
        assert_eq!(read().unwrap(), "abc");
        assert_eq!(read().unwrap(), "abc");
        assert_eq!(scope.totals().bytes, 6);
        assert!(read().is_err());
        assert_eq!(scope.totals().bytes, 6);
        drop(scope);
        db.execute_batch("PRAGMA synchronous=FULL; PRAGMA foreign_keys=ON;")
            .unwrap();
        let scope = self::scope(2, 16);
        assert_eq!(
            super::super::ledger::configuration_scalar(&db, "synchronous").unwrap(),
            2
        );
        assert_eq!(
            super::super::ledger::configuration_scalar(&db, "foreign_keys").unwrap(),
            1
        );
        assert_eq!(
            scope.totals(),
            Totals {
                visits: 2,
                bytes: 16
            }
        );
        assert!(super::super::ledger::configuration_scalar(&db, "synchronous").is_err());
        assert_eq!(
            scope.totals(),
            Totals {
                visits: 2,
                bytes: 16
            }
        );
    }
    #[test]
    fn audit_cancellation_refuses_at_work_boundary() {
        let cancelled = Arc::new(AtomicBool::new(false));
        let scope = Scope::enter(AuditLimits::default(), cancelled.clone()).unwrap();
        let reserve = reserve_retirement(32).unwrap();
        visit().unwrap();
        cancelled.store(true, Ordering::Release);
        assert!(bytes(1).unwrap_err().to_string().contains("cancelled"));
        assert_eq!(
            scope.totals(),
            Totals {
                visits: 1,
                bytes: 0
            }
        );
        {
            let _retirement = retirement();
            bytes(32).unwrap();
        }
        drop(reserve);
        assert_eq!(scope.totals().bytes, 32);
        assert!(check().is_err(), "retirement must not clear cancellation");
    }
    #[test]
    fn audit_deadline_refuses_before_work() {
        let before_bootstrap = Instant::now() - Duration::from_millis(2);
        assert!(
            Scope::enter_at(
                AuditLimits {
                    milliseconds: 1,
                    ..AuditLimits::default()
                },
                Arc::new(AtomicBool::new(false)),
                before_bootstrap,
            )
            .is_err()
        );
        assert!(!active(), "exhausted bootstrap must not install a scope");
        let budget = Budget {
            limits: AuditLimits::default(),
            deadline: Cell::new(Instant::now()),
            cancel: Arc::new(AtomicBool::new(false)),
            totals: Cell::new(Totals::default()),
            retirement_bytes: Cell::new(0),
            retiring: Cell::new(false),
        };
        assert!(
            budget
                .charge(1, 0)
                .unwrap_err()
                .to_string()
                .contains("deadline")
        );
        assert_eq!(budget.totals.get(), Totals::default());
    }
    #[test]
    fn audit_limits_and_scope_do_not_enroll_ordinary_work() {
        assert!(!active());
        visit().unwrap();
        bytes(u64::MAX).unwrap();
        for limits in [
            AuditLimits {
                visits: 0,
                ..AuditLimits::default()
            },
            AuditLimits {
                bytes: 268_435_457,
                ..AuditLimits::default()
            },
            AuditLimits {
                milliseconds: 60_001,
                ..AuditLimits::default()
            },
        ] {
            assert!(Scope::enter(limits, Arc::new(AtomicBool::new(false))).is_err());
            assert!(!active());
        }
        {
            let _scope = scope(1, 1);
            assert!(active());
        }
        assert!(!active());
    }
}
