#![cfg(test)]

use super::test_fixture::{enter_child, run_child};
use super::{native, test_root};

mod enrollment_controls {
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::time::{Duration, Instant};

    use enrollment::{EvidenceRole as N, EvidenceStage as Stage, RetainedEvidence};
    use serde_json::{Value, json};

    use super::super::{enrollment, ledger};
    use super::native::private_acl::{
        Kind, SYSTEM, TokenFailure, absent_thread_token, fault, parse, token_user,
    };
    use super::{enter_child, native, run_child, test_root};
    use crate::canonical_hash::{canonical_json_bytes, sha256_bytes_hex};
    use crate::crypto::TestEd25519Signer;
    use crate::error::{Result, ShoreError};
    use crate::session::event::Writer;
    use crate::session::store::authority_lock::StoreAuthorityLock;
    use crate::session::store::backend::{Journal, LocalJournal};
    use crate::session::store::capabilities;

    const USER: &[u8] = &[1, 2, 0, 0, 0, 0, 0, 5, 21, 0, 0, 0, 232, 3, 0, 0];
    const OTHER: &[u8] = &[1, 2, 0, 0, 0, 0, 0, 5, 21, 0, 0, 0, 233, 3, 0, 0];
    const REQUEST: &str = "1111111111111111111111111111111111111111111111111111111111111111";
    const PROFILE: &str = "review_change_revision_progress_v1";
    const TIME: &str = "2026-09-18T00:00:00Z";

