//! Shared HTTP harness for the `pointbreak inspect` integration suites.
//!
//! The inspector's JSON builders live in the binary crate
//! (`src/cli/inspect/api.rs`), so they are not reachable from an integration
//! test by a direct call. These tests instead exercise the genuine production
//! JSON end to end: they spawn the real `pointbreak inspect --port 0` server (which
//! prints its bound URL and supports an ephemeral port) and issue raw HTTP/1.1
//! GETs. The store is always built at test time through the `pointbreak` CLI, so it
//! tracks the current on-disk layout without hard-coding any store path.

use std::ffi::OsString;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{Shutdown, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use serde_json::Value;

use super::git_repo::GitRepo;
use super::pointbreak;

/// A repository plus a worktree on a fresh branch with one captured Revision.
pub struct WorktreeCapture {
    pub _main: GitRepo,
    pub _parent: tempfile::TempDir,
    pub worktree: PathBuf,
    pub revision_id: String,
}

impl WorktreeCapture {
    pub fn on_branch(dir_name: &str, branch: &str) -> Self {
        let main = GitRepo::new();
        main.write("src/lib.rs", "pub fn value() -> u32 { 1 }\n");
        main.commit_all("base");

        let parent = tempfile::tempdir().expect("worktree parent");
        let worktree = parent.path().join(dir_name);
        add_worktree(main.path(), &worktree, branch);
        std::fs::write(worktree.join("src/lib.rs"), "pub fn value() -> u32 { 2 }\n").unwrap();

        let revision_id = capture(&worktree);

        Self {
            _main: main,
            _parent: parent,
            worktree,
            revision_id,
        }
    }
}

/// A spawned `pointbreak inspect` server bound to an ephemeral port, killed on drop.
pub struct Inspector {
    child: Child,
    addr: String,
    startup_output: String,
    bearer: Option<String>,
    stderr: Arc<Mutex<String>>,
    _stdout_drain: thread::JoinHandle<()>,
    _legacy_clone: Option<tempfile::TempDir>,
}

fn legacy_mirrors() -> &'static Mutex<std::collections::HashMap<PathBuf, Vec<PathBuf>>> {
    static MIRRORS: std::sync::OnceLock<Mutex<std::collections::HashMap<PathBuf, Vec<PathBuf>>>> =
        std::sync::OnceLock::new();
    MIRRORS.get_or_init(|| Mutex::new(std::collections::HashMap::new()))
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InspectSurface {
    Web,
    ApiOnly,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InspectOutput {
    Text,
    Json,
}

impl Inspector {
    pub fn spawn(repo: &Path) -> Self {
        Self::spawn_web_text(repo)
    }

    /// Start against the supplied authority without constructing the explicit
    /// L0 compatibility fixture. Use this for capability-neutral and v2 routes.
    pub fn spawn_current(repo: &Path) -> Self {
        let inspector =
            Self::spawn_with_env_mode(repo, InspectSurface::Web, InspectOutput::Text, &[], false);
        inspector.wait_for_default_derived_generation();
        inspector
    }

    pub fn spawn_human(repo: &Path) -> Self {
        Self::spawn_web_text(repo)
    }

    pub fn spawn_authenticated(repo: &Path) -> Self {
        Self::spawn_api_json(repo)
    }

    pub fn spawn_authenticated_with_env(repo: &Path, env: &[(&str, &str)]) -> Self {
        Self::spawn_with_env(repo, InspectSurface::ApiOnly, InspectOutput::Json, env)
    }

    pub fn spawn_web_text_with_env(repo: &Path, env: &[(&str, &str)]) -> Self {
        Self::spawn_with_env(repo, InspectSurface::Web, InspectOutput::Text, env)
    }

    pub fn spawn_web_json_with_env(repo: &Path, env: &[(&str, &str)]) -> Self {
        Self::spawn_with_env(repo, InspectSurface::Web, InspectOutput::Json, env)
    }

    pub fn spawn_api_text_with_env(repo: &Path, env: &[(&str, &str)]) -> Self {
        Self::spawn_with_env(repo, InspectSurface::ApiOnly, InspectOutput::Text, env)
    }

    pub fn spawn_web_text(repo: &Path) -> Self {
        Self::spawn_with(repo, InspectSurface::Web, InspectOutput::Text)
    }

    pub fn spawn_web_json(repo: &Path) -> Self {
        Self::spawn_with(repo, InspectSurface::Web, InspectOutput::Json)
    }

    pub fn spawn_api_text(repo: &Path) -> Self {
        Self::spawn_with(repo, InspectSurface::ApiOnly, InspectOutput::Text)
    }

    pub fn spawn_api_json(repo: &Path) -> Self {
        Self::spawn_with(repo, InspectSurface::ApiOnly, InspectOutput::Json)
    }

    fn spawn_with(repo: &Path, surface: InspectSurface, output: InspectOutput) -> Self {
        let inspector = Self::spawn_with_env(repo, surface, output, &[]);
        inspector.wait_for_default_derived_generation();
        inspector
    }

    fn spawn_with_env(
        repo: &Path,
        surface: InspectSurface,
        output: InspectOutput,
        env: &[(&str, &str)],
    ) -> Self {
        Self::spawn_with_env_mode(repo, surface, output, env, true)
    }

    fn spawn_with_env_mode(
        repo: &Path,
        surface: InspectSurface,
        output: InspectOutput,
        env: &[(&str, &str)],
        legacy_compatibility: bool,
    ) -> Self {
        let legacy_clone = legacy_compatibility
            .then(|| prepare_legacy_inspector_clone(repo))
            .flatten();
        let effective_repo = legacy_clone
            .as_ref()
            .map_or(repo, |(_, clone)| clone.as_path());
        let mut command = Command::new(env!("CARGO_BIN_EXE_pointbreak"));
        command.args([
            "inspect",
            "--repo",
            effective_repo.to_str().unwrap(),
            "--host",
            "127.0.0.1",
            "--port",
            "0",
        ]);
        if surface == InspectSurface::ApiOnly {
            command.arg("--api-only");
        }
        if output == InspectOutput::Json {
            command.args(["--format", "json"]);
        }
        if !env
            .iter()
            .any(|(name, _)| *name == "POINTBREAK_DERIVED_ACCESS")
        {
            // Default-rollout coverage must remain hermetic when the parent
            // developer shell happens to carry an explicit rollback selector.
            command.env_remove("POINTBREAK_DERIVED_ACCESS");
        }
        command.envs(env.iter().copied());
        let mut child = command
            .env_remove("POINTBREAK_LOG")
            .env_remove("RUST_LOG")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("spawn pointbreak inspect");

        // Drain stderr in the background so it never blocks the server and is
        // available to explain a failure.
        let stderr = Arc::new(Mutex::new(String::new()));
        let mut child_stderr = child.stderr.take().expect("inspector stderr");
        {
            let sink = Arc::clone(&stderr);
            thread::spawn(move || {
                let mut buffer = String::new();
                let _ = child_stderr.read_to_string(&mut buffer);
                if let Ok(mut guard) = sink.lock() {
                    *guard = buffer;
                }
            });
        }

        // Read the complete startup output for the selected mode, then keep
        // draining stdout so the server never stalls on a full pipe.
        let stdout = child.stdout.take().expect("inspector stdout");
        let mut reader = BufReader::new(stdout);
        let mut startup_output = String::new();
        let line_count = if output == InspectOutput::Json { 1 } else { 4 };
        for _ in 0..line_count {
            let mut line = String::new();
            match reader.read_line(&mut line) {
                Ok(0) | Err(_) => break,
                Ok(_) => startup_output.push_str(&line),
            }
        }

        let (addr, bearer) = if output == InspectOutput::Json {
            let startup: Value =
                serde_json::from_str(startup_output.trim()).unwrap_or_else(|error| {
                    panic!(
                        "parse JSON inspector startup: {error}; stderr: {}",
                        drained(&stderr)
                    )
                });
            let host = startup["host"].as_str().expect("startup host");
            let port = startup["port"].as_u64().expect("startup port");
            let token = startup["token"].as_str().expect("startup token").to_owned();
            (format!("{host}:{port}"), Some(token))
        } else if surface == InspectSurface::Web {
            let capability = startup_output
                .lines()
                .find_map(|line| line.split_once("http://").map(|(_, value)| value.trim()))
                .unwrap_or_default();
            let (addr, fragment) = capability
                .split_once("/#")
                .unwrap_or_else(|| {
                    panic!(
                        "text capability has a fragment route; startup output: {startup_output:?}; stderr: {}",
                        drained(&stderr)
                    )
                });
            let token = fragment
                .split_once('?')
                .map(|(_, query)| query)
                .and_then(|query| {
                    query
                        .split('&')
                        .find_map(|pair| pair.strip_prefix("token="))
                })
                .map(str::to_owned)
                .expect("text web startup capability token");
            (addr.to_owned(), Some(token))
        } else {
            let addr = startup_output
                .lines()
                .find_map(|line| line.strip_prefix("  endpoint: http://"))
                .map(|value| value.trim().trim_end_matches('/').to_owned())
                .unwrap_or_default();
            let token = startup_output
                .lines()
                .find_map(|line| line.strip_prefix("  token: "))
                .map(str::to_owned);
            (addr, token)
        };
        let stdout_drain = thread::spawn(move || {
            let mut sink = String::new();
            let _ = reader.read_to_string(&mut sink);
        });

        assert!(
            !addr.is_empty(),
            "inspector did not print a bound url; stderr: {}",
            drained(&stderr)
        );

        // Wait until the server actually accepts connections, failing fast with
        // diagnostics if it exits before listening.
        let mut ready = false;
        for _ in 0..100 {
            if TcpStream::connect(&addr).is_ok() {
                ready = true;
                break;
            }
            if let Ok(Some(status)) = child.try_wait() {
                panic!(
                    "inspector exited before listening (status {status}) at {addr}; stderr: {}",
                    drained(&stderr)
                );
            }
            thread::sleep(Duration::from_millis(20));
        }
        assert!(
            ready,
            "inspector never accepted a connection at {addr}; stderr: {}",
            drained(&stderr)
        );

        Self {
            child,
            addr,
            startup_output,
            bearer,
            stderr,
            _stdout_drain: stdout_drain,
            _legacy_clone: legacy_clone.map(|(temp, _)| temp),
        }
    }

    pub fn startup_output(&self) -> &str {
        &self.startup_output
    }

    pub fn canonical_host(&self) -> &str {
        &self.addr
    }

    pub fn token(&self) -> Option<&str> {
        self.bearer.as_deref()
    }

    pub fn stderr_text(&self) -> String {
        drained(&self.stderr)
    }

    /// Rebuild the disposable legacy mirror's derived projection after a test
    /// appends through the current Change-aware source store. Production writes
    /// update their own sidecar; the compatibility mirror copies only authority
    /// bytes, so its running Inspector must exercise the explicit retry action.
    pub fn rebuild_legacy_derived_projection(&self) {
        let (status, body) =
            self.request_with_retry("POST", "/api/derived-access/retry", &self.default_headers());
        assert!(status.contains("200 OK"), "{status}: {body}");
        self.wait_for_default_derived_generation();
    }

    fn wait_for_default_derived_generation(&self) {
        let deadline = std::time::Instant::now() + Duration::from_secs(30);
        loop {
            // Activated Change stores intentionally do not build the legacy
            // Revision-history sidecar. Their `/api/v2` reader is already the
            // ready surface, so waiting for a generation that must remain
            // absent would turn a contract failure into a misleading timeout.
            if let Ok((_, body)) = self.try_get("/api/v2/profile")
                && let Ok(profile) = serde_json::from_str::<Value>(&body)
                && profile["availability"] == "ready"
            {
                return;
            }
            if let Ok((_, body)) = self.try_get("/api/derived-access/status")
                && let Ok(status) = serde_json::from_str::<Value>(&body)
            {
                match status["availability"].as_str() {
                    Some("current") => return,
                    Some("rebuild_required" | "quarantined" | "unavailable")
                        if status["rebuildInFlight"] != true =>
                    {
                        panic!("default derived generation failed to become current: {status}")
                    }
                    _ => {}
                }
            }
            if std::time::Instant::now() >= deadline {
                panic!(
                    "default derived generation did not become current; stderr: {}",
                    drained(&self.stderr)
                );
            }
            thread::sleep(Duration::from_millis(20));
        }
    }

    pub fn get_json(&self, path: &str) -> Value {
        let body = self.get_text(path);
        serde_json::from_str(&body).unwrap_or_else(|error| panic!("parse {path} body: {error}"))
    }

    /// GET a path expected to succeed, returning the raw 200 response body.
    ///
    /// The inspector is a blocking HTTP/1.1 server that closes each connection
    /// after responding. Under load (notably on Windows and Linux CI) the close can race
    /// ahead of the client's read and surface as a connection reset before the
    /// body is drained. GETs are idempotent, so retry a few times with a short
    /// backoff before giving up.
    pub fn get_text(&self, path: &str) -> String {
        let mut last_error = String::new();
        for attempt in 0..12 {
            match self.try_get(path) {
                Ok((_, body)) => return body,
                Err(error) => {
                    last_error = error;
                    thread::sleep(Duration::from_millis(20 * (attempt + 1)));
                }
            }
        }
        panic!(
            "GET {path} failed after retries: {last_error}; server stderr: {}",
            drained(&self.stderr)
        );
    }

    /// GET a path expected to fail, returning the raw status head and the
    /// parsed JSON error body.
    pub fn get_error(&self, path: &str) -> (String, Value) {
        let (status, body) = self.raw_get(path);
        let body: Value = serde_json::from_str(&body).expect("error body is json");
        (status, body)
    }

    /// GET a path returning the raw status line and body, with no status
    /// assertion — for socket-level route/error coverage.
    pub fn raw_get(&self, path: &str) -> (String, String) {
        self.request_with_retry("GET", path, &self.default_headers())
    }

    pub fn raw_request(
        &self,
        method: &str,
        path: &str,
        headers: &[(&str, &str)],
    ) -> (String, String) {
        let headers = headers
            .iter()
            .map(|(name, value)| ((*name).to_owned(), (*value).to_owned()))
            .collect::<Vec<_>>();
        self.request_with_retry(method, path, &headers)
    }

    /// Retry transport failures while draining responses from the read-only
    /// inspector. Non-GET methods are rejected before any application action,
    /// so replaying these harness requests cannot duplicate a write.
    fn request_with_retry(
        &self,
        method: &str,
        path: &str,
        headers: &[(String, String)],
    ) -> (String, String) {
        let mut last_error = String::new();
        for attempt in 0..12 {
            match self.try_request(method, path, headers) {
                Ok(response) => return response,
                Err(error) => {
                    last_error = error;
                    thread::sleep(Duration::from_millis(20 * (attempt + 1)));
                }
            }
        }
        panic!(
            "{method} {path} failed after retries: {last_error}; server stderr: {}",
            drained(&self.stderr)
        );
    }

    fn try_request(
        &self,
        method: &str,
        path: &str,
        headers: &[(String, String)],
    ) -> Result<(String, String), String> {
        let mut stream = TcpStream::connect(&self.addr).map_err(|error| error.to_string())?;
        let mut request = format!("{method} {path} HTTP/1.1\r\n");
        for (name, value) in headers {
            request.push_str(name);
            request.push_str(": ");
            request.push_str(value);
            request.push_str("\r\n");
        }
        request.push_str("Connection: close\r\n\r\n");
        stream
            .write_all(request.as_bytes())
            .map_err(|error| error.to_string())?;
        let _ = stream.shutdown(Shutdown::Write);
        let mut response = Vec::new();
        stream
            .read_to_end(&mut response)
            .map_err(|error| error.to_string())?;
        let text = String::from_utf8_lossy(&response);
        let (head, body) = text
            .split_once("\r\n\r\n")
            .ok_or_else(|| "response has no header/body delimiter".to_owned())?;
        Ok((head.to_owned(), body.to_owned()))
    }

    fn default_headers(&self) -> Vec<(String, String)> {
        let mut headers = vec![("Host".to_owned(), self.addr.clone())];
        if let Some(token) = self.bearer.as_deref() {
            headers.push(("Authorization".to_owned(), format!("Bearer {token}")));
        }
        headers
    }

    /// Issue a raw request line (method + target) and return the status line,
    /// for exercising non-GET routes.
    pub fn request(&self, method: &str, path: &str) -> String {
        let (head, _) = self.request_with_retry(method, path, &self.default_headers());
        head.lines().next().unwrap_or_default().to_owned()
    }

    fn try_get(&self, path: &str) -> Result<(String, String), String> {
        let (head, body) = self.try_request("GET", path, &self.default_headers())?;
        if !head.starts_with("HTTP/1.1 200") {
            return Err(format!(
                "unexpected status for {path}: {}; body: {}",
                head.lines().next().unwrap_or_default(),
                body
            ));
        }
        Ok((head, body))
    }
}

/// Keep legacy Inspector tests honest after the Change hard cutover.
///
/// Their setup runs the current CLI against a complete L2 store. The signed
/// v0.9-shaped `/api/*` routes, however, are only valid on L0. For those tests
/// we open an independent clone whose ephemeral store contains the append-only
/// legacy event/content subset. Later CLI appends are mirrored into every live
/// clone, so freshness tests still exercise a running reader rather than a
/// frozen response fixture.
fn prepare_legacy_inspector_clone(repo: &Path) -> Option<(tempfile::TempDir, PathBuf)> {
    let source_store = pointbreak::session::store_dir_for_repo(repo).ok()?;
    let events = source_store.join("events");
    let activated = std::fs::read_dir(&events)
        .ok()?
        .filter_map(Result::ok)
        .any(|entry| {
            std::fs::read(entry.path())
                .ok()
                .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok())
                .and_then(|value| value["schema"].as_str().map(str::to_owned))
                .is_some_and(|schema| schema == "pointbreak.store-capability-activation")
        });
    if !activated {
        return None;
    }

    let temp = tempfile::tempdir().expect("legacy Inspector clone parent");
    let clone = temp.path().join(
        repo.file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("repository"),
    );
    copy_legacy_repository(repo, &clone);
    let config_dir = clone.join(".pointbreak");
    std::fs::create_dir_all(&config_dir).expect("create legacy Inspector config directory");
    copy_worktree_config(&repo.join(".pointbreak"), &config_dir);
    std::fs::write(
        config_dir.join("store.local.json"),
        b"{\"schema\":\"shore.store-config\",\"version\":1,\"mode\":\"ephemeral\"}\n",
    )
    .expect("write legacy Inspector ephemeral store config");
    sync_legacy_store(&source_store, &clone);
    let source_key = repo.canonicalize().unwrap_or_else(|_| repo.to_path_buf());
    legacy_mirrors()
        .lock()
        .expect("legacy mirror registry lock")
        .entry(source_key)
        .or_default()
        .push(clone.clone());
    Some((temp, clone))
}

/// Materialize and retain the explicit L0 reader fixture used by tests that
/// need to inspect its disposable derived sidecar before server startup.
pub fn legacy_reader_clone(repo: &Path) -> (tempfile::TempDir, PathBuf) {
    prepare_legacy_inspector_clone(repo).expect("source store has complete Change authority")
}

const LEGACY_REPOSITORY_COPY_ATTEMPTS: usize = 3;

fn copy_legacy_repository(source_root: &Path, destination_root: &Path) {
    for attempt in 1..=LEGACY_REPOSITORY_COPY_ATTEMPTS {
        match copy_legacy_repository_once(source_root, destination_root, Path::new("")) {
            Ok(()) => return,
            Err(error)
                if error.kind() == std::io::ErrorKind::NotFound
                    && attempt < LEGACY_REPOSITORY_COPY_ATTEMPTS =>
            {
                match std::fs::remove_dir_all(destination_root) {
                    Ok(()) => {}
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                    Err(error) => panic!(
                        "reset legacy Inspector repository snapshot {}: {error}",
                        destination_root.display()
                    ),
                }
                std::thread::yield_now();
            }
            Err(error) => panic!(
                "copy legacy Inspector repository {} to {} after {attempt} attempt(s): {error}",
                source_root.display(),
                destination_root.display()
            ),
        }
    }
}

fn copy_legacy_repository_once(
    source_root: &Path,
    destination_root: &Path,
    relative: &Path,
) -> std::io::Result<()> {
    let source = source_root.join(relative);
    let destination = destination_root.join(relative);
    std::fs::create_dir_all(&destination)?;
    let entries = std::fs::read_dir(&source)?;
    for entry in entries {
        let entry = entry?;
        let child_relative = relative.join(entry.file_name());
        if child_relative == Path::new(".git/pointbreak")
            || child_relative == Path::new(".pointbreak/data")
            || is_transient_git_lock(&child_relative)
        {
            continue;
        }
        let source_path = entry.path();
        let destination_path = destination_root.join(&child_relative);
        if entry.file_type()?.is_dir() {
            copy_legacy_repository_once(source_root, destination_root, &child_relative)?;
        } else {
            std::fs::copy(&source_path, &destination_path)?;
        }
    }
    Ok(())
}

fn is_transient_git_lock(relative: &Path) -> bool {
    relative.starts_with(".git")
        && relative
            .extension()
            .is_some_and(|extension| extension == "lock")
}

pub(super) fn sync_legacy_mirrors(repo: &Path) {
    let source_key = repo.canonicalize().unwrap_or_else(|_| repo.to_path_buf());
    let output = Command::new("git")
        .args(["rev-parse", "--path-format=absolute", "--git-common-dir"])
        .current_dir(repo)
        .output();
    let Ok(output) = output else {
        return;
    };
    if !output.status.success() {
        return;
    }
    let Ok(common_dir) = String::from_utf8(output.stdout) else {
        return;
    };
    let source_store = Path::new(common_dir.trim()).join("pointbreak");
    let mut registry = legacy_mirrors()
        .lock()
        .expect("legacy mirror registry lock");
    let Some(mirrors) = registry.get_mut(&source_key) else {
        return;
    };
    mirrors.retain(|mirror_repo| {
        if !mirror_repo.exists() {
            return false;
        }
        sync_legacy_store(&source_store, mirror_repo);
        true
    });
}

fn sync_legacy_store(source: &Path, destination_repo: &Path) {
    let destination = destination_repo.join(".pointbreak/data");
    std::fs::create_dir_all(&destination).expect("create legacy mirror store");
    for directory in ["artifacts", "objects"] {
        copy_tree_if_present(&source.join(directory), &destination.join(directory));
    }
    let destination_events = destination.join("events");
    std::fs::create_dir_all(&destination_events).expect("create legacy mirror events");
    let Ok(entries) = std::fs::read_dir(source.join("events")) else {
        return;
    };
    let events = entries
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let bytes = std::fs::read(entry.path()).ok()?;
            let event =
                serde_json::from_slice::<pointbreak::session::event::ShoreEvent>(&bytes).ok();
            Some((entry.file_name(), bytes, event))
        })
        .collect::<Vec<_>>();

    // Legacy-route regression tests need a historically valid v0.9 proposal
    // shape. New L2 proposals deliberately carry no proposal-borne supersedes,
    // so translate active Change relation claims at this fixture boundary only.
    // Production projections never infer or rewrite this representation.
    let mut relation_claims = std::collections::BTreeMap::new();
    let mut withdrawn_claims = std::collections::BTreeSet::new();
    for (_, _, event) in &events {
        let Some(event) = event else {
            continue;
        };
        match event.event_type {
            pointbreak::session::event::EventType::ChangeRevisionRelationAsserted => {
                let payload: pointbreak::session::event::ChangeRevisionRelationAssertedPayload =
                    serde_json::from_value(event.payload.clone())
                        .expect("decode fixture Change relation assertion");
                relation_claims.insert(
                    payload.relation_claim_id.as_str().to_owned(),
                    (
                        payload.successor.revision_id,
                        payload.predecessor.revision_id,
                    ),
                );
            }
            pointbreak::session::event::EventType::ChangeRevisionRelationWithdrawn => {
                let payload: pointbreak::session::event::ChangeRevisionRelationWithdrawnPayload =
                    serde_json::from_value(event.payload.clone())
                        .expect("decode fixture Change relation withdrawal");
                withdrawn_claims.insert(payload.relation_claim_id.as_str().to_owned());
            }
            _ => {}
        }
    }
    let mut supersedes = std::collections::BTreeMap::<
        pointbreak::model::RevisionId,
        Vec<pointbreak::model::RevisionId>,
    >::new();
    for (claim_id, (successor, predecessor)) in relation_claims {
        if !withdrawn_claims.contains(&claim_id) {
            supersedes.entry(successor).or_default().push(predecessor);
        }
    }
    for predecessors in supersedes.values_mut() {
        predecessors.sort();
        predecessors.dedup();
    }

    for (file_name, bytes, event) in events {
        let target = destination_events.join(file_name);
        let Some(event) = event else {
            let schema = serde_json::from_slice::<Value>(&bytes)
                .ok()
                .and_then(|value| value["schema"].as_str().map(str::to_owned));
            if matches!(
                schema.as_deref(),
                Some(
                    "pointbreak.store-capability-activation"
                        | "pointbreak.bulk-adoption-completion"
                )
            ) {
                continue;
            }
            if !target.exists() {
                std::fs::write(target, bytes).expect("copy legacy fixture raw record");
            }
            continue;
        };
        if matches!(
            event.event_type,
            pointbreak::session::event::EventType::ChangeDeclared
                | pointbreak::session::event::EventType::ChangeMembershipAsserted
                | pointbreak::session::event::EventType::ChangeMembershipWithdrawn
                | pointbreak::session::event::EventType::ChangeLinkAsserted
                | pointbreak::session::event::EventType::ChangeRevisionRelationAsserted
                | pointbreak::session::event::EventType::ChangeRevisionRelationWithdrawn
                | pointbreak::session::event::EventType::RevisionRelationAttested
                | pointbreak::session::event::EventType::ReviewFactPorted
        ) {
            continue;
        }
        if event.event_type == pointbreak::session::event::EventType::WorkObjectProposed {
            let mut payload: pointbreak::session::event::WorkObjectProposedPayload =
                serde_json::from_value(event.payload.clone())
                    .expect("decode fixture Revision proposal");
            if let pointbreak::session::event::WorkObjectProposal::Revision {
                revision,
                supersedes: proposal_supersedes,
                ..
            } = &mut payload.work_object
                && let Some(predecessors) = supersedes.get(&revision.id)
            {
                proposal_supersedes.extend(predecessors.iter().cloned());
                proposal_supersedes.sort();
                proposal_supersedes.dedup();
                let translated = pointbreak::session::event::ShoreEvent::new(
                    event.event_type,
                    event.idempotency_key,
                    event.target,
                    event.writer,
                    payload,
                    event.occurred_at,
                )
                .expect("build historical fixture proposal");
                std::fs::write(
                    target,
                    serde_json::to_vec(&translated).expect("encode historical fixture proposal"),
                )
                .expect("write historical fixture proposal");
                continue;
            }
        }
        if !target.exists() {
            std::fs::write(target, bytes).expect("copy legacy fixture event");
        }
    }
}

