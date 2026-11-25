use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use super::*;

#[test]
fn exact_role_grammar_before_normalization() {
    // SAFETY: this native control requires its own disposable test process.
    let _owner = unsafe { crate::session::append_progress::owner::isolated_test_owner() };
    let fixed = [
        (Role::Authority, "authority.writer.lock"),
        (Role::Progress, "append-progress"),
        (Role::Enrollment, "enrollment"),
        (Role::Incarnations, "incarnations"),
        (Role::Events, "events"),
        (Role::Artifacts, "artifacts"),
        (Role::Notes, "notes"),
        (Role::Objects, "objects"),
        (Role::Active, "active.json"),
        (Role::Main, "ledger.sqlite"),
        (Role::Wal, "ledger.sqlite-wal"),
        (Role::Shm, "ledger.sqlite-shm"),
        (Role::Journal, "ledger.sqlite-journal"),
    ];
    for (role, name) in fixed {
        assert_eq!(role.leaf(OsStr::new(name)).unwrap(), name);
        for suffix in ["/", "/.", "\\", ":stream", ".", " ", "\0"] {
            assert!(role.leaf(OsStr::new(&format!("{name}{suffix}"))).is_err());
        }
        for (other, other_name) in fixed {
            if name != other_name {
                assert!(other.leaf(OsStr::new(name)).is_err());
            }
        }
    }
    let token = "a9".repeat(32);
    assert!(Role::Incarnation.leaf(OsStr::new(&token)).is_ok());
    for role in [Role::Request, Role::Carrier] {
        assert!(role.leaf(OsStr::new(&format!("{token}.json"))).is_ok());
        assert!(role.leaf(OsStr::new(&token)).is_err());
    }
    for bad in [
        "",
        ".",
        "..",
        "CON",
        "NUL",
        "C:",
        "\\\\?\\x",
        "*",
        "é",
        "../events",
    ] {
        assert!(Role::Events.leaf(OsStr::new(bad)).is_err());
        assert!(Role::Incarnation.leaf(OsStr::new(bad)).is_err());
    }
    for bad in [
        "A".repeat(64),
        "a".repeat(63),
        "a".repeat(65),
        "g".repeat(64),
    ] {
        assert!(Role::Incarnation.leaf(OsStr::new(&bad)).is_err());
    }
    #[cfg(target_os = "macos")]
    {
        use std::os::unix::ffi::OsStrExt as _;
        assert!(Role::Events.leaf(OsStr::from_bytes(b"events\xff")).is_err());
    }
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStringExt as _;
        assert!(
            Role::Events
                .leaf(&std::ffi::OsString::from_wide(&[0xd800]))
                .is_err()
        );
    }
}

fn parent(root: &Path) -> File {
    #[cfg(target_os = "macos")]
    {
        File::open(root).unwrap()
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt as _;
        std::fs::OpenOptions::new()
            .read(true)
            .share_mode(7)
            .custom_flags(0x02000000)
            .open(root)
            .unwrap()
    }
}

fn sample(parent: &File, name: &str, role: Role) -> Result<Option<BorrowedMetadata>> {
    assert_eq!(
        std::env::var("POINTBREAK_NAMESPACE_CHILD").as_deref(),
        Ok("worker")
    );
    // SAFETY: only the explicitly isolated diagnostic worker reaches this path;
    // one synchronous query at a time, valid parent retained by each test case.
    unsafe { observe_private_worker(parent, OsStr::new(name), role) }
}