    fn guarded<T>(
        path: &Path,
        run: impl FnOnce(&native::NativeRootHandle, &StoreAuthorityLock) -> T,
    ) -> T {
        super::super::owner::require_quiescent().unwrap();
        let root = native::physical_root(path).unwrap();
        let guard =
            StoreAuthorityLock::acquire_until(path, Instant::now() + Duration::from_secs(5), None)
                .unwrap();
        run(&root, &guard)
    }
    fn enroll(path: &Path, seed: u8) -> Result<()> {
        guarded(path, |root, guard| {
            enrollment::enroll(
                root,
                guard,
                REQUEST,
                true,
                PROFILE,
                &TestEd25519Signer::from_seed([seed; 32]),
                Writer::shore_local("test"),
                TIME,
            )
        })
    }
    fn read_selected(path: &Path) -> Result<()> {
        guarded(path, |root, guard| {
            enrollment::selected(
                root,
                guard,
                REQUEST,
                Writer::shore_local("test").actor_id.as_str(),
            )
        })
    }
    fn stopped(path: &Path, stage: &'static str) {
        let result = enrollment::with_boundary(
            move |at| {
                if at == stage {
                    Err(ShoreError::Message(format!("stop {stage}")))
                } else {
                    Ok(())
                }
            },
            || enroll(path, 71),
        );
        assert_eq!(result.unwrap_err().to_string(), format!("stop {stage}"));
    }
    fn plan_path(root: &Path) -> PathBuf {
        root.join("append-progress/enrollment")
            .join(format!("{REQUEST}.json"))
    }
    fn plan(root: &Path) -> Value {
        serde_json::from_slice(&fs::read(plan_path(root)).unwrap()).unwrap()
    }
    fn database(root: &Path) -> PathBuf {
        root.join("append-progress/incarnations")
            .join(plan(root)["incarnationId"].as_str().unwrap())
            .join("ledger.sqlite")
    }
    fn hex_bytes(value: &str) -> Vec<u8> {
        (0..value.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&value[i..i + 2], 16).unwrap())
            .collect()
    }
    fn carrier(root: &Path, completion: bool) -> PathBuf {
        let p = plan(root);
        let key = p[if completion {
            "completionKey"
        } else {
            "activationKey"
        }]
        .as_str()
        .unwrap();
        root.join("events")
            .join(format!("{}.json", sha256_bytes_hex(key.as_bytes())))
    }
    fn rewrite(path: &Path, value: &Value) {
        let previous = fs::read(path).unwrap();
        println!(
            "negative mutation {} previous={}",
            path.display(),
            String::from_utf8_lossy(&previous)
        );
        fs::write(path, canonical_json_bytes(value).unwrap()).unwrap();
    }
    fn inputs(root: &Path) -> ledger::InitialInputs {
        let p = plan(root);
        ledger::InitialInputs::from_signed(
            p["incarnationId"].as_str().unwrap().into(),
            p["progressEpoch"].as_str().unwrap().into(),
            p["rootWitness"].clone(),
            hex_bytes(p["activationBytesHex"].as_str().unwrap()),
            hex_bytes(p["completionBytesHex"].as_str().unwrap()),
        )
        .unwrap()
    }
    fn retained<'a>(
        root: &'a native::NativeRootHandle,
        guard: &'a StoreAuthorityLock,
        stage: Stage,
    ) -> Result<RetainedEvidence<'a>> {
        RetainedEvidence::capture_before_sql(
            root,
            guard,
            stage,
            REQUEST,
            Writer::shore_local("test").actor_id.as_str(),
        )
    }
    fn snapshot(path: &Path) -> std::collections::BTreeMap<PathBuf, Vec<u8>> {
        fn visit(path: &Path, out: &mut std::collections::BTreeMap<PathBuf, Vec<u8>>) {
            for item in fs::read_dir(path).unwrap() {
                let item = item.unwrap();
                if item.file_type().unwrap().is_dir() {
                    visit(&item.path(), out);
                } else {
                    out.insert(
                        item.path(),
                        fs::read(item.path()).unwrap_or_else(|error| {
                            panic!("could not snapshot {}: {error}", item.path().display())
                        }),
                    );
                }
            }
        }
        let mut out = std::collections::BTreeMap::new();
        visit(path, &mut out);
        out
    }
    fn oracle(root: &Path) -> Result<()> {
        let p = plan(root);
        let a = fs::read(carrier(root, false)).map_err(|e| ShoreError::Message(e.to_string()))?;
        let c = fs::read(carrier(root, true)).map_err(|e| ShoreError::Message(e.to_string()))?;
        capabilities::validate_progress_pair(&a, &c)?;
        assert_eq!(a, hex_bytes(p["activationBytesHex"].as_str().unwrap()));
        assert_eq!(c, hex_bytes(p["completionBytesHex"].as_str().unwrap()));
        assert_eq!(fs::read_dir(root.join("events")).unwrap().count(), 2);
        for content in ["artifacts/notes", "artifacts/objects"] {
            if root.join(content).exists() {
                assert_eq!(fs::read_dir(root.join(content)).unwrap().count(), 0);
            }
        }
        let a: Value = serde_json::from_slice(&a)?;
        let c: Value = serde_json::from_slice(&c)?;
        assert_eq!(a["minimumReaderProfile"], PROFILE);
        assert_eq!(a["bulkAdoptionManifest"]["reservedRecords"], json!([]));
        assert_eq!(
            a["bulkAdoptionManifest"]["sourceAuthorityCursor"]["eventCount"],
            0
        );
        assert_eq!(a["activationId"], c["activationId"]);
        let db = rusqlite::Connection::open_with_flags(
            database(root),
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
        )
        .unwrap();
        let mut persist = 1i32;
        // SAFETY: this independent quiescent oracle owns its stock connection;
        // preserve the fixture's WAL/SHM instead of deleting it on oracle close.
        assert_eq!(
            unsafe {
                libsqlite3_sys::sqlite3_file_control(
                    db.handle(),
                    c"main".as_ptr(),
                    libsqlite3_sys::SQLITE_FCNTL_PERSIST_WAL,
                    (&raw mut persist).cast(),
                )
            },
            libsqlite3_sys::SQLITE_OK
        );
        assert_eq!(
            db.query_row("SELECT state,head FROM meta", [], |r| Ok((
                r.get::<_, String>(0)?,
                r.get::<_, i64>(1)?
            )))
            .unwrap(),
            ("ready".into(), 0)
        );
        assert_eq!(
            db.query_row(
                "SELECT phase,original_obligation_json IS NULL,proof_json IS NULL FROM current",
                [],
                |r| Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, bool>(1)?,
                    r.get::<_, bool>(2)?
                ))
            )
            .unwrap(),
            ("INIT".into(), true, true)
        );
        for table in ["attempt", "receipt"] {
            assert_eq!(
                db.query_row(&format!("SELECT count(*) FROM {table}"), [], |r| r
                    .get::<_, i64>(0))
                    .unwrap(),
                0
            );
        }
        let mut rows = db
            .prepare(
                "SELECT key_digest,record_sha256,record_length FROM baseline ORDER BY key_digest",
            )
            .unwrap();
        let actual = rows
            .query_map([], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, i64>(2)?,
                ))
            })
            .unwrap()
            .collect::<rusqlite::Result<Vec<_>>>()
            .unwrap();
        let mut expected = [false, true]
            .into_iter()
            .map(|completion| {
                let raw = fs::read(carrier(root, completion)).unwrap();
                (
                    sha256_bytes_hex(
                        p[if completion {
                            "completionKey"
                        } else {
                            "activationKey"
                        }]
                        .as_str()
                        .unwrap()
                        .as_bytes(),
                    ),
                    format!("sha256:{}", sha256_bytes_hex(&raw)),
                    raw.len() as i64,
                )
            })
            .collect::<Vec<_>>();
        expected.sort();
        assert_eq!(actual, expected);
        assert_eq!(
            fs::read(root.join("append-progress/active.json")).unwrap(),
            canonical_json_bytes(&p["selection"]).unwrap()
        );
        Ok(())
    }

    #[test]
    fn enrollment_s01_request() {
        // SAFETY: this native control requires its own disposable test process.
        let _owner = unsafe { crate::session::append_progress::owner::isolated_test_owner() };
        for request in [
            "",
            "1",
            &"A".repeat(64),
            &"g".repeat(64),
            &format!("../{}", "1".repeat(61)),
            &"1".repeat(65),
        ] {
            assert!(enrollment::validate_request(request, true, PROFILE).is_err());
        }
        assert!(enrollment::validate_request(REQUEST, false, PROFILE).is_err());
        assert!(enrollment::validate_request(REQUEST, true, "review_change_revision_v1").is_err());
        enrollment::validate_request(REQUEST, true, PROFILE).unwrap();
    }
    #[cfg(any(target_os = "macos", windows))]
    #[cfg_attr(
        windows,
        ignore = "mandatory owner/DACL gate: just test-append-progress-native"
    )]
    #[test]
    fn enrollment_s02_empty() {
        // SAFETY: this native control requires its own disposable test process.
        let _owner = unsafe { crate::session::append_progress::owner::isolated_test_owner() };
        for kind in 0..6 {
            let root = test_root();
            match kind {
                0 => {
                    enroll(root.path(), 71).unwrap();
                    oracle(root.path()).unwrap();
                    continue;
                }
                1 => {
                    LocalJournal::new(root.path())
                        .create_record_once("event", b"foreign event")
                        .unwrap();
                }
                2 => {
                    fs::create_dir_all(root.path().join("artifacts/objects")).unwrap();
                    fs::write(root.path().join("artifacts/objects/body"), b"content").unwrap();
                }
                3 => fs::write(root.path().join("state.json"), b"{}").unwrap(),
                4 => {
                    fs::create_dir(root.path().join("events")).unwrap();
                    fs::write(root.path().join("events/unknown"), b"foreign").unwrap();
                }
                _ => capabilities::write_capability_fixture_for_test(
                    &LocalJournal::new(root.path()),
                    capabilities::CapabilityFixtureState::EmptyL2,
                )
                .unwrap(),
            }
            assert!(enroll(root.path(), 71).is_err());
            assert!(!plan_path(root.path()).exists());
        }
    }
    #[cfg(any(target_os = "macos", windows))]
    #[cfg_attr(
        windows,
        ignore = "mandatory owner/DACL gate: just test-append-progress-native"
    )]
    #[test]
    fn enrollment_s03_signed_plan() {
        // SAFETY: this native control requires its own disposable test process.
        let _owner = unsafe { crate::session::append_progress::owner::isolated_test_owner() };
        let root = test_root();
        stopped(root.path(), "E1");
        let before = fs::read(plan_path(root.path())).unwrap();
        let p = plan(root.path());
        assert_eq!(before, canonical_json_bytes(&p).unwrap());
        assert_ne!(p["incarnationId"], p["progressEpoch"]);
        capabilities::validate_progress_pair(
            &hex_bytes(p["activationBytesHex"].as_str().unwrap()),
            &hex_bytes(p["completionBytesHex"].as_str().unwrap()),
        )
        .unwrap();
        guarded(root.path(), |r, g| {
            // Signed bytes in the plan cannot replace the missing actual carrier.
            assert!(retained(r, g, Stage::CreateInit).is_err());
            let mut writer = Writer::shore_local("test");
            writer.actor_id = crate::model::ActorId::new("actor:other");
            assert!(
                enrollment::enroll(
                    r,
                    g,
                    REQUEST,
                    true,
                    PROFILE,
                    &TestEd25519Signer::from_seed([71; 32]),
                    writer,
                    TIME
                )
                .is_err()
            );
        });
        assert_eq!(before, fs::read(plan_path(root.path())).unwrap());
        enroll(root.path(), 71).unwrap();
    }
    #[cfg(any(target_os = "macos", windows))]
    #[cfg_attr(
        windows,
        ignore = "mandatory owner/DACL gate: just test-append-progress-native"
    )]
    #[test]
    fn enrollment_s04_resume_signer() {
        // SAFETY: this native control requires its own disposable test process.
        let _owner = unsafe { crate::session::append_progress::owner::isolated_test_owner() };
        let root = test_root();
        stopped(root.path(), "E1");
        let before = fs::read(plan_path(root.path())).unwrap();
        enroll(root.path(), 72).unwrap();
        assert_eq!(before, fs::read(plan_path(root.path())).unwrap());
        oracle(root.path()).unwrap();
    }
    #[cfg(any(target_os = "macos", windows))]
    #[cfg_attr(
        windows,
        ignore = "mandatory owner/DACL gate: just test-append-progress-native"
    )]
    #[test]
    fn enrollment_s05_e1_interleaving() {
        // SAFETY: this native control requires its own disposable test process.
        let _owner = unsafe { crate::session::append_progress::owner::isolated_test_owner() };
        let root = test_root();
        stopped(root.path(), "E1");
        LocalJournal::new(root.path())
            .create_record_once("old", b"intervening")
            .unwrap();
        assert!(enroll(root.path(), 71).is_err());
        assert!(!carrier(root.path(), false).exists());
    }
    #[cfg(any(target_os = "macos", windows))]
    #[cfg_attr(
        windows,
        ignore = "mandatory owner/DACL gate: just test-append-progress-native"
    )]
    #[test]
    fn enrollment_s06_activation() {
        // SAFETY: this native control requires its own disposable test process.
        let _owner = unsafe { crate::session::append_progress::owner::isolated_test_owner() };
        for kind in 0..3 {
            let root = test_root();
            stopped(root.path(), if kind == 2 { "E1" } else { "E2" });
            if kind == 1 {
                fs::write(carrier(root.path(), false), b"different activation").unwrap();
                guarded(root.path(), |r, g| {
                    assert!(retained(r, g, Stage::CreateInit).is_err());
                });
                assert!(enroll(root.path(), 71).is_err());
            } else {
                assert!(!database(root.path()).exists());
                guarded(root.path(), |r, g| {
                    if kind == 2 {
                        assert!(retained(r, g, Stage::CreateInit).is_err());
                        return;
                    }
                    let mut n = retained(r, g, Stage::CreateInit).unwrap();
                    assert_eq!(
                        n.members().iter().map(|m| m.role()).collect::<Vec<_>>(),
                        [
                            N::Root,
                            N::Authority,
                            N::Progress,
                            N::Enrollment,
                            N::Plan,
                            N::Incarnations,
                            N::Incarnation,
                            N::Events,
                            N::Artifacts,
                            N::Activation,
                            N::Completion,
                            N::Selector,
                        ]
                    );
                    // Empty enrollment retains the artifact namespace absence fence.
                    for role in [
                        N::Incarnations,
                        N::Incarnation,
                        N::Artifacts,
                        N::Completion,
                        N::Selector,
                    ] {
                        let member = n.members().iter().find(|m| m.role() == role).unwrap();
                        assert!(member.identity().is_none());
                    }
                    assert!(n.retain_created_directories_before_sql().is_err());
                    assert!(n.read_before_sql(N::Completion).is_err());
                    assert!(n.read_before_sql(N::Root).is_err());
                    assert_eq!(
                        n.read_before_sql(N::Plan).unwrap(),
                        fs::read(plan_path(root.path())).unwrap()
                    );
                    let actual = fs::read(carrier(root.path(), false)).unwrap();
                    assert_eq!(n.read_before_sql(N::Activation).unwrap(), actual);
                    assert_eq!(n.inputs().carriers()[0].1, actual);
                    #[cfg(unix)]
                    {
                        use std::os::fd::{AsFd as _, AsRawFd as _};
                        let members = n.members();
                        assert_eq!(
                            members[0].borrowed_fd().unwrap().as_raw_fd(),
                            native::retained_root_file(r).unwrap().as_fd().as_raw_fd()
                        );
                        assert_eq!(
                            members[1].borrowed_fd().unwrap().as_raw_fd(),
                            g.retained_file().as_fd().as_raw_fd()
                        );
                    }
                    #[cfg(windows)]
                    {
                        use std::os::windows::io::{AsHandle as _, AsRawHandle as _};
                        let members = n.members();
                        assert_eq!(
                            members[0].borrowed_handle().unwrap().as_raw_handle(),
                            native::retained_root_file(r)
                                .unwrap()
                                .as_handle()
                                .as_raw_handle()
                        );
                        assert_eq!(
                            members[1].borrowed_handle().unwrap().as_raw_handle(),
                            g.retained_file().as_handle().as_raw_handle()
                        );
                    }
                    let incarnation = n
                        .members()
                        .iter()
                        .find(|m| m.role() == N::Incarnation)
                        .unwrap()
                        .path()
                        .to_path_buf();
                    let mut dirs = fs::DirBuilder::new();
                    dirs.recursive(true);
                    #[cfg(unix)]
                    {
                        use std::os::unix::fs::DirBuilderExt as _;
                        dirs.mode(0o700);
                    }
                    dirs.create(incarnation).unwrap();
                    n.retain_created_directories_before_sql().unwrap();
                    for role in [N::Incarnations, N::Incarnation] {
                        let member = n.members().iter().find(|m| m.role() == role).unwrap();
                        assert!(member.identity().is_some());
                    }
                    assert!(!database(root.path()).exists());
                    ledger::owner_foundation_control(n);

                    // Legitimate directory setup is complete; capture a clean live baseline.
                    let n = retained(r, g, Stage::CreateInit).unwrap();
                    n.validate_live().unwrap();
                    let activation = n
                        .members()
                        .iter()
                        .find(|m| m.role() == N::Activation)
                        .unwrap();
                    let identity = activation.identity().unwrap();
                    let carrier_path = carrier(root.path(), false);
                    let moved = root.path().join("retained-activation");
                    let renamed = match fs::rename(&carrier_path, &moved) {
                        Ok(()) => true,
                        #[cfg(windows)]
                        Err(error) if error.raw_os_error() == Some(32) => false,
                        Err(error) => panic!("unexpected activation rename failure: {error}"),
                    };
                    if renamed {
                        // The actual borrowed owner survives; current authority does not.
                        #[cfg(target_os = "macos")]
                        assert_eq!(
                            native::borrowed_metadata(activation.borrowed_fd().unwrap())
                                .unwrap()
                                .identity,
                            identity
                        );
                        #[cfg(windows)]
                        assert_eq!(
                            native::borrowed_metadata(activation.borrowed_handle().unwrap())
                                .unwrap()
                                .identity,
                            identity
                        );
                        assert!(n.validate_live().is_err());
                        fs::rename(&moved, &carrier_path).unwrap();
                        println!(
                            "S06 activation rename observed; retained identity and live refusal checked"
                        );
                    } else {
                        n.validate_live().unwrap();
                        assert_eq!(n.read_before_sql(N::Activation).unwrap(), actual);
                        println!("S06 activation rename excluded by sharing; no rename coverage");
                    }
                    drop(n);

                    // Prior namespace movement must not satisfy the byte-mutation negative.
                    let n = retained(r, g, Stage::CreateInit).unwrap();
                    assert_eq!(n.read_before_sql(N::Activation).unwrap(), actual);
                    fs::write(&carrier_path, vec![b'x'; actual.len()]).unwrap();
                    assert!(n.read_before_sql(N::Activation).is_err());
                    fs::write(&carrier_path, &actual).unwrap();
                    drop(n);
                });
                if kind == 0 {
                    // Foundation setup retains a leaf that creation must not adopt.
                    let database = database(root.path());
                    assert!(database.parent().unwrap().is_dir());
                    assert!(!database.exists());
                    let before = snapshot(root.path());
                    assert!(enroll(root.path(), 71).is_err());
                    assert_eq!(before, snapshot(root.path()));
                    assert!(database.parent().unwrap().is_dir());
                    assert!(!database.exists());
                    // Prove clean E2 resume independently, retaining the refusal root.
                    let fresh = test_root();
                    stopped(fresh.path(), "E2");
                    enroll(fresh.path(), 71).unwrap();
                    oracle(fresh.path()).unwrap();
                } else {
                    enroll(root.path(), 71).unwrap();
                    oracle(root.path()).unwrap();
                }
            }
        }
    }
    #[cfg(any(target_os = "macos", windows))]
    #[cfg_attr(
        windows,
        ignore = "mandatory owner/DACL gate: just test-append-progress-native"
    )]
    #[test]
    fn enrollment_s07_init() {
        // SAFETY: this native control requires its own disposable test process.
        let _owner = unsafe { crate::session::append_progress::owner::isolated_test_owner() };
        let root = test_root();
        let (_, schema) = ledger::observe_initial_schema(|| stopped(root.path(), "E3"));
        assert!(
            schema.is_some(),
            "owned pre-DDL observation must be reached"
        );
        assert_eq!(schema.unwrap(), 0);
        assert!(!carrier(root.path(), true).exists());
        guarded(root.path(), |r, g| {
            let i = inputs(root.path());
            ledger::InitialLedger::reopen(r, g, &i).unwrap();
        });
        assert!(read_selected(root.path()).is_err());
        enroll(root.path(), 71).unwrap();
    }
    #[cfg(any(target_os = "macos", windows))]
    #[cfg_attr(
        windows,
        ignore = "mandatory owner/DACL gate: just test-append-progress-native"
    )]
    #[test]
    fn enrollment_s08_completion() {
        // SAFETY: this native control requires its own disposable test process.
        let _owner = unsafe { crate::session::append_progress::owner::isolated_test_owner() };
        for kind in 0..3 {
            let root = test_root();
            stopped(root.path(), "E4");
            let db = rusqlite::Connection::open_with_flags(
                database(root.path()),
                rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
            )
            .unwrap();
            let before: String = db
                .query_row("SELECT native_stamp FROM meta", [], |r| r.get(0))
                .unwrap();
            drop(db);
            guarded(root.path(), |r, g| {
                for stage in [Stage::Certify, Stage::ReadyResume] {
                    let mut n = retained(r, g, stage).unwrap();
                    assert_eq!(
                        n.read_before_sql(N::Completion).unwrap(),
                        fs::read(carrier(root.path(), true)).unwrap()
                    );
                    assert!(n.read_before_sql(N::Selector).is_err());
                    assert!(n.retain_created_directories_before_sql().is_err());
                    ledger::owner_foundation_control(n);
                }
                assert!(retained(r, g, Stage::ReopenInit).is_err());
                let i = inputs(root.path());
                assert!(ledger::InitialLedger::reopen(r, g, &i).is_err());
            });
            if kind > 0 {
                if kind == 1 {
                    fs::write(carrier(root.path(), true), b"invalid").unwrap();
                    guarded(root.path(), |r, g| {
                        assert!(retained(r, g, Stage::Certify).is_err());
                    });
                } else {
                    LocalJournal::new(root.path())
                        .create_record_once("foreign-completion", b"{}")
                        .unwrap();
                }
                assert!(enroll(root.path(), 71).is_err());
            } else {
                let db = rusqlite::Connection::open_with_flags(
                    database(root.path()),
                    rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
                )
                .unwrap();
                assert_eq!(
                    before,
                    db.query_row("SELECT native_stamp FROM meta", [], |r| r
                        .get::<_, String>(0))
                        .unwrap()
                );
                drop(db);
                enroll(root.path(), 71).unwrap();
                oracle(root.path()).unwrap();
            }
        }
    }
    #[cfg(any(target_os = "macos", windows))]
    #[cfg_attr(
        windows,
        ignore = "mandatory owner/DACL gate: just test-append-progress-native"
    )]
    #[test]
    fn enrollment_s09_certify() {
        // SAFETY: this native control requires its own disposable test process.
        let _owner = unsafe { crate::session::append_progress::owner::isolated_test_owner() };
        let root = test_root();
        let path = root.path().to_path_buf();
        let seen = std::rc::Rc::new(std::cell::Cell::new(false));
        let observed = seen.clone();
        enrollment::with_boundary(
            move |at| {
                if at == "E4" {
                    let db = database(&path);
                    for suffix in ["-wal", "-shm"] {
                        assert!(PathBuf::from(format!("{}{suffix}", db.display())).is_file());
                    }
                    observed.set(true);
                }
                if at == "E5-before-update" {
                    let db = database(&path);
                    for suffix in ["-wal", "-shm"] {
                        assert!(PathBuf::from(format!("{}{suffix}", db.display())).is_file());
                    }
                }
                Ok(())
            },
            || enroll(root.path(), 71),
        )
        .expect("semantic Red until signed E5/E6 is connected");
        assert!(seen.get());
        oracle(root.path()).unwrap();
        let before = snapshot(root.path());
        read_selected(root.path()).unwrap();
        assert_eq!(before, snapshot(root.path()));
    }
    #[cfg(any(target_os = "macos", windows))]
    #[cfg_attr(
        windows,
        ignore = "mandatory owner/DACL gate: just test-append-progress-native"
    )]
    #[test]
    fn enrollment_s26_reader_bootstrap_transaction() {
        // SAFETY: this native control owns its disposable test process.
        let _owner = unsafe { super::super::owner::isolated_test_owner() };
        let root = test_root();
        enroll(root.path(), 71).unwrap();
        let before = snapshot(root.path());
        guarded(root.path(), |r, g| {
            ledger::reader_transaction_control(retained(r, g, Stage::Selected).unwrap(), false);
        });
        assert_eq!(before, snapshot(root.path()));
    }
    #[cfg(any(target_os = "macos", windows))]
    #[cfg_attr(
        windows,
        ignore = "mandatory owner/DACL gate: just test-append-progress-native"
    )]
    #[test]
    fn enrollment_s27_reader_after_commit_refused() {
        // SAFETY: this native control owns its disposable test process.
        let _owner = unsafe { super::super::owner::isolated_test_owner() };
        let root = test_root();
        enroll(root.path(), 71).unwrap();
        let before = snapshot(root.path());
        guarded(root.path(), |r, g| {
            ledger::reader_transaction_control(retained(r, g, Stage::Selected).unwrap(), true);
        });
        assert_eq!(before, snapshot(root.path()));
    }
    #[cfg(any(target_os = "macos", windows))]
    #[cfg_attr(
        windows,
        ignore = "mandatory owner/DACL gate: just test-append-progress-native"
    )]
    #[test]
    fn enrollment_s10_selector() {
        // SAFETY: this native control requires its own disposable test process.
        let _owner = unsafe { crate::session::append_progress::owner::isolated_test_owner() };
        for kind in 0..4 {
            let root = test_root();
            stopped(root.path(), if kind == 3 { "E4" } else { "E5" });
            let selector = root.path().join("append-progress/active.json");
            assert!(!selector.exists());
            match kind {
                0 => {
                    enroll(root.path(), 71).unwrap();
                    let before = fs::read(&selector).unwrap();
                    enroll(root.path(), 71).unwrap();
                    assert_eq!(before, fs::read(&selector).unwrap());
                }
                1 | 2 => {
                    let bytes = if kind == 1 {
                        b"{}".as_slice()
                    } else {
                        b"{".as_slice()
                    };
                    fs::write(&selector, bytes).unwrap();
                    #[cfg(unix)]
                    {
                        use std::os::unix::fs::PermissionsExt as _;
                        fs::set_permissions(&selector, fs::Permissions::from_mode(0o600)).unwrap();
                    }
                    assert_eq!(
                        enroll(root.path(), 71).unwrap_err().to_string(),
                        "progress-initialization-required: retained evidence changed during read"
                    );
                    assert_eq!(fs::read(&selector).unwrap(), bytes);
                }
                _ => {
                    assert!(read_selected(root.path()).is_err());
                    assert!(!selector.exists());
                }
            }
        }
    }
    #[cfg(any(target_os = "macos", windows))]
    #[cfg_attr(
        windows,
        ignore = "mandatory owner/DACL gate: just test-append-progress-native"
    )]
    #[test]
    fn enrollment_s11_selected_read() {
        // SAFETY: this native control requires its own disposable test process.
        let _owner = unsafe { crate::session::append_progress::owner::isolated_test_owner() };
        for kind in 0..6 {
            let root = test_root();
            if kind == 1 {
                stopped(root.path(), "E3");
            } else {
                enroll(root.path(), 71).unwrap();
            }
            match kind {
                2 => {
                    fs::rename(database(root.path()), root.path().join("retained-ledger")).unwrap()
                }
                3 => fs::write(database(root.path()), b"partial").unwrap(),
                4 => {
                    let db = rusqlite::Connection::open(database(root.path())).unwrap();
                    db.execute_batch("UPDATE meta SET epoch=printf('%064d',9)")
                        .unwrap();
                }
                5 => fs::write(root.path().join("append-progress/active.json"), b"{").unwrap(),
                _ => {}
            }
            let before = snapshot(root.path());
            guarded(root.path(), |r, g| {
                if kind == 1 {
                    let n = retained(r, g, Stage::ReopenInit).unwrap();
                    assert!(n.read_before_sql(N::Completion).is_err());
                    assert!(retained(r, g, Stage::Selected).is_err());
                    ledger::owner_foundation_control(n);
                } else if kind == 0 {
                    for stage in [Stage::Selected, Stage::Certify, Stage::ReadyResume] {
                        let n = retained(r, g, stage).unwrap();
                        assert_eq!(
                            n.read_before_sql(N::Selector).unwrap(),
                            fs::read(root.path().join("append-progress/active.json")).unwrap()
                        );
                        ledger::owner_foundation_control(n);
                    }
                } else if kind == 5 {
                    assert!(retained(r, g, Stage::Selected).is_err());
                }
            });
            let result = read_selected(root.path());
            assert_eq!(result.is_ok(), kind == 0);
            assert_eq!(before, snapshot(root.path()));
        }
    }
    #[cfg(any(target_os = "macos", windows))]
    #[cfg_attr(
        windows,
        ignore = "mandatory owner/DACL gate: just test-append-progress-native"
    )]
    #[test]
    fn enrollment_s12_missing_plan() {
        // SAFETY: this native control requires its own disposable test process.
        let _owner = unsafe { crate::session::append_progress::owner::isolated_test_owner() };
        for stage in ["E1", "E2", "E4", "E6"] {
            let root = test_root();
            stopped(root.path(), stage);
            fs::rename(
                plan_path(root.path()),
                root.path().join("lost-plan-retained"),
            )
            .unwrap();
            let before = snapshot(root.path());
            guarded(root.path(), |r, g| {
                assert!(retained(r, g, Stage::CreateInit).is_err());
            });
            assert!(enroll(root.path(), 71).is_err());
            assert_eq!(before, snapshot(root.path()));
        }
    }
    #[cfg(any(target_os = "macos", windows))]
    #[cfg_attr(
        windows,
        ignore = "mandatory owner/DACL gate: just test-append-progress-native"
    )]
    #[test]
    fn enrollment_s13_signature() {
        // SAFETY: this native control requires its own disposable test process.
        let _owner = unsafe { crate::session::append_progress::owner::isolated_test_owner() };
        for completion in [false, true] {
            for kind in 0..5 {
                let root = test_root();
                stopped(root.path(), "E1");
                let mut p = plan(root.path());
                let field = if completion {
                    "completionBytesHex"
                } else {
                    "activationBytesHex"
                };
                let mut record: Value =
                    serde_json::from_slice(&hex_bytes(p[field].as_str().unwrap())).unwrap();
                match kind {
                    0 => record["signature"]["sig"] = json!("invalid"),
                    1 => {
                        p[if completion {
                            "completionKey"
                        } else {
                            "activationKey"
                        }] = json!("wrong:key")
                    }
                    2 => record["minimumReaderProfile"] = json!("foreign"),
                    3 => record["writer"]["actorId"] = json!("actor:other"),
                    _ => {
                        if completion {
                            record["activationId"] = json!("capability-activation:sha256:invalid")
                        } else {
                            record["bulkAdoptionManifestHash"] = json!("sha256:invalid")
                        }
                    }
                }
                p[field] = json!(
                    canonical_json_bytes(&record)
                        .unwrap()
                        .iter()
                        .map(|b| format!("{b:02x}"))
                        .collect::<String>()
                );
                rewrite(&plan_path(root.path()), &p);
                let a = hex_bytes(p["activationBytesHex"].as_str().unwrap());
                let c = hex_bytes(p["completionBytesHex"].as_str().unwrap());
                if kind != 1 {
                    assert!(capabilities::validate_progress_pair(&a, &c).is_err());
                }
                assert!(enroll(root.path(), 71).is_err());
                assert!(!root.path().join("events").exists());
            }
        }
    }
    #[cfg(any(target_os = "macos", windows))]
    #[cfg_attr(
        windows,
        ignore = "mandatory owner/DACL gate: just test-append-progress-native"
    )]
    #[test]
    fn enrollment_s14_extra_authority() {
        // SAFETY: this native control requires its own disposable test process.
        let _owner = unsafe { crate::session::append_progress::owner::isolated_test_owner() };
        for kind in 0..6 {
            let root = test_root();
            stopped(root.path(), if kind == 5 { "E6" } else { "E4" });
            match kind {
                0 | 5 => {
                    LocalJournal::new(root.path())
                        .create_record_once("extra-event", b"extra")
                        .unwrap();
                }
                1 => {
                    fs::create_dir_all(root.path().join("artifacts/objects")).unwrap();
                    fs::write(root.path().join("artifacts/objects/content"), b"content").unwrap();
                }
                2 => fs::write(root.path().join("events/unknown"), b"unknown").unwrap(),
                _ => fs::write(carrier(root.path(), kind == 4), b"changed").unwrap(),
            }
            assert!(
                if kind == 5 {
                    read_selected(root.path())
                } else {
                    enroll(root.path(), 71)
                }
                .is_err()
            );
        }
    }
    fn process_child() -> bool {
        let Ok(mode) = std::env::var("POINTBREAK_ENROLLMENT_CHILD") else {
            return false;
        };
        let root = PathBuf::from(std::env::var_os("POINTBREAK_ENROLLMENT_CHILD_ROOT").unwrap());
        let current = std::thread::current();
        let name = current.name().unwrap();
        assert_eq!(std::env::var("POINTBREAK_ENROLLMENT_CASE").unwrap(), name);
        enter_child(&root, &mode);
        match mode.as_str() {
            "cut" => {
                enroll(&root, 71).unwrap();
                panic!("requested cut was not reached");
            }
            "resume" => {
                enroll(&root, 72).unwrap();
                oracle(&root).unwrap();
            }
            "read" => {
                let report =
                    |label: &str,
                     left: &std::collections::BTreeMap<PathBuf, Vec<u8>>,
                     right: &std::collections::BTreeMap<PathBuf, Vec<u8>>| {
                        let paths: std::collections::BTreeSet<_> =
                            left.keys().chain(right.keys()).collect();
                        let changed: Vec<_> = paths
                            .into_iter()
                            .filter(|p| left.get(*p) != right.get(*p))
                            .collect();
                        println!(
                            "S22 {label} equal={} changed={}",
                            left == right,
                            changed.len()
                        );
                        for path in changed.iter().take(64) {
                            let summary = |bytes: Option<&Vec<u8>>| {
                                bytes.map(|b| {
                                    let shm = (path.file_name().unwrap() == "ledger.sqlite-shm"
                                        && b.len() >= 132)
                                        .then(|| {
                                            [8usize, 16, 20, 40, 44, 56, 64, 68, 88, 92, 96, 128]
                                                .map(|at| {
                                                    u32::from_ne_bytes(
                                                        b[at..at + 4].try_into().unwrap(),
                                                    )
                                                })
                                        });
                                    json!({"length": b.len(), "sha256": sha256_bytes_hex(b),
                                    "shm_offsets_8_16_20_40_44_56_64_68_88_92_96_128": shm})
                                })
                            };
                            println!(
                                "S22 {label} {}",
                                json!({
                                    "path": path.strip_prefix(&root).unwrap().to_string_lossy(),
                                    "before": summary(left.get(*path)),
                                    "after": summary(right.get(*path))
                                })
                            );
                        }
                        assert!(changed.len() <= 64, "S22 diagnostic path bound exceeded");
                    };
                let before = snapshot(&root); // T0: before the real Selected boundary.
                read_selected(&root).unwrap();
                let selected = snapshot(&root); // T1: native finish has returned.
                report("T0-T1-selected", &before, &selected);
                assert!(before == selected, "S22 Selected persistent mutation");
                oracle(&root).unwrap();
                let after = snapshot(&root); // T2: oracle statements/connection dropped.
                report("T1-T2-oracle", &selected, &after);
                report("T0-T2-combined-diagnostic", &before, &after);
                // The stock oracle may rewrite SHM; Selected's no-mutation check is above.
            }
            _ => panic!("unknown child mode"),
        }
        true
    }
    fn child(root: &Path, mode: &str, stage: Option<&str>) {
        let current = std::thread::current();
        let name = current.name().unwrap();
        let mut command = std::process::Command::new(std::env::current_exe().unwrap());
        command
            .env("POINTBREAK_ENROLLMENT_CHILD", mode)
            .env("POINTBREAK_ENROLLMENT_CHILD_ROOT", root)
            .env("POINTBREAK_ENROLLMENT_CASE", name);
        for key in [
            "POINTBREAK_CHECKPOINT_2_CHILD",
            "POINTBREAK_CHECKPOINT_2_ROOT",
            "POINTBREAK_INITIAL_LEDGER_CUT",
            "POINTBREAK_INITIAL_LEDGER_ROOT",
            "POINTBREAK_ENROLLMENT_CUT",
        ] {
            command.env_remove(key);
        }
        if let Some(stage) = stage {
            command.env("POINTBREAK_ENROLLMENT_CUT", stage);
        }
        let status = run_child(command, root, name, mode);
        assert_eq!(status.code(), Some(if mode == "cut" { 79 } else { 0 }));
    }
    #[cfg(any(target_os = "macos", windows))]
    #[cfg_attr(
        windows,
        ignore = "mandatory owner/DACL gate: just test-append-progress-native"
    )]
    #[test]
    fn enrollment_s15_boundary_cuts() {
        // SAFETY: this native control requires its own disposable test process.
        let _owner = unsafe { crate::session::append_progress::owner::isolated_test_owner() };
        if process_child() {
            return;
        }
        for stage in ["E1", "E2", "E3", "E4", "E5", "E6"] {
            let root = test_root();
            child(root.path(), "cut", Some(stage));
            child(root.path(), "resume", None);
            oracle(root.path()).unwrap();
        }
    }
    #[cfg(any(target_os = "macos", windows))]
    #[cfg_attr(
        windows,
        ignore = "mandatory owner/DACL gate: just test-append-progress-native"
    )]
    #[test]
    fn enrollment_s16_sql_uncertainty() {
        // SAFETY: this native control requires its own disposable test process.
        let _owner = unsafe { crate::session::append_progress::owner::isolated_test_owner() };
        if process_child() {
            return;
        }
        for stage in ["E5-before-update", "E5-before-commit", "E5-after-commit"] {
            let root = test_root();
            child(root.path(), "cut", Some(stage));
            let db = rusqlite::Connection::open_with_flags(
                database(root.path()),
                rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
            )
            .unwrap();
            let state: String = db
                .query_row("SELECT state FROM meta", [], |r| r.get(0))
                .unwrap();
            assert!(["initializing", "ready"].contains(&state.as_str()));
            drop(db);
            child(root.path(), "resume", None);
            oracle(root.path()).unwrap();
            if stage == "E5-before-update" {
                guarded(root.path(), |r, g| {
                    let canonical = native::root_snapshot(r).unwrap().canonical;
                    let wal = PathBuf::from(format!("{}-journal", database(&canonical).display()));
                    assert!(!wal.exists());
                    let gate = ledger::access(r, g, &[(wal.clone(), false)]).unwrap();
                    fs::write(&wal, b"unexpected endpoint movement").unwrap();
                    let error = gate.finish().unwrap_err().to_string();
                    assert!(error.contains("absent progress object moved"));
                });
            }
        }
    }
    #[cfg(any(target_os = "macos", windows))]
    #[cfg_attr(
        windows,
        ignore = "mandatory owner/DACL gate: just test-append-progress-native"
    )]
    #[test]
    fn enrollment_s17_barriers() {
        // SAFETY: this native control requires its own disposable test process.
        let _owner = unsafe { crate::session::append_progress::owner::isolated_test_owner() };
        for kind in 0..5 {
            let root = test_root();
            stopped(root.path(), "E4");
            let reached = std::rc::Rc::new(std::cell::Cell::new(false));
            let observed = reached.clone();
            let result = if kind == 0 {
                // L16 retains the distinct busy-checkpoint keeper using stock SQL.
                // A second private owner must fail before N/Q acquisition.
                let permit = super::super::owner::SqlPermit::enter().unwrap();
                let result = super::super::owner::require_quiescent();
                drop(permit);
                result
            } else if kind == 1 {
                ledger::with_hook(
                    move |at| {
                        if at == "ready-checkpoint" {
                            observed.set(true);
                            Err(ShoreError::Message("injected checkpoint error".into()))
                        } else {
                            Ok(())
                        }
                    },
                    || enroll(root.path(), 71),
                )
            } else {
                native::with_hook(
                    move |path, stage| {
                        let target = if kind == 4 {
                            "active.json"
                        } else {
                            "ledger.sqlite"
                        };
                        if path.file_name().is_some_and(|v| v == target)
                            && stage
                                == if kind == 3 {
                                    native::Barrier::Parent
                                } else {
                                    native::Barrier::File
                                }
                        {
                            observed.set(true);
                            Err(ShoreError::Message("injected enrollment barrier".into()))
                        } else {
                            Ok(())
                        }
                    },
                    || enroll(root.path(), 71),
                )
            };
            assert!(result.is_err(), "fault {kind} must refuse");
            if kind != 0 {
                assert!(reached.get(), "fault {kind} must be reached");
            }
            let expected = match kind {
                0 => "overlapping native owner refused before evidence acquisition",
                1 => "injected checkpoint error",
                _ => "injected enrollment barrier",
            };
            assert_eq!(result.unwrap_err().to_string(), expected);
            enroll(root.path(), 71).unwrap();
            oracle(root.path()).unwrap();
        }
    }
    #[cfg(any(target_os = "macos", windows))]
    #[cfg_attr(
        windows,
        ignore = "mandatory owner/DACL gate: just test-append-progress-native"
    )]
    #[test]
    fn enrollment_s18_rebinding() {
        // SAFETY: this native control requires its own disposable test process.
        let _owner = unsafe { crate::session::append_progress::owner::isolated_test_owner() };
        for kind in 0..5 {
            let root = test_root();
            let foreign = test_root();
            stopped(root.path(), "E5");
            let path = root.path().to_path_buf();
            let replacement = foreign.path().join("retained");
            let target = match kind {
                0 => path.clone(),
                1 => path.join("authority.writer.lock"),
                2 => plan_path(&path),
                3 => {
                    enroll(&path, 71).unwrap();
                    path.join("append-progress/active.json")
                }
                _ => database(&path),
            };
            let changed = std::rc::Rc::new(std::cell::Cell::new(false));
            let flag = changed.clone();
            let result = guarded(&path, |r, g| {
                let canonical = native::root_snapshot(r).unwrap().canonical;
                if kind == 0 {
                    // Keep the ParentDir component literal even for Windows verbatim paths.
                    let mut parent_dir = canonical.as_os_str().to_os_string();
                    parent_dir.push(std::path::MAIN_SEPARATOR_STR);
                    parent_dir.push("..");
                    let parent_dir = PathBuf::from(parent_dir);
                    assert!(
                        parent_dir
                            .components()
                            .any(|c| { matches!(c, std::path::Component::ParentDir) })
                    );
                    for outside in [foreign.path().canonicalize().unwrap(), parent_dir] {
                        let error = ledger::access(r, g, &[(outside, true)])
                            .err()
                            .expect("foreign and parent paths must be rejected");
                        assert!(error.to_string().contains("access path escaped root"));
                    }
                }
                // Access already retains and validates the authority lock.
                let check_paths = if kind == 1 {
                    Vec::new()
                } else {
                    let checked_target = canonical.join(target.strip_prefix(&path).unwrap());
                    vec![(checked_target, kind == 0)]
                };
                let check = ledger::access(r, g, &check_paths).unwrap();
                match fs::rename(&target, &replacement) {
                    Ok(()) => {
                        if kind == 0 {
                            fs::create_dir(&target).unwrap();
                        } else {
                            fs::copy(&replacement, &target).unwrap();
                        }
                        flag.set(true);
                    }
                    Err(e) => {
                        #[cfg(windows)]
                        assert_eq!(e.raw_os_error(), Some(32));
                        #[cfg(not(windows))]
                        panic!("unexpected rename failure: {e}");
                    }
                }
                check.finish()
            });
            assert_eq!(result.is_err(), changed.get());
            println!(
                "S18 subject {kind}: replacement={}, otherwise native exclusion",
                changed.get()
            );
        }
    }
    #[cfg(windows)]
    #[ignore = "diagnostic only: separately approved nonadmin owner-console sequence"]
    #[test]
    fn enrollment_s18_root_rename_diagnostic() {
        use std::os::windows::fs::OpenOptionsExt as _;
        use std::os::windows::io::AsHandle as _;
        type Diagnostic<T> = std::result::Result<T, Box<dyn std::error::Error>>;
        // SAFETY: the operator admits one isolated disposable diagnostic process.
        let _owner = unsafe { crate::session::append_progress::owner::isolated_test_owner() };
        println!("S18 diagnostic: inconclusive unless a terminal classification is emitted");
        let result = (|| -> Diagnostic<&str> {
            let root = test_root();
            let foreign = test_root();
            stopped(root.path(), "E5");
            super::super::owner::require_quiescent()?;
            let path = root.path();
            let replacement = foreign.path().join("retained");
            let absent = |p: &Path| -> Diagnostic<()> {
                match fs::symlink_metadata(p) {
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
                    other => Err(format!("expected absent {p:?}: {other:?}").into()),
                }
            };
            let facts = |p: &Path| -> Diagnostic<_> {
                let file = fs::OpenOptions::new()
                    .access_mode(0x0002_0080)
                    .share_mode(7)
                    .custom_flags(0x0020_0000 | 0x0200_0000)
                    .open(p)?;
                let facts = native::windows_acl::borrowed_private_descriptor(
                    file.as_handle(),
                    p,
                    Kind::Root,
                )?;
                println!("S18 facts {p:?}: {facts:?}; nonimpersonating TokenUser equals owner");
                Ok(facts) // Only detached values escape; the observation handle closes here.
            };
            let rename = |phase: &str, from: &Path, to: &Path| {
                let result = fs::rename(from, to);
                match &result {
                    Ok(()) => println!("S18 rename {phase}: {from:?} -> {to:?}: success"),
                    Err(e) => println!(
                        "S18 rename {phase}: {from:?} -> {to:?}: raw={:?} kind={:?} error={e}",
                        e.raw_os_error(),
                        e.kind()
                    ),
                }
                result
            };
            absent(&replacement)?;
            let original = facts(path)?;
            let destination_parent = facts(foreign.path())?;
            let unchanged = |at: &Path| -> Diagnostic<()> {
                if facts(at)? != original || facts(foreign.path())? != destination_parent {
                    return Err("identity, owner/DACL or token facts changed".into());
                }
                Ok(())
            };
            rename("before-out", path, &replacement)?;
            absent(path)?;
            unchanged(&replacement)?;
            rename("before-return", &replacement, path)?;
            absent(&replacement)?;
            unchanged(path)?;
            let denied = guarded(path, |r, g| -> Diagnostic<bool> {
                let canonical = native::root_snapshot(r)?.canonical;
                let check = ledger::access(r, g, &[(canonical, true)])?;
                unchanged(path)?;
                match rename("held", path, &replacement) {
                    Ok(()) => {
                        let finish = check.finish();
                        println!("S18 held rename succeeded; finish={finish:?}");
                        Ok(false)
                    }
                    Err(_) => {
                        unchanged(path)?;
                        absent(&replacement)?;
                        let finish = check.finish();
                        println!("S18 held rename denied; finish={finish:?}");
                        finish?;
                        Ok(true)
                    }
                }
            })?;
            // guarded and Access have released every owned guard before either branch.
            super::super::owner::require_quiescent()?;
            if !denied {
                absent(path)?;
                unchanged(&replacement)?;
                rename("unexpected-success-restore", &replacement, path)?;
                absent(&replacement)?;
                unchanged(path)?;
                return Ok("held-rename-succeeded; denial not reproduced; inspect finish outcome");
            }
            unchanged(path)?;
            absent(&replacement)?;
            rename("after-out", path, &replacement)?;
            absent(path)?;
            unchanged(&replacement)?;
            rename("after-return", &replacement, path)?;
            absent(&replacement)?;
            unchanged(path)?;
            Ok("denial associated with complete guard lifetime in this fixture only")
        })();
        match result {
            Ok(classification) => println!("S18 diagnostic classification: {classification}"),
            Err(error) => panic!("S18 diagnostic classification: inconclusive: {error}"),
        }
    }
    #[cfg(any(target_os = "macos", windows))]
    #[cfg_attr(
        windows,
        ignore = "mandatory owner/DACL gate: just test-append-progress-native"
    )]
    #[test]
    fn enrollment_s19_native_boundaries() {
        // SAFETY: this native control requires its own disposable test process.
        let _owner = unsafe { crate::session::append_progress::owner::isolated_test_owner() };
        let public = test_root();
        #[cfg(windows)]
        {
            native::physical_root(public.path()).unwrap();
            assert!(!public.path().join("events").exists());
        }
        #[cfg(not(windows))]
        {
            native::physical_root(public.path()).unwrap();
        }
        let private = test_root();
        enroll(private.path(), 71).unwrap();
        oracle(private.path()).unwrap();
    }
    #[cfg(target_os = "macos")]
    #[ignore = "mandatory predecessor gate: just test-append-progress-native"]
    #[test]
    fn enrollment_s20_predecessor_fence() {
        // SAFETY: this native control requires its own disposable test process.
        let _owner = unsafe { crate::session::append_progress::owner::isolated_test_owner() };
        #[cfg(target_os = "macos")]
        {
            let binary = PathBuf::from(
                std::env::var_os("POINTBREAK_ENROLLMENT_PREDECESSOR")
                    .expect("source-bound predecessor"),
            );
            let hash = std::env::var("POINTBREAK_ENROLLMENT_PREDECESSOR_SHA256").unwrap();
            assert_eq!(sha256_bytes_hex(&fs::read(&binary).unwrap()), hash);
            let git = PathBuf::from(
                std::env::var_os("POINTBREAK_ENROLLMENT_GIT").expect("pinned original git"),
            );
            assert!(git.is_absolute());
            assert_eq!(
                sha256_bytes_hex(&fs::read(&git).unwrap()),
                std::env::var("POINTBREAK_ENROLLMENT_GIT_SHA256").unwrap()
            );
            for kind in 0..5 {
                let root = test_root();
                let repo = root.path().join("repo");
                fs::create_dir(&repo).unwrap();
                let init = std::process::Command::new(&git)
                    .args(["init", "-q"])
                    .arg(&repo)
                    .env("GIT_CONFIG_NOSYSTEM", "1")
                    .env("GIT_CONFIG_GLOBAL", "/dev/null")
                    .output()
                    .unwrap();
                assert!(init.status.success());
                let store = repo.join(".git/pointbreak");
                fs::create_dir(&store).unwrap();
                if kind == 4 {
                    capabilities::write_capability_fixture_for_test(
                        &LocalJournal::new(&store),
                        capabilities::CapabilityFixtureState::EmptyL2,
                    )
                    .unwrap();
                } else {
                    stopped(&store, if kind < 2 { "E2" } else { "E6" });
                }
                let before = snapshot(&store);
                let mut command = std::process::Command::new(&binary);
                command
                    .args(["change", "create", "--repo"])
                    .arg(&repo)
                    .args([
                        "--operation-id",
                        "change-operation:checkpoint-enrollment-fence",
                        "--nonce",
                        REQUEST,
                    ])
                    .env("POINTBREAK_HOME", root.path().join("home"))
                    .env("POINTBREAK_SIGNING", "off")
                    .env_remove("POINTBREAK_SIGNING_KEY")
                    .env_remove("POINTBREAK_BACKEND")
                    .env("GIT_CONFIG_NOSYSTEM", "1")
                    .env("GIT_CONFIG_GLOBAL", "/dev/null");
                if kind % 2 == 1 {
                    command.env("POINTBREAK_DERIVED_ACCESS", "off");
                } else {
                    command.env_remove("POINTBREAK_DERIVED_ACCESS");
                }
                let output = command.output().unwrap();
                let stderr = String::from_utf8_lossy(&output.stderr);
                println!(
                    "S20 kind={kind} status={} stdout={} stderr={stderr}",
                    output.status,
                    String::from_utf8_lossy(&output.stdout)
                );
                if kind == 4 {
                    assert!(output.status.success(), "known old-profile command control");
                } else {
                    assert!(!output.status.success());
                    assert!(stderr.contains("unsupported") && stderr.contains("capability"));
                    for (path, bytes) in before {
                        assert_eq!(fs::read(path).unwrap(), bytes);
                    }
                    assert_eq!(
                        fs::read_dir(store.join("events")).unwrap().count(),
                        if kind < 2 { 1 } else { 2 }
                    );
                }
                assert_eq!(sha256_bytes_hex(&fs::read(&binary).unwrap()), hash);
            }
        }
    }
    #[test]
    fn enrollment_s21_no_public_route() {
        // SAFETY: this native control requires its own disposable test process.
        let _owner = unsafe { crate::session::append_progress::owner::isolated_test_owner() };
        let (a, c) = capabilities::build_progress_pair(
            &TestEd25519Signer::from_seed([71; 32]),
            Writer::shore_local("test"),
            TIME.into(),
            "nonce".into(),
        )
        .unwrap();
        capabilities::validate_progress_pair(&a, &c).unwrap();
        let a: capabilities::StoreCapabilityActivationV1 = serde_json::from_slice(&a).unwrap();
        assert!(a.validate_for_execution().is_err());
        assert_eq!(
            capabilities::SupportedReaderProfileV1::ChangeRevision.minimum_reader_profile(),
            "review_change_revision_v1"
        );
    }
    #[cfg(any(target_os = "macos", windows))]
    #[cfg_attr(
        windows,
        ignore = "mandatory owner/DACL gate: just test-append-progress-native"
    )]
    #[test]
    fn enrollment_s22_fresh_process() {
        // This isolated diagnostic must never print assertion payload file bodies.
        std::panic::set_hook(Box::new(|_| {
            eprintln!("S22 assertion failed; payload redacted")
        }));
        // SAFETY: this native control requires its own disposable test process.
        let _owner = unsafe { crate::session::append_progress::owner::isolated_test_owner() };
        if process_child() {
            return;
        }
        let root = test_root();
        enroll(root.path(), 71).expect("semantic Red until real signed enrollment is connected");
        child(root.path(), "read", None);
        oracle(root.path()).unwrap();
    }
    #[cfg(windows)]
    #[ignore = "mandatory owner/DACL gate: just test-append-progress-native"]
    #[test]
    fn enrollment_s24_acl_native() {
        // SAFETY: this native control requires its own disposable test process.
        let _owner = unsafe { crate::session::append_progress::owner::isolated_test_owner() };
        #[cfg(windows)]
        {
            use super::super::fixture_acl;
            let positive = test_root();
            let path = positive.path().to_path_buf();
            let seen = std::rc::Rc::new(std::cell::RefCell::new(std::collections::BTreeSet::<
                String,
            >::new()));
            fn inspect(
                root: &Path,
                seen: &std::rc::Rc<std::cell::RefCell<std::collections::BTreeSet<String>>>,
            ) {
                let mut paths = vec![root.to_path_buf(), root.join("authority.writer.lock")];
                fn walk(p: &Path, out: &mut Vec<PathBuf>) {
                    if p.exists() {
                        out.push(p.to_path_buf());
                        if p.is_dir() {
                            for e in fs::read_dir(p).unwrap() {
                                walk(&e.unwrap().path(), out);
                            }
                        }
                    }
                }
                walk(&root.join("append-progress"), &mut paths);
                for p in paths {
                    fixture_acl("Inspect", root, &p);
                    seen.borrow_mut()
                        .insert(p.file_name().unwrap().to_string_lossy().into_owned());
                }
            }
            let at_sql = path.clone();
            let observed = seen.clone();
            let precommit_seen = std::rc::Rc::new(std::cell::Cell::new(false));
            let precommit_observed = precommit_seen.clone();
            let at_selector = path.clone();
            let selector_seen = seen.clone();
            ledger::with_hook(
                move |at| {
                    if at == "owned-create-before-commit" {
                        inspect(&at_sql, &observed);
                        precommit_observed.set(true);
                    }
                    Ok(())
                },
                || {
                    enrollment::with_boundary(
                        move |at| {
                            if at == "E6" {
                                inspect(&at_selector, &selector_seen);
                            }
                            Ok(())
                        },
                        || enroll(&path, 71),
                    )
                },
            )
            .unwrap();
            assert!(
                precommit_seen.get(),
                "owned pre-commit inspection not reached"
            );
            for role in [
                "authority.writer.lock",
                "append-progress",
                "enrollment",
                "incarnations",
                "ledger.sqlite",
                "ledger.sqlite-wal",
                "ledger.sqlite-shm",
                "active.json",
            ] {
                assert!(seen.borrow().contains(role), "missing actual object {role}");
            }
            assert!(seen.borrow().contains(&format!("{REQUEST}.json")));
            oracle(&path).unwrap();
            let before = snapshot(&path);
            read_selected(&path).unwrap();
            assert_eq!(before, snapshot(&path));

            let negative = test_root();
            let error = enroll(negative.path(), 71).unwrap_err().to_string();
            assert!(error.contains("unsupported descriptor"));
            assert!(!negative.path().join("append-progress").exists());
            assert!(!negative.path().join("events").exists());

            let query = test_root();
            let query_lock = guarded(query.path(), |_, g| {
                use std::os::windows::fs::OpenOptionsExt as _;
                use std::os::windows::io::AsHandle as _;

                use crate::session::store::authority_lock::file_identity;
                let owned = fs::OpenOptions::new()
                    .access_mode(0x0002_0080)
                    .share_mode(7)
                    .custom_flags(0x0220_0000)
                    .open(query.path())
                    .unwrap();
                let expected = file_identity(&owned).unwrap();
                let (observed, descriptor) = native::windows_acl::borrowed_private_descriptor(
                    owned.as_handle(),
                    query.path(),
                    Kind::Root,
                )
                .unwrap();
                assert_eq!(observed.identity, expected);
                assert!(observed.directory);
                let wrong_kind = native::windows_acl::borrowed_private_descriptor(
                    owned.as_handle(),
                    query.path(),
                    Kind::File,
                )
                .unwrap_err()
                .to_string();
                assert!(wrong_kind.contains("object type"));
                for (injected, class) in [
                    (fault::Fault::Query(1234), "query error"),
                    (fault::Fault::Descriptor(vec![]), "unsupported descriptor"),
                ] {
                    let mut injected = Some(injected);
                    let error = fault::with(
                        move |_| injected.take(),
                        || {
                            native::windows_acl::borrowed_private_descriptor(
                                owned.as_handle(),
                                query.path(),
                                Kind::Root,
                            )
                        },
                    )
                    .unwrap_err()
                    .to_string();
                    assert!(error.contains(class));
                    assert_eq!(file_identity(&owned).unwrap(), expected);
                }
                let restricted = fs::OpenOptions::new()
                    .access_mode(0x80)
                    .share_mode(7)
                    .custom_flags(0x0220_0000)
                    .open(query.path())
                    .unwrap();
                let error = native::windows_acl::borrowed_private_descriptor(
                    restricted.as_handle(),
                    query.path(),
                    Kind::Root,
                )
                .unwrap_err()
                .to_string();
                assert!(error.contains("ACL permission/identity change"));
                assert_eq!(file_identity(&restricted).unwrap(), expected);
                let (_, after) = native::windows_acl::borrowed_private_descriptor(
                    owned.as_handle(),
                    query.path(),
                    Kind::Root,
                )
                .unwrap();
                assert_eq!(after, descriptor);
                drop(restricted);
                drop(owned);
                g.validate_physical_root(query.path()).unwrap();
                g.physical_lock_identity().unwrap()
            });
            // Include the stable lock file in both byte snapshots, but release its
            // Windows byte-range lock before opening a separate read handle.
            let before = snapshot(query.path());
            guarded(query.path(), |r, g| {
                assert_eq!(g.physical_lock_identity().unwrap(), query_lock);
                g.validate_physical_root(query.path()).unwrap();
                let err = fault::with(
                    |_| Some(fault::Fault::Query(1234)),
                    || ledger::access(r, g, &[]).and_then(|gate| gate.finish()),
                )
                .unwrap_err()
                .to_string();
                assert!(err.contains("query error") && err.contains("1234"));
                g.validate_physical_root(query.path()).unwrap();
            });
            assert_eq!(before, snapshot(query.path()));
            // The actual-Q query-failure assertion now belongs to S25's
            // disposable child: unknown native admission cannot return safely.

            let drift = test_root();
            enroll(drift.path(), 71).unwrap();
            let directory = drift.path().join("append-progress");
            let original = fixture_acl("Inspect", drift.path(), &directory);
            assert_eq!(original["target"]["protected"], false);
            let user = hex_bytes(original["user_sid_hex"].as_str().unwrap());
            let injected = descriptor(&user, &[(&user, 3)], true);
            assert!(parse(&injected, &user, Kind::Directory).is_some());
            let mut calls = 0;
            let before = snapshot(drift.path());
            guarded(drift.path(), |r, g| {
                let directory = native::root_snapshot(r)
                    .unwrap()
                    .canonical
                    .join("append-progress");
                let matching = directory.clone();
                let err = fault::with(
                    move |path| {
                        if path == matching {
                            calls += 1;
                            if calls == 2 {
                                return Some(fault::Fault::Descriptor(injected.clone()));
                            }
                        }
                        None
                    },
                    || {
                        ledger::access(r, g, &[(directory.clone(), true)])
                            .and_then(|gate| gate.finish())
                    },
                )
                .unwrap_err()
                .to_string();
                assert!(err.contains("descriptor changed"), "{err}");
            });
            assert_eq!(before, snapshot(drift.path()));
            let unchanged = fixture_acl("Inspect", drift.path(), &directory);
            assert_eq!(original["target"]["sddl"], unchanged["target"]["sddl"]);
            assert_eq!(
                original["target"]["identity_before"],
                unchanged["target"]["identity_after"]
            );
            guarded(drift.path(), |r, g| {
                let selector = native::root_snapshot(r)
                    .unwrap()
                    .canonical
                    .join("append-progress/active.json");
                let gate = ledger::access(r, g, &[(selector.clone(), false)]).unwrap();
                let retained = drift.path().join("retained-selector");
                fs::rename(&selector, &retained).expect("single actual identity replacement");
                fs::copy(&retained, &selector).unwrap();
                assert!(gate.finish().is_err());
            });
        }
    }

    #[cfg(any(target_os = "macos", windows))]
    #[cfg_attr(
        windows,
        ignore = "mandatory owner/DACL gate: just test-append-progress-native"
    )]
    #[test]
    fn enrollment_s25_actual_native_alias_and_retirement() {
        // SAFETY: the focused runner launches one disposable process per test.
        let _owner = unsafe { super::super::owner::isolated_test_owner() };
        if let Ok(mode) = std::env::var("POINTBREAK_NATIVE_NEGATIVE") {
            let root = PathBuf::from(std::env::var_os("POINTBREAK_NATIVE_NEGATIVE_ROOT").unwrap());
            enter_child(&root, &mode);
            assert!(["alias-n", "alias-q", "retire", "acl"].contains(&mode.as_str()));
            if mode == "acl" {
                let armed = std::rc::Rc::new(std::cell::Cell::new(false));
                let inject_armed = armed.clone();
                let result = ledger::with_hook(
                    move |at| {
                        if at == "before-owned-open" {
                            armed.set(true);
                            eprintln!("S25 ACL: armed before owned native open");
                        }
                        Ok(())
                    },
                    || {
                        fault::with(
                            move |path| {
                                if inject_armed.get()
                                    && path.file_name().is_some_and(|name| name == "ledger.sqlite")
                                {
                                    eprintln!("S25 ACL: injecting main descriptor query failure");
                                    Some(fault::Fault::Query(1234))
                                } else {
                                    None
                                }
                            },
                            || enroll(&root, 71),
                        )
                    },
                );
                panic!(
                    "actual-Q ACL query failure must terminate the owned child; returned {result:?}"
                );
            }
            let target = root.clone();
            ledger::with_hook(
                move |at| {
                    if at == "before-owned-open" && mode != "retire" {
                        let db = database(&target);
                        let (source, destination) = if mode == "alias-n" {
                            (carrier(&target, false), db)
                        } else {
                            (db.clone(), PathBuf::from(format!("{}-wal", db.display())))
                        };
                        fs::remove_file(&destination).unwrap();
                        fs::hard_link(source, destination).unwrap();
                    }
                    if at == "native-retire-main" && mode == "retire" {
                        return Err(ShoreError::Message(
                            "injected retirement uncertainty".into(),
                        ));
                    }
                    Ok(())
                },
                || {
                    let _ = enroll(&root, 71);
                },
            );
            panic!("actual native failure must terminate the owned child");
        }
        assert!(
            std::env::var_os("POINTBREAK_NATIVE_NEGATIVE_ONLY").is_none(),
            "complete gate requires every native-negative mode"
        );
        let modes: &[&str] = if cfg!(windows) {
            &["alias-n", "alias-q", "retire", "acl"]
        } else {
            &["alias-n", "alias-q", "retire"]
        };
        for &mode in modes {
            let root = test_root();
            stopped(root.path(), "E4");
            let mut command = std::process::Command::new(std::env::current_exe().unwrap());
            command
                .env("POINTBREAK_NATIVE_NEGATIVE", mode)
                .env("POINTBREAK_NATIVE_NEGATIVE_ROOT", root.path());
            let status = run_child(
                command,
                root.path(),
                "session::append_progress::tests::enrollment_controls::enrollment_s25_actual_native_alias_and_retirement",
                mode,
            );
            assert_eq!(status.code(), Some(90), "{mode}");
            assert!(!root.path().join("append-progress/active.json").exists());
            println!(
                "{mode}: confirmed terminal child; SQL outcome remains unknown, fixture retained without resume"
            );
        }
    }

    fn descriptor(owner: &[u8], grants: &[(&[u8], u8)], protected: bool) -> Vec<u8> {
        let mut acl = vec![2, 0, 0, 0, grants.len() as u8, 0, 0, 0];
        for (sid, flags) in grants {
            acl.extend_from_slice(&[0, *flags]);
            acl.extend_from_slice(&((8 + sid.len()) as u16).to_le_bytes());
            acl.extend_from_slice(&0x001f_01ffu32.to_le_bytes());
            acl.extend_from_slice(sid);
        }
        let size = acl.len() as u16;
        acl[2..4].copy_from_slice(&size.to_le_bytes());
        let mut bytes = vec![0; 20];
        bytes[0] = 1;
        bytes[2..4]
            .copy_from_slice(&(0x8004u16 | if protected { 0x1000 } else { 0 }).to_le_bytes());
        bytes[4..8].copy_from_slice(&20u32.to_le_bytes());
        bytes[16..20].copy_from_slice(&((20 + owner.len()) as u32).to_le_bytes());
        bytes.extend_from_slice(owner);
        bytes.extend_from_slice(&acl);
        bytes
    }

    #[test]
    fn enrollment_s23_acl_shapes() {
        // SAFETY: this native control requires its own disposable test process.
        let _owner = unsafe { crate::session::append_progress::owner::isolated_test_owner() };
        assert_eq!(absent_thread_token(false, 1008), Ok(()));
        for code in [0, 5, 6, 87, 122, 1008] {
            assert_eq!(
                absent_thread_token(true, code),
                Err(TokenFailure::Impersonation)
            );
            if code != 1008 {
                assert_eq!(
                    absent_thread_token(false, code),
                    Err(TokenFailure::Query(code))
                );
            }
        }
        let word = std::mem::size_of::<usize>();
        let base = 0x1000usize;
        let mut token = vec![0; 2 * word];
        token[..word].copy_from_slice(&(base + 2 * word).to_ne_bytes());
        token.extend_from_slice(USER);
        assert_eq!(token_user(&token, base), Some(USER));
        for length in 0..token.len() {
            assert!(token_user(&token[..length], base).is_none());
        }
        for pointer in [
            0,
            base - 4,
            base,
            base + word,
            base + 2 * word + 1,
            base + token.len(),
            usize::MAX,
        ] {
            let mut invalid = token.clone();
            invalid[..word].copy_from_slice(&pointer.to_ne_bytes());
            assert!(token_user(&invalid, base).is_none());
        }
        let mut service = token[..2 * word].to_vec();
        service.extend_from_slice(SYSTEM);
        assert!(token_user(&service, base).is_none());
        for (at, value) in [(2 * word, 2), (2 * word + 1, 16)] {
            let mut invalid = token.clone();
            invalid[at] = value;
            assert!(token_user(&invalid, base).is_none());
        }
        let role = std::path::Path::new("ledger.sqlite");
        assert!(fault::take(role).is_none());
        fault::with(
            |_| Some(fault::Fault::Query(5)),
            || {
                assert!(matches!(fault::take(role), Some(fault::Fault::Query(5))));
                let caught = std::panic::catch_unwind(|| {
                    fault::with(
                        |_| Some(fault::Fault::Descriptor(vec![])),
                        || {
                            assert!(matches!(
                                fault::take(role),
                                Some(fault::Fault::Descriptor(_))
                            ));
                            panic!("finite nested hook reset control");
                        },
                    )
                });
                assert!(caught.is_err());
                assert!(matches!(fault::take(role), Some(fault::Fault::Query(5))));
            },
        );
        assert!(fault::take(role).is_none());
        let root = descriptor(USER, &[(USER, 3)], true);
        let both = descriptor(USER, &[(USER, 3), (SYSTEM, 3)], true);
        let reversed = descriptor(USER, &[(SYSTEM, 3), (USER, 3)], true);
        assert!(parse(&root, USER, Kind::Root).is_some());
        assert!(parse(&both, USER, Kind::Root).is_some());
        assert_eq!(
            parse(&both, USER, Kind::Root),
            parse(&reversed, USER, Kind::Root)
        );
        assert!(parse(&descriptor(USER, &[(USER, 3)], false), USER, Kind::Root).is_none());
        for kind in [Kind::Directory, Kind::File] {
            let explicit = if kind == Kind::Directory { 3 } else { 0 };
            for flags in [explicit, explicit | 16] {
                for protected in [false, true] {
                    assert!(
                        parse(&descriptor(USER, &[(USER, flags)], protected), USER, kind).is_some()
                    );
                }
            }
        }
        for owner in [OTHER, SYSTEM] {
            assert!(parse(&descriptor(owner, &[(USER, 3)], true), USER, Kind::Root).is_none());
        }
        assert!(
            parse(
                &descriptor(SYSTEM, &[(SYSTEM, 3)], true),
                SYSTEM,
                Kind::Root
            )
            .is_none()
        );
        for grants in [
            vec![],
            vec![(SYSTEM, 3)],
            vec![(USER, 3), (USER, 3)],
            vec![(USER, 3), (OTHER, 3)],
            vec![(USER, 3), (SYSTEM, 3), (OTHER, 3)],
        ] {
            assert!(parse(&descriptor(USER, &grants, true), USER, Kind::Root).is_none());
        }
        let acl = 20 + USER.len();
        let ace = acl + 8;
        for ace_type in 1..=255 {
            let mut bytes = root.clone();
            bytes[ace] = ace_type;
            assert!(
                parse(&bytes, USER, Kind::Root).is_none(),
                "ACE type {ace_type}"
            );
        }
        for kind in [Kind::Root, Kind::Directory, Kind::File] {
            for flags in 0..=255 {
                let accepted = match kind {
                    Kind::Root => flags == 3,
                    Kind::Directory => matches!(flags, 3 | 19),
                    Kind::File => matches!(flags, 0 | 16),
                };
                assert_eq!(
                    parse(&descriptor(USER, &[(USER, flags)], true), USER, kind).is_some(),
                    accepted,
                    "{kind:?} flags {flags}"
                );
            }
        }
        for mask in [
            0,
            1,
            0x001f_01fe,
            0x001f_03ff,
            0x1000_0000,
            0x0200_0000,
            0xffff_ffff,
        ] {
            let mut bytes = root.clone();
            bytes[ace + 4..ace + 8].copy_from_slice(&u32::to_le_bytes(mask));
            assert!(parse(&bytes, USER, Kind::Root).is_none());
        }
        for len in 0..root.len() {
            assert!(parse(&root[..len], USER, Kind::Root).is_none());
        }
        for at in [4, 16] {
            for offset in [0u32, 1, 19, 21, u32::MAX] {
                let mut bytes = root.clone();
                bytes[at..at + 4].copy_from_slice(&offset.to_le_bytes());
                assert!(parse(&bytes, USER, Kind::Root).is_none());
            }
        }
        for (at, value) in [
            (0, 2),
            (2, 0),
            (3, 0x10),
            (acl, 4),
            (acl + 2, 7),
            (acl + 4, 0),
            (acl + 4, 3),
            (ace + 2, 8),
            (ace + 2, 255),
            (ace + 8, 2),
            (ace + 9, 16),
        ] {
            let mut bytes = root.clone();
            bytes[at] = value;
            assert!(
                parse(&bytes, USER, Kind::Root).is_none(),
                "malformed byte {at}"
            );
        }
        let mut overlap = root.clone();
        overlap[16..20].copy_from_slice(&20u32.to_le_bytes());
        assert!(parse(&overlap, USER, Kind::Root).is_none());
        let mut trailing = root.clone();
        trailing.extend_from_slice(&[0; 4]);
        let acl_size = (trailing.len() - acl) as u16;
        trailing[acl + 2..acl + 4].copy_from_slice(&acl_size.to_le_bytes());
        assert_eq!(
            parse(&trailing, USER, Kind::Root),
            parse(&root, USER, Kind::Root)
        );
        let ace_size = (trailing.len() - ace) as u16;
        trailing[ace + 2..ace + 4].copy_from_slice(&ace_size.to_le_bytes());
        assert!(
            parse(&trailing, USER, Kind::Root).is_none(),
            "trailing ACE payload"
        );
        println!(
            "S23 bounded token/descriptor vectors and injection reset; no native token or ACL proof"
        );
    }
}

