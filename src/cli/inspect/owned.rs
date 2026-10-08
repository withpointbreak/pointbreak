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
    },
    Request {
        epoch: u64,
        operation: u64,
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
    response: Response,
    status: serde_json::Value,
}
struct Job {
    operation: u64,
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
    outcome: &'static str,
    detail: String,
    cached: Option<serde_json::Value>,
    running: Option<Running>,
}
pub(super) struct Supervisor {
    repo: PathBuf,
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
    pub(super) fn new(repo: PathBuf) -> Self {
        let this = Self {
            repo,
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
            state.epoch
        };
        let state = self.state.clone();
        let repo = self.repo.clone();
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
                            &CommandFrame::Start { epoch, repo },
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
                        Ok(reply) if matches_reply(epoch, 0, &reply) => {
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
                                continue;
                            }
                            state.active = Some(job.operation);
                            state.outcome = "running";
                        }
                        let result = (|| -> std::io::Result<Reply> {
                            io_control
                                .try_send(CommandFrame::Request {
                                    epoch,
                                    operation: job.operation,
                                    request: job.request,
                                })
                                .map_err(std::io::Error::other)?;
                            let reply: Reply = read_frame(&mut output, RESPONSE_LIMIT)?;
                            if !matches_reply(epoch, job.operation, &reply) {
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
                                state.active = None;
                                state.last_operation = Some(job.operation);
                                state.outcome = "completed";
                                state.detail.clear();
                                state.cached = Some(reply.status);
                                let _ = job.reply.try_send(reply.response);
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
            if matches!(state.outcome, "idle" | "completed") && state.active.is_none() {
                state.next_operation += 1;
                let operation = state.next_operation;
                let (reply, _receive) = mpsc::sync_channel(1);
                if let Some(running) = &state.running {
                    if running
                        .jobs
                        .try_send(Job {
                            operation,
                            request: DataRequest {
                                method: "POST".into(),
                                target: "/api/derived-access/retry".into(),
                                #[cfg(feature = "longitudinal-counting")]
                                counting: Vec::new(),
                            },
                            abandoned: Arc::new(AtomicBool::new(false)),
                            reply,
                        })
                        .is_err()
                    {
                        return unavailable("rebuild control queue full or retired");
                    }
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
        }
        self.start();
        self.status()
    }
    pub(super) fn status(&self) -> Response {
        let state = lock(&self.state);
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
        doc["child"] = serde_json::json!({"epoch":state.epoch,"operation":state.active,
            "state":if state.outcome.is_empty() { "unavailable" } else { state.outcome },"lastCompletedOperation":state.last_operation,"detail":state.detail,"cooperativeCancellation":true});
        if state.active.is_some() {
            doc["actions"] = serde_json::json!(["wait", "cancel"]);
        }
        if matches!(state.outcome, "unknown" | "unavailable") {
            doc["actions"] = serde_json::json!(["retry"]);
        }
        Response::json_ok(doc.to_string())
    }
}
fn matches_reply(epoch: u64, operation: u64, reply: &Reply) -> bool {
    reply.epoch == epoch && reply.operation == operation
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
    fn terminal(_: pointbreak::session::OwnedProcessFailure) -> ! {
        // SAFETY: this private entry owns the entire process before CLI setup.
        unsafe { libc::_exit(90) }
    }
    let result = (|| -> Result<(), Box<dyn std::error::Error>> {
        if !arguments.is_empty() {
            return Err("private Inspector accepts no positional arguments".into());
        }
        // SAFETY: main dispatches here before tracing, services or threads. The
        // token remains on this sole data thread until process exit.
        let _owner = unsafe { pointbreak::session::enter_owned_data_command(terminal) }?;
        let mut input = BufReader::new(std::io::stdin());
        let CommandFrame::Start { epoch, repo } = read_frame(&mut input, REQUEST_LIMIT)? else {
            return Err("missing Inspector bootstrap".into());
        };
        let state = Arc::new(InspectState::new(repo)?);
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
                        request,
                    } if request_epoch == epoch => {
                        let cancelled = Arc::new(AtomicBool::new(false));
                        *lock(&control_cancellation) = Some((operation, cancelled.clone()));
                        if requests.try_send((operation, request, cancelled)).is_err() {
                            return;
                        }
                    }
                    CommandFrame::Cancel {
                        epoch: request_epoch,
                        operation,
                    } if request_epoch == epoch => {
                        if operation == 0 {
                            background_control
                                .derived_history
                                .request_background_cancel();
                        }
                        if let Some((active, cancelled)) = &*lock(&control_cancellation) {
                            if *active == operation {
                                cancelled.store(true, Ordering::Release);
                                background_control
                                    .derived_history
                                    .request_background_cancel();
                            }
                        }
                    }
                    _ => return,
                }
            }
        });
        let mut output = std::io::stdout().lock();
        let status = || -> serde_json::Value {
            super::api::derived_access_status_json(&state.derived_history, false)
                .ok()
                .and_then(|raw| serde_json::from_str(&raw).ok())
                .unwrap_or(serde_json::Value::Null)
        };
        write_frame(
            &Reply {
                epoch,
                operation: 0,
                response: Response::json_ok("null".into()),
                status: status(),
            },
            &mut output,
            RESPONSE_LIMIT,
        )?;
        for (operation, request, cancelled) in receive {
            let response = if cancelled.load(Ordering::Acquire) {
                unavailable("operation cancelled before dispatch")
            } else {
                super::server::data_response(&state, &request)?
            };
            write_frame(
                &Reply {
                    epoch,
                    operation,
                    response,
                    status: status(),
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
            response: Response::json_ok("null".into()),
            status: serde_json::Value::Null,
        };
        assert!(matches_reply(7, 4, &reply));
        assert!(!matches_reply(6, 4, &reply));
        assert!(!matches_reply(7, 3, &reply));
        for (reaped, retired) in [(false, false), (true, false), (false, true)] {
            assert!(!replacement_admitted(reaped, retired));
        }
        assert!(replacement_admitted(true, true));
    }
    #[test]
    fn status_is_detached_and_never_reports_cached_currentness() {
        let supervisor = Supervisor {
            repo: PathBuf::from("/not-opened"),
            state: Arc::new(Mutex::new(Detached::default())),
        };
        let response = supervisor.status();
        let value: serde_json::Value = serde_json::from_slice(&response.body).unwrap();
        assert_eq!(value["servingCurrent"], false);
        assert_eq!(value["child"]["epoch"], 0);
    }
}
