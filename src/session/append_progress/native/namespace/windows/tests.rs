use std::sync::atomic::{AtomicUsize, Ordering};

use super::*;

#[test]
fn native_c_layouts() {
    // SAFETY: this native control requires its own disposable test process.
    let _owner = unsafe { crate::session::append_progress::owner::isolated_test_owner() };
    use std::mem::{align_of, offset_of, size_of};
    let wide = size_of::<usize>() == 8;
    assert_eq!(size_of::<UnicodeString>(), if wide { 16 } else { 8 });
    assert_eq!(offset_of!(UnicodeString, buffer), if wide { 8 } else { 4 });
    assert_eq!(size_of::<ObjectAttributes>(), if wide { 48 } else { 24 });
    for (actual, expected) in [
        (offset_of!(ObjectAttributes, root), if wide { 8 } else { 4 }),
        (
            offset_of!(ObjectAttributes, name),
            if wide { 16 } else { 8 },
        ),
        (
            offset_of!(ObjectAttributes, attributes),
            if wide { 24 } else { 12 },
        ),
        (
            offset_of!(ObjectAttributes, security),
            if wide { 32 } else { 16 },
        ),
        (
            offset_of!(ObjectAttributes, qos),
            if wide { 40 } else { 20 },
        ),
    ] {
        assert_eq!(actual, expected);
    }
    assert_eq!(size_of::<IoStatusBlock>(), 2 * size_of::<usize>());
    assert_eq!(offset_of!(IoStatusBlock, information), size_of::<usize>());
    assert_eq!(align_of::<StatusOrPointer>(), align_of::<usize>());
    assert_eq!(align_of::<ObjectAttributes>(), align_of::<usize>());
}

#[test]
fn only_exact_native_missing_status_is_absence() {
    // SAFETY: this native control requires its own disposable test process.
    let _owner = unsafe { crate::session::append_progress::owner::isolated_test_owner() };
    assert!(matches!(
        acquired(0xc0000034u32 as i32, ptr::null_mut()),
        Ok(None)
    ));
    for status in [
        0xc000003a_u32,
        0xc0000022,
        0xc0000043,
        0xc0000056,
        0xc0000008,
        0xc0000033,
        0xc00000bb,
    ] {
        assert!(acquired(status as i32, ptr::null_mut()).is_err());
    }
}

static CLOSES: AtomicUsize = AtomicUsize::new(0);
unsafe extern "system" fn confirmed(_: Handle) -> i32 {
    CLOSES.fetch_add(1, Ordering::SeqCst);
    1
}
unsafe extern "system" fn failed(_: Handle) -> i32 {
    if CLOSES.fetch_add(1, Ordering::SeqCst) != 0 {
        std::process::abort(); // A second close must not look like terminal90.
    }
    0
}
fn fake(close: unsafe extern "system" fn(Handle) -> i32) -> Query {
    Query {
        raw: Some(std::ptr::dangling_mut::<u8>().cast()),
        close,
    }
}

#[test]
fn first_error_and_single_close_including_unwind() {
    // SAFETY: this native control requires its own disposable test process.
    let _owner = unsafe { crate::session::append_progress::owner::isolated_test_owner() };
    CLOSES.store(0, Ordering::SeqCst);
    let error = fake(confirmed)
        .finish(Err(ShoreError::Message("first failure".into())))
        .unwrap_err();
    assert_eq!(error.to_string(), "first failure");
    assert_eq!(CLOSES.load(Ordering::SeqCst), 1);
    let unwind = std::panic::catch_unwind(|| {
        let _query = fake(confirmed);
        panic!("query failed during unwind");
    });
    assert!(unwind.is_err());
    assert_eq!(CLOSES.load(Ordering::SeqCst), 2);
}

#[test]
#[ignore = "private child only; never invoke in a shared process"]
fn unknown_cleanup_worker() {
    // SAFETY: this native control requires its own disposable test process.
    let _owner = unsafe { crate::session::append_progress::owner::isolated_test_owner() };
    match std::env::var("POINTBREAK_QUERY_FAULT").unwrap().as_str() {
        "close" => {
            let _ = fake(failed).finish(Err(ShoreError::Message("first sentinel".into())));
        }
        "pending" => {
            let _ = acquired(0x103, ptr::null_mut());
        }
        "ambiguous" => {
            let _ = acquired(0xc0000034_u32 as i32, std::ptr::dangling_mut::<u8>().cast());
        }
        "invalid" => {
            let _ = acquired(0, ptr::null_mut());
        }
        _ => panic!("unknown fault"),
    }
    panic!("unknown cleanup returned");
}

#[test]
#[ignore = "fault subprocesses require separate diagnostic authorization"]
fn unknown_cleanup_is_terminal_before_return() {
    // SAFETY: this native control requires its own disposable test process.
    let _owner = unsafe { crate::session::append_progress::owner::isolated_test_owner() };
    for fault in ["close", "pending", "ambiguous", "invalid"] {
        let mut child = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "session::append_progress::native::namespace::windows::tests::unknown_cleanup_worker", "--ignored", "--nocapture"])
            .env("POINTBREAK_QUERY_FAULT", fault)
            .stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::piped())
            .spawn().unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while child.try_wait().unwrap().is_none() {
            if std::time::Instant::now() >= deadline {
                child.kill().unwrap();
                let output = child.wait_with_output().unwrap();
                panic!("fault child timed out: {output:?}");
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        let output = child.wait_with_output().unwrap();
        assert_eq!(output.status.code(), Some(90), "{output:?}");
        if fault == "close" {
            assert!(String::from_utf8_lossy(&output.stderr).contains("first sentinel"));
        }
    }
}
