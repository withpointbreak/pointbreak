//! Actual loopback transport and owned child controls; never part of default tests.
use std::io::{Read as _, Write as _};
use std::net::TcpStream;

use super::*;

fn fixture(parent: &Path) -> (PathBuf, PathBuf, Value) {
    assert!(parent.is_absolute() && parent.is_dir());
    assert_eq!(std::fs::read_dir(parent).unwrap().count(), 0);
    let repo = parent.join("repo");
    let home = parent.join("home");
    std::fs::create_dir(&repo).unwrap();
    std::fs::create_dir(&home).unwrap();
    assert!(
        Command::new("/usr/bin/git")
            .env_clear()
            .env("PATH", "/usr/bin:/bin:/usr/sbin:/sbin")
            .env("HOME", &home)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .args(["init", "--quiet"])
            .current_dir(&repo)
            .status()
            .unwrap()
            .success()
    );
    std::fs::create_dir(home.join("keys")).unwrap();
    let key = pointbreak::keys::generate_key_in(
        &home.join("keys"),
        &pointbreak::keys::KeyName::parse("consumer").unwrap(),
    )
    .unwrap();
    let declaration = success(&invoke(
        &home,
        "sqlite-wal-bodyless-v1",
        &owned(
            "enroll-empty",
            &args(
                &repo,
                key.private_key_path(),
                "control-first",
                &"71".repeat(32),
            ),
        ),
    ));
    success(&invoke_data(&home, data_request(&repo, "rebuild")));
    (repo, home, declaration)
}

fn request(port: u16, token: Option<&str>, method: &str, path: &str, extra: &str) -> TcpStream {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(40)))
        .unwrap();
    stream
        .set_write_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    write!(
        stream,
        "{method} {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\n"
    )
    .unwrap();
    if let Some(token) = token {
        write!(stream, "Authorization: Bearer {token}\r\n").unwrap();
    }
    write!(stream, "{extra}Connection: close\r\n\r\n").unwrap();
    stream
}
fn response(mut stream: TcpStream) -> (u16, String, Vec<u8>) {
    let mut bytes = Vec::new();
    std::io::Read::by_ref(&mut stream)
        .take(2 * 1024 * 1024)
        .read_to_end(&mut bytes)
        .unwrap();
    let split = bytes
        .windows(4)
        .position(|part| part == b"\r\n\r\n")
        .unwrap();
    let headers = String::from_utf8(bytes[..split].to_vec()).unwrap();
    let code = headers.split_whitespace().nth(1).unwrap().parse().unwrap();
    (code, headers, bytes[split + 4..].to_vec())
}
fn status(port: u16, token: &str) -> Value {
    let start = Instant::now();
    let (code, _, bytes) = response(request(
        port,
        Some(token),
        "GET",
        "/api/derived-access/status",
        "",
    ));
    assert_eq!(code, 200);
    assert!(
        start.elapsed() < Duration::from_secs(2),
        "detached status blocked"
    );
    let value: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(value["servingCurrent"], false);
    value
}
fn until(port: u16, token: &str, accept: impl Fn(&Value) -> bool) -> Value {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let value = status(port, token);
        if accept(&value) {
            return value;
        }
        assert!(Instant::now() < deadline, "control did not settle: {value}");
        std::thread::sleep(Duration::from_millis(50));
    }
}
fn children(shell: u32) -> Vec<u32> {
    let out = Command::new("/bin/ps")
        .args(["-axo", "pid=,ppid=,comm="])
        .output()
        .unwrap();
    assert!(out.status.success());
    String::from_utf8(out.stdout)
        .unwrap()
        .lines()
        .filter_map(|line| {
            let fields: Vec<_> = line.split_whitespace().collect();
            (fields.get(1)?.parse::<u32>().ok()? == shell
                && fields.get(2)? == &env!("CARGO_BIN_EXE_pointbreak"))
                .then(|| fields[0].parse().unwrap())
        })
        .collect()
}
fn signal_child(shell: u32, child: u32, signal: i32) {
    assert_eq!(children(shell), [child], "ownership changed; no signal");
    // SAFETY: exact newly spawned shell's only child, revalidated immediately.
    assert_eq!(unsafe { libc::kill(child as i32, signal) }, 0);
}

