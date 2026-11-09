use std::fs;
use std::process::{Command as ProcessCommand, Stdio};
use std::time::{Duration, Instant};

use super::*;
use crate::crypto::{EventSigner, TestEd25519Signer};
use crate::model::JournalId;
use crate::session::event::{
    EventSignature, EventTarget, EventToBeSigned, EventType, ReviewInitializedPayload, Writer,
    event_signature_pre_authentication_encoding,
};
const REQUEST: &str = "1111111111111111111111111111111111111111111111111111111111111111";
const ACTOR: &str = "actor:local";
pub(super) fn event(key: &str, seed: u8) -> ShoreEvent {
    let signer = TestEd25519Signer::from_seed([seed; 32]);
    let mut event = ShoreEvent::new(
        EventType::ReviewInitialized,
        key,
        EventTarget::for_journal(JournalId::new("journal:c4")),
        Writer::shore_local("test"),
        ReviewInitializedPayload {},
        "2026-10-01T00:00:00Z",
    )
    .unwrap();
    sign(&mut event, &signer);
    event
}
pub(super) fn sign(event: &mut ShoreEvent, signer: &impl EventSigner) {
    event.signer = Some(signer.signer_id().clone());
    let view = EventToBeSigned::from_event(event, signer.signer_id()).unwrap();
    event.signature = Some(EventSignature::ed25519_v1(
        signer
            .sign_event_message(&event_signature_pre_authentication_encoding(&view).unwrap())
            .unwrap(),
    ));
}
fn assigned(name: &str) -> PathBuf {
    let parent = PathBuf::from(
        std::env::var_os("POINTBREAK_C4_FIXTURE_PARENT").expect("assigned C4 fixture parent"),
    );
    assert!(parent.is_absolute());
    let root = parent.join(name);
    assert!(
        root.is_dir(),
        "operator-created private root {}",
        root.display()
    );
    assert_eq!(
        fs::read_dir(&root).unwrap().count(),
        0,
        "never reuse a consumed fixture"
    );
    root.canonicalize().unwrap()
}
pub(super) fn guarded<T>(
    path: &Path,
    run: impl FnOnce(&NativeRootHandle, &StoreAuthorityLock) -> T,
) -> T {
    super::super::super::owner::require_quiescent().unwrap();
    let root = native::physical_root(path).unwrap();
    let guard =
        StoreAuthorityLock::acquire_until(path, Instant::now() + Duration::from_secs(2), None)
            .unwrap();
    run(&root, &guard)
}
pub(super) fn enroll(path: &Path) {
    guarded(path, |r, g| {
        super::super::super::enrollment::enroll(
            r,
            g,
            REQUEST,
            true,
            "review_change_revision_progress_v1",
            &TestEd25519Signer::from_seed([71; 32]),
            Writer::shore_local("test"),
            "2026-10-01T00:00:00Z",
        )
        .unwrap()
    });
}
pub(super) fn publish(path: &Path, key: &str, seed: u8, attempt: u64) -> Result<AppendExit> {
    guarded(path, |r, g| {
        append(
            r,
            g,
            REQUEST,
            ACTOR,
            &format!("{attempt:064x}"),
            prepare(event(key, seed))?,
        )
    })
}
pub(super) fn resume(path: &Path) -> Result<AppendExit> {
    guarded(path, |r, g| reconcile(r, g, REQUEST, ACTOR))
}
pub(super) fn database(path: &Path) -> PathBuf {
    let plan: Value = serde_json::from_slice(
        &fs::read(
            path.join("append-progress/enrollment")
                .join(format!("{REQUEST}.json")),
        )
        .unwrap(),
    )
    .unwrap();
    path.join("append-progress/incarnations")
        .join(plan["incarnationId"].as_str().unwrap())
        .join("ledger.sqlite")
}
pub(super) fn oracle<T>(path: &Path, run: impl FnOnce(&Connection) -> T) -> T {
    super::super::super::owner::require_quiescent().unwrap();
    let db =
        Connection::open_with_flags(database(path), OpenFlags::SQLITE_OPEN_READ_WRITE).unwrap();
    let value = run(&db);
    db.close().unwrap();
    value
}
pub(super) fn counts(path: &Path) -> (i64, i64, i64, String) {
    oracle(path, |db| {
        let head = db
            .query_row("SELECT head FROM meta", [], |r| r.get(0))
            .unwrap();
        let receipts = db
            .query_row("SELECT count(*) FROM receipt", [], |r| r.get(0))
            .unwrap();
        let attempts = db
            .query_row("SELECT count(*) FROM attempt", [], |r| r.get(0))
            .unwrap();
        let phase = db
            .query_row("SELECT phase FROM current", [], |r| r.get(0))
            .unwrap();
        (head, receipts, attempts, phase)
    })
}
pub(super) fn pure_state() -> State {
    let native: JournalChangeStamp = if cfg!(windows) {
        serde_json::from_value(json!({"state":"observed","identity_sha256":"a".repeat(64),"change_sha256":"b".repeat(64),"entry_count":null,"native_cursor":{"journalId":1,"nextUsn":100,"directoryFileReference":2,"volumeSerialNumber":3}})).unwrap()
    } else {
        JournalChangeStamp::Observed {
            identity_sha256: "a".repeat(64),
            change_sha256: "b".repeat(64),
            entry_count: Some(2),
            native_cursor: None,
        }
    };
    State {
        binding: Binding {
            selection_sha256: "c".repeat(64),
            baseline_sha256: "d".repeat(64),
            capability_pair_sha256: "e".repeat(64),
            epoch: "f".repeat(64),
        },
        head: 0,
        prefix: "0".repeat(64),
        native,
        phase: Phase::Init,
        obligation: None,
        proof: None,
    }
}
#[test]
fn closed_state_and_transition_rules() {
    let initial = pure_state();
    initial.validate(&"0".repeat(64)).unwrap();
    let o = Obligation {
        version: 1,
        binding: initial.binding.clone(),
        minimum_reader_profile: "review_change_revision_progress_v1".into(),
        attempt_id: "1".repeat(64),
        key: "event:one".into(),
        expected_sha256: "2".repeat(64),
        expected_length: 10,
        next_head: 1,
        previous_head: 0,
        previous_prefix: initial.prefix.clone(),
        previous_native: initial.native.clone(),
    };
    let encoded = text(&serde_json::to_value(&o).unwrap()).unwrap();
    assert_eq!(closed::<Obligation>(&encoded).unwrap(), o);
    assert!(closed::<Obligation>(&encoded.replacen("{", "{\"version\":1,", 1)).is_err());
    let mut unknown = serde_json::to_value(&o).unwrap();
    unknown["body"] = json!("forbidden");
    assert!(closed::<Obligation>(&text(&unknown).unwrap()).is_err());
    let mut intent = initial.clone();
    intent.phase = Phase::Intent;
    intent.obligation = Some(o.clone());
    intent.validate(&initial.prefix).unwrap();
    let c = Change {
        old: initial.clone(),
        new: intent.clone(),
        attempt: Some(o.clone()),
        receipt: None,
    };
    assert!(validate_change(&c, ScopeProfile::Append).is_ok());
    assert!(validate_change(&c, ScopeProfile::Reconcile).is_err());
    for present in [false, true] {
        let mut proof = intent.clone();
        proof.phase = if present {
            Phase::ProvedPresent
        } else {
            Phase::ProvedAbsent
        };
        proof.proof = Some(if present {
            Proof::LivePublication {
                native: initial.native.clone(),
                stored_sha256: o.expected_sha256.clone(),
                stored_length: o.expected_length,
            }
        } else {
            Proof::StableAbsence {
                native: initial.native.clone(),
            }
        });
        proof.validate(&initial.prefix).unwrap();
        let mut settled = proof.clone();
        settled.phase = if present {
            Phase::SettledPresent
        } else {
            Phase::SettledAbsent
        };
        if present {
            settled.head = 1;
            settled.prefix = prefix_for(&o).unwrap();
        }
        settled.validate(&initial.prefix).unwrap();
        assert!(
            validate_change(
                &Change {
                    old: intent.clone(),
                    new: settled.clone(),
                    attempt: None,
                    receipt: present.then(|| o.receipt())
                },
                ScopeProfile::Reconcile
            )
            .is_err()
        );
        settled.head += 1;
        assert!(settled.validate(&initial.prefix).is_err());
    }
    let mut bad = o;
    bad.previous_head = i64::MAX;
    bad.next_head = i64::MAX;
    assert!(bad.validate(&initial.binding).is_err());
    bad = intent.obligation.clone().unwrap();
    bad.expected_length = -1;
    assert!(bad.validate(&initial.binding).is_err());
    let mut bad = intent.clone();
    bad.obligation = None;
    assert!(bad.validate(&initial.prefix).is_err());
    let mut bad = intent;
    bad.prefix = "3".repeat(64);
    assert!(bad.validate(&initial.prefix).is_err());
}
#[test]
fn prepared_bytes_are_immutable_and_signed_before_intent() {
    let event = event("c4:prepared", 1);
    let bytes = serde_json::to_vec(&event).unwrap();
    let prepared = prepare(event.clone()).unwrap();
    assert_eq!(prepared.bytes(), bytes);
    assert_eq!(prepared.event(), &event);
    let mut unsigned = event.clone();
    unsigned.signature = None;
    assert!(prepare(unsigned).is_err());
    let mut invalid = event;
    invalid.payload_hash = "sha256:".to_owned() + &"0".repeat(64);
    assert!(prepare(invalid).is_err());
}
#[test]
#[ignore = "requires assigned native private roots; one disposable test process"]
fn native_append_duplicate_and_constant_work() {
    // SAFETY: this explicitly selected native test runs in its own test process.
    let _owner = unsafe { super::super::super::owner::isolated_test_owner() };
    let root = assigned("duplicate");
    enroll(&root);
    let first = publish(&root, "c4:one", 1, 1).unwrap();
    assert!(first.finalized);
    assert_eq!(first.outcome, Some(EventWriteOutcome::Created));
    assert_eq!(first.head, Some(1));
    let stored = LocalJournal::new(&root)
        .read_event_bytes("c4:one")
        .unwrap()
        .unwrap();
    let (head, receipts, attempts, _) = counts(&root);
    assert_eq!((head, receipts, attempts), (1, 1, 1));
    for (seed, expected) in [
        (1, EventWriteOutcome::Existing),
        (2, EventWriteOutcome::ExistingDivergentSignature),
    ] {
        let duplicate = publish(&root, "c4:one", seed, 2).unwrap();
        assert!(duplicate.finalized);
        assert_eq!(duplicate.outcome, Some(expected));
        assert_eq!(duplicate.head, Some(1));
        assert_eq!(
            LocalJournal::new(&root)
                .read_event_bytes("c4:one")
                .unwrap()
                .unwrap(),
            stored
        );
        assert_eq!(counts(&root), (1, 1, 1, "SETTLED_PRESENT".into()));
    }
    // A valid signature on conflicting payload still invokes the native conflict rule.
    let mut conflict = event("c4:one", 1);
    conflict.payload = json!({"conflict":true});
    conflict.payload_hash = crate::canonical_hash::sha256_json_prefixed(&conflict.payload).unwrap();
    sign(&mut conflict, &TestEd25519Signer::from_seed([1; 32]));
    assert!(
        guarded(&root, |r, g| append(
            r,
            g,
            REQUEST,
            ACTOR,
            &"2".repeat(64),
            prepare(conflict).unwrap()
        ))
        .is_err()
    );
    assert_eq!(counts(&root), (1, 1, 1, "SETTLED_PRESENT".into()));
    let mut measured = Vec::new();
    for n in 2..=18u64 {
        let scope = crate::bench_support::longitudinal::LongitudinalCountingScopeV1::new(format!(
            "{n:064x}"
        ))
        .unwrap();
        reset_work();
        let result = {
            let _entered = scope.enter();
            publish(&root, &format!("c4:{n:02}"), 1, n).unwrap()
        };
        assert!(result.finalized);
        assert_eq!(result.head, Some(n));
        let counters = scope.snapshot().counters;
        assert_eq!(counters.directory_entries_walked, 0);
        assert_eq!(counters.authority_identity_rows_scanned, 0);
        assert_eq!(counters.strict_journal_inspections, 0);
        let work = work();
        assert!(work.0 > 0 && work.1 > 0);
        measured.push(work);
        println!(
            "C4 constant work head={n} rows={} prefix_hash_bytes={} carrier_opens={}",
            work.0, work.1, counters.carrier_opens
        );
    }
    assert!(measured.iter().all(|v| v.0 == measured[0].0));
    assert!(measured.last().unwrap().1 <= measured[0].1 + 512);
    assert_eq!(counts(&root), (18, 18, 18, "SETTLED_PRESENT".into()));
    assert_eq!(resume(&root).unwrap().head, Some(18));
}