#[cfg(any(target_os = "macos", windows))]
#[cfg_attr(
    windows,
    ignore = "mandatory owner/DACL gate: just test-append-progress-native"
)]
#[test]
fn checkpoint_progress_native_admission_creates_no_enrollment() {
    // SAFETY: this native control requires its own disposable test process.
    let _owner = unsafe { crate::session::append_progress::owner::isolated_test_owner() };
    let root = test_root();
    let admitted = native::physical_root(root.path());
    admitted.expect("owned native root barrier must be available");
    assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 0);
}

#[cfg(target_os = "macos")]
#[test]
fn checkpoint_progress_native_sync_checks_files_and_parent_binding() {
    // SAFETY: this native control requires its own disposable test process.
    let _owner = unsafe { crate::session::append_progress::owner::isolated_test_owner() };
    let root = test_root();
    let handle = native::physical_root(root.path()).unwrap();
    let db = root.path().join("ledger.sqlite");
    std::fs::write(
        &db,
        b"durability primitive bytes, not a SQLite qualification",
    )
    .unwrap();
    std::fs::write(root.path().join("ledger.sqlite-wal"), b"retained WAL bytes").unwrap();
    {
        use std::os::fd::{AsFd as _, AsRawFd as _};
        use std::os::unix::fs::{MetadataExt as _, PermissionsExt as _};
        let owned = std::fs::File::open(&db).unwrap();
        let raw = owned.as_raw_fd();
        let before = owned.metadata().unwrap();
        let observed = native::borrowed_metadata(owned.as_fd()).unwrap();
        assert_eq!(
            observed.identity.parts(),
            (before.dev(), u128::from(before.ino()))
        );
        assert!(!observed.directory);
        assert_eq!(observed.mode, before.permissions().mode() & 0o777);
        assert!(native::borrowed_private_mode(owned.as_fd(), true).is_err());
        assert_eq!(
            native::borrowed_private_mode(owned.as_fd(), false).is_ok(),
            observed.mode == 0o600
        );
        let mut changed = observed;
        changed.mode = 0o640;
        assert!(changed.require_private_mode(false).is_err());
        changed.mode = 0o600;
        assert!(changed.require_private_mode(false).is_ok());
        assert!(changed.require_private_mode(true).is_err());
        changed.directory = true;
        changed.mode = 0o700;
        assert!(changed.require_private_mode(true).is_ok());
        assert_eq!(owned.as_raw_fd(), raw);
        assert_eq!(owned.metadata().unwrap().len(), before.len());
        assert_eq!(native::borrowed_metadata(owned.as_fd()).unwrap(), observed);
    }
    native::sync_progress_files(&handle, &db).unwrap();
    let foreign = test_root();
    let outside = foreign.path().join("foreign.sqlite");
    std::fs::write(&outside, b"foreign").unwrap();
    assert!(native::sync_progress_files(&handle, &outside).is_err());
    assert!(native::sync_progress_files(&handle, &root.path().join("missing.sqlite")).is_err());
    let link = root.path().join("linked.sqlite");
    std::os::unix::fs::symlink(&outside, &link).unwrap();
    {
        use std::os::fd::AsFd as _;
        use std::os::unix::fs::OpenOptionsExt as _;
        let owned_link = std::fs::OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_SYMLINK)
            .open(&link)
            .unwrap();
        assert!(native::borrowed_metadata(owned_link.as_fd()).is_err());
    }
    assert!(native::sync_progress_files(&handle, &link).is_err());
    let linked_parent = root.path().join("linked-parent");
    std::os::unix::fs::symlink(foreign.path(), &linked_parent).unwrap();
    assert!(native::sync_progress_files(&handle, &linked_parent.join("foreign.sqlite")).is_err());
    assert_eq!(std::fs::read(&outside).unwrap(), b"foreign");
    let nested = root.path().join("incarnations").join("first");
    std::fs::create_dir_all(&nested).unwrap();
    std::fs::write(nested.join("ledger.sqlite"), b"nested primitive bytes").unwrap();
    native::sync_progress_files(&handle, &nested.join("ledger.sqlite")).unwrap();
    let replacement_base = test_root();
    let replaced = replacement_base.path().join("root");
    std::fs::create_dir(&replaced).unwrap();
    let retained = native::physical_root(&replaced).unwrap();
    std::fs::rename(
        &replaced,
        replacement_base.path().join("retained-original-root"),
    )
    .unwrap();
    std::fs::create_dir(&replaced).unwrap();
    std::fs::write(replaced.join("ledger.sqlite"), b"replacement").unwrap();
    assert!(native::sync_progress_files(&retained, &replaced.join("ledger.sqlite")).is_err());
}

