#![cfg(test)]

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::canonical_hash::sha256_bytes_hex;

const INVENTORY: &str = include_str!("../../../scripts/append-progress-cases.json");
const CHILD_MARKER: &str = "append-progress-child:";
thread_local! {
    static ROOTS: RefCell<Vec<Assignment>> = const { RefCell::new(Vec::new()) };
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Assignment {
    version: u32,
    run: String,
    case: String,
    ordinal: usize,
    root: PathBuf,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ChildAssignment {
    fixture: Assignment,
    child: String,
    mode: String,
}

pub(crate) struct TestRoot(PathBuf);
impl TestRoot {
    pub(crate) fn path(&self) -> &Path {
        &self.0
    }
}

fn case(name: &str) -> Value {
    let cases: Vec<Value> = serde_json::from_str(INVENTORY).unwrap();
    cases
        .into_iter()
        .find(|case| case["name"] == name)
        .expect("unmapped fixture consumer")
}

fn current_case() -> String {
    std::thread::current()
        .name()
        .expect("named isolated test")
        .to_owned()
}

fn read_bound(path_key: &str, hash_key: &str) -> Vec<u8> {
    let bytes = std::fs::read(std::env::var_os(path_key).expect(path_key)).unwrap();
    assert_eq!(
        sha256_bytes_hex(&bytes),
        std::env::var(hash_key).expect(hash_key)
    );
    bytes
}

pub(crate) fn test_root() -> TestRoot {
    assert!(
        std::env::var_os("POINTBREAK_APPEND_TEST_CHILD").is_none(),
        "child cannot allocate"
    );
    for key in [
        "POINTBREAK_ENROLLMENT_RUN",
        "POINTBREAK_ENROLLMENT_MANIFEST",
    ] {
        assert!(
            std::env::var_os(key).is_none(),
            "historical fixture environment is not admitted"
        );
    }
    let name = current_case();
    let control = case(&name);
    let ordinal = ROOTS.with(|roots| roots.borrow().len() + 1);
    assert!(
        ordinal
            <= control[std::env::consts::OS]
                .as_u64()
                .expect("supported native host") as usize,
        "excess fixture allocation for {name}"
    );
    assert_eq!(
        std::env::var_os("POINTBREAK_APPEND_FIXTURES").is_some(),
        std::env::var_os("POINTBREAK_APPEND_FIXTURES_SHA256").is_some(),
        "incomplete fixture binding"
    );
    let assignment = if std::env::var_os("POINTBREAK_APPEND_FIXTURES").is_some() {
        let bytes = read_bound(
            "POINTBREAK_APPEND_FIXTURES",
            "POINTBREAK_APPEND_FIXTURES_SHA256",
        );
        let manifest: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(manifest["schema"], "append-progress-fixtures-v1");
        assert_eq!(manifest["host"], std::env::consts::OS);
        let run = manifest["run"].as_str().unwrap();
        assert!(run.starts_with("append-gate-"));
        let assigned: Vec<Assignment> =
            serde_json::from_value(manifest["allocations"].clone()).unwrap();
        let mut matches = assigned
            .into_iter()
            .filter(|a| a.case == name && a.ordinal == ordinal);
        let assignment = matches.next().expect("unassigned case/ordinal");
        assert!(matches.next().is_none(), "ambiguous allocation");
        assert_eq!(assignment.version, 1);
        assert_eq!(assignment.run, run);
        assert_eq!(
            assignment.root.parent().unwrap(),
            Path::new(manifest["parent"].as_str().unwrap())
        );
        assignment
    } else {
        assert_eq!(
            std::env::consts::OS,
            "macos",
            "Windows requires the explicit native gate"
        );
        let parent = ROOTS
            .with(|roots| {
                roots
                    .borrow()
                    .first()
                    .map(|a| a.root.parent().unwrap().to_owned())
            })
            .unwrap_or_else(|| {
                tempfile::Builder::new()
                    .prefix("append-gate-")
                    .tempdir()
                    .unwrap()
                    .keep()
                    .canonicalize()
                    .unwrap()
            });
        Assignment {
            version: 1,
            run: parent.file_name().unwrap().to_str().unwrap().to_owned(),
            case: name,
            ordinal,
            root: parent.join(format!("root-{ordinal}")),
        }
    };
    assert!(assignment.root.is_absolute());
    assert!(
        !assignment.root.exists(),
        "absent-only fixture; preserve collision"
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt as _;
        std::fs::DirBuilder::new()
            .mode(0o700)
            .create(&assignment.root)
            .unwrap();
    }
    #[cfg(windows)]
    fixture_acl(
        if control["id"] == "S24" && ordinal == 2 {
            "Negative"
        } else {
            "Prepare"
        },
        &assignment.root,
        &assignment.root,
    );
    println!(
        "append-progress-allocation:{}",
        serde_json::to_string(&assignment).unwrap()
    );
    let root = TestRoot(assignment.root.clone());
    ROOTS.with(|roots| roots.borrow_mut().push(assignment));
    root
}

fn child_assignment(
    bytes: &[u8],
    hash: &str,
    root: &Path,
    context: (&str, &str, &str),
) -> Result<Assignment, String> {
    if sha256_bytes_hex(bytes) != hash {
        return Err("assignment hash".into());
    }
    let envelope: ChildAssignment =
        serde_json::from_slice(bytes).map_err(|_| "assignment shape")?;
    let (parent, child, mode) = context;
    if envelope.child != child || envelope.mode != mode {
        return Err("child context".into());
    }
    let assignment = envelope.fixture;
    if assignment.version != 1
        || !assignment.run.starts_with("append-gate-")
        || assignment.ordinal == 0
        || !assignment.root.is_absolute()
        || assignment.root != root
        || assignment.case != parent
    {
        return Err("assignment identity".into());
    }
    Ok(assignment)
}

pub(super) fn enter_child(root: &Path, mode: &str) {
    let bytes = std::env::var("POINTBREAK_APPEND_TEST_ASSIGNMENT").expect("assigned child");
    let hash = std::env::var("POINTBREAK_APPEND_TEST_ASSIGNMENT_SHA256").unwrap();
    let child = current_case();
    assert_eq!(
        std::env::var("POINTBREAK_APPEND_TEST_CHILD").unwrap(),
        child
    );
    assert_eq!(std::env::var("POINTBREAK_APPEND_TEST_MODE").unwrap(), mode);
    let parent = if child.ends_with("::checkpoint_progress_child") {
        "session::append_progress::tests::controls::checkpoint_progress_fresh_process_recovery"
            .to_owned()
    } else {
        child.clone()
    };
    let assignment = child_assignment(bytes.as_bytes(), &hash, root, (&parent, &child, mode))
        .expect("bound parent assignment");
    assert!(assignment.ordinal <= case(&parent)[std::env::consts::OS].as_u64().unwrap() as usize);
    assert!(root.is_dir());
    println!("\n{CHILD_MARKER}{hash}:{child}:{mode}");
    use std::io::Write as _;
    std::io::stdout().flush().unwrap();
}

pub(super) fn run_child(mut command: Command, root: &Path, child: &str, mode: &str) -> ExitStatus {
    let assignment = ROOTS
        .with(|roots| roots.borrow().iter().find(|a| a.root == root).cloned())
        .expect("parent owns assigned root");
    assert_eq!(assignment.case, current_case());
    let envelope = ChildAssignment {
        fixture: assignment,
        child: child.into(),
        mode: mode.into(),
    };
    let bytes = serde_json::to_string(&envelope).unwrap();
    let hash = sha256_bytes_hex(bytes.as_bytes());
    command.args(["--exact", child, "--nocapture"]);
    if cfg!(windows) || child.ends_with("::checkpoint_progress_child") {
        command.arg("--ignored");
    }
    command
        .env("POINTBREAK_APPEND_TEST_ASSIGNMENT", &bytes)
        .env("POINTBREAK_APPEND_TEST_ASSIGNMENT_SHA256", &hash)
        .env("POINTBREAK_APPEND_TEST_CHILD", child)
        .env("POINTBREAK_APPEND_TEST_MODE", mode);
    let output = command.output().expect("isolated child launch");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    println!(
        "child {child} mode={mode} status={}\n{stdout}\n{stderr}",
        output.status
    );
    let expected = format!("{CHILD_MARKER}{hash}:{child}:{mode}");
    assert_eq!(
        stdout.lines().filter(|line| *line == expected).count(),
        1,
        "child must enter the exact case once; exit0 alone does not prove test selection"
    );
    output.status
}

#[cfg(windows)]
pub(crate) fn fixture_acl(mode: &str, root: &Path, target: &Path) -> Value {
    let script = std::env::var_os("POINTBREAK_ENROLLMENT_ACL_SCRIPT").expect("bound ACL helper");
    let _ = read_bound(
        "POINTBREAK_ENROLLMENT_ACL_SCRIPT",
        "POINTBREAK_ENROLLMENT_ACL_SHA256",
    );
    let shell = r"C:\Windows\System32\WindowsPowerShell\v1.0\powershell.exe";
    assert_eq!(
        sha256_bytes_hex(&std::fs::read(shell).unwrap()),
        std::env::var("POINTBREAK_APPEND_POWERSHELL_SHA256").expect("original shell pin")
    );
    let output = Command::new(shell)
        .args(["-NoLogo", "-NoProfile", "-NonInteractive", "-File"])
        .arg(script)
        .arg("-Mode")
        .arg(mode)
        .arg("-Root")
        .arg(root)
        .arg("-Target")
        .arg(target)
        .output()
        .expect("native fixture helper launch");
    println!(
        "fixture ACL mode={mode} status={} stdout={} stderr={}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        output.status.success(),
        "native helper failed; preserve root, no retry"
    );
    let receipt: Value = serde_json::from_slice(&output.stdout).expect("independent oracle JSON");
    assert_eq!(receipt["passed"], true);
    receipt
}

#[test]
fn child_assignment_rejects_wrong_source_and_root() {
    let root = std::env::temp_dir().join("append-assignment-never-created");
    let assignment = Assignment {
        version: 1,
        run: "append-gate-fixture".into(),
        case: "parent".into(),
        ordinal: 1,
        root: root.clone(),
    };
    // Reject a child entering another case/root/mode even when its exit status is zero.
    let envelope = ChildAssignment {
        fixture: assignment,
        child: "child".into(),
        mode: "cut".into(),
    };
    let bytes = serde_json::to_vec(&envelope).unwrap();
    let hash = sha256_bytes_hex(&bytes);
    let context = ("parent", "child", "cut");
    assert!(child_assignment(&bytes, &hash, &root, context).is_ok());
    assert!(child_assignment(&bytes, "wrong", &root, context).is_err());
    assert!(child_assignment(&bytes, &hash, &root.with_extension("foreign"), context).is_err());
    for wrong in [
        ("other", "child", "cut"),
        ("parent", "other", "cut"),
        ("parent", "child", "resume"),
    ] {
        assert!(child_assignment(&bytes, &hash, &root, wrong).is_err());
    }
    for (field, value) in [
        ("version", serde_json::json!(2)),
        ("run", serde_json::json!("ne-m")),
        ("ordinal", serde_json::json!(0)),
    ] {
        let mut changed = serde_json::to_value(&envelope).unwrap();
        changed["fixture"][field] = value;
        let changed = serde_json::to_vec(&changed).unwrap();
        assert!(child_assignment(&changed, &sha256_bytes_hex(&changed), &root, context).is_err());
    }
}