fn child(root: &Path, mode: &str) {
    let mut process = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "session::append_progress::native::namespace::tests::namespace_worker",
            "--ignored",
            "--nocapture",
        ])
        .env("POINTBREAK_NAMESPACE_CHILD", mode)
        .env("POINTBREAK_NAMESPACE_ROOT", root)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(if mode == "worker" { 120 } else { 10 });
    while process.try_wait().unwrap().is_none() {
        if Instant::now() >= deadline {
            process.kill().unwrap();
            let output = process.wait_with_output().unwrap();
            panic!("private child deadline; retained root {root:?}; {output:?}");
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    let output = process.wait_with_output().unwrap();
    assert!(output.status.success(), "{output:?}");
    let marker = format!("namespace child completed: {mode}");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(
        stderr.lines().filter(|line| *line == marker).count(),
        1,
        "{output:?}"
    );
}

#[test]
#[ignore = "native diagnostic requires separate qualification authorization"]
fn namespace_diagnostic_in_private_worker() {
    // SAFETY: this native control requires its own disposable test process.
    let _owner = unsafe { crate::session::append_progress::owner::isolated_test_owner() };
    let root = tempfile::tempdir().unwrap().keep();
    eprintln!("retained namespace diagnostic root: {root:?}");
    child(&root, "worker");
}

#[test]
#[ignore = "entry only for the isolated diagnostic child"]
fn namespace_worker() {
    // SAFETY: this native control requires its own disposable test process.
    let _owner = unsafe { crate::session::append_progress::owner::isolated_test_owner() };
    let mode = std::env::var("POINTBREAK_NAMESPACE_CHILD").expect("private child only");
    let root = std::path::PathBuf::from(std::env::var_os("POINTBREAK_NAMESPACE_ROOT").unwrap());
    if mode != "worker" {
        let state = mode.strip_prefix("main-").unwrap_or(&mode);
        assert!(matches!(state, "held" | "released"));
        let sql = rusqlite::Connection::open(root.join("ledger.sqlite")).unwrap();
        sql.busy_timeout(Duration::ZERO).unwrap();
        let outcome = sql.execute_batch(if mode.starts_with("main-") {
            "BEGIN EXCLUSIVE; ROLLBACK;"
        } else {
            "BEGIN IMMEDIATE; ROLLBACK;"
        });
        if state == "held" {
            assert!(
                matches!(outcome, Err(rusqlite::Error::SqliteFailure(error, _))
                if error.code == rusqlite::ErrorCode::DatabaseBusy)
            );
        } else {
            outcome.unwrap();
        }
    } else {
        namespace_cases(&root);
        stock_sqlite_locks(&root);
    }
    eprintln!("namespace child completed: {mode}");
}

fn namespace_cases(root: &Path) {
    let dir = parent(root);
    assert_eq!(sample(&dir, "active.json", Role::Active).unwrap(), None);
    std::fs::write(root.join("active.json"), b"one").unwrap();
    let first = sample(&dir, "active.json", Role::Active).unwrap().unwrap();
    std::fs::rename(root.join("active.json"), root.join("retained-first")).unwrap();
    std::fs::write(root.join("active.json"), b"two").unwrap();
    assert_ne!(
        first.identity,
        sample(&dir, "active.json", Role::Active)
            .unwrap()
            .unwrap()
            .identity
    );
    std::fs::remove_file(root.join("active.json")).unwrap();
    assert_eq!(sample(&dir, "active.json", Role::Active).unwrap(), None);
    for (role, name) in [
        (Role::Artifacts, "artifacts"),
        (Role::Notes, "notes"),
        (Role::Objects, "objects"),
    ] {
        std::fs::create_dir(root.join(name)).unwrap();
        assert!(sample(&dir, name, role).unwrap().unwrap().directory);
        std::fs::remove_dir(root.join(name)).unwrap();
        std::fs::write(root.join(name), b"wrong kind").unwrap();
        assert!(sample(&dir, name, role).is_err());
    }
    std::fs::create_dir(root.join("active.json")).unwrap();
    assert!(sample(&dir, "active.json", Role::Active).is_err());
    std::fs::remove_dir(root.join("active.json")).unwrap();
    #[cfg(target_os = "macos")]
    std::os::unix::fs::symlink(root.join("retained-first"), root.join("active.json")).unwrap();
    #[cfg(windows)]
    std::os::windows::fs::symlink_file(root.join("retained-first"), root.join("active.json"))
        .unwrap();
    assert!(sample(&dir, "active.json", Role::Active).is_err());
    std::fs::remove_file(root.join("active.json")).unwrap();
    let invalid_parent = File::open(root.join("retained-first")).unwrap();
    // Raw rejection precedes any attempt to use this non-directory parent.
    assert!(
        sample(&invalid_parent, "events/.", Role::Events)
            .unwrap_err()
            .to_string()
            .contains("invalid generated")
    );
    assert!(sample(&invalid_parent, "active.json", Role::Active).is_err());
}

fn stock_sqlite_locks(root: &Path) {
    let dir = parent(root);
    // Stock owners only: rollback read + EXCLUSIVE probes the main shared lock;
    // WAL write + IMMEDIATE independently probes the real SHM writer lock.
    let sql = rusqlite::Connection::open(root.join("ledger.sqlite")).unwrap();
    sql.execute_batch(
        "PRAGMA journal_mode=DELETE; CREATE TABLE t(value); INSERT INTO t VALUES(1);",
    )
    .unwrap();
    std::fs::hard_link(root.join("ledger.sqlite"), root.join("active.json")).unwrap();
    std::fs::hard_link(root.join("ledger.sqlite"), root.join("events")).unwrap();
    for wal in [false, true] {
        sql.execute_batch(if wal {
            "PRAGMA journal_mode=WAL; BEGIN IMMEDIATE; INSERT INTO t VALUES(2);"
        } else {
            "BEGIN; SELECT * FROM t;"
        })
        .unwrap();
        let mode: String = sql
            .query_row("PRAGMA journal_mode", [], |row| row.get(0))
            .unwrap();
        assert_eq!(mode, if wal { "wal" } else { "delete" });
        let blocked = || child(root, if wal { "held" } else { "main-held" });
        blocked();
        let main = sample(&dir, "ledger.sqlite", Role::Main).unwrap().unwrap();
        blocked();
        if wal {
            for (role, name) in [
                (Role::Wal, "ledger.sqlite-wal"),
                (Role::Shm, "ledger.sqlite-shm"),
            ] {
                assert!(!sample(&dir, name, role).unwrap().unwrap().directory);
                blocked();
            }
        }
        let alias = sample(&dir, "active.json", Role::Active).unwrap().unwrap();
        assert_eq!(main.identity, alias.identity); // caller must refuse this N/Q alias
        blocked();
        assert!(sample(&dir, "events", Role::Events).is_err()); // wrong-kind alias closes
        blocked();
        assert_eq!(
            sample(&dir, "ledger.sqlite-journal", Role::Journal).unwrap(),
            None
        );
        sql.execute_batch("ROLLBACK;").unwrap();
        child(root, if wal { "released" } else { "main-released" });
    }
    drop(sql);
}