fn child(root: &Path, mode: &str, cut: Option<&str>) -> std::process::Output {
    let mut command = ProcessCommand::new(std::env::current_exe().unwrap());
    command
        .args([
            "--exact",
            "session::append_progress::ledger::c4::tests::native_restart_cuts",
            "--ignored",
            "--nocapture",
        ])
        .env("POINTBREAK_C4_CHILD", "1")
        .env("POINTBREAK_C4_CHILD_ROOT", root)
        .env("POINTBREAK_C4_CHILD_MODE", mode)
        .env_remove("POINTBREAK_C4_CUT")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(cut) = cut {
        command.env("POINTBREAK_C4_CUT", cut);
    }
    let mut child = command.spawn().unwrap();
    let until = Instant::now() + Duration::from_secs(180);
    println!("C4 child pid={} mode={mode} cut={cut:?}", child.id());
    while child.try_wait().unwrap().is_none() {
        assert!(
            Instant::now() < until,
            "UNKNOWN live child {}; preserve, stop dependent work",
            child.id()
        );
        std::thread::sleep(Duration::from_millis(20));
    }
    let output = child.wait_with_output().unwrap();
    println!(
        "C4 terminal {} stdout={} stderr={}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    output
}
#[test]
#[ignore = "requires assigned native private roots; bounded fresh-process cuts"]
fn native_restart_cuts() {
    if std::env::var("POINTBREAK_C4_CHILD").as_deref() == Ok("1") {
        // SAFETY: parent launched this exact one-test child with owned terminal exit.
        let _owner = unsafe { super::super::super::owner::isolated_test_owner() };
        let root = PathBuf::from(std::env::var_os("POINTBREAK_C4_CHILD_ROOT").unwrap());
        match std::env::var("POINTBREAK_C4_CHILD_MODE").unwrap().as_str() {
            "append" => {
                enroll(&root);
                let result = publish(&root, "c4:cut", 1, 1).unwrap();
                assert!(result.finalized);
            }
            "absent" => {
                assert_eq!(resume(&root).unwrap().head, Some(0));
                let used = counts(&root).2 != 0;
                if used {
                    assert!(publish(&root, "c4:cut", 1, 1).is_err());
                }
                assert_eq!(publish(&root, "c4:other", 1, 2).unwrap().head, Some(1));
                assert_eq!(publish(&root, "c4:cut", 1, 3).unwrap().head, Some(2));
                let row = oracle(&root, |db| {
                    db.query_row(
                        "SELECT original_attempt FROM receipt WHERE key='c4:cut'",
                        [],
                        |r| r.get::<_, String>(0),
                    )
                    .unwrap()
                });
                assert_eq!(row, format!("{:064x}", 3));
            }
            "present" => {
                assert_eq!(resume(&root).unwrap().head, Some(1));
                assert_eq!(
                    publish(&root, "c4:cut", 1, 1).unwrap().outcome,
                    Some(EventWriteOutcome::Existing)
                );
                assert_eq!(counts(&root), (1, 1, 1, "SETTLED_PRESENT".into()));
            }
            "audit" => {
                assert!(resume(&root).is_err());
                assert!(publish(&root, "c4:cut", 1, 1).is_err());
                assert_eq!(counts(&root), (0, 0, 1, "INTENT".into()));
                assert!(
                    LocalJournal::new(&root)
                        .read_event_bytes("c4:cut")
                        .unwrap()
                        .is_some()
                );
            }
            _ => panic!("unknown child mode"),
        }
        return;
    }
    let cuts = [
        ("INTENT-before-commit", "absent"),
        ("INTENT-after-commit", "absent"),
        ("INTENT-after-barrier", "absent"),
        ("before-publication", "absent"),
        ("after-publication", "audit"),
        ("before-presence-proof", "audit"),
        ("PROVED_PRESENT-before-commit", "audit"),
        ("PROVED_PRESENT-after-commit", "present"),
        ("PROVED_PRESENT-after-barrier", "present"),
        ("SETTLED_PRESENT-before-commit", "present"),
        ("SETTLED_PRESENT-after-commit", "present"),
        ("SETTLED_PRESENT-after-barrier", "present"),
    ];
    for (n, (cut, resume)) in cuts.iter().enumerate() {
        let root = assigned(&format!("cut-{n:02}"));
        assert_eq!(child(&root, "append", Some(cut)).status.code(), Some(79));
        assert!(child(&root, resume, None).status.success(), "cut={cut}");
    }
    for (n, cut) in [
        "PROVED_ABSENT-before-commit",
        "PROVED_ABSENT-after-commit",
        "PROVED_ABSENT-after-barrier",
        "SETTLED_ABSENT-before-commit",
        "SETTLED_ABSENT-after-commit",
        "SETTLED_ABSENT-after-barrier",
    ]
    .iter()
    .enumerate()
    {
        let root = assigned(&format!("cut-{:02}", n + 12));
        assert_eq!(
            child(&root, "append", Some("before-publication"))
                .status
                .code(),
            Some(79)
        );
        assert_eq!(child(&root, "absent", Some(cut)).status.code(), Some(79));
        assert!(
            child(&root, "absent", None).status.success(),
            "absence cut={cut}"
        );
    }
}
#[test]
#[ignore = "requires assigned native private roots; preserved fault evidence"]
fn native_absence_proof_and_finalization_failure() {
    // SAFETY: explicitly isolated native test process.
    let _owner = unsafe { super::super::super::owner::isolated_test_owner() };
    let root = assigned("absence");
    enroll(&root);
    let failed = super::super::with_hook(
        |at| {
            if at == "before-publication" {
                Err(fail("injected prepublication stop"))
            } else {
                Ok(())
            }
        },
        || publish(&root, "c4:absent", 1, 1),
    );
    assert!(failed.is_err());
    // Stop after durable absence proof; another complete owner scope settles it.
    let stopped = super::super::with_hook(
        |at| {
            if at == "SETTLED_ABSENT-before-commit" {
                Err(fail("injected absence settlement stop"))
            } else {
                Ok(())
            }
        },
        || resume(&root),
    );
    assert!(stopped.is_err());
    assert_eq!(counts(&root), (0, 0, 1, "PROVED_ABSENT".into()));
    assert_eq!(resume(&root).unwrap().head, Some(0));
    assert_eq!(counts(&root), (0, 0, 1, "SETTLED_ABSENT".into()));
    let root = assigned("unsettled");
    enroll(&root);
    let result = super::super::with_hook(
        |at| {
            if at == "before-presence-proof" {
                Err(fail("injected finalization stop"))
            } else {
                Ok(())
            }
        },
        || publish(&root, "c4:known", 1, 1),
    )
    .unwrap();
    assert_eq!(result.outcome, Some(EventWriteOutcome::Created));
    assert!(!result.finalized);
    assert!(result.head.is_none());
    assert!(result.detail.unwrap().contains("finalization stop"));
    assert_eq!(counts(&root), (0, 0, 1, "INTENT".into()));
    assert!(resume(&root).is_err());
}
#[test]
#[ignore = "requires assigned native private roots; corruption must fence publication"]
fn native_corruption_and_permit_denials() {
    // SAFETY: explicitly isolated native test process.
    let _owner = unsafe { super::super::super::owner::isolated_test_owner() };
    for (n,mutation) in ["DELETE FROM current","DELETE FROM attempt","DELETE FROM receipt","UPDATE meta SET prefix_sha256='0000000000000000000000000000000000000000000000000000000000000000'","UPDATE current SET proof_json='{}'"].iter().enumerate(){
        let root=assigned(&format!("corrupt-{n:02}"));enroll(&root);assert!(publish(&root,"c4:kept",1,1).unwrap().finalized);
        oracle(&root,|db| {
            // This test deliberately creates an orphaned receipt. Disable only
            // this raw fixture connection's FK check; production stays strict.
            if *mutation == "DELETE FROM attempt" {
                db.pragma_update(None, "foreign_keys", false).unwrap();
                assert_eq!(db.pragma_query_value(None, "foreign_keys", |row| row.get::<_, i64>(0)).unwrap(), 0);
            }
            db.execute_batch(mutation).unwrap();
        });
        let before=fs::read_dir(root.join("events")).unwrap().count();
        assert!(publish(&root,"c4:forbidden",1,2).is_err());assert_eq!(fs::read_dir(root.join("events")).unwrap().count(),before);
    }
    let root = assigned("permit");
    enroll(&root);
    guarded(&root, |r, g| {
        let session = Session::new(r, g, REQUEST, ACTOR).unwrap();
        let snapshot = session
            .read(Some("c4:forbidden"), Some(&"2".repeat(64)))
            .unwrap();
        let prepared = prepare(event("c4:forbidden", 1)).unwrap();
        let permit = PublicationPermit {
            session: &session,
            snapshot: &snapshot,
            prepared_sha256: sha256_bytes_hex(prepared.bytes()),
            prepared_length: prepared.bytes().len(),
        };
        assert!(EventStore::publish_owned_progress_event(permit, &prepared).is_err());
        let permit = PublicationPermit {
            session: &session,
            snapshot: &snapshot,
            prepared_sha256: sha256_bytes_hex(prepared.bytes()),
            prepared_length: prepared.bytes().len(),
        };
        assert!(
            permit
                .authorize(&prepared)
                .unwrap_err()
                .to_string()
                .contains("already attempted")
        );
    });
    assert_eq!(fs::read_dir(root.join("events")).unwrap().count(), 2);
    let wrong = assigned("wrong-root");
    enroll(&wrong);
    guarded(&root, |_, guard| {
        let other = native::physical_root(&wrong).unwrap();
        assert!(Session::new(&other, guard, REQUEST, ACTOR).is_err());
    });
    // Detected uncooperative movement is refused, not made atomic. Reuse the
    // two allocated fixture objects and preserve both under their final names.
    let moved = wrong.with_file_name("wrong-root-retained");
    assert!(!moved.exists());
    let changed = std::rc::Rc::new(Cell::new(false));
    let hook_changed = changed.clone();
    let hook_wrong = wrong.clone();
    let hook_root = root.clone();
    let hook_moved = moved.clone();
    let result = super::super::with_hook(
        move |at| {
            if at == "after-publication" {
                fs::rename(&hook_wrong, &hook_moved).unwrap();
                fs::rename(&hook_root, &hook_wrong).unwrap();
                hook_changed.set(true);
            }
            Ok(())
        },
        || publish(&wrong, "c4:moved", 1, 1),
    );
    assert!(changed.get());
    assert!(
        result.is_err(),
        "detected movement must not report known Created"
    );
    assert_eq!(counts(&moved), (0, 0, 1, "INTENT".into()));
    assert!(
        LocalJournal::new(&moved)
            .read_event_bytes("c4:moved")
            .unwrap()
            .is_some()
    );
    assert!(
        LocalJournal::new(&wrong)
            .read_event_bytes("c4:moved")
            .unwrap()
            .is_none()
    );
    assert!(publish(&wrong, "c4:no-retarget", 1, 2).is_err());
}

fn denied(evidence: RetainedEvidence<'_>, profile: ScopeProfile, statement: &str) {
    let permit = LiveSqlPermit(super::super::super::owner::SqlPermit::enter().unwrap());
    let mut owner = NativeSqlOwner::prepare(evidence, profile);
    owner.open_native(&permit).unwrap();
    let sql = CString::new(statement).unwrap();
    // SAFETY: controlled test SQL while the actual native owner retains the slot.
    let rc = unsafe {
        ffi::sqlite3_exec(
            owner.connection,
            sql.as_ptr(),
            None,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        )
    };
    assert_eq!(
        rc,
        ffi::SQLITE_AUTH,
        "must deny in authorizer {profile:?}: {statement}"
    );
    let finish = owner.finish();
    drop(permit);
    if let Err(error) = finish {
        assert!(
            error.to_string().contains("Native(23)"),
            "unexpected finish error {error}"
        );
    }
    super::super::super::owner::require_quiescent().unwrap();
}
#[test]
#[ignore = "requires assigned native private roots; actual native profile denial"]
fn native_closed_sql_profile_permissions() {
    // SAFETY: explicitly isolated native test process.
    let _owner = unsafe { super::super::super::owner::isolated_test_owner() };
    let root = assigned("profiles");
    enroll(&root);
    guarded(&root, |r, g| {
        for profile in [
            ScopeProfile::Selected,
            ScopeProfile::CreateInit,
            ScopeProfile::ReopenInit,
            ScopeProfile::Certify,
            ScopeProfile::ReadyResume,
        ] {
            for statement in [
                "INSERT INTO attempt VALUES('x','k','0000000000000000000000000000000000000000000000000000000000000000',1)",
                "INSERT INTO receipt VALUES(1,'k','0000000000000000000000000000000000000000000000000000000000000000',1,'x')",
                "UPDATE current SET phase='INTENT'",
            ] {
                let stage = if profile == ScopeProfile::Selected {
                    EvidenceStage::Selected
                } else {
                    EvidenceStage::ReadyResume
                };
                let evidence =
                    RetainedEvidence::capture_before_sql(r, g, stage, REQUEST, ACTOR).unwrap();
                denied(evidence, profile, statement);
            }
        }
        for profile in [ScopeProfile::Append, ScopeProfile::Reconcile] {
            for statement in [
                "DELETE FROM current",
                "UPDATE meta SET state='fenced'",
                "UPDATE meta SET epoch='0000000000000000000000000000000000000000000000000000000000000000'",
                "UPDATE baseline SET record_length=0",
                "CREATE TABLE extra(x)",
            ] {
                let stage = if profile == ScopeProfile::Append {
                    EvidenceStage::Append
                } else {
                    EvidenceStage::Reconcile
                };
                denied(
                    RetainedEvidence::capture_before_sql(r, g, stage, REQUEST, ACTOR).unwrap(),
                    profile,
                    statement,
                );
            }
        }
        denied(
            RetainedEvidence::capture_before_sql(r, g, EvidenceStage::Reconcile, REQUEST, ACTOR)
                .unwrap(),
            ScopeProfile::Reconcile,
            "INSERT INTO attempt VALUES('x','k','0000000000000000000000000000000000000000000000000000000000000000',1)",
        );
    });
    assert_eq!(counts(&root), (0, 0, 0, "INIT".into()));
    assert!(publish(&root, "c4:allowed", 1, 1).unwrap().finalized);
}

// This independent Windows control does not replace the preserved failed
// parent-root movement case or claim that a live store tree can be renamed.
#[cfg(windows)]
#[test]
#[ignore = "requires one assigned native private root; preserve selector movement"]
fn native_selector_movement_after_publication() {
    // SAFETY: the focused runner launches this in one disposable test process.
    let _owner = unsafe { super::super::super::owner::isolated_test_owner() };
    let root = assigned("move-selector");
    enroll(&root);
    let active = root.join("append-progress/active.json");
    let retained = root.join("append-progress/active-retained.json");
    assert!(!retained.exists());
    let original = fs::read(&active).unwrap();
    let prepared = prepare(event("c4:selector-moved", 1)).unwrap();
    let expected_carrier = prepared.bytes().to_vec();
    let changed = std::rc::Rc::new(Cell::new(false));
    let hook_changed = changed.clone();
    let hook_active = active.clone();
    let hook_retained = retained.clone();
    let result = guarded(&root, |r, g| {
        let root_before = native::root_snapshot(r).unwrap();
        let authority_before = g.physical_lock_identity().unwrap();
        let result = super::super::with_hook(
            move |at| {
                if at == "after-publication" {
                    assert!(!hook_changed.get(), "exactly one movement injection");
                    fs::rename(&hook_active, &hook_retained)
                        .expect("selector movement must occur; denial is not coverage");
                    hook_changed.set(true);
                }
                Ok(())
            },
            || append(r, g, REQUEST, ACTOR, &format!("{:064x}", 1), prepared),
        );
        // Keep actual root and authority ownership throughout append and refusal.
        assert_eq!(g.physical_lock_identity().unwrap(), authority_before);
        g.validate_physical_root(&root).unwrap();
        assert_eq!(native::root_snapshot(r).unwrap(), root_before);
        result
    });
    assert!(changed.get());
    let error = result.unwrap_err().to_string();
    assert!(
        error.contains("retained evidence namespace changed"),
        "{error}"
    );
    assert!(!active.exists());
    assert_eq!(fs::read(&retained).unwrap(), original);
    assert_eq!(counts(&root), (0, 0, 1, "INTENT".into()));
    assert_eq!(
        LocalJournal::new(&root)
            .read_event_bytes("c4:selector-moved")
            .unwrap()
            .as_deref(),
        Some(expected_carrier.as_slice())
    );
    // No restore, retry, retarget or cleanup: retain both ledger and carrier.
}
