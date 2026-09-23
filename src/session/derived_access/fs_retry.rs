//! Bounded retry for derived-root renames that a transient Windows file
//! holder can block.
//!
//! Windows refuses to rename a directory while any process holds a file
//! inside it without delete sharing: an antivirus scan of a database that was
//! just written, or a derived read in flight on another thread. Those holders
//! let go on their own, so the rename is retried briefly before it is reported.

use std::io;
use std::path::Path;
use std::time::Duration;

#[derive(Clone, Copy, Debug)]
struct SharingRetryPolicy {
    attempts: u32,
    initial: Duration,
    max: Duration,
}

impl SharingRetryPolicy {
    /// 8 attempts, 10 ms doubling, capped at 500 ms: about 1.1 s in total.
    const RENAME: Self = Self {
        attempts: 8,
        initial: Duration::from_millis(10),
        max: Duration::from_millis(500),
    };
}

/// Raw OS error 5 (ERROR_ACCESS_DENIED) or 32 (ERROR_SHARING_VIOLATION), on Windows only.
pub(crate) fn is_transient_sharing_violation(error: &io::Error) -> bool {
    cfg!(windows) && matches!(error.raw_os_error(), Some(5 | 32))
}

fn retry_io<T>(
    policy: SharingRetryPolicy,
    transient: impl Fn(&io::Error) -> bool,
    mut sleep: impl FnMut(Duration),
    mut op: impl FnMut() -> io::Result<T>,
) -> io::Result<T> {
    let mut delay = policy.initial;
    let mut attempt = 1;
    loop {
        match op() {
            Err(error) if attempt < policy.attempts && transient(&error) => {
                sleep(delay);
                delay = delay.saturating_mul(2).min(policy.max);
                attempt += 1;
            }
            outcome => return outcome,
        }
    }
}

pub(crate) fn rename_with_sharing_retry(from: &Path, to: &Path) -> io::Result<()> {
    retry_io(
        SharingRetryPolicy::RENAME,
        is_transient_sharing_violation,
        std::thread::sleep,
        || std::fs::rename(from, to),
    )
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;

    use super::*;

    fn sharing_violation() -> io::Error {
        io::Error::from_raw_os_error(32)
    }

    #[test]
    fn retry_returns_first_success_after_transient_failures() {
        let calls = RefCell::new(0_u32);
        let mut delays = Vec::new();

        let outcome = retry_io(
            SharingRetryPolicy::RENAME,
            |_| true,
            |delay| delays.push(delay),
            || {
                *calls.borrow_mut() += 1;
                if *calls.borrow() < 3 {
                    Err(sharing_violation())
                } else {
                    Ok(())
                }
            },
        );

        assert!(outcome.is_ok(), "{outcome:?}");
        assert_eq!(*calls.borrow(), 3);
        assert_eq!(
            delays,
            [Duration::from_millis(10), Duration::from_millis(20)]
        );
    }

    #[test]
    fn retry_stops_after_budget_and_returns_last_error() {
        let mut calls = 0_u32;
        let mut delays = Vec::new();

        let error = retry_io(
            SharingRetryPolicy::RENAME,
            |_| true,
            |delay| delays.push(delay),
            || -> io::Result<()> {
                calls += 1;
                Err(io::Error::other(format!("attempt {calls}")))
            },
        )
        .expect_err("every attempt fails");

        assert_eq!(calls, SharingRetryPolicy::RENAME.attempts);
        assert_eq!(
            delays,
            [10, 20, 40, 80, 160, 320, 500].map(Duration::from_millis)
        );
        assert_eq!(
            error.to_string(),
            format!("attempt {}", SharingRetryPolicy::RENAME.attempts)
        );
    }

    #[test]
    fn non_transient_error_is_not_retried() {
        let mut calls = 0_u32;
        let mut delays = Vec::new();

        let error = retry_io(
            SharingRetryPolicy::RENAME,
            |_| false,
            |delay| delays.push(delay),
            || -> io::Result<()> {
                calls += 1;
                Err(sharing_violation())
            },
        )
        .expect_err("a non-transient failure is returned");

        assert_eq!(calls, 1);
        assert!(delays.is_empty(), "{delays:?}");
        assert_eq!(error.raw_os_error(), Some(32));
    }

    #[cfg(windows)]
    #[test]
    fn sharing_violation_codes_are_transient() {
        assert!(is_transient_sharing_violation(
            &io::Error::from_raw_os_error(5)
        ));
        assert!(is_transient_sharing_violation(
            &io::Error::from_raw_os_error(32)
        ));
        assert!(!is_transient_sharing_violation(
            &io::Error::from_raw_os_error(2)
        ));
    }

    #[cfg(not(windows))]
    #[test]
    fn sharing_violation_codes_are_never_transient_off_windows() {
        // On Unix, 32 is EPIPE.
        assert!(!is_transient_sharing_violation(&sharing_violation()));
    }

    #[cfg(windows)]
    #[test]
    fn directory_rename_retry_outlasts_a_transient_child_handle() {
        use std::os::windows::fs::OpenOptionsExt;

        use crate::test_timing::spawn_bounded;

        let temp = tempfile::TempDir::new().unwrap();
        let from = temp.path().join("root");
        let to = temp.path().join("moved");
        std::fs::create_dir(&from).unwrap();
        let child = from.join("cursor.sqlite3");
        std::fs::write(&child, b"held").unwrap();
        let holder = std::fs::OpenOptions::new()
            .read(true)
            .share_mode(1 | 2) // FILE_SHARE_READ | FILE_SHARE_WRITE, without FILE_SHARE_DELETE
            .open(&child)
            .unwrap();

        let blocked = std::fs::rename(&from, &to)
            .expect_err("a held child file blocks renaming its directory");
        assert!(
            matches!(blocked.raw_os_error(), Some(5 | 32)),
            "unexpected rename error while the child is held: {blocked:?}"
        );

        let release = spawn_bounded(move || {
            std::thread::sleep(Duration::from_millis(100));
            drop(holder);
        });
        let renamed = rename_with_sharing_retry(&from, &to);
        release.wait("the child handle release");

        renamed.expect("the retried rename outlasts the transient holder");
        assert!(!from.exists());
        assert_eq!(std::fs::read(to.join("cursor.sqlite3")).unwrap(), b"held");
    }
}