#[cfg(any(target_os = "macos", windows))]
mod controls {
    use std::path::Path;

    use super::*;
    use crate::error::{Result, ShoreError};
    use crate::session::store::authority_lock::StoreAuthorityLock;
    use crate::storage::{CreateOutcome, Durability, LocalStorage};

    fn root(path: &Path) -> native::NativeRootHandle {
        #[cfg(windows)]
        {
            native::ntfs_candidate_root(path).unwrap()
        }
        #[cfg(target_os = "macos")]
        {
            native::physical_root(path).unwrap()
        }
    }
    fn guard(path: &Path) -> StoreAuthorityLock {
        StoreAuthorityLock::acquire_until(
            path,
            std::time::Instant::now() + std::time::Duration::from_secs(5),
            None,
        )
        .unwrap()
    }
    fn publish(path: &Path) -> CreateOutcome {
        LocalStorage::new(path)
            .create_file_exclusive(Path::new("carrier"), b"exact carrier", Durability::Durable)
            .unwrap()
    }
    fn replace(from: &Path, to: &Path) -> bool {
        match std::fs::rename(from, to) {
            Ok(()) => true,
            #[cfg(windows)]
            Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => false,
            Err(e) => panic!("unexpected namespace replacement failure: {e}"),
        }
    }
    fn fail() -> ShoreError {
        ShoreError::Message("injected required barrier failure".into())
    }