#[test]
#[ignore = "exact source/caller review and explicit signal/native admission; fresh retained fixture"]
fn http_shell_bounds_queue_and_retires_failed_child() {
    let parent = PathBuf::from(std::env::var_os("POINTBREAK_CONSUMER_FIXTURE").unwrap());
    let (repo, home, declaration) = fixture(&parent);
    #[cfg(not(feature = "longitudinal-counting"))]
    let _ = declaration;
    let stable = tree_bytes(&repo.join(".git/pointbreak"));
    let startup = parent.join("shell-startup.private.jsonl");
    let mut shell = Command::new(env!("CARGO_BIN_EXE_pointbreak"))
        .env_clear()
        .env("PATH", "/usr/bin:/bin:/usr/sbin:/sbin")
        .env("HOME", &home)
        .env("POINTBREAK_HOME", &home)
        .env("POINTBREAK_ACTOR_ID", "actor:consumer-test")
        .env("POINTBREAK_DERIVED_ACCESS", "sqlite-wal-bodyless-v1")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .arg("__owned-inspect-v1")
        .stdin(Stdio::piped())
        .stdout(std::fs::File::create_new(&startup).unwrap())
        .stderr(std::fs::File::create_new(parent.join("shell.stderr")).unwrap())
        .spawn()
        .unwrap();
    std::fs::write(parent.join("shell.pid"), shell.id().to_string()).unwrap();
    writeln!(shell.stdin.take().unwrap(), "{}", serde_json::json!({"version":1,"repository":repo,"selection":{"requestId":"1".repeat(64),"enrollmentActor":"actor:consumer-test"},"host":"127.0.0.1","port":0})).unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    let start: Value = loop {
        if let Ok(value) = serde_json::from_slice(&std::fs::read(&startup).unwrap()) {
            break value;
        }
        assert!(
            Instant::now() < deadline,
            "shell startup unknown; retain PID"
        );
        std::thread::sleep(Duration::from_millis(20));
    };
    let port = start["port"].as_u64().unwrap() as u16;
    let token = start["token"].as_str().unwrap();
    until(port, token, |s| {
        s["child"]["operation"].is_null()
            && !s["child"]["observationPending"].as_bool().unwrap()
            && s["child"]["state"] == "completed"
    });
    assert_eq!(
        response(request(port, Some(token), "GET", "/api/v2/changes", "")).0,
        200
    );
    #[cfg(feature = "longitudinal-counting")]
    {
        use base64::Engine as _;
        let codec = base64::engine::general_purpose::URL_SAFE_NO_PAD;
        let encoded = codec.encode(serde_json::to_vec(&serde_json::json!({"runIdentity":"a".repeat(64),"context":{"rootIdentity":"b".repeat(64),"operation":"WARM_HEAD","phase":"warm","baseExecutionIdentitySha256":"c".repeat(64),"derivativeExecutionIdentitySha256":"d".repeat(64),"manifestSha256":"e".repeat(64),"scheduleSha256":"f".repeat(64),"success":false,"semanticResultSha256":"1".repeat(64),"includeCapacityOwnership":true}})).unwrap());
        let (code, headers, body) = response(request(
            port,
            Some(token),
            "GET",
            "/api/v2/history?limit=10",
            &format!("X-Pointbreak-Longitudinal-Counting: {encoded}\r\n"),
        ));
        // Retain transport evidence before decoding or asserting its contents.
        std::fs::write(parent.join("counted-response.body.json"), &body).unwrap();
        std::fs::write(parent.join("counted-response.headers.txt"), &headers).unwrap();
        let encoded = headers
            .lines()
            .find_map(|line| {
                line.split_once(':')
                    .filter(|(name, _)| {
                        name.eq_ignore_ascii_case("X-Pointbreak-Longitudinal-Receipt")
                    })
                    .map(|(_, value)| value.trim())
            })
            .unwrap();
        let receipt: Value = serde_json::from_slice(&codec.decode(encoded).unwrap()).unwrap();
        std::fs::write(
            parent.join("counted-request.json"),
            serde_json::to_vec(&receipt).unwrap(),
        )
        .unwrap();
        assert_eq!(code, 200);
        let timeline: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(timeline["schema"], "pointbreak.inspect-event-history");
        assert_eq!(timeline["version"], 1);
        let entries = timeline["entries"].as_array().unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0]["eventType"], "change_declared");
        assert_eq!(entries[0]["eventId"], declaration["events"][0]["eventId"]);
        assert_eq!(entries[0]["subject"]["changeId"], declaration["changeId"]);
        assert_eq!(receipt["counters"]["responseBytes"], body.len());
        assert_eq!(receipt["success"], true);
        for counter in [
            "carrierOpens",
            "timelineSqliteCandidates",
            "timelineSqliteWindowRows",
        ] {
            assert!(
                receipt["counters"][counter].as_u64().unwrap() > 0,
                "counted Timeline performed no {counter} work"
            );
        }
        assert!(
            !response(request(port, Some(token), "GET", "/api/v2/changes", ""))
                .1
                .to_ascii_lowercase()
                .contains("x-pointbreak-longitudinal-receipt")
        );
    }
    until(port, token, |s| {
        s["child"]["operation"].is_null() && s["child"]["observationPending"] == false
    });
    let owned = children(shell.id());
    assert_eq!(owned.len(), 1);
    let child = owned[0];
    std::fs::write(parent.join("data-child.pid"), child.to_string()).unwrap();
    signal_child(shell.id(), child, libc::SIGSTOP);
    // An active read pins the detached identity and prevents status refresh from
    // occupying a queue slot. No fake data response participates.
    let active = request(port, Some(token), "GET", "/api/v2/changes", "");
    let running = until(port, token, |s| !s["child"]["operation"].is_null());
    let identity = running["child"]["requestId"].clone();
    for auth in [None, Some("wrong")] {
        let began = Instant::now();
        assert_eq!(
            response(request(port, auth, "GET", "/api/v2/changes", "")).0,
            401
        );
        assert!(began.elapsed() < Duration::from_secs(2));
    }
    let began = Instant::now();
    assert_eq!(response(request(port, None, "GET", "/", "")).0, 200);
    assert!(began.elapsed() < Duration::from_secs(2));
    let (send, receive) = std::sync::mpsc::channel();
    let queued: Vec<_> = (0..9)
        .map(|_| {
            let stream = request(port, Some(token), "GET", "/api/v2/changes", "");
            let send = send.clone();
            std::thread::spawn(move || {
                let result = response(stream);
                send.send(result).unwrap();
            })
        })
        .collect();
    let overflow = receive.recv_timeout(Duration::from_secs(5)).unwrap();
    assert_eq!(overflow.0, 503);
    assert!(String::from_utf8_lossy(&overflow.2).contains("queue full"));
    assert_eq!(status(port, token)["child"]["requestId"], identity);
    assert_eq!(
        response(request(
            port,
            Some(token),
            "POST",
            "/api/derived-access/retry",
            ""
        ))
        .0,
        409
    );
    let expired = response(active);
    assert_eq!(expired.0, 503);
    assert!(String::from_utf8_lossy(&expired.2).contains("no replay"));
    for _ in 0..8 {
        let result = receive.recv_timeout(Duration::from_secs(5)).unwrap();
        assert_eq!(result.0, 503);
        assert!(String::from_utf8_lossy(&result.2).contains("wait expired"));
    }
    for thread in queued {
        thread.join().unwrap();
    }
    assert_eq!(status(port, token)["child"]["state"], "unknown");
    assert_eq!(
        response(request(
            port,
            Some(token),
            "POST",
            "/api/derived-access/retry",
            ""
        ))
        .0,
        409
    );
    signal_child(shell.id(), child, libc::SIGCONT);
    let settled = until(port, token, |s| {
        s["child"]["lastCompletedRequestId"] == identity && s["child"]["state"] == "completed"
    });
    assert_eq!(settled["child"]["lastCompletedOutcome"], "succeeded");
    // A held explicit mutation is never replayed after transport loss. Cancel
    // responsiveness is measured here; execution is proven by the pin control.
    until(port, token, |s| {
        s["child"]["operation"].is_null() && s["child"]["observationPending"] == false
    });
    signal_child(shell.id(), child, libc::SIGSTOP);
    assert_eq!(
        response(request(
            port,
            Some(token),
            "POST",
            "/api/derived-access/retry",
            ""
        ))
        .0,
        200
    );
    let mutation = until(port, token, |s| !s["child"]["operation"].is_null());
    assert_eq!(
        response(request(
            port,
            Some(token),
            "POST",
            "/api/derived-access/retry",
            ""
        ))
        .0,
        200
    );
    assert_eq!(
        status(port, token)["child"]["requestId"],
        mutation["child"]["requestId"]
    );
    let began = Instant::now();
    assert_eq!(
        response(request(
            port,
            Some(token),
            "POST",
            "/api/derived-access/cancel",
            ""
        ))
        .0,
        200
    );
    assert!(began.elapsed() < Duration::from_secs(2));
    signal_child(shell.id(), child, libc::SIGKILL);
    until(port, token, |s| s["child"]["state"] == "unknown");
    assert_eq!(
        tree_bytes(&repo.join(".git/pointbreak")),
        stable,
        "held rebuild changed fixture"
    );
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let retry = response(request(
            port,
            Some(token),
            "POST",
            "/api/derived-access/retry",
            "",
        ))
        .0;
        if retry == 200 {
            break;
        }
        assert_eq!(retry, 409, "replacement refused for an unexpected reason");
        assert!(Instant::now() < deadline, "reap or I/O retirement UNKNOWN");
        assert_eq!(status(port, token)["child"]["epoch"], 1);
        std::thread::sleep(Duration::from_millis(20));
    }
    let replaced = until(port, token, |s| {
        s["child"]["epoch"] == 2
            && s["child"]["state"] == "completed"
            && s["child"]["operation"].is_null()
    });
    assert!(replaced["child"]["lastCompletedRequestId"].is_null());
    let replacement = children(shell.id());
    assert_eq!(replacement.len(), 1);
    assert_ne!(replacement[0], child);
    assert_eq!(
        response(request(port, Some(token), "GET", "/api/v2/changes", "")).0,
        200
    );
    assert_eq!(
        tree_bytes(&repo.join(".git/pointbreak")),
        stable,
        "replacement replayed mutation"
    );
    std::fs::write(parent.join("http-control-result.json"), serde_json::to_vec(&serde_json::json!({"shell":shell.id(),"firstChild":child,"replacementChild":replacement[0],"active":running,"unknownMutation":mutation,"settled":settled,"replacement":replaced,"queueAdmitted":8,"queueRefused":1})).unwrap()).unwrap();
    // Normal documented Ctrl-C shutdown, then child's inherited stdin EOF.
    assert_eq!(unsafe { libc::kill(shell.id() as i32, libc::SIGINT) }, 0);
    let deadline = Instant::now() + Duration::from_secs(5);
    while shell.try_wait().unwrap().is_none() {
        assert!(Instant::now() < deadline, "shell shutdown unknown");
        std::thread::sleep(Duration::from_millis(20));
    }
    // Ownership inventory alone loses an orphan. Probe this known PID as well.
    while unsafe { libc::kill(replacement[0] as i32, 0) } == 0 {
        assert!(Instant::now() < deadline, "child EOF exit unknown");
        std::thread::sleep(Duration::from_millis(20));
    }
    assert_eq!(
        std::io::Error::last_os_error().raw_os_error(),
        Some(libc::ESRCH)
    );
    println!(
        "HTTP queue/auth/unknown settlement/cancel response/reap-replacement controls passed; fixtures retained"
    );
}

