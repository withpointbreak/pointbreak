//! The HTTP process owns transport only. Data services live in one disposable
//! child, and only its main thread enters whole-process native ownership.
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::time::Duration;

use serde::{Deserialize, Serialize};

use super::server::{InspectState, Response};

const REQUEST_LIMIT: usize = 64 * 1024;
const RESPONSE_LIMIT: usize = 32 * 1024 * 1024;
const QUEUED: usize = 8;

#[cfg(target_os = "macos")]
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct EnrollmentSelection {
    request_id: String,
    enrollment_actor: String,
    #[serde(default)]
    limits: pointbreak::session::AuditLimits,
}
#[cfg(not(target_os = "macos"))]
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct EnrollmentSelection {}
#[cfg(target_os = "macos")]
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PrivateBootstrap {
    version: u32,
    repository: PathBuf,
    selection: EnrollmentSelection,
    host: std::net::IpAddr,
    port: u16,
    #[serde(default)]
    api_only: bool,
}
#[cfg(target_os = "macos")]
pub(crate) fn shell_main(arguments: Vec<std::ffi::OsString>) -> std::process::ExitCode {
    let result = (|| -> Result<(), Box<dyn std::error::Error>> {
        if !arguments.is_empty() {
            return Err("private Inspector requires a stdin bootstrap".into());
        }
        let bootstrap: PrivateBootstrap =
            read_frame(&mut BufReader::new(std::io::stdin()), REQUEST_LIMIT)?;
        if bootstrap.version != 1
            || !bootstrap.repository.is_absolute()
            || !bootstrap.host.is_loopback()
            || bootstrap.selection.request_id.is_empty()
            || bootstrap.selection.enrollment_actor.is_empty()
        {
            return Err("invalid private Inspector bootstrap".into());
        }
        super::server::serve_selected(
            std::net::SocketAddr::new(bootstrap.host, bootstrap.port),
            bootstrap.repository,
            false,
            bootstrap.api_only,
            super::StartupOutputFormat::Json,
            &mut std::io::stdout(),
            Some(bootstrap.selection),
        )
    })();
    match result {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("owned Inspector shell: {e}");
            std::process::ExitCode::from(2)
        }
    }
}