    fn exact_path(path: &Path, key: &str) -> std::path::PathBuf {
        path.join("events").join(format!(
            "{}.json",
            crate::canonical_hash::sha256_bytes_hex(key.as_bytes())
        ))
    }
    fn exact_publish(path: &Path, key: &str, bytes: &[u8]) -> CreateOutcome {
        use crate::session::store::backend::{Journal as _, LocalJournal};
        LocalJournal::new(path)
            .create_record_once(key, bytes)
            .unwrap()
    }
    fn exact_sync(held: &native::NativeRootHandle, key: &str, bytes: &[u8]) -> Result<()> {
        native::sync_exact_carrier(
            held,
            key,
            &crate::canonical_hash::sha256_bytes_hex(bytes),
            bytes.len() as u64,
        )
    }

    #[cfg(any(target_os = "macos", windows))]
    #[cfg_attr(
        windows,
        ignore = "mandatory owner/DACL gate: just test-append-progress-native"
    )]
    #[test]
    fn checkpoint_progress_exact_interface() {
        // SAFETY: this native control requires its own disposable test process.
        let _owner = unsafe { crate::session::append_progress::owner::isolated_test_owner() };
        let dir = test_root();
        let _guard = guard(dir.path());
        let held = root(dir.path());
        let bytes = vec![0x5a; 20003];
        let key = "receipt-bound-carrier";
        assert_eq!(
            exact_publish(dir.path(), key, &bytes),
            CreateOutcome::Created
        );
        exact_sync(&held, key, &bytes).unwrap();
        println!("X01 real Journal Created exact carrier");
        assert_eq!(
            exact_publish(dir.path(), key, b"different incoming"),
            CreateOutcome::AlreadyExists
        );
        exact_sync(&held, key, &bytes).unwrap();
        assert!(exact_sync(&held, key, b"different incoming").is_err());
        assert_eq!(std::fs::read(exact_path(dir.path(), key)).unwrap(), bytes);
        println!("X02 existing original retained; different incoming refused");
        let witness = crate::canonical_hash::sha256_bytes_hex(&bytes);
        native::with_hook(
            |_, _| panic!("invalid identity must fail before barriers"),
            || {
                assert!(
                    native::sync_exact_carrier(&held, "", &witness, bytes.len() as u64).is_err()
                );
                for bad in [
                    String::new(),
                    "a".repeat(63),
                    "a".repeat(65),
                    "G".repeat(64),
                    witness.to_uppercase(),
                    format!("sha256:{witness}"),
                ] {
                    assert!(
                        native::sync_exact_carrier(&held, key, &bad, bytes.len() as u64).is_err()
                    );
                }
            },
        );
        println!("X03 empty logical key and malformed/noncanonical witness refused");
        for length in [0, bytes.len() as u64 - 1, bytes.len() as u64 + 1, u64::MAX] {
            assert!(native::sync_exact_carrier(&held, key, &witness, length).is_err());
        }
        assert!(
            native::sync_exact_carrier(&held, key, &"0".repeat(64), bytes.len() as u64).is_err()
        );
        exact_sync(&held, key, &bytes).unwrap();
        println!("X04 wrong length/digest refused; multibuffer exact bytes accepted");
        assert!(exact_sync(&held, "absent", &bytes).is_err());
        println!("X05 missing key refused");
        for opaque in ["../outside", "/absolute", "C:\\outside", "unicode-雪", "\0"] {
            assert_eq!(
                exact_publish(dir.path(), opaque, b"opaque"),
                CreateOutcome::Created
            );
            exact_sync(&held, opaque, b"opaque").unwrap();
            assert_eq!(
                std::fs::read(exact_path(dir.path(), opaque)).unwrap(),
                b"opaque"
            );
        }
        assert!(!dir.path().join("outside").exists());
        println!("X06 opaque path-like and Unicode keys use native hash mapping");
        for stage in [native::Barrier::File, native::Barrier::Parent] {
            let result = native::with_hook(
                move |_, at| if at == stage { Err(fail()) } else { Ok(()) },
                || exact_sync(&held, key, &bytes),
            );
            assert!(
                result
                    .unwrap_err()
                    .to_string()
                    .contains("injected required barrier")
            );
        }
        println!("X11 required file and parent barriers refuse");
        let changed = exact_path(dir.path(), key);
        let result = native::with_hook(
            move |_, at| {
                if at == native::Barrier::AfterFile {
                    std::fs::write(&changed, vec![0x59; 20003]).unwrap();
                }
                Ok(())
            },
            || exact_sync(&held, key, &bytes),
        );
        assert!(result.is_err());
        println!("X12 same-file mutation caught by final exact witness check");
    }

    #[cfg(any(target_os = "macos", windows))]
    #[cfg_attr(
        windows,
        ignore = "mandatory owner/DACL gate: just test-append-progress-native"
    )]
    #[test]
    fn checkpoint_progress_exact_reparse() {
        // SAFETY: this native control requires its own disposable test process.
        let _owner = unsafe { crate::session::append_progress::owner::isolated_test_owner() };
        let outside = test_root();
        exact_publish(outside.path(), "key", b"outside");
        for parent_link in [false, true] {
            let dir = test_root();
            let _guard = guard(dir.path());
            let held = root(dir.path());
            let (from, to) = if parent_link {
                (outside.path().join("events"), dir.path().join("events"))
            } else {
                std::fs::create_dir(dir.path().join("events")).unwrap();
                (
                    exact_path(outside.path(), "key"),
                    exact_path(dir.path(), "key"),
                )
            };
            #[cfg(windows)]
            if parent_link {
                std::os::windows::fs::symlink_dir(&from, &to).unwrap();
            } else {
                std::os::windows::fs::symlink_file(&from, &to).unwrap();
            }
            #[cfg(target_os = "macos")]
            std::os::unix::fs::symlink(&from, &to).unwrap();
            assert!(exact_sync(&held, "key", b"outside").is_err());
        }
        assert_eq!(
            std::fs::read(exact_path(outside.path(), "key")).unwrap(),
            b"outside"
        );
        println!("X07 file and parent reparse refused; outside bytes preserved");
    }

    #[cfg(any(target_os = "macos", windows))]
    #[cfg_attr(
        windows,
        ignore = "mandatory owner/DACL gate: just test-append-progress-native"
    )]
    #[test]
    fn checkpoint_progress_exact_rebinding() {
        // SAFETY: this native control requires its own disposable test process.
        let _owner = unsafe { crate::session::append_progress::owner::isolated_test_owner() };
        for parent in [false, true] {
            let dir = test_root();
            exact_publish(dir.path(), "key", b"exact");
            let source = if parent {
                dir.path().join("events")
            } else {
                exact_path(dir.path(), "key")
            }
            .canonicalize()
            .unwrap();
            let moved = dir.path().join("retained-original");
            std::fs::rename(&source, &moved).unwrap();
            std::fs::rename(&moved, &source).unwrap();
            let _guard = guard(dir.path());
            let held = root(dir.path());
            let replaced = std::rc::Rc::new(std::cell::Cell::new(false));
            let happened = replaced.clone();
            let result = native::with_hook(
                move |path, stage| {
                    let selected = if parent {
                        stage == native::Barrier::Parent && path == source
                    } else {
                        stage == native::Barrier::File && path == source
                    };
                    if selected && replace(&source, &moved) {
                        if parent {
                            std::fs::create_dir(&source).unwrap();
                            let filename = moved.join(format!(
                                "{}.json",
                                crate::canonical_hash::sha256_bytes_hex(b"key")
                            ));
                            std::fs::copy(&filename, source.join(filename.file_name().unwrap()))
                                .unwrap();
                        } else {
                            std::fs::write(&source, b"exact").unwrap();
                        }
                        happened.set(true);
                    }
                    Ok(())
                },
                || exact_sync(&held, "key", b"exact"),
            );
            assert_eq!(result.is_err(), replaced.get());
            println!(
                "{} replacement occurred={}; otherwise native handle exclusion",
                if parent { "X09 parent" } else { "X08 file" },
                replaced.get()
            );
        }
        let outer = test_root();
        let path = outer.path().join("root");
        std::fs::create_dir(&path).unwrap();
        exact_publish(&path, "key", b"exact");
        let moved = outer.path().join("original-root");
        std::fs::rename(&path, &moved).unwrap();
        std::fs::rename(&moved, &path).unwrap();
        let _guard = guard(&path);
        let held = root(&path);
        let replaced = replace(&path, &moved);
        if replaced {
            std::fs::create_dir(&path).unwrap();
            exact_publish(&path, "key", b"exact");
        }
        assert_eq!(exact_sync(&held, "key", b"exact").is_err(), replaced);
        println!("X10 root replacement occurred={replaced}; otherwise native handle exclusion");
    }

    #[cfg(any(target_os = "macos", windows))]
    #[cfg_attr(
        windows,
        ignore = "mandatory owner/DACL gate: just test-append-progress-native"
    )]
    #[test]
    fn checkpoint_progress_carrier_controls() {
        // SAFETY: this native control requires its own disposable test process.
        let _owner = unsafe { crate::session::append_progress::owner::isolated_test_owner() };
        let dir = test_root();
        let _guard = guard(dir.path());
        let held = root(dir.path());
        println!("N01 private owned root; public enrollment remains disabled");
        let file = dir.path().join("carrier");
        assert_eq!(publish(dir.path()), CreateOutcome::Created);
        native::sync_carrier(&held, &file, b"exact carrier").unwrap();
        assert_eq!(std::fs::read(&file).unwrap(), b"exact carrier");
        println!("N02 final carrier");
        assert_eq!(publish(dir.path()), CreateOutcome::AlreadyExists);
        native::sync_carrier(&held, &file, b"exact carrier").unwrap();
        println!("N03 same existing");
        assert!(native::sync_carrier(&held, &file, b"different").is_err());
        println!("N04 different existing refused");
        let nested = dir.path().join("new").join("nested");
        std::fs::create_dir_all(&nested).unwrap();
        publish(&nested);
        native::sync_carrier(&held, &nested.join("carrier"), b"exact carrier").unwrap();
        println!("N05 nested chain");
        let outside = test_root();
        publish(outside.path());
        for path in [
            dir.path().join("missing"),
            outside.path().join("carrier"),
            dir.path().join("new/../carrier"),
        ] {
            assert!(native::sync_carrier(&held, &path, b"exact carrier").is_err());
        }
        assert_eq!(
            std::fs::read(outside.path().join("carrier")).unwrap(),
            b"exact carrier"
        );
        println!("N06 missing/outside/traversal refused");
        for (stage, id) in [
            (native::Barrier::File, "N10"),
            (native::Barrier::Parent, "N11"),
        ] {
            let result = native::with_hook(
                move |_, at| if at == stage { Err(fail()) } else { Ok(()) },
                || native::sync_carrier(&held, &file, b"exact carrier"),
            );
            assert!(
                result
                    .unwrap_err()
                    .to_string()
                    .contains("injected required barrier")
            );
            assert_eq!(std::fs::read(&file).unwrap(), b"exact carrier");
            println!("{id} required barrier fault refused");
        }
        let parent = nested.canonicalize().unwrap();
        let displaced = dir.path().join("retained-parent");
        std::fs::rename(&parent, &displaced).unwrap();
        std::fs::rename(&displaced, &parent).unwrap();
        let replaced = std::rc::Rc::new(std::cell::Cell::new(false));
        let did_replace = replaced.clone();
        let result = native::with_hook(
            move |path, stage| {
                if stage == native::Barrier::Parent
                    && path == parent
                    && replace(&parent, &displaced)
                {
                    std::fs::create_dir(&parent).unwrap();
                    did_replace.set(true);
                }
                Ok(())
            },
            || native::sync_carrier(&held, &nested.join("carrier"), b"exact carrier"),
        );
        assert_eq!(result.is_err(), replaced.get());
        println!(
            "N08 parent replacement occurred={}; otherwise native handle exclusion",
            replaced.get()
        );
        let outer = test_root();
        let old = outer.path().join("root");
        std::fs::create_dir(&old).unwrap();
        let moved = outer.path().join("retained-root");
        std::fs::rename(&old, &moved).unwrap();
        std::fs::rename(&moved, &old).unwrap();
        let _old_guard = guard(&old);
        let old_handle = root(&old);
        publish(&old);
        let replaced = replace(&old, &moved);
        if replaced {
            std::fs::create_dir(&old).unwrap();
            publish(&old);
        }
        assert_eq!(
            native::sync_carrier(&old_handle, &old.join("carrier"), b"exact carrier").is_err(),
            replaced
        );
        println!("N07 root replacement occurred={replaced}; otherwise native handle exclusion");
    }

    #[cfg(any(target_os = "macos", windows))]
    #[cfg_attr(
        windows,
        ignore = "mandatory owner/DACL gate: just test-append-progress-native"
    )]
    #[test]
    fn checkpoint_progress_reparse_controls() {
        // SAFETY: this native control requires its own disposable test process.
        let _owner = unsafe { crate::session::append_progress::owner::isolated_test_owner() };
        let dir = test_root();
        let _guard = guard(dir.path());
        let held = root(dir.path());
        let outside = test_root();
        publish(outside.path());
        #[cfg(windows)]
        {
            std::os::windows::fs::symlink_file(
                outside.path().join("carrier"),
                dir.path().join("link"),
            )
            .expect("ordinary-user file symlink readiness");
            std::os::windows::fs::symlink_dir(outside.path(), dir.path().join("parent"))
                .expect("ordinary-user directory symlink readiness");
        }
        #[cfg(target_os = "macos")]
        {
            std::os::unix::fs::symlink(outside.path().join("carrier"), dir.path().join("link"))
                .unwrap();
            std::os::unix::fs::symlink(outside.path(), dir.path().join("parent")).unwrap();
        }
        assert!(native::sync_carrier(&held, &dir.path().join("link"), b"exact carrier").is_err());
        assert!(
            native::sync_carrier(&held, &dir.path().join("parent/carrier"), b"exact carrier")
                .is_err()
        );
        assert_eq!(
            std::fs::read(outside.path().join("carrier")).unwrap(),
            b"exact carrier"
        );
        println!("N09 reparse file/parent refused");
    }

    fn database(path: &Path) -> rusqlite::Connection {
        let db = rusqlite::Connection::open(path).unwrap();
        db.execute_batch(
            "PRAGMA journal_mode=WAL; PRAGMA synchronous=FULL; PRAGMA foreign_keys=ON;",
        )
        .unwrap();
        db.busy_timeout(std::time::Duration::ZERO).unwrap();
        assert_eq!(
            db.pragma_query_value(None, "synchronous", |r| r.get::<_, i64>(0))
                .unwrap(),
            2
        );
        assert_eq!(
            db.pragma_query_value(None, "foreign_keys", |r| r.get::<_, i64>(0))
                .unwrap(),
            1
        );
        db
    }
    fn checkpoint(db: &rusqlite::Connection) -> Result<()> {
        let (busy, log, copied): (i64, i64, i64) = db
            .query_row("PRAGMA wal_checkpoint(FULL)", [], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?))
            })
            .map_err(|error| ShoreError::Message(format!("checkpoint query failed: {error}")))?;
        if busy != 0 || log < 0 || log != copied {
            return Err(fail());
        }
        Ok(())
    }
    fn seed(db: &rusqlite::Connection) {
        db.execute_batch("CREATE TABLE parent(id INTEGER PRIMARY KEY); CREATE TABLE child(id INTEGER REFERENCES parent(id)); BEGIN; INSERT INTO parent VALUES(1); INSERT INTO child VALUES(1); COMMIT;").unwrap();
    }
    #[cfg(any(target_os = "macos", windows))]
    #[cfg_attr(
        windows,
        ignore = "mandatory owner/DACL gate: just test-append-progress-native"
    )]
    #[test]
    fn checkpoint_progress_sqlite_controls() {
        // SAFETY: this native control requires its own disposable test process.
        let _owner = unsafe { crate::session::append_progress::owner::isolated_test_owner() };
        let dir = test_root();
        let _guard = guard(dir.path());
        let held = root(dir.path());
        let path = dir.path().join("ledger.sqlite");
        let db = database(&path);
        seed(&db);
        checkpoint(&db).unwrap();
        native::sync_progress_files(&held, &path).unwrap();
        assert!(db.execute("INSERT INTO child VALUES(99)", []).is_err());
        let reader = database(&path);
        reader.execute_batch("BEGIN;").unwrap();
        assert_eq!(
            reader
                .query_row("SELECT COUNT(*) FROM child", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            1
        );
        println!("N12 WAL/FULL/FK complete checkpoint and fresh connection");
        db.execute("INSERT INTO parent VALUES(2)", []).unwrap();
        assert!(checkpoint(&db).is_err());
        println!("N13 busy/incomplete checkpoint refused");
        reader.execute_batch("ROLLBACK;").unwrap();
        checkpoint(&db).unwrap();
        native::sync_progress_files(&held, &path).unwrap();
    }

    #[ignore = "auxiliary child; invoked by fresh_process_recovery"]
    #[test]
    fn checkpoint_progress_child() {
        // SAFETY: this native control requires its own disposable test process.
        let _owner = unsafe { crate::session::append_progress::owner::isolated_test_owner() };
        let mode =
            std::env::var("POINTBREAK_CHECKPOINT_2_CHILD").expect("child entry requires a mode");
        assert!(
            [
                "before-publication",
                "after-link",
                "after-file",
                "after-barriers",
                "sqlite-commit",
                "sqlite-checkpoint"
            ]
            .contains(&mode.as_str())
        );
        let path =
            std::path::PathBuf::from(std::env::var_os("POINTBREAK_CHECKPOINT_2_ROOT").unwrap());
        enter_child(&path, &mode);
        assert_eq!(
            std::fs::read(path.join("checkpoint-2-owned")).unwrap(),
            b"fixture child only"
        );
        let _guard = guard(&path);
        let held = root(&path);
        if mode == "before-publication" {
            std::process::exit(86);
        }
        if mode.starts_with("sqlite-") {
            let file = path.join("ledger.sqlite");
            let db = database(&file);
            seed(&db);
            if mode == "sqlite-checkpoint" {
                checkpoint(&db).unwrap();
                native::sync_progress_files(&held, &file).unwrap();
            }
            std::process::exit(86);
        }
        publish(&path);
        if mode == "after-link" {
            std::process::exit(86);
        }
        if mode == "after-file" {
            native::with_hook(
                |_, stage| {
                    if stage == native::Barrier::AfterFile {
                        std::process::exit(86);
                    }
                    Ok(())
                },
                || native::sync_carrier(&held, &path.join("carrier"), b"exact carrier"),
            )
            .unwrap();
            panic!("required cut was not reached");
        }
        native::sync_carrier(&held, &path.join("carrier"), b"exact carrier").unwrap();
        std::process::exit(86);
    }

    #[cfg(any(target_os = "macos", windows))]
    #[cfg_attr(
        windows,
        ignore = "mandatory owner/DACL gate: just test-append-progress-native"
    )]
    #[test]
    fn checkpoint_progress_fresh_process_recovery() {
        // SAFETY: this native control requires its own disposable test process.
        let _owner = unsafe { crate::session::append_progress::owner::isolated_test_owner() };
        for (i, mode) in [
            "before-publication",
            "after-link",
            "after-file",
            "after-barriers",
            "sqlite-commit",
            "sqlite-checkpoint",
        ]
        .into_iter()
        .enumerate()
        {
            let dir = test_root();
            std::fs::write(dir.path().join("checkpoint-2-owned"), b"fixture child only").unwrap();
            let mut command = std::process::Command::new(std::env::current_exe().unwrap());
            command
                .env("POINTBREAK_CHECKPOINT_2_CHILD", mode)
                .env("POINTBREAK_CHECKPOINT_2_ROOT", dir.path());
            let status = run_child(
                command,
                dir.path(),
                "session::append_progress::tests::controls::checkpoint_progress_child",
                mode,
            );
            assert_eq!(
                status.code(),
                Some(86),
                "{mode}: worker must reach exact cut"
            );
            let _guard = guard(dir.path());
            let held = root(dir.path());
            if mode == "before-publication" {
                assert!(!dir.path().join("carrier").exists());
            } else if mode.starts_with("sqlite-") {
                let path = dir.path().join("ledger.sqlite");
                let db = database(&path);
                assert_eq!(
                    db.query_row("SELECT COUNT(*) FROM child WHERE id=1", [], |r| r
                        .get::<_, i64>(0))
                        .unwrap(),
                    1
                );
                checkpoint(&db).unwrap();
                native::sync_progress_files(&held, &path).unwrap();
            } else {
                native::sync_carrier(&held, &dir.path().join("carrier"), b"exact carrier").unwrap();
            }
            println!("N{:02} {mode}: fresh process recovery", i + 14);
        }
    }
}