fn copy_worktree_config(source: &Path, destination: &Path) {
    let Ok(entries) = std::fs::read_dir(source) else {
        return;
    };
    for entry in entries.filter_map(Result::ok) {
        let source_path = entry.path();
        if !source_path.is_file() || entry.file_name() == "store.local.json" {
            continue;
        }
        std::fs::copy(&source_path, destination.join(entry.file_name()))
            .expect("copy legacy Inspector reader configuration");
    }
}

fn copy_tree_if_present(source: &Path, destination: &Path) {
    let Ok(entries) = std::fs::read_dir(source) else {
        return;
    };
    std::fs::create_dir_all(destination).expect("create legacy mirror directory");
    for entry in entries.filter_map(Result::ok) {
        let source_path = entry.path();
        let destination_path = destination.join(entry.file_name());
        if source_path.is_dir() {
            copy_tree_if_present(&source_path, &destination_path);
        } else if !destination_path.exists() {
            std::fs::copy(&source_path, &destination_path).expect("copy legacy mirror content");
        }
    }
}

impl Drop for Inspector {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Snapshot the background-captured server stderr, after a brief flush window so
/// an early-exiting server's output has a chance to land.
fn drained(stderr: &Arc<Mutex<String>>) -> String {
    thread::sleep(Duration::from_millis(50));
    stderr.lock().map(|guard| guard.clone()).unwrap_or_default()
}

/// Run `pointbreak capture` against a repo, returning the captured Revision id.
pub fn capture(repo: &Path) -> String {
    let output = pointbreak(["capture", "--repo", repo.to_str().unwrap(), "--allow-empty"]);
    assert!(
        output.status.success(),
        "capture stderr:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let json: Value = serde_json::from_slice(&output.stdout).expect("parse capture JSON");
    json["revision"]["id"]
        .as_str()
        .expect("capture returns a Revision id")
        .to_owned()
}

/// A populated store assembled once through the `pointbreak` CLI, reused by the
/// endpoint-contract and validation read-surface suites: one captured Revision
/// carrying a range-targeted observation, an operative input request, a
/// superseded + a superseding assessment (two tracks), and two validation checks
/// (passed `cargo test` on the agent track, failed `cargo clippy` on the human
/// track).
pub struct RepresentativeStore {
    pub repo: GitRepo,
    pub revision_id: String,
    pub snapshot_id: String,
}

/// Generated ids printed by the isolated decision-continuity materializer.
///
/// These values are resolved from real command output on every build. Keeping
/// them together lets socket suites reuse one generated store without pinning
/// timestamp-derived event or association identities.
#[derive(Debug, serde::Deserialize)]
pub struct DecisionContinuityMatrixIds {
    pub primary_revision: String,
    pub live_revision: String,
    pub unassessed_revision: String,
    pub superseded_revision: String,
    pub ambiguous_assessment_revision: String,
    pub competing_revision: String,
    pub range_revision: String,
    pub root_revision: String,
    pub staged_revision: String,
    pub unstaged_revision: String,
    pub detached_revision: String,
    pub missing_revision: String,
    pub base_commit: String,
    pub first_landing: String,
    pub second_landing: String,
    pub live_landing: String,
}

/// An isolated repository/store carrying the generated decision matrix.
pub struct DecisionContinuityMatrix {
    _root: tempfile::TempDir,
    repo: PathBuf,
    pub ids: DecisionContinuityMatrixIds,
}

impl DecisionContinuityMatrix {
    pub fn repo(&self) -> &Path {
        &self.repo
    }
}

fn decision_matrix_shell() -> Command {
    #[cfg(windows)]
    {
        let git_exec_path = Command::new("git")
            .arg("--exec-path")
            .output()
            .expect("locate Git for Windows");
        assert!(
            git_exec_path.status.success(),
            "git --exec-path failed: {}",
            String::from_utf8_lossy(&git_exec_path.stderr)
        );
        let git_exec_path =
            String::from_utf8(git_exec_path.stdout).expect("Git for Windows exec path is UTF-8");
        let bash = Path::new(git_exec_path.trim())
            .ancestors()
            .map(|ancestor| ancestor.join("bin/bash.exe"))
            .find(|candidate| candidate.is_file())
            .unwrap_or_else(|| {
                panic!(
                    "could not find Git Bash above git exec path {}",
                    git_exec_path.trim()
                )
            });
        Command::new(bash)
    }

    #[cfg(not(windows))]
    {
        Command::new("bash")
    }
}

/// Materialize the reusable synthetic matrix with the exact binary built for
/// the integration test. The generator owns a fresh temporary repository, and
/// it rejects any store path that escapes that repository.
pub fn decision_continuity_matrix() -> DecisionContinuityMatrix {
    let root = tempfile::tempdir().expect("decision matrix root");
    let repo = root.path().join("repository");
    let script = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("scripts/materialize-inspector-decision-matrix.sh");
    let output = decision_matrix_shell()
        .arg(&script)
        .arg(&repo)
        .env("POINTBREAK_BINARY", env!("CARGO_BIN_EXE_pointbreak"))
        .env(
            "POINTBREAK_CHANGE_READY_FIXTURE_DIR",
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("tests/support/assets/change-ready-store"),
        )
        .env_remove("POINTBREAK_HOME")
        .env_remove("POINTBREAK_FORMAT")
        .env_remove("POINTBREAK_SIGNING_KEY")
        .output()
        .unwrap_or_else(|error| panic!("run {}: {error}", script.display()));
    assert!(
        output.status.success(),
        "decision matrix materialization failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let ids = serde_json::from_slice(&output.stdout)
        .unwrap_or_else(|error| panic!("parse generated matrix ids: {error}"));

    DecisionContinuityMatrix {
        _root: root,
        repo,
        ids,
    }
}

/// Build the [`RepresentativeStore`] entirely through the real CLI, so it tracks
/// the current on-disk store layout without hard-coding any path.
pub fn representative_store() -> RepresentativeStore {
    let repo = GitRepo::new();
    repo.write(
        "src/lib.rs",
        "pub fn value() -> u32 {\n    1\n}\n\npub fn other() -> u32 {\n    2\n}\n",
    );
    repo.commit_all("base");
    repo.write(
        "src/lib.rs",
        "pub fn value() -> u32 {\n    42\n}\n\npub fn other() -> u32 {\n    7\n}\n",
    );

    let repo_arg = repo.path().to_str().unwrap().to_owned();
    let capture = run_shore_json(&["capture", "--repo", &repo_arg]);
    let revision_id = capture["revision"]["id"]
        .as_str()
        .expect("capture returns a Revision id")
        .to_owned();
    let snapshot_id = capture["revision"]["objectId"]
        .as_str()
        .expect("capture returns a snapshot id")
        .to_owned();

    // Capture no longer infers ref continuity. Keep this representative legacy
    // reader fixture explicit about the structural ref association it exercises.
    let head = repo.git(["rev-parse", "HEAD"]).stdout.trim().to_owned();
    run_shore(&[
        "association",
        "record",
        "--repo",
        &repo_arg,
        "--exact-revision",
        &revision_id,
        "--track",
        "agent:codex",
        "--ref",
        "main",
        "--head",
        &head,
    ]);

    // Range-targeted observation on the agent track.
    run_shore(&[
        "observation",
        "add",
        "--repo",
        &repo_arg,
        "--track",
        "agent:codex",
        "--title",
        "Observed change",
        "--body",
        "the return value changed",
        "--file",
        "src/lib.rs",
        "--start-line",
        "2",
        "--end-line",
        "2",
    ]);

    // Operative input request.
    run_shore(&[
        "input-request",
        "open",
        "--repo",
        &repo_arg,
        "--track",
        "agent:codex",
        "--title",
        "Need a decision",
        "--reason",
        "manual-decision-required",
        "--body",
        "should we ship this?",
    ]);

    // A first assessment (agent track), then a superseding assessment (human
    // track) that replaces it, so current-assessment resolution is exercised.
    let first = run_shore_json(&[
        "assessment",
        "add",
        "--repo",
        &repo_arg,
        "--track",
        "agent:codex",
        "--assessment",
        "needs-changes",
        "--summary",
        "not yet",
    ]);
    let first_assessment_id = first["assessmentId"]
        .as_str()
        .expect("assessment add returns an assessment id")
        .to_owned();
    run_shore(&[
        "assessment",
        "add",
        "--repo",
        &repo_arg,
        "--track",
        "human:kevin",
        "--assessment",
        "accepted",
        "--summary",
        "ship it",
        "--replaces",
        &first_assessment_id,
    ]);

    // Two validation checks across two tracks: a passed cargo test and a failed
    // cargo clippy carrying an exit code + command.
    run_shore(&[
        "validation",
        "add",
        "--repo",
        &repo_arg,
        "--track",
        "agent:codex",
        "--check-name",
        "cargo test",
        "--status",
        "passed",
    ]);
    run_shore(&[
        "validation",
        "add",
        "--repo",
        &repo_arg,
        "--track",
        "human:kevin",
        "--check-name",
        "cargo clippy",
        "--status",
        "failed",
        "--exit-code",
        "1",
        "--command",
        "cargo clippy -- -D warnings",
    ]);

    RepresentativeStore {
        repo,
        revision_id,
        snapshot_id,
    }
}

/// Run a `pointbreak` subcommand, asserting success and surfacing stderr on failure.
fn run_shore(args: &[&str]) {
    let output = pointbreak(args);
    assert!(
        output.status.success(),
        "pointbreak {args:?} failed:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

/// Run a `pointbreak` subcommand and parse its stdout as JSON.
fn run_shore_json(args: &[&str]) -> Value {
    let output = pointbreak(args);
    assert!(
        output.status.success(),
        "pointbreak {args:?} failed:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout)
        .unwrap_or_else(|error| panic!("parse pointbreak {args:?} JSON: {error}"))
}

/// Capture a revision in a supersession thread. With no `predecessor`, captures
/// the current worktree as a fresh revision. With a `predecessor`, mutates a
/// tracked file first (so the snapshot id differs) and records the new revision
/// as superseding the predecessor. Returns the captured revision id.
pub fn capture_supersession_round(repo: &Path, predecessor: Option<&str>) -> String {
    if let Some(predecessor) = predecessor {
        let (predecessor_capture, latest_cursor, latest_revision) = {
            let state = super::fixture_change_state()
                .lock()
                .expect("fixture Change state lock");
            let repo_state = state.get(repo).expect("fixture repo capture state");
            let predecessor_capture = super::fixture_capture(repo_state, predecessor).clone();
            let latest_cursor = repo_state
                .latest_cursor
                .clone()
                .expect("fixture has a latest review cursor");
            let latest_revision = pointbreak::session::ReviewCursorV1::decode_token(&latest_cursor)
                .expect("fixture review cursor is valid")
                .revision
                .revision_id;
            (predecessor_capture, latest_cursor, latest_revision)
        };
        if latest_revision.as_str() != predecessor_capture.revision_id {
            let target = repo.join("src/lib.rs");
            let mut contents = std::fs::read_to_string(&target).unwrap_or_default();
            contents.push_str(&format!(
                "\n// supersedes {}\n",
                predecessor.replace(':', "_")
            ));
            std::fs::write(&target, contents).expect("write fork successor content");

            let repo_arg = repo.to_str().expect("fixture repo path is utf-8");
            let captured = pointbreak([
                "capture",
                "--repo",
                repo_arg,
                "--review-cursor",
                &latest_cursor,
                "--advance",
                "parallel",
            ]);
            assert!(
                captured.status.success(),
                "capture fork round stderr:\n{}",
                String::from_utf8_lossy(&captured.stderr)
            );
            let document: Value =
                serde_json::from_slice(&captured.stdout).expect("parse fork capture JSON");
            let revision = document["revision"]["revisionId"]
                .as_str()
                .expect("fork capture Revision id")
                .to_owned();
            let artifact_hash = document["revision"]["objectArtifactContentHash"]
                .as_str()
                .expect("fork capture artifact hash")
                .to_owned();
            let operation_id = format!(
                "change-operation:fixture-fork-{}-{}",
                revision.trim_start_matches("rev:sha256:"),
                predecessor_capture
                    .revision_id
                    .trim_start_matches("rev:sha256:")
            );
            let asserted = pointbreak([
                "change",
                "assert-relation",
                &predecessor_capture.change_id,
                &revision,
                &predecessor_capture.revision_id,
                "--successor-artifact-hash",
                &artifact_hash,
                "--predecessor-artifact-hash",
                &predecessor_capture.artifact_hash,
                "--operation-id",
                &operation_id,
                "--repo",
                repo_arg,
            ]);
            assert!(
                asserted.status.success(),
                "assert fork relation stderr:\n{}",
                String::from_utf8_lossy(&asserted.stderr)
            );
            return revision;
        }
    }

    let mut args = vec![
        "capture".to_owned(),
        "--repo".to_owned(),
        repo.to_str().unwrap().to_owned(),
    ];
    if let Some(predecessor) = predecessor {
        // A successor must carry different content, or it would collapse to the
        // same snapshot id. Append a unique line to a tracked file.
        let target = repo.join("src/lib.rs");
        let mut contents = std::fs::read_to_string(&target).unwrap_or_default();
        contents.push_str(&format!(
            "\n// supersedes {}\n",
            predecessor.replace(':', "_")
        ));
        std::fs::write(&target, contents).expect("write successor content");
        let cursor = {
            let state = super::fixture_change_state()
                .lock()
                .expect("fixture Change state lock");
            let repo_state = state.get(repo).expect("fixture repo capture state");
            let predecessor_capture = super::fixture_capture(repo_state, predecessor);
            super::fresh_fixture_cursor(
                repo,
                &predecessor_capture.revision_id,
                &predecessor_capture.change_id,
            )
        };
        args.extend([
            "--review-cursor".to_owned(),
            cursor,
            "--advance".to_owned(),
            "replace".to_owned(),
        ]);
    } else {
        args.push("--allow-empty".to_owned());
    }

    let output = pointbreak(args);
    assert!(
        output.status.success(),
        "capture supersession round stderr:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let json: Value =
        serde_json::from_slice(&output.stdout).expect("parse supersession capture JSON");
    json["revision"]["id"]
        .as_str()
        .expect("supersession capture returns a Revision id")
        .to_owned()
}

pub fn add_worktree(repo: &Path, path: &Path, branch: &str) {
    run_git(
        repo,
        [
            OsString::from("worktree"),
            OsString::from("add"),
            OsString::from("-b"),
            OsString::from(branch),
            path.as_os_str().to_owned(),
        ],
    );
}

pub fn run_git<I>(cwd: &Path, args: I)
where
    I: IntoIterator<Item = OsString>,
{
    let output = Command::new("git")
        .args(args)
        .current_dir(cwd)
        .output()
        .unwrap_or_else(|error| panic!("run git in {}: {error}", cwd.display()));
    assert!(
        output.status.success(),
        "git failed in {}:\n{}",
        cwd.display(),
        String::from_utf8_lossy(&output.stderr)
    );
}

/// Minimal percent-encoding for the `:` characters in a Revision id query.
pub fn urlencode(value: &str) -> String {
    value.replace(':', "%3A")
}