#[cfg(not(target_os = "macos"))]
pub(crate) fn shell_main(_: Vec<std::ffi::OsString>) -> std::process::ExitCode {
    eprintln!("owned_process_required; enrolled Inspector is unavailable on this platform");
    std::process::ExitCode::from(2)
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct DataRequest {
    pub method: String,
    pub target: String,
    #[cfg(feature = "longitudinal-counting")]
    pub counting: Vec<String>,
}
#[derive(Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum CommandFrame {
    Start {
        epoch: u64,
        repo: PathBuf,
        selection: Option<EnrollmentSelection>,
    },
    Request {
        epoch: u64,
        operation: u64,
        request_id: String,
        request: DataRequest,
    },
    Cancel {
        epoch: u64,
        operation: u64,
    },
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Reply {
    epoch: u64,
    operation: u64,
    request_id: String,
    response: Response,
    status: serde_json::Value,
}
struct Job {
    operation: u64,
    request_id: String,
    request: DataRequest,
    abandoned: Arc<AtomicBool>,
    reply: mpsc::SyncSender<Response>,
}
struct Running {
    epoch: u64,
    jobs: mpsc::SyncSender<Job>,
    control: mpsc::SyncSender<CommandFrame>,
    child: Arc<Mutex<Child>>,
    retired: Arc<AtomicBool>,
}
#[derive(Default)]
struct Detached {
    epoch: u64,
    next_operation: u64,
    last_operation: Option<u64>,
    active: Option<u64>,
    active_request: Option<String>,
    last_request: Option<String>,
    last_kind: Option<&'static str>,
    last_outcome: Option<&'static str>,
    observed_at: Option<std::time::SystemTime>,
    status_pending: bool,
    pending_rebuild: Option<(u64, Arc<AtomicBool>)>,
    outcome: &'static str,
    detail: String,
    cached: Option<serde_json::Value>,
    running: Option<Running>,
}
pub(super) struct Supervisor {
    repo: PathBuf,
    selection: Option<EnrollmentSelection>,
    state: Arc<Mutex<Detached>>,
}
fn unavailable(message: &str) -> Response {
    Response::json_error("503 Service Unavailable", message)
}
fn lock<T>(value: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    value.lock().unwrap_or_else(|poison| poison.into_inner())
}
fn write_frame(
    value: &impl Serialize,
    output: &mut impl Write,
    limit: usize,
) -> std::io::Result<()> {
    let bytes = serde_json::to_vec(value)?;
    if bytes.len() > limit {
        return Err(std::io::Error::other("owned Inspector frame limit"));
    }
    output.write_all(&bytes)?;
    output.write_all(b"\n")?;
    output.flush()
}
fn read_frame<T: serde::de::DeserializeOwned>(
    input: &mut impl BufRead,
    limit: usize,
) -> std::io::Result<T> {
    use std::io::Read as _;
    let mut bytes = Vec::new();
    input
        .take((limit + 2) as u64)
        .read_until(b'\n', &mut bytes)?;
    if bytes.last() != Some(&b'\n') || bytes.len() > limit + 1 {
        return Err(std::io::Error::other(
            "owned Inspector truncated/oversized frame",
        ));
    }
    Ok(serde_json::from_slice(&bytes)?)
}
impl Supervisor {
    pub(super) fn new(repo: PathBuf, selection: Option<EnrollmentSelection>) -> Self {
        let this = Self {
            repo,
            selection,
            state: Arc::new(Mutex::new(Detached::default())),
        };
        this.start();
        this
    }
    // The initial launch and user-elected replacement use the same admission.
    // No I/O or process wait occurs under the detached status lock.
    fn start(&self) {
        let epoch = {
            let mut state = lock(&self.state);
            if state.running.is_some() || state.outcome == "starting" {
                return;
            }
            state.epoch += 1;
            state.outcome = "starting";
            state.cached = None;
            state.last_operation = None;
            state.last_request = None;
            state.last_kind = None;
            state.last_outcome = None;
            state.observed_at = None;
            state.epoch
        };
        let state = self.state.clone();
        let repo = self.repo.clone();
        let selection = self.selection.clone();
        std::thread::spawn(move || {
            let result = (|| -> std::io::Result<Running> {
                let mut child = Command::new(std::env::current_exe()?)
                    .arg("__inspect-data-v1")
                    .stdin(Stdio::piped())
                    .stdout(Stdio::piped())
                    .stderr(Stdio::inherit())
                    .spawn()?;
                let mut input = child.stdin.take().expect("piped stdin");
                let output = child.stdout.take().expect("piped stdout");
                let child = Arc::new(Mutex::new(child));
                let (jobs, job_rx) = mpsc::sync_channel::<Job>(QUEUED);
                let (control, control_rx) = mpsc::sync_channel::<CommandFrame>(2);
                let retired = Arc::new(AtomicBool::new(false));
                let writer_state = state.clone();
                std::thread::spawn(move || {
                    let result = (|| -> std::io::Result<()> {
                        write_frame(
                            &CommandFrame::Start {
                                epoch,
                                repo,
                                selection,
                            },
                            &mut input,
                            REQUEST_LIMIT,
                        )?;
                        for frame in control_rx {
                            write_frame(&frame, &mut input, REQUEST_LIMIT)?;
                        }
                        Ok(())
                    })();
                    if let Err(error) = result {
                        fail(&writer_state, epoch, error.to_string());
                    }
                });
                let io_state = state.clone();
                let io_control = control.clone();
                let io_retired = retired.clone();
                let io_child = child.clone();
                std::thread::spawn(move || {
                    let mut output = BufReader::new(output);
                    let startup = read_frame::<Reply>(&mut output, RESPONSE_LIMIT);
                    match startup {
                        Ok(reply) if matches_reply(epoch, 0, "", &reply) => {
                            let mut state = lock(&io_state);
                            if state.epoch == epoch && state.outcome != "unknown" {
                                state.outcome = "idle";
                                state.cached = Some(reply.status);
                            }
                        }
                        other => {
                            fail(
                                &io_state,
                                epoch,
                                format!(
                                    "child initialization failed: {}",
                                    other
                                        .err()
                                        .map(|error| error.to_string())
                                        .unwrap_or_else(|| "response epoch mismatch".into())
                                ),
                            );
                            io_retired.store(true, Ordering::Release);
                            return;
                        }
                    }
                    loop {
                        let job = match job_rx.recv_timeout(Duration::from_millis(100)) {
                            Ok(job) => job,
                            Err(mpsc::RecvTimeoutError::Timeout) => {
                                let exited = lock(&io_child).try_wait();
                                if !matches!(exited, Ok(None))
                                    || lock(&io_state).outcome == "unknown"
                                {
                                    fail(
                                        &io_state,
                                        epoch,
                                        "child exited or transport failed; explicit retry required"
                                            .into(),
                                    );
                                    break;
                                }
                                continue;
                            }
                            Err(mpsc::RecvTimeoutError::Disconnected) => break,
                        };
                        {
                            let mut state = lock(&io_state);
                            if state.epoch != epoch || state.outcome == "unknown" {
                                break;
                            }
                            if job.abandoned.load(Ordering::Acquire) {
                                if state
                                    .pending_rebuild
                                    .as_ref()
                                    .is_some_and(|(id, _)| *id == job.operation)
                                {
                                    state.pending_rebuild = None;
                                }
                                continue;
                            }
                            state.active = Some(job.operation);
                            state.active_request = Some(job.request_id.clone());
                            state.outcome = "running";
                        }
                        let observation = job.request.method == "GET"
                            && job.request.target == "/api/derived-access/status";
                        let rebuild = job.request.method == "POST"
                            && job.request.target == "/api/derived-access/retry";
                        let result = (|| -> std::io::Result<Reply> {
                            io_control
                                .try_send(CommandFrame::Request {
                                    epoch,
                                    operation: job.operation,
                                    request_id: job.request_id.clone(),
                                    request: job.request,
                                })
                                .map_err(std::io::Error::other)?;
                            let reply: Reply = read_frame(&mut output, RESPONSE_LIMIT)?;
                            if !matches_reply(epoch, job.operation, &job.request_id, &reply) {
                                return Err(std::io::Error::other(
                                    "owned Inspector response epoch mismatch",
                                ));
                            }
                            Ok(reply)
                        })();
                        match result {
                            Ok(reply) => {
                                let mut state = lock(&io_state);
                                if state.epoch != epoch {
                                    break;
                                }
                                let Some(response) = accept_reply(
                                    &mut state,
                                    epoch,
                                    job.operation,
                                    &job.request_id,
                                    observation,
                                    rebuild,
                                    reply,
                                ) else {
                                    break;
                                };
                                let _ = job.reply.try_send(response);
                            }
                            Err(error) => {
                                fail(&io_state, epoch, error.to_string());
                                let _ = job.reply.try_send(unavailable(
                                    "data child response lost; operation outcome unknown",
                                ));
                                break;
                            }
                        }
                    }
                    io_retired.store(true, Ordering::Release);
                });
                Ok(Running {
                    epoch,
                    jobs,
                    control,
                    child,
                    retired,
                })
            })();
            let mut state = lock(&state);
            match result {
                Ok(running) => {
                    state.running = Some(running);
                    if state.outcome == "starting" {
                        state.outcome = "initializing";
                    }
                }
                Err(error) => {
                    state.outcome = "unavailable";
                    state.detail = error.to_string();
                }
            }
        });
    }
    pub(super) fn request(&self, request: DataRequest) -> Response {
        let request_id = match new_request_id() {
            Ok(id) => id,
            Err(error) => return unavailable(&error.to_string()),
        };
        let abandoned = Arc::new(AtomicBool::new(false));
        let (reply, receive) = mpsc::sync_channel(1);
        let operation = {
            let mut state = lock(&self.state);
            if matches!(state.outcome, "unknown" | "starting" | "unavailable") {
                return unavailable("data child unavailable; inspect status before explicit retry");
            }
            state.next_operation += 1;
            let operation = state.next_operation;
            let Some(running) = &state.running else {
                return unavailable("data child unavailable");
            };
            if running
                .jobs
                .try_send(Job {
                    operation,
                    request_id,
                    request,
                    abandoned: abandoned.clone(),
                    reply,
                })
                .is_err()
            {
                return unavailable("data child queue full or retired");
            }
            operation
        };
        match receive.recv_timeout(Duration::from_secs(30)) {
            Ok(response) => response,
            Err(_) => {
                let mut state = lock(&self.state);
                abandoned.store(true, Ordering::Release);
                if state.active == Some(operation) {
                    state.outcome = "unknown";
                    state.detail =
                        "request wait expired; operation outcome unknown until child responds"
                            .into();
                }
                unavailable(
                    "request wait expired; no replay was attempted; inspect operation status",
                )
            }
        }
    }
    pub(super) fn cancel(&self) -> Response {
        let state = lock(&self.state);
        if let Some((operation, cancelled)) = &state.pending_rebuild
            && state.active != Some(*operation)
        {
            cancelled.store(true, Ordering::Release);
        }
        if let Some(running) = &state.running {
            let operation = state.active.unwrap_or(0);
            if running
                .control
                .try_send(CommandFrame::Cancel {
                    epoch: running.epoch,
                    operation,
                })
                .is_err()
            {
                return unavailable("cancel delivery unavailable; operation may still be running");
            }
        }
        drop(state);
        self.status()
    }
    pub(super) fn retry(&self) -> Response {
        // A healthy child receives an explicit rebuild control exactly once.
        // This is not replay of a timed-out or lost request.
        {
            let mut state = lock(&self.state);
            if state.pending_rebuild.is_some()
                && !matches!(state.outcome, "unknown" | "unavailable")
            {
                drop(state);
                return self.status();
            }
            if matches!(state.outcome, "idle" | "completed") && state.active.is_none() {
                state.next_operation += 1;
                let operation = state.next_operation;
                let (reply, _receive) = mpsc::sync_channel(1);
                let abandoned = Arc::new(AtomicBool::new(false));
                if let Some(running) = &state.running {
                    if running
                        .jobs
                        .try_send(Job {
                            operation,
                            request_id: match new_request_id() {
                                Ok(id) => id,
                                Err(error) => return unavailable(&error.to_string()),
                            },
                            request: DataRequest {
                                method: "POST".into(),
                                target: "/api/derived-access/retry".into(),
                                #[cfg(feature = "longitudinal-counting")]
                                counting: Vec::new(),
                            },
                            abandoned: abandoned.clone(),
                            reply,
                        })
                        .is_err()
                    {
                        return unavailable("rebuild control queue full or retired");
                    }
                    state.pending_rebuild = Some((operation, abandoned));
                    drop(state);
                    return self.status();
                }
            }
        }
        // try_wait is nonjoining. Replacement additionally requires the I/O
        // worker to have retired; it can never consume a successor response.
        let candidate = {
            let state = lock(&self.state);
            state
                .running
                .as_ref()
                .map(|run| (run.epoch, run.child.clone(), run.retired.clone()))
        };
        if let Some((epoch, child, retired)) = candidate {
            let reaped = child
                .try_lock()
                .ok()
                .and_then(|mut child| child.try_wait().ok())
                .flatten()
                .is_some();
            if !replacement_admitted(reaped, retired.load(Ordering::Acquire)) {
                return Response::json_error(
                    "409 Conflict",
                    "data child not reaped and retired; retry cannot replace it",
                );
            }
            let mut state = lock(&self.state);
            if state.epoch != epoch {
                return unavailable("child epoch changed");
            }
            state.running = None;
            state.active = None;
            state.active_request = None;
            state.status_pending = false;
            state.pending_rebuild = None;
        }
        self.start();
        self.status()
    }
    pub(super) fn status(&self) -> Response {
        let mut state = lock(&self.state);
        // Status never waits for data or instantiates a service. One bounded
        // observation refreshes the detached snapshot on the owning thread.
        if state.active.is_none()
            && !state.status_pending
            && state.pending_rebuild.is_none()
            && matches!(state.outcome, "idle" | "completed")
            && state
                .observed_at
                .is_none_or(|at| at.elapsed().is_ok_and(|age| age >= Duration::from_secs(1)))
            && let (Some(running), Ok(request_id)) = (&state.running, new_request_id())
        {
            let operation = state.next_operation + 1;
            let (reply, _receive) = mpsc::sync_channel(1);
            if running
                .jobs
                .try_send(Job {
                    operation,
                    request_id,
                    request: DataRequest {
                        method: "GET".into(),
                        target: "/api/derived-access/status".into(),
                        #[cfg(feature = "longitudinal-counting")]
                        counting: Vec::new(),
                    },
                    abandoned: Arc::new(AtomicBool::new(false)),
                    reply,
                })
                .is_ok()
            {
                state.next_operation = operation;
                state.status_pending = true;
            }
        }
        let mut doc = state
            .cached
            .clone()
            .filter(serde_json::Value::is_object)
            .unwrap_or_else(|| {
                serde_json::json!({
                    "schema":"pointbreak.inspect-derived-access-status","version":1,
                    "active":false,"availability":"unavailable","namespace":"absent",
                    "rebuildInFlight":false,"rebuildPaused":false,"servingCurrent":false,
                    "fallbackInFlight":false,"actions":[]
                })
            });
        // Cached data status is an observation, never fresh authority.
        doc["servingCurrent"] = false.into();
        doc["child"] = serde_json::json!({"epoch":state.epoch,"operation":state.active,"requestId":state.active_request,"lastCompletedRequestId":state.last_request,"lastCompletedKind":state.last_kind,"lastCompletedOutcome":state.last_outcome,
            "observationOnly":true,"observationPending":state.status_pending,
            "observedAtUnixMilliseconds":state.observed_at.and_then(|t|t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d|d.as_millis() as u64),
            "observationAgeMilliseconds":state.observed_at.and_then(|t|t.elapsed().ok()).map(|d|d.as_millis() as u64),
            "state":if state.outcome.is_empty() { "unavailable" } else { state.outcome },"lastCompletedOperation":state.last_operation,"detail":state.detail,"cooperativeCancellation":true});
        if state.pending_rebuild.is_some() {
            doc["rebuildInFlight"] = true.into();
        }
        if state.active.is_some() || state.pending_rebuild.is_some() {
            doc["actions"] = serde_json::json!(["wait", "cancel"]);
        }
        if matches!(state.outcome, "unknown" | "unavailable") {
            doc["actions"] = serde_json::json!(["retry"]);
        }
        Response::json_ok(doc.to_string())
    }
}
/// A reply may change detached state only while its exact active identity is
/// still outstanding. A late, proven reply can settle an unknown wait once.
fn accept_reply(
    state: &mut Detached,
    epoch: u64,
    operation: u64,
    request_id: &str,
    observation: bool,
    rebuild: bool,
    reply: Reply,
) -> Option<Response> {
    if state.epoch != epoch
        || state.active != Some(operation)
        || state.active_request.as_deref() != Some(request_id)
        || !matches_reply(epoch, operation, request_id, &reply)
    {
        return None;
    }
    state.active = None;
    state.active_request = None;
    if !observation {
        state.last_request = Some(request_id.into());
        state.last_operation = Some(operation);
        state.last_kind = Some(if rebuild { "rebuild" } else { "read" });
        state.last_outcome = Some(if reply.response.status.starts_with('2') {
            "succeeded"
        } else {
            "failed"
        });
        state.detail = if reply.response.status.starts_with('2') {
            String::new()
        } else {
            serde_json::from_slice::<serde_json::Value>(&reply.response.body)
                .ok()
                .and_then(|body| {
                    body.get("message")
                        .or_else(|| body.get("error"))
                        .and_then(|value| value.as_str())
                        .map(str::to_owned)
                })
                .unwrap_or_else(|| reply.response.status.clone())
        };
    }
    state.status_pending = false;
    if rebuild {
        state.pending_rebuild = None;
    }
    state.observed_at = Some(std::time::SystemTime::now());
    state.outcome = "completed";
    state.cached = Some(reply.status);
    Some(reply.response)
}
fn new_request_id() -> Result<String, getrandom::Error> {
    let mut bytes = [0u8; 16];
    getrandom::fill(&mut bytes)?;
    Ok(bytes.iter().map(|byte| format!("{byte:02x}")).collect())
}
fn matches_reply(epoch: u64, operation: u64, request_id: &str, reply: &Reply) -> bool {
    reply.epoch == epoch && reply.operation == operation && reply.request_id == request_id
}
fn replacement_admitted(reaped: bool, retired: bool) -> bool {
    reaped && retired
}
fn fail(state: &Mutex<Detached>, epoch: u64, detail: String) {
    let mut state = lock(state);
    if state.epoch == epoch {
        state.outcome = "unknown";
        state.detail = detail;
        state.cached = None;
    }
}

pub(crate) fn child_main(arguments: Vec<std::ffi::OsString>) -> std::process::ExitCode {
    #[cfg(target_os = "macos")]
    fn terminal(_: pointbreak::session::OwnedProcessFailure) -> ! {
        // SAFETY: this private entry owns the entire process before CLI setup.
        unsafe { libc::_exit(90) }
    }
    let result = (|| -> Result<(), Box<dyn std::error::Error>> {
        if !arguments.is_empty() {
            return Err("private Inspector accepts no positional arguments".into());
        }
        let mut input = BufReader::new(std::io::stdin());
        let CommandFrame::Start {
            epoch,
            repo,
            selection,
        } = read_frame(&mut input, REQUEST_LIMIT)?
        else {
            return Err("missing Inspector bootstrap".into());
        };
        // This transport is shared by ordinary Inspector. Admit its ordinary
        // bootstrap without the experimental feature, but refuse any enrolled
        // selection before native ownership or repository/service acquisition.
        if selection.is_some() && !cfg!(feature = "experimental-enrolled-progress") {
            return Err(
                "enrolled Inspector requires the experimental-enrolled-progress feature".into(),
            );
        }
        #[cfg(not(target_os = "macos"))]
        if selection.is_some() {
            return Err("enrolled Inspector unavailable on this platform".into());
        }
        // SAFETY: main dispatches here before tracing, services or threads. The
        // token remains on this sole data thread until process exit.
        #[cfg(target_os = "macos")]
        let owner = unsafe { pointbreak::session::enter_owned_data_command(terminal) }?;
        let state = if selection.is_none() {
            Some(Arc::new(InspectState::new(repo.clone())?))
        } else {
            None
        };
        #[cfg(target_os = "macos")]
        let signer = super::page_token::PageTokenSigner::generate()?;
        let cancellation = Arc::new(Mutex::new(None::<(u64, Arc<AtomicBool>)>));
        let control_cancellation = cancellation.clone();
        let background_control = state.clone();
        let (requests, receive) = mpsc::sync_channel(1);
        std::thread::spawn(move || {
            while let Ok(frame) = read_frame::<CommandFrame>(&mut input, REQUEST_LIMIT) {
                match frame {
                    CommandFrame::Request {
                        epoch: request_epoch,
                        operation,
                        request_id,
                        request,
                    } if request_epoch == epoch => {
                        let cancelled = Arc::new(AtomicBool::new(false));
                        *lock(&control_cancellation) = Some((operation, cancelled.clone()));
                        if requests
                            .try_send((operation, request_id, request, cancelled))
                            .is_err()
                        {
                            return;
                        }
                    }
                    CommandFrame::Cancel {
                        epoch: request_epoch,
                        operation,
                    } if request_epoch == epoch => {
                        if operation == 0
                            && let Some(state) = &background_control
                        {
                            state.derived_history.request_background_cancel();
                        }
                        if let Some((active, cancelled)) = &*lock(&control_cancellation)
                            && *active == operation
                        {
                            cancelled.store(true, Ordering::Release);
                            if let Some(state) = &background_control {
                                state.derived_history.request_background_cancel();
                            }
                        }
                    }
                    _ => return,
                }
            }
        });
        let mut output = std::io::stdout().lock();
        let status = || -> serde_json::Value {
            state
                .as_ref()
                .and_then(|state| {
                    super::api::derived_access_status_json(&state.derived_history, false).ok()
                })
                .and_then(|raw| serde_json::from_str(&raw).ok())
                .unwrap_or_else(|| enrolled_status("unavailable", None))
        };
        write_frame(
            &Reply {
                epoch,
                operation: 0,
                request_id: String::new(),
                response: Response::json_ok("null".into()),
                status: status(),
            },
            &mut output,
            RESPONSE_LIMIT,
        )?;
        for (operation, request_id, request, cancelled) in receive {
            let mut observed = status();
            let response = if cancelled.load(Ordering::Acquire) {
                unavailable("operation cancelled before dispatch")
            } else {
                #[cfg(target_os = "macos")]
                {
                    if let Some(selection) = &selection {
                        super::server::counted_data_response(&request, |method, path, query| {
                            let (response, status) = enrolled_response(
                                &owner,
                                &repo,
                                selection,
                                &signer,
                                cancelled.clone(),
                                method,
                                path,
                                query,
                            );
                            observed = status;
                            response
                        })?
                    } else {
                        let response = super::server::data_response(
                            state.as_ref().expect("legacy state"),
                            &request,
                        )?;
                        observed = status();
                        response
                    }
                }
                #[cfg(not(target_os = "macos"))]
                {
                    let response = super::server::data_response(
                        state.as_ref().expect("legacy state"),
                        &request,
                    )?;
                    observed = status();
                    response
                }
            };
            write_frame(
                &Reply {
                    epoch,
                    operation,
                    request_id,
                    response,
                    status: observed,
                },
                &mut output,
                RESPONSE_LIMIT,
            )?;
            let mut active = lock(&cancellation);
            if active.as_ref().is_some_and(|(id, _)| *id == operation) {
                *active = None;
            }
        }
        Ok(())
    })();
    match result {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("owned Inspector: {error}");
            std::process::ExitCode::from(2)
        }
    }
}