#[test]
#[ignore = "exact library caller review and native admission; four fresh retained pin fixtures"]
fn request_pin_controls_use_real_finished_native_phases() {
    use sha2::{Digest as _, Sha256};
    let parent = PathBuf::from(std::env::var_os("POINTBREAK_CONSUMER_FIXTURE").unwrap());
    assert_eq!(std::fs::read_dir(&parent).unwrap().count(), 0);
    let library = PathBuf::from(std::env::var_os("POINTBREAK_CONSUMER_LIBTEST").unwrap());
    assert!(library.is_absolute());
    assert_eq!(
        format!("{:x}", Sha256::digest(std::fs::read(&library).unwrap())),
        std::env::var("POINTBREAK_CONSUMER_LIBTEST_SHA256").unwrap()
    );
    for mode in ["cancel", "selection", "publication", "journal"] {
        let root = parent.join(mode);
        std::fs::create_dir(&root).unwrap();
        let (_, home, _) = fixture(&root);
        std::fs::write(root.join("pin-control-owner"), b"owned-pin-control-v1\n").unwrap();
        let mut child = Command::new(&library).env_clear().env("PATH", "/usr/bin:/bin:/usr/sbin:/sbin")
            .env("HOME", &home).env("POINTBREAK_HOME", &home).env("POINTBREAK_ACTOR_ID", "actor:consumer-test")
            .env("POINTBREAK_DERIVED_ACCESS", "sqlite-wal-bodyless-v1").env("GIT_CONFIG_NOSYSTEM", "1").env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("POINTBREAK_CONSUMER_FIXTURE", &root).env("POINTBREAK_PIN_CONTROL", mode)
            .env("POINTBREAK_CONSUMER_PRODUCT", env!("CARGO_BIN_EXE_pointbreak"))
            .env("POINTBREAK_CONSUMER_PRODUCT_SHA256", format!("{:x}", Sha256::digest(std::fs::read(env!("CARGO_BIN_EXE_pointbreak")).unwrap())))
            .args(["--exact", "session::derived_access::enrolled::pin_controls::finished_request_refuses_between_phase_mutation", "--ignored", "--nocapture", "--test-threads=1"])
            .stdin(Stdio::null()).stdout(std::fs::File::create_new(root.join("pin.stdout")).unwrap())
            .stderr(std::fs::File::create_new(root.join("pin.stderr")).unwrap()).spawn().unwrap();
        std::fs::write(root.join("pin.pid"), child.id().to_string()).unwrap();
        let deadline = Instant::now() + Duration::from_secs(90);
        while child.try_wait().unwrap().is_none() {
            assert!(
                Instant::now() < deadline,
                "pin process UNKNOWN; retain and stop"
            );
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(
            child.wait().unwrap().success(),
            "pin {mode} failed; retained output at {}",
            root.display()
        );
        let result: Value =
            serde_json::from_slice(&std::fs::read(root.join("pin-control-result.json")).unwrap())
                .unwrap();
        assert_eq!(result["mode"], mode);
        assert_eq!(result["documentEscaped"], false);
    }
}