#[cfg(any(target_os = "macos", windows))]
mod ledger_controls {
    use std::path::{Path, PathBuf};

    use rusqlite::Connection;
    use serde_json::json;

    use super::super::ledger::{self, InitialLedger};
    use super::*;
    use crate::session::store::authority_lock::StoreAuthorityLock;
    use crate::session::store::backend::{
        Journal, JournalChangeCheck, JournalChangeVerdict, LocalJournal,
    };

    const B0: &str = "690275afefaf02f7e1e90c0abd11dd41df18d22953313dcf1142b230c4b5a7d3";
    fn root(path: &Path) -> native::NativeRootHandle {
        #[cfg(windows)]
        {
            native::ntfs_candidate_root(path).unwrap()
        }
        #[cfg(target_os = "macos")]
        {
            native::physical_root(path).unwrap()
        }
    }
    fn guard(path: &Path) -> StoreAuthorityLock {
        StoreAuthorityLock::acquire_until(
            path,
            std::time::Instant::now() + std::time::Duration::from_secs(5),
            None,
        )
        .unwrap()
    }
    fn activate(path: &Path) {
        LocalJournal::new(path)
            .create_record_once(
                "store_capability_activation:review_change_revision_v1:root",
                b"synthetic activation\n",
            )
            .unwrap();
    }
    fn db_path(path: &Path) -> PathBuf {
        path.join("append-progress/incarnations")
            .join("1".repeat(64))
            .join("ledger.sqlite")
    }
    fn visit(
        f: impl FnOnce(
            &InitialLedger<'_>,
            &ledger::InitialInputs,
            &native::NativeRootHandle,
            &StoreAuthorityLock,
            &Path,
        ),
    ) {
        let dir = test_root();
        let g = guard(dir.path());
        let r = root(dir.path());
        activate(dir.path());
        let inputs = ledger::fixture_inputs(ledger::root_witness(&r).unwrap());
        let db = InitialLedger::create(&r, &g, &inputs).unwrap();
        f(&db, &inputs, &r, &g, dir.path());
    }
    fn reject_sql(statement: &str) {
        let dir = test_root();
        let g = guard(dir.path());
        let r = root(dir.path());
        activate(dir.path());
        let inputs = ledger::fixture_inputs(ledger::root_witness(&r).unwrap());
        drop(InitialLedger::create(&r, &g, &inputs).unwrap());
        let db = Connection::open(db_path(dir.path())).unwrap();
        db.execute_batch(statement).unwrap();
        drop(db);
        assert!(
            InitialLedger::reopen(&r, &g, &inputs).is_err(),
            "{statement}"
        );
        assert!(db_path(dir.path()).exists());
        println!("refused SQL case: {statement}");
    }
    fn anchor(db: &InitialLedger<'_>) -> String {
        db.fixture_db()
            .query_row("SELECT native_stamp FROM meta", [], |r| r.get(0))
            .unwrap()
    }
    fn calls(f: impl FnOnce()) -> Vec<String> {
        let stages = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
        let captured = stages.clone();
        ledger::with_hook(
            move |at| {
                captured.borrow_mut().push(at.to_owned());
                Ok(())
            },
            f,
        );
        stages.borrow().clone()
    }
    #[cfg(any(target_os = "macos", windows))]
    #[cfg_attr(
        windows,
        ignore = "mandatory owner/DACL gate: just test-append-progress-native"
    )]
    #[test]
    fn ledger_l01_init_and_gate_order() {
        // SAFETY: this native control requires its own disposable test process.
        let _owner = unsafe { crate::session::append_progress::owner::isolated_test_owner() };
        let stages = calls(|| {
            visit(|db, _, _, _, _| {
                let row:(i64,String,String,i64,String)=db.fixture_db().query_row("SELECT head,prefix_sha256,state,(SELECT count(*) FROM baseline),baseline_sha256 FROM meta",[],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?))).unwrap();
                assert_eq!(
                    (row.0, row.2, row.3, row.4),
                    (0, "initializing".into(), 2, B0.into())
                );
                assert_eq!(row.1.len(), 64);
                let nulls:i64=db.fixture_db().query_row("SELECT count(*) FROM current WHERE phase='INIT' AND typeof(original_obligation_json)='null' AND typeof(proof_json)='null'",[],|r|r.get(0)).unwrap();
                assert_eq!(nulls, 1);
                assert_eq!(
                    db.fixture_db()
                        .query_row("SELECT count(*) FROM attempt", [], |r| r.get::<_, i64>(0))
                        .unwrap(),
                    0
                );
            })
        });
        assert_eq!(
            stages,
            [
                "create-before-path",
                "create-before-commit",
                "before-checkpoint",
                "checkpoint",
                "after-checkpoint",
                "after-barriers"
            ]
        );
    }
    #[cfg(any(target_os = "macos", windows))]
    #[cfg_attr(
        windows,
        ignore = "mandatory owner/DACL gate: just test-append-progress-native"
    )]
    #[test]
    fn ledger_l02_reopen_and_vector() {
        // SAFETY: this native control requires its own disposable test process.
        let _owner = unsafe { crate::session::append_progress::owner::isolated_test_owner() };
        let inputs = ledger::fixture_inputs(
            json!({"platform":"unix","device":"7","inode":"11","canonicalPath":{"encoding":"unix-bytes-hex","value":"2f746d702f70726f67726573732dff"}}),
        );
        let b = inputs.bindings().unwrap();
        for (value, expected) in [
            (&b.baseline, B0),
            (
                &b.pair,
                "0e0a6cad81fd74e53ada90af830779917b77526bce2281645c6c730d909e81bc",
            ),
            (
                &b.selection,
                "89af98aced06a6743a0b08bf0008234e6a4992515c91bf0da2a3c498e67f3873",
            ),
            (
                &b.prefix,
                "058999c1003c01c68895d44965cba424e7fbc384a08316808efb6355310d2e30",
            ),
        ] {
            assert_eq!(
                crate::canonical_hash::sha256_json_hex(value).unwrap(),
                expected
            );
        }
        visit(|db, inputs, r, g, path| {
            let a = anchor(db);
            std::fs::write(path.join("unrelated"), b"volume churn").unwrap();
            let reopened = InitialLedger::reopen(r, g, inputs).unwrap();
            assert_eq!(anchor(&reopened), a);
        });
    }
    #[cfg(any(target_os = "macos", windows))]
    #[cfg_attr(
        windows,
        ignore = "mandatory owner/DACL gate: just test-append-progress-native"
    )]
    #[test]
    fn ledger_l03_missing_database() {
        // SAFETY: this native control requires its own disposable test process.
        let _owner = unsafe { crate::session::append_progress::owner::isolated_test_owner() };
        let dir = test_root();
        let g = guard(dir.path());
        let r = root(dir.path());
        activate(dir.path());
        let inputs = ledger::fixture_inputs(ledger::root_witness(&r).unwrap());
        assert!(InitialLedger::reopen(&r, &g, &inputs).is_err());
        assert!(!db_path(dir.path()).exists());
        std::fs::create_dir_all(db_path(dir.path()).parent().unwrap()).unwrap();
        assert!(InitialLedger::reopen(&r, &g, &inputs).is_err());
        assert!(!db_path(dir.path()).exists());
        assert!(InitialLedger::create(&r, &g, &inputs).is_err());
    }
    #[cfg(any(target_os = "macos", windows))]
    #[cfg_attr(
        windows,
        ignore = "mandatory owner/DACL gate: just test-append-progress-native"
    )]
    #[test]
    fn ledger_l04_missing_singletons() {
        // SAFETY: this native control requires its own disposable test process.
        let _owner = unsafe { crate::session::append_progress::owner::isolated_test_owner() };
        for table in ["meta", "current"] {
            reject_sql(&format!("DELETE FROM {table}"));
        }
    }
    #[cfg(any(target_os = "macos", windows))]
    #[cfg_attr(
        windows,
        ignore = "mandatory owner/DACL gate: just test-append-progress-native"
    )]
    #[test]
    fn ledger_l05_schema_rejections() {
        // SAFETY: this native control requires its own disposable test process.
        let _owner = unsafe { crate::session::append_progress::owner::isolated_test_owner() };
        for statement in [
            "DROP TABLE attempt",
            "CREATE TABLE extra(x)",
            "ALTER TABLE meta RENAME TO alien",
            "ALTER TABLE meta ADD COLUMN extra TEXT",
            "ALTER TABLE meta RENAME COLUMN epoch TO alien",
            "CREATE INDEX extra ON meta(epoch)",
            "CREATE VIEW extra AS SELECT * FROM meta",
            "CREATE TRIGGER extra AFTER UPDATE ON meta BEGIN SELECT 1; END",
            "PRAGMA writable_schema=ON; UPDATE sqlite_schema SET sql=replace(sql,' STRICT','') WHERE name='meta'",
            "PRAGMA writable_schema=ON; UPDATE sqlite_schema SET sql=replace(sql,'CHECK(format=1)','') WHERE name='meta'",
            "PRAGMA writable_schema=ON; UPDATE sqlite_schema SET sql=replace(sql,'format INTEGER','format TEXT') WHERE name='meta'",
        ] {
            reject_sql(statement);
        }
    }
    #[cfg(any(target_os = "macos", windows))]
    #[cfg_attr(
        windows,
        ignore = "mandatory owner/DACL gate: just test-append-progress-native"
    )]
    #[test]
    fn ledger_l06_version_phase_and_json() {
        // SAFETY: this native control requires its own disposable test process.
        let _owner = unsafe { crate::session::append_progress::owner::isolated_test_owner() };
        for statement in [
            "PRAGMA user_version=2",
            "PRAGMA application_id=7",
            "UPDATE meta SET state='ready'",
            "PRAGMA ignore_check_constraints=ON; UPDATE meta SET format=2",
            "UPDATE current SET phase='INTENT',original_obligation_json='{}'",
            "UPDATE meta SET native_stamp='{}'",
            "UPDATE meta SET native_stamp='null'",
            "UPDATE meta SET native_stamp=' {\"state\":\"absent\"}'",
            "UPDATE meta SET native_stamp='{\"state\":\"absent\",\"extra\":1}'",
            "UPDATE meta SET native_stamp='{\"state\":\"absent\",\"state\":\"absent\"}'",
        ] {
            reject_sql(statement);
        }
    }
    #[cfg(any(target_os = "macos", windows))]
    #[cfg_attr(
        windows,
        ignore = "mandatory owner/DACL gate: just test-append-progress-native"
    )]
    #[test]
    fn ledger_l07_baseline_rejections() {
        // SAFETY: this native control requires its own disposable test process.
        let _owner = unsafe { crate::session::append_progress::owner::isolated_test_owner() };
        for statement in [
            "DELETE FROM baseline WHERE kind='activation'",
            "UPDATE baseline SET kind='completion'",
            "INSERT INTO baseline SELECT printf('%064d',1),record_sha256,record_length,kind FROM baseline WHERE kind='activation'",
        ] {
            reject_sql(statement);
        }
        visit(|db, _, _, _, _| {
            assert!(
                db.fixture_db()
                    .execute_batch("INSERT INTO baseline SELECT * FROM baseline LIMIT 1")
                    .is_err()
            );
        });
    }
    #[cfg(any(target_os = "macos", windows))]
    #[cfg_attr(
        windows,
        ignore = "mandatory owner/DACL gate: just test-append-progress-native"
    )]
    #[test]
    fn ledger_l08_binding_rejections() {
        // SAFETY: this native control requires its own disposable test process.
        let _owner = unsafe { crate::session::append_progress::owner::isolated_test_owner() };
        for field in [
            "selection_sha256",
            "baseline_sha256",
            "capability_pair_sha256",
            "epoch",
        ] {
            reject_sql(&format!("UPDATE meta SET {field}=printf('%064d',7)"));
        }
    }
    #[cfg(any(target_os = "macos", windows))]
    #[cfg_attr(
        windows,
        ignore = "mandatory owner/DACL gate: just test-append-progress-native"
    )]
    #[test]
    fn ledger_l09_prefix_rejections() {
        // SAFETY: this native control requires its own disposable test process.
        let _owner = unsafe { crate::session::append_progress::owner::isolated_test_owner() };
        for value in ["'sha256:bad'", "upper(prefix_sha256)", "printf('%064d',7)"] {
            reject_sql(&format!(
                "PRAGMA ignore_check_constraints=ON; UPDATE meta SET prefix_sha256={value}"
            ));
        }
    }
    #[cfg(any(target_os = "macos", windows))]
    #[cfg_attr(
        windows,
        ignore = "mandatory owner/DACL gate: just test-append-progress-native"
    )]
    #[test]
    fn ledger_l10_numeric_rejections() {
        // SAFETY: this native control requires its own disposable test process.
        let _owner = unsafe { crate::session::append_progress::owner::isolated_test_owner() };
        for statement in [
            "PRAGMA ignore_check_constraints=ON; UPDATE meta SET head=-1",
            "UPDATE meta SET head=9223372036854775807",
            "PRAGMA ignore_check_constraints=ON; UPDATE baseline SET record_length=-1",
            "UPDATE baseline SET record_length=0",
            "UPDATE baseline SET record_length=9223372036854775807",
        ] {
            reject_sql(statement);
        }
        visit(|db, _, _, _, _| {
            for value in ["9223372036854775808", "1.5", "'no'"] {
                assert!(
                    db.fixture_db()
                        .execute_batch(&format!("UPDATE baseline SET record_length={value}"))
                        .is_err()
                );
            }
        });
    }
    #[cfg(any(target_os = "macos", windows))]
    #[cfg_attr(
        windows,
        ignore = "mandatory owner/DACL gate: just test-append-progress-native"
    )]
    #[test]
    fn ledger_l11_attempt_receipt_rejections() {
        // SAFETY: this native control requires its own disposable test process.
        let _owner = unsafe { crate::session::append_progress::owner::isolated_test_owner() };
        reject_sql("INSERT INTO attempt VALUES('a','k',printf('%064d',0),0)");
        reject_sql(
            "PRAGMA foreign_keys=OFF; INSERT INTO receipt VALUES(1,'k',printf('%064d',0),0,'missing')",
        );
        visit(|db, _, _, _, _| {
            assert!(
                db.fixture_db()
                    .execute_batch(
                        "INSERT INTO receipt VALUES(1,'k',printf('%064d',0),0,'missing')"
                    )
                    .is_err()
            );
        });
    }
    #[cfg(any(target_os = "macos", windows))]
    #[cfg_attr(
        windows,
        ignore = "mandatory owner/DACL gate: just test-append-progress-native"
    )]
    #[test]
    fn ledger_l12_foreign_root_guard() {
        // SAFETY: this native control requires its own disposable test process.
        let _owner = unsafe { crate::session::append_progress::owner::isolated_test_owner() };
        let one = test_root();
        let two = test_root();
        let g = guard(one.path());
        let r = root(two.path());
        activate(two.path());
        let inputs = ledger::fixture_inputs(ledger::root_witness(&r).unwrap());
        assert!(InitialLedger::create(&r, &g, &inputs).is_err());
        assert!(!db_path(two.path()).exists());
        let g2 = guard(two.path());
        let wrong = ledger::fixture_inputs(json!({}));
        assert!(InitialLedger::create(&r, &g2, &wrong).is_err());
        assert!(!db_path(two.path()).exists());
    }
    #[cfg(any(target_os = "macos", windows))]
    #[cfg_attr(
        windows,
        ignore = "mandatory owner/DACL gate: just test-append-progress-native"
    )]
    #[test]
    fn ledger_l13_physical_paths() {
        // SAFETY: this native control requires its own disposable test process.
        let _owner = unsafe { crate::session::append_progress::owner::isolated_test_owner() };
        for suffix in ["", "-wal", "-shm"] {
            let dir = test_root();
            let g = guard(dir.path());
            let r = root(dir.path());
            activate(dir.path());
            let inputs = ledger::fixture_inputs(ledger::root_witness(&r).unwrap());
            drop(InitialLedger::create(&r, &g, &inputs).unwrap());
            let mut path = db_path(dir.path()).into_os_string();
            path.push(suffix);
            let path = PathBuf::from(path);
            if path.exists() {
                std::fs::rename(&path, path.with_extension("saved")).unwrap();
            }
            let outside = dir.path().join("outside");
            std::fs::write(&outside, b"untouched").unwrap();
            #[cfg(unix)]
            std::os::unix::fs::symlink(&outside, &path).unwrap();
            #[cfg(windows)]
            std::os::windows::fs::symlink_file(&outside, &path).unwrap();
            assert!(InitialLedger::reopen(&r, &g, &inputs).is_err());
            assert_eq!(std::fs::read(&outside).unwrap(), b"untouched");
        }
        {
            let dir = test_root();
            let g = guard(dir.path());
            let r = root(dir.path());
            activate(dir.path());
            let inputs = ledger::fixture_inputs(ledger::root_witness(&r).unwrap());
            let db = InitialLedger::create(&r, &g, &inputs).unwrap();
            let source = db.fixture_path().to_path_buf();
            let destination = source.with_extension("held");
            match std::fs::rename(&source, &destination) {
                Ok(()) => {
                    std::fs::write(&source, b"replacement").unwrap();
                    assert!(db.flush().is_err());
                    println!("L13 actual database replacement refused");
                }
                Err(e) => {
                    eprintln!(
                        "L13 rename diagnostic: raw_os_error={:?}; kind={:?}; text={}; source={:?}; destination={:?}; context=live InitialLedger with SQLite connection and retained path handles; fixture root and authority guard remain in scope",
                        e.raw_os_error(),
                        e.kind(),
                        e,
                        source,
                        destination
                    );
                    #[cfg(windows)]
                    assert_eq!(e.raw_os_error(), Some(32));
                    #[cfg(not(windows))]
                    assert_eq!(e.kind(), std::io::ErrorKind::PermissionDenied);
                    // Drop the complete resource bundle; do not attribute exclusion
                    // to any individual handle. The root and authority guard stay live.
                    drop(db);
                    let original = std::fs::read(&source).unwrap();
                    std::fs::rename(&source, &destination).unwrap();
                    std::fs::rename(&destination, &source).unwrap();
                    assert_eq!(std::fs::read(&source).unwrap(), original);
                    println!(
                        "L13 live ledger bundle exclusion; same-path post-drop rename/restore passed"
                    );
                }
            }
        }
        assert!(ledger::fixture_sqlite_paths(Path::new("ordinary-ledger.sqlite")).is_ok());
        assert!(ledger::fixture_sqlite_paths(Path::new("nul\0ledger.sqlite")).is_err());
        println!("L13 ordinary path accepted and NUL refused by pure preflight");
        #[cfg(windows)]
        for unit in [0xd800, 0xdc00] {
            use std::os::windows::ffi::OsStringExt;
            let outer = test_root();
            let path = outer
                .path()
                .join(std::ffi::OsString::from_wide(&[b'n' as u16, unit]));
            std::fs::create_dir(&path).unwrap();
            let g = guard(&path);
            let r = root(&path);
            let inputs = ledger::fixture_inputs(ledger::root_witness(&r).unwrap());
            assert!(InitialLedger::create(&r, &g, &inputs).is_err());
            assert!(!path.join("append-progress").exists());
            let db = db_path(&path);
            std::fs::create_dir_all(db.parent().unwrap()).unwrap();
            let mut names = vec![db.clone()];
            for suffix in ["-wal", "-shm"] {
                let mut name = db.as_os_str().to_os_string();
                name.push(suffix);
                names.push(PathBuf::from(name));
            }
            for name in &names {
                std::fs::write(name, b"sentinel must remain exact").unwrap();
            }
            assert!(InitialLedger::reopen(&r, &g, &inputs).is_err());
            for name in &names {
                assert_eq!(std::fs::read(name).unwrap(), b"sentinel must remain exact");
            }
            assert_eq!(std::fs::read_dir(db.parent().unwrap()).unwrap().count(), 3);
            println!(
                "L13 Windows surrogate {unit:x} refused before SQLite open; sentinels unchanged"
            );
        }
        let outer = test_root();
        let mut name = std::ffi::OsString::from("native-");
        #[cfg(unix)]
        {
            use std::os::unix::ffi::OsStringExt;
            name.push(std::ffi::OsString::from_vec(vec![255]));
        }
        #[cfg(windows)]
        {
            use std::os::windows::ffi::OsStringExt;
            name.push(std::ffi::OsString::from_wide(&[0xd83d, 0xde42]));
        }
        let path = outer.path().join(name);
        assert!(ledger::fixture_sqlite_paths(&db_path(&path)).is_ok());
        if let Err(error) = std::fs::create_dir(&path) {
            #[cfg(target_os = "macos")]
            {
                assert_eq!(error.raw_os_error(), Some(libc::EILSEQ));
                println!(
                    "L13 APFS rejected invalid UTF-8 filename before native admission; pure raw-byte path preflight passed; no positive native claim"
                );
                return;
            }
            #[cfg(not(target_os = "macos"))]
            panic!("native paired-surrogate fixture failed: {error}");
        }
        let g = guard(&path);
        let r = root(&path);
        activate(&path);
        let inputs = ledger::fixture_inputs(ledger::root_witness(&r).unwrap());
        drop(InitialLedger::create(&r, &g, &inputs).unwrap());
        InitialLedger::reopen(&r, &g, &inputs).unwrap();
        println!("L13 native path create/reopen passed");
    }
    #[cfg(any(target_os = "macos", windows))]
    #[cfg_attr(
        windows,
        ignore = "mandatory owner/DACL gate: just test-append-progress-native"
    )]
    #[test]
    fn ledger_l14_native_anchor_verdicts() {
        // SAFETY: this native control requires its own disposable test process.
        let _owner = unsafe { crate::session::append_progress::owner::isolated_test_owner() };
        visit(|db, _, _, _, path| {
            let a = ledger::fixture_decode_stamp(&anchor(db)).unwrap();
            for verdict in [
                JournalChangeVerdict::Stable,
                JournalChangeVerdict::Changed,
                JournalChangeVerdict::Indeterminate,
            ] {
                let check = JournalChangeCheck {
                    after: a.clone(),
                    verdict,
                    native_bytes_examined: 0,
                    native_records_examined: 0,
                    relevant_file_references: vec![],
                    mechanism: "synthetic consumer verdict".into(),
                };
                assert_eq!(
                    ledger::fixture_check(&a, &check).is_ok(),
                    verdict == JournalChangeVerdict::Stable
                );
            }
            let mut malformed = serde_json::to_value(&a).unwrap();
            malformed["identity_sha256"] = json!("bad");
            assert!(
                ledger::fixture_decode_stamp(
                    &String::from_utf8(
                        crate::canonical_hash::canonical_json_bytes(&malformed).unwrap()
                    )
                    .unwrap()
                )
                .is_err()
            );
            #[cfg(windows)]
            {
                malformed = serde_json::to_value(&a).unwrap();
                malformed["native_cursor"] = serde_json::Value::Null;
                assert!(
                    ledger::fixture_decode_stamp(
                        &String::from_utf8(
                            crate::canonical_hash::canonical_json_bytes(&malformed).unwrap()
                        )
                        .unwrap()
                    )
                    .is_err()
                );
            }
            LocalJournal::new(path)
                .create_record_once("completion-or-other-event", b"changed")
                .unwrap();
            assert!(db.flush().is_err());
            assert_eq!(
                anchor(db),
                String::from_utf8(
                    crate::canonical_hash::canonical_json_bytes(&serde_json::to_value(a).unwrap())
                        .unwrap()
                )
                .unwrap()
            );
        });
    }
    #[cfg(any(target_os = "macos", windows))]
    #[cfg_attr(
        windows,
        ignore = "mandatory owner/DACL gate: just test-append-progress-native"
    )]
    #[test]
    fn ledger_l15_pragmas_and_gate_bounds() {
        // SAFETY: this native control requires its own disposable test process.
        let _owner = unsafe { crate::session::append_progress::owner::isolated_test_owner() };
        visit(|db, inputs, r, g, _| {
            let a = anchor(db);
            for (key, expected) in [("synchronous", 2), ("foreign_keys", 1)] {
                assert_eq!(
                    db.fixture_db()
                        .pragma_query_value(None, key, |r| r.get::<_, i64>(0))
                        .unwrap(),
                    expected
                );
            }
            assert_eq!(
                db.fixture_db()
                    .pragma_query_value(None, "journal_mode", |r| r.get::<_, String>(0))
                    .unwrap(),
                "wal"
            );
            assert_eq!(
                calls(|| db.flush().unwrap()),
                [
                    "before-checkpoint",
                    "checkpoint",
                    "after-checkpoint",
                    "after-barriers"
                ]
            );
            assert_eq!(
                calls(|| {
                    InitialLedger::reopen(r, g, inputs).unwrap();
                }),
                [
                    "reopen-before-configure",
                    "before-checkpoint",
                    "checkpoint",
                    "after-checkpoint",
                    "after-barriers"
                ]
            );
            assert_eq!(anchor(db), a);
        });
    }
    #[cfg(any(target_os = "macos", windows))]
    #[cfg_attr(
        windows,
        ignore = "mandatory owner/DACL gate: just test-append-progress-native"
    )]
    #[test]
    fn ledger_l16_checkpoint_barrier_failures() {
        // SAFETY: this native control requires its own disposable test process.
        let _owner = unsafe { crate::session::append_progress::owner::isolated_test_owner() };
        for stage in [
            "create-before-path",
            "create-before-commit",
            "before-checkpoint",
            "checkpoint",
            "after-checkpoint",
            "after-barriers",
        ] {
            let dir = test_root();
            let g = guard(dir.path());
            let r = root(dir.path());
            activate(dir.path());
            let inputs = ledger::fixture_inputs(ledger::root_witness(&r).unwrap());
            ledger::with_hook(
                move |at| {
                    if at == stage {
                        Err(crate::error::ShoreError::Message(
                            "injected gate/I/O failure".into(),
                        ))
                    } else {
                        Ok(())
                    }
                },
                || assert!(InitialLedger::create(&r, &g, &inputs).is_err()),
            );
        }
        visit(|db, _, _, _, _| {
            let a = anchor(db);
            for barrier in [native::Barrier::File, native::Barrier::Parent] {
                native::with_hook(
                    move |_, at| {
                        if at == barrier {
                            Err(crate::error::ShoreError::Message("injected barrier".into()))
                        } else {
                            Ok(())
                        }
                    },
                    || assert!(db.flush().is_err()),
                );
            }
            assert_eq!(anchor(db), a);
            let reader = Connection::open(db.fixture_path()).unwrap();
            reader.execute_batch("BEGIN; SELECT * FROM meta;").unwrap();
            db.fixture_db()
                .execute_batch(
                    "UPDATE meta SET state='fenced'; UPDATE meta SET state='initializing';",
                )
                .unwrap();
            assert!(db.flush().is_err());
            reader.execute_batch("ROLLBACK").unwrap();
            db.flush().unwrap();
        });
    }
    fn process_cuts(test: &str, modes: &[&str], committed: bool) {
        if let Ok(mode) = std::env::var("POINTBREAK_INITIAL_LEDGER_CUT") {
            assert!(modes.contains(&mode.as_str()));
            let path = PathBuf::from(std::env::var_os("POINTBREAK_INITIAL_LEDGER_ROOT").unwrap());
            enter_child(&path, &mode);
            let g = guard(&path);
            let r = root(&path);
            activate(&path);
            let inputs = ledger::fixture_inputs(ledger::root_witness(&r).unwrap());
            let _ = InitialLedger::create(&r, &g, &inputs);
            panic!("cut not reached");
        }
        for mode in modes {
            let dir = test_root();
            std::fs::write(
                dir.path().join("initial-ledger-owned"),
                b"fixture child only",
            )
            .unwrap();
            let mut command = std::process::Command::new(std::env::current_exe().unwrap());
            command
                .env("POINTBREAK_INITIAL_LEDGER_CUT", mode)
                .env("POINTBREAK_INITIAL_LEDGER_ROOT", dir.path());
            let status = run_child(command, dir.path(), test, mode);
            assert_eq!(status.code(), Some(86));
            let g = guard(dir.path());
            let r = root(dir.path());
            let inputs = ledger::fixture_inputs(ledger::root_witness(&r).unwrap());
            let outcome = InitialLedger::reopen(&r, &g, &inputs);
            assert_eq!(outcome.is_ok(), committed, "{mode}");
            if let Ok(db) = outcome {
                assert_eq!(
                    db.fixture_db()
                        .query_row("SELECT state FROM meta", [], |r| r.get::<_, String>(0))
                        .unwrap(),
                    "initializing"
                );
            }
            assert!(db_path(dir.path()).exists());
            println!("fresh process {mode}, exact INIT={committed}");
        }
    }
    #[cfg(any(target_os = "macos", windows))]
    #[cfg_attr(
        windows,
        ignore = "mandatory owner/DACL gate: just test-append-progress-native"
    )]
    #[test]
    fn ledger_l17_precommit_process_cuts() {
        // SAFETY: this native control requires its own disposable test process.
        let _owner = unsafe { crate::session::append_progress::owner::isolated_test_owner() };
        process_cuts(
            "session::append_progress::tests::ledger_controls::ledger_l17_precommit_process_cuts",
            &["before-transaction", "during-transaction"],
            false,
        );
    }
    #[cfg(any(target_os = "macos", windows))]
    #[cfg_attr(
        windows,
        ignore = "mandatory owner/DACL gate: just test-append-progress-native"
    )]
    #[test]
    fn ledger_l18_postcommit_process_cuts() {
        // SAFETY: this native control requires its own disposable test process.
        let _owner = unsafe { crate::session::append_progress::owner::isolated_test_owner() };
        process_cuts(
            "session::append_progress::tests::ledger_controls::ledger_l18_postcommit_process_cuts",
            &["after-commit", "after-checkpoint", "after-barriers"],
            true,
        );
    }
}