fn enrolled_status(availability: &str, generation: Option<&str>) -> serde_json::Value {
    let mut doc = serde_json::json!({"schema":"pointbreak.inspect-derived-access-status","version":1,
        "active":true,"availability":availability,"namespace":"stable",
        "rebuildInFlight":false,"rebuildPaused":false,"servingCurrent":availability=="current",
        "fallbackInFlight":false,"actions":if availability=="current" {vec![]}else{vec!["retry"]}});
    if let Some(generation) = generation {
        doc["generationId"] = generation.into();
    }
    doc
}
#[cfg(target_os = "macos")]
#[allow(
    clippy::too_many_arguments,
    reason = "private owner, selection and HTTP request are separate inputs"
)]
fn enrolled_response(
    owner: &pointbreak::session::OwnedDataCommand,
    repo: &std::path::Path,
    selection: &EnrollmentSelection,
    signer: &super::page_token::PageTokenSigner,
    cancelled: Arc<AtomicBool>,
    method: &str,
    path: &str,
    query: Option<&str>,
) -> (Response, serde_json::Value) {
    if method == "GET" && path == "/api/identity" {
        let result = pointbreak::session::run_owned_data_request(
            owner,
            pointbreak::session::OwnedDataRequest {
                version: 1,
                repository: repo.into(),
                request_id: selection.request_id.clone(),
                enrollment_actor: selection.enrollment_actor.clone(),
                operation: pointbreak::session::OwnedDataOperation::Identity,
                limits: selection.limits,
            },
            cancelled,
        );
        return match result {
            Ok(document) => (
                Response::json_ok(document.to_string()),
                enrolled_status("unavailable", None),
            ),
            Err(error) => (
                unavailable(&error.to_string()),
                enrolled_status("unavailable", None),
            ),
        };
    }
    if method == "POST" && path == "/api/derived-access/retry" {
        let result = pointbreak::session::run_owned_data_request(
            owner,
            pointbreak::session::OwnedDataRequest {
                version: 1,
                repository: repo.into(),
                request_id: selection.request_id.clone(),
                enrollment_actor: selection.enrollment_actor.clone(),
                operation: pointbreak::session::OwnedDataOperation::Rebuild,
                limits: selection.limits,
            },
            cancelled,
        );
        return match result {
            Ok(value) => (
                Response::json_ok(value.to_string()),
                enrolled_status("unavailable", None),
            ),
            Err(error) => (
                unavailable(&error.to_string()),
                enrolled_status("unavailable", None),
            ),
        };
    }
    if method != "GET" {
        return (
            Response::json_error("405 Method Not Allowed", "unsupported method"),
            enrolled_status("unavailable", None),
        );
    }
    let result = pointbreak::session::with_owned_inspector_request(
        owner,
        repo,
        &selection.request_id,
        &selection.enrollment_actor,
        selection.limits,
        cancelled,
        |reader| {
            let response = if path == "/api/derived-access/status" {
                super::api::ChangeV2Json::Ok(reader.status_document().to_string())
            } else {
                super::api::enrolled::compose(reader, repo, path, query, signer)
                    .map_err(pointbreak::error::ShoreError::Message)?
            };
            let status = reader.status_document();
            Ok((super::server::change_v2_response(Ok(response)), status))
        },
    );
    match result {
        Ok(value) => value,
        Err(error) => {
            let body = serde_json::json!({"schema":"pointbreak.inspect-change-projection-error","version":1,
                "code":"projection_unstable","message":error.to_string(),"retryable":true});
            (
                super::server::change_v2_response(Ok(super::api::ChangeV2Json::Retryable(
                    body.to_string(),
                ))),
                enrolled_status("unavailable", None),
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bounded_frames_reject_truncation_oversize_and_unknown_commands() {
        for bytes in [
            b"{}".as_slice(),
            b"{}\n".as_slice(),
            b"{\"kind\":\"restart\"}\n".as_slice(),
        ] {
            assert!(read_frame::<CommandFrame>(&mut std::io::Cursor::new(bytes), 64).is_err());
        }
        assert!(read_frame::<serde_json::Value>(&mut std::io::Cursor::new(b"12345\n"), 4).is_err());
    }
    #[test]
    fn stale_responses_and_partial_retirement_never_admit_reuse() {
        let reply = Reply {
            epoch: 7,
            operation: 4,
            request_id: "request-a".into(),
            response: Response::json_ok("null".into()),
            status: serde_json::Value::Null,
        };
        assert!(matches_reply(7, 4, "request-a", &reply));
        assert!(!matches_reply(6, 4, "request-a", &reply));
        assert!(!matches_reply(7, 3, "request-a", &reply));
        assert!(!matches_reply(7, 4, "request-b", &reply));
        for (reaped, retired) in [(false, false), (true, false), (false, true)] {
            assert!(!replacement_admitted(reaped, retired));
        }
        assert!(replacement_admitted(true, true));
    }
    #[test]
    fn unknown_wait_settles_only_once_from_its_actual_response() {
        let make_reply = |request_id: &str| Reply {
            epoch: 2,
            operation: 8,
            request_id: request_id.into(),
            response: Response::json_error("503 Service Unavailable", "rebuild failed"),
            status: serde_json::json!({"generationId":"new"}),
        };
        let mut state = Detached {
            epoch: 2,
            active: Some(8),
            active_request: Some("request-a".into()),
            outcome: "unknown",
            cached: Some(serde_json::json!({"generationId":"old"})),
            ..Default::default()
        };
        assert!(
            accept_reply(
                &mut state,
                2,
                8,
                "request-a",
                false,
                true,
                make_reply("other")
            )
            .is_none()
        );
        assert_eq!(state.cached.as_ref().unwrap()["generationId"], "old");
        assert!(
            accept_reply(
                &mut state,
                2,
                8,
                "request-a",
                false,
                true,
                make_reply("request-a")
            )
            .is_some()
        );
        assert_eq!(state.last_outcome, Some("failed"));
        assert_eq!(state.detail, "rebuild failed");
        assert!(
            accept_reply(
                &mut state,
                2,
                8,
                "request-a",
                false,
                true,
                make_reply("request-a")
            )
            .is_none()
        );
    }
    #[test]
    fn foreground_queue_refuses_a_ninth_waiting_request() {
        let (sender, _receiver) = mpsc::sync_channel::<Job>(QUEUED);
        for operation in 0..QUEUED + 1 {
            let (reply, _receive) = mpsc::sync_channel(1);
            let job = Job {
                operation: operation as u64,
                request_id: format!("request-{operation}"),
                request: DataRequest {
                    method: "GET".into(),
                    target: "/api/v2/profile".into(),
                    #[cfg(feature = "longitudinal-counting")]
                    counting: Vec::new(),
                },
                abandoned: Arc::new(AtomicBool::new(false)),
                reply,
            };
            assert_eq!(sender.try_send(job).is_ok(), operation < QUEUED);
        }
    }
    #[test]
    fn status_is_detached_and_never_reports_cached_currentness() {
        let supervisor = Supervisor {
            repo: PathBuf::from("/not-opened"),
            selection: None,
            state: Arc::new(Mutex::new(Detached::default())),
        };
        let response = supervisor.status();
        let value: serde_json::Value = serde_json::from_slice(&response.body).unwrap();
        assert_eq!(value["servingCurrent"], false);
        assert_eq!(value["child"]["epoch"], 0);
    }
}
