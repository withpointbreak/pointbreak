//! Minimal synchronous HTTP/1.1 server for the Pointbreak Review inspector.
//!
//! This is deliberately small and blocking: one OS thread per connection,
//! `Connection: close` responses, and read-only GET routing plus two exact
//! lifecycle-control POSTs. It introduces no async runtime and no third-party
//! HTTP crate, in keeping with the storage-model rule against pulling in a
//! runtime before a remote backend forces it. It is a localhost developer
//! tool, not a production server.

use std::collections::{BTreeSet, HashMap};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::time::Duration;
use std::{fmt, thread};

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use pointbreak::documents::{
    ChangeQueryUnavailableDocumentV1, InspectStartupDocument, ReaderUpgradeRequiredDocumentV1,
    version_document,
};
use pointbreak::model::EventId;
use pointbreak::session::{
    HistoryOrder, HistoryPage, HistoryQuery, QueryDiagnosticCode, QuerySurface,
    SnapshotSummaryCache, parse_search_query_for,
};

use super::{StartupOutputFormat, api};

const TOKEN_BYTES: usize = 32;
const MAX_REQUEST_LINE_BYTES: usize = 8 * 1024;
const MAX_HEADER_COUNT: usize = 64;
const MAX_HEADER_BYTES: usize = 32 * 1024;

struct RequestPolicy {
    canonical_host: String,
    token: SecretToken,
    serve_static: bool,
}

struct SecretToken(String);

impl SecretToken {
    fn generate() -> Result<Self, getrandom::Error> {
        let mut bytes = [0_u8; TOKEN_BYTES];
        getrandom::fill(&mut bytes)?;
        Ok(Self(URL_SAFE_NO_PAD.encode(bytes)))
    }

    fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for SecretToken {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("SecretToken([redacted])")
    }
}

impl fmt::Display for SecretToken {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("[redacted]")
    }
}

struct RequestHead {
    method: String,
    target: String,
    hosts: Vec<String>,
    authorizations: Vec<String>,
    #[cfg(feature = "longitudinal-counting")]
    longitudinal_counting: Vec<String>,
}

#[cfg(feature = "longitudinal-counting")]
#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct LongitudinalCountingRequest {
    run_identity: String,
    context: pointbreak::bench_support::longitudinal::LongitudinalCounterReceiptContextV1,
}

#[derive(Debug)]
enum RequestParseError {
    Io(std::io::Error),
    BadRequest,
    HeaderTooLarge,
}

impl From<std::io::Error> for RequestParseError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

/// Shared, read-only inspector server state. Holds the resolved store path and the read-time
/// highlight cache; cloned cheaply behind an `Arc` to every connection thread.
pub(super) struct InspectState {
    pub repo: PathBuf,
    pub derived_history: pointbreak::session::DerivedHistoryAccess,
    pub highlight_cache: RwLock<HighlightCache>,
    /// The single-slot exhaustive-search cache (#255). Default-off requests
    /// retain the legacy behavior; the active derived profile touches it only
    /// for explicit body search.
    pub history_cache: super::cache::HistoryProjectionCache,
    /// Content-hash-keyed snapshot summary counts shared across requested
    /// `/api/revisions` pages: an artifact loaded on more than one page is
    /// decoded once per server process.
    pub snapshot_summaries: Arc<SnapshotSummaryCache>,
    /// One complete Change reader snapshot keyed by the append-only Journal
    /// marker. The mutex is also the rebuild permit: concurrent requests wait
    /// for one fold rather than multiplying decoded histories.
    pub change_reader_cache: ChangeReaderCache,
    /// The eager cache warm is delayed until the first authenticated API request,
    /// so serving the recovery shell never opens the store.
    initial_warm_started: AtomicBool,
    /// One service-wide permit for user-elected authoritative fallback. The
    /// fallback is intentionally expensive and request-local; concurrent
    /// callers receive a typed busy response instead of multiplying complete
    /// loose-store replay and decoded ownership.
    authoritative_fallback: AuthoritativeFallbackGate,
}

impl InspectState {
    pub(super) fn new(repo: PathBuf) -> Result<Self, String> {
        Self::new_with_background_rebuild(repo, true)
    }

    fn new_with_background_rebuild(
        repo: PathBuf,
        start_background_rebuild: bool,
    ) -> Result<Self, String> {
        let derived_history =
            pointbreak::session::DerivedHistoryAccess::resolve_for_inspector(&repo)?;
        if start_background_rebuild && let Err(error) = derived_history.start_background_rebuild() {
            tracing::warn!(error = %error, "derived_access_background_rebuild_start_failed");
        }
        Ok(Self {
            repo,
            derived_history,
            highlight_cache: RwLock::new(HighlightCache::new(HIGHLIGHT_CACHE_CAPACITY)),
            history_cache: super::cache::HistoryProjectionCache::new(),
            snapshot_summaries: Arc::new(SnapshotSummaryCache::new()),
            change_reader_cache: ChangeReaderCache::new(),
            initial_warm_started: AtomicBool::new(false),
            authoritative_fallback: AuthoritativeFallbackGate::new(),
        })
    }
}

struct CachedChangeReaderState {
    marker: u64,
    state: Arc<pointbreak::session::ChangeReaderStateV1>,
}

/// Single-generation cache for the warm Change reader.
///
/// The cheap marker is only an invalidation detector. A miss always performs a
/// complete capability-validated fold, and the marker is re-read afterward so
/// a moving Journal can never publish a mixed generation. Cold CLI commands do
/// not use this process-local cache.
pub(super) struct ChangeReaderCache {
    slot: Mutex<Option<CachedChangeReaderState>>,
}

impl ChangeReaderCache {
    fn new() -> Self {
        Self {
            slot: Mutex::new(None),
        }
    }

    pub(super) fn load(
        &self,
        repo: &std::path::Path,
    ) -> Result<Arc<pointbreak::session::ChangeReaderStateV1>, String> {
        let mut slot = self
            .slot
            .lock()
            .map_err(|_| "Change reader cache lock is poisoned".to_owned())?;
        for _ in 0..2 {
            let before = pointbreak::session::change_reader_head_marker_for_repo(repo)
                .map_err(|error| error.to_string())?;
            if let Some(cached) = slot.as_ref()
                && cached.marker == before
            {
                return Ok(Arc::clone(&cached.state));
            }
            let state = Arc::new(
                pointbreak::session::change_reader_state_for_repo(repo)
                    .map_err(|error| error.to_string())?,
            );
            let after = pointbreak::session::change_reader_head_marker_for_repo(repo)
                .map_err(|error| error.to_string())?;
            if before == after {
                *slot = Some(CachedChangeReaderState {
                    marker: after,
                    state: Arc::clone(&state),
                });
                return Ok(state);
            }
        }
        Err("Journal changed while the Change reader generation was loading; retry".to_owned())
    }
}

struct AuthoritativeFallbackGate {
    in_flight: AtomicBool,
}

impl AuthoritativeFallbackGate {
    const fn new() -> Self {
        Self {
            in_flight: AtomicBool::new(false),
        }
    }

    fn is_in_flight(&self) -> bool {
        self.in_flight.load(Ordering::Acquire)
    }

    fn try_acquire(&self) -> Option<AuthoritativeFallbackGuard<'_>> {
        self.in_flight
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .ok()
            .map(|_| AuthoritativeFallbackGuard(&self.in_flight))
    }
}

struct AuthoritativeFallbackGuard<'a>(&'a AtomicBool);

impl Drop for AuthoritativeFallbackGuard<'_> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}

/// How many `snapshot_json` responses to retain. Snapshots are opened on demand (not polled), so a
/// small cap amortizes repeat opens without holding the whole history.
const HIGHLIGHT_CACHE_CAPACITY: usize = 64;

/// A bounded, content-hash-keyed cache of fully-rendered `snapshot_json` responses. Eviction is
/// always safe: the value is recomputable from the content-addressed artifact, so there is no
/// invalidation — entries only age out by insertion order once the cap is reached.
pub(super) struct HighlightCache {
    cap: usize,
    map: HashMap<String, String>,
    order: Vec<String>,
}

impl HighlightCache {
    pub(super) fn new(cap: usize) -> Self {
        Self {
            cap,
            map: HashMap::new(),
            order: Vec::new(),
        }
    }

    pub(super) fn get(&self, key: &str) -> Option<String> {
        #[cfg(feature = "longitudinal-counting")]
        self.record_longitudinal_ownership();
        self.map.get(key).cloned()
    }

    pub(super) fn put(&mut self, key: &str, value: String) {
        if self.map.contains_key(key) {
            self.map.insert(key.to_owned(), value);
            #[cfg(feature = "longitudinal-counting")]
            self.record_longitudinal_ownership();
            return;
        }
        if self.cap == 0 {
            #[cfg(feature = "longitudinal-counting")]
            self.record_longitudinal_ownership();
            return;
        }
        while self.order.len() >= self.cap {
            let evicted = self.order.remove(0);
            self.map.remove(&evicted);
        }
        self.order.push(key.to_owned());
        self.map.insert(key.to_owned(), value);
        #[cfg(feature = "longitudinal-counting")]
        self.record_longitudinal_ownership();
    }

    #[cfg(feature = "longitudinal-counting")]
    fn record_longitudinal_ownership(&self) {
        pointbreak::bench_support::longitudinal::set_retained_snapshot_highlight_entries(
            self.map.len(),
        );
        pointbreak::bench_support::longitudinal::set_retained_snapshot_highlight_bytes(
            self.map.values().map(String::len).sum(),
        );
    }
}

const INDEX_HTML: &str = include_str!("assets/index.html");
const TOKENS_CSS: &str = include_str!("assets/tokens.css");
const APP_CSS: &str = include_str!("assets/app.css");
const APP_JS: &str = include_str!("assets/app.js");
const POINTBREAK_LOGO_MONO_SVG: &[u8] = include_bytes!("assets/pointbreak-logo-mono.svg");
const FAVICON_SVG: &[u8] = include_bytes!("assets/favicon.svg");
const FAVICON_PNG: &[u8] = include_bytes!("assets/favicon.png");
const FAVICON_DARK_PNG: &[u8] = include_bytes!("assets/favicon-dark.png");

struct Response {
    status: &'static str,
    content_type: &'static str,
    body: Vec<u8>,
    content_security_policy: bool,
    headers: Vec<(&'static str, &'static str)>,
}

impl Response {
    fn new(status: &'static str, content_type: &'static str, body: Vec<u8>) -> Self {
        Self {
            status,
            content_type,
            body,
            content_security_policy: false,
            headers: Vec::new(),
        }
    }

    fn with_header(mut self, name: &'static str, value: &'static str) -> Self {
        self.headers.push((name, value));
        self
    }

    fn asset(content_type: &'static str, body: &str) -> Self {
        Self::new("200 OK", content_type, body.as_bytes().to_vec())
    }

    fn shell(body: &str) -> Self {
        let mut response = Self::asset("text/html; charset=utf-8", body);
        response.content_security_policy = true;
        response
    }

    fn asset_bytes(content_type: &'static str, body: &[u8]) -> Self {
        Self::new("200 OK", content_type, body.to_vec())
    }

    fn json_ok(body: String) -> Self {
        Self::new(
            "200 OK",
            "application/json; charset=utf-8",
            body.into_bytes(),
        )
    }

    fn json_error(status: &'static str, message: &str) -> Self {
        let body = serde_json::json!({ "error": message }).to_string();
        Self::new(status, "application/json; charset=utf-8", body.into_bytes())
    }

    fn text(status: &'static str, message: &str) -> Self {
        Self::new(
            status,
            "text/plain; charset=utf-8",
            message.as_bytes().to_vec(),
        )
    }

    fn unauthorized() -> Self {
        Self::new("401 Unauthorized", "text/plain; charset=utf-8", Vec::new())
    }
}

pub(super) fn serve(
    addr: SocketAddr,
    repo: PathBuf,
    open: bool,
    api_only: bool,
    output_format: StartupOutputFormat,
    stdout: &mut dyn Write,
) -> Result<(), Box<dyn std::error::Error>> {
    let listener =
        TcpListener::bind(addr).map_err(|error| format!("could not bind {addr}: {error}"))?;
    // Resolve the actually-bound address so an ephemeral port (`--port 0`)
    // is shown and opened correctly rather than `:0`.
    let bound = listener.local_addr().unwrap_or(addr);
    let url = format!("http://{bound}/");
    let policy = Arc::new(RequestPolicy {
        canonical_host: bound.to_string(),
        token: SecretToken::generate()
            .map_err(|error| format!("could not generate inspect bearer: {error}"))?,
        serve_static: !api_only,
    });
    let capability_url = format!("{url}#/timeline?token={}", policy.token.expose());
    let state = Arc::new(InspectState::new(repo.clone())?);

    match (api_only, output_format) {
        (false, StartupOutputFormat::Text) => {
            writeln!(stdout, "Pointbreak Review inspector")?;
            writeln!(stdout, "  store: {}", repo.display())?;
            writeln!(stdout, "  url:   {capability_url}")?;
            writeln!(stdout, "  stop:  Ctrl-C")?;
        }
        (true, StartupOutputFormat::Text) => {
            writeln!(stdout, "Pointbreak Review inspector API")?;
            writeln!(stdout, "  endpoint: {url}")?;
            writeln!(stdout, "  token: {}", policy.token.expose())?;
            writeln!(stdout, "  stop:  Ctrl-C")?;
        }
        (_, StartupOutputFormat::Json) => {
            serde_json::to_writer(
                &mut *stdout,
                &InspectStartupDocument::new(
                    bound.ip().to_string(),
                    bound.port(),
                    policy.token.expose(),
                ),
            )?;
            writeln!(stdout)?;
        }
    }
    stdout.flush().ok();

    if open {
        open_browser(&capability_url);
    }

    for stream in listener.incoming() {
        match stream {
            Ok(stream) => {
                let state = Arc::clone(&state);
                let policy = Arc::clone(&policy);
                thread::spawn(move || {
                    if let Err(error) = handle_connection(stream, &state, &policy) {
                        tracing::debug!(error = %error, "inspect_connection_error");
                    }
                });
            }
            Err(error) => {
                tracing::debug!(error = %error, "inspect_accept_error");
            }
        }
    }

    Ok(())
}

fn handle_connection(
    stream: TcpStream,
    state: &Arc<InspectState>,
    policy: &RequestPolicy,
) -> std::io::Result<()> {
    stream.set_read_timeout(Some(Duration::from_secs(15)))?;
    stream.set_write_timeout(Some(Duration::from_secs(30)))?;

    let read_half = stream.try_clone()?;
    let mut reader = BufReader::new(read_half);

    let request = match parse_request_head(&mut reader) {
        Ok(Some(request)) => request,
        Ok(None) => return Ok(()),
        Err(RequestParseError::Io(error)) => return Err(error),
        Err(RequestParseError::BadRequest) => {
            return write_response(stream, &Response::text("400 Bad Request", "bad request"));
        }
        Err(RequestParseError::HeaderTooLarge) => {
            return write_response(
                stream,
                &Response::text(
                    "431 Request Header Fields Too Large",
                    "request headers too large",
                ),
            );
        }
    };

    let (path, query) = split_target(&request.target);
    if !has_exact_host(policy, &request)
        || (is_api_path(path) && !has_exact_bearer(policy, &request))
    {
        return write_response(stream, &Response::unauthorized());
    }
    #[cfg(feature = "longitudinal-counting")]
    if !request.longitudinal_counting.is_empty() && !is_api_path(path) {
        return write_response(stream, &Response::unauthorized());
    }
    if is_api_path(path) {
        warm_caches_after_auth(state);
    }

    #[cfg(feature = "longitudinal-counting")]
    if let Some(counting_request) = longitudinal_counting_request(&request)? {
        let scope = pointbreak::bench_support::longitudinal::LongitudinalCountingScopeV1::new(
            counting_request.run_identity,
        )
        .map_err(std::io::Error::other)?;
        let _guard = scope.enter();
        let response = route(state, policy.serve_static, &request.method, path, query);
        record_response_body(&response);
        let receipt = longitudinal_receipt_header(&scope, counting_request.context, &response)?;
        return write_response_inner(
            stream,
            &response,
            Some(("X-Pointbreak-Longitudinal-Receipt", receipt.as_str())),
        );
    }

    let response = route(state, policy.serve_static, &request.method, path, query);
    write_response(stream, &response)
}

#[cfg(feature = "longitudinal-counting")]
fn longitudinal_receipt_header(
    scope: &pointbreak::bench_support::longitudinal::LongitudinalCountingScopeV1,
    mut context: pointbreak::bench_support::longitudinal::LongitudinalCounterReceiptContextV1,
    response: &Response,
) -> std::io::Result<String> {
    context.success = response.status == "200 OK";
    let receipt = scope
        .receipt(context)
        .map_err(|error| std::io::Error::other(error.to_string()))?;
    let receipt =
        serde_json::to_vec(&receipt).map_err(|error| std::io::Error::other(error.to_string()))?;
    Ok(URL_SAFE_NO_PAD.encode(receipt))
}

#[cfg(feature = "longitudinal-counting")]
fn longitudinal_counting_request(
    request: &RequestHead,
) -> std::io::Result<Option<LongitudinalCountingRequest>> {
    match request.longitudinal_counting.as_slice() {
        [] => Ok(None),
        [encoded] => {
            let bytes = URL_SAFE_NO_PAD
                .decode(encoded)
                .map_err(|error| std::io::Error::other(error.to_string()))?;
            serde_json::from_slice(&bytes)
                .map(Some)
                .map_err(|error| std::io::Error::other(error.to_string()))
        }
        _ => Err(std::io::Error::other(
            "multiple longitudinal counting headers",
        )),
    }
}

fn warm_caches_after_auth(state: &Arc<InspectState>) {
    if state.initial_warm_started.swap(true, Ordering::AcqRel) {
        return;
    }
    let state = Arc::clone(state);
    thread::spawn(move || {
        if let Err(error) = state.change_reader_cache.load(state.repo.as_path()) {
            tracing::debug!(error = %error, "inspect_change_reader_cache_warm_failed");
        }
        if !state.derived_history.is_active()
            && let Err(error) = api::warm_history_cache(state.repo.as_path(), &state.history_cache)
        {
            tracing::debug!(error = %error, "inspect_history_cache_warm_failed");
        }
    });
}

fn parse_request_head(reader: &mut impl BufRead) -> Result<Option<RequestHead>, RequestParseError> {
    let Some(request_line) = read_bounded_line(reader, MAX_REQUEST_LINE_BYTES)? else {
        return Ok(None);
    };
    let mut parts = request_line.split_whitespace();
    let method = parts.next().ok_or(RequestParseError::BadRequest)?;
    let target = parts.next().ok_or(RequestParseError::BadRequest)?;
    let protocol = parts.next().ok_or(RequestParseError::BadRequest)?;
    if parts.next().is_some() || !protocol.starts_with("HTTP/1.") {
        return Err(RequestParseError::BadRequest);
    }

    let mut header_count = 0_usize;
    let mut header_bytes = 0_usize;
    let mut hosts = Vec::new();
    let mut authorizations = Vec::new();
    #[cfg(feature = "longitudinal-counting")]
    let mut longitudinal_counting = Vec::new();
    loop {
        let Some(line) = read_bounded_line(reader, MAX_HEADER_BYTES)? else {
            return Err(RequestParseError::BadRequest);
        };
        header_bytes = header_bytes.saturating_add(line.len());
        if header_bytes > MAX_HEADER_BYTES {
            return Err(RequestParseError::HeaderTooLarge);
        }
        if line == "\r\n" || line == "\n" {
            break;
        }
        header_count += 1;
        if header_count > MAX_HEADER_COUNT {
            return Err(RequestParseError::HeaderTooLarge);
        }

        let line = line.trim_end_matches(['\r', '\n']);
        let (name, value) = line.split_once(':').ok_or(RequestParseError::BadRequest)?;
        let value = value.trim_matches([' ', '\t']);
        if name.eq_ignore_ascii_case("host") {
            hosts.push(value.to_owned());
        } else if name.eq_ignore_ascii_case("authorization") {
            authorizations.push(value.to_owned());
        } else if cfg!(feature = "longitudinal-counting")
            && name.eq_ignore_ascii_case("x-pointbreak-longitudinal-counting")
        {
            #[cfg(feature = "longitudinal-counting")]
            longitudinal_counting.push(value.to_owned());
        }
    }

    Ok(Some(RequestHead {
        method: method.to_owned(),
        target: target.to_owned(),
        hosts,
        authorizations,
        #[cfg(feature = "longitudinal-counting")]
        longitudinal_counting,
    }))
}

fn read_bounded_line(
    reader: &mut impl BufRead,
    limit: usize,
) -> Result<Option<String>, RequestParseError> {
    let mut bytes = Vec::new();
    let read = reader
        .take((limit + 1) as u64)
        .read_until(b'\n', &mut bytes)?;
    if read == 0 {
        return Ok(None);
    }
    if bytes.len() > limit || !bytes.ends_with(b"\n") {
        return Err(RequestParseError::HeaderTooLarge);
    }
    String::from_utf8(bytes)
        .map(Some)
        .map_err(|_| RequestParseError::BadRequest)
}

fn has_exact_host(policy: &RequestPolicy, request: &RequestHead) -> bool {
    let [host] = request.hosts.as_slice() else {
        return false;
    };
    host == &policy.canonical_host
}

fn has_exact_bearer(policy: &RequestPolicy, request: &RequestHead) -> bool {
    let [authorization] = request.authorizations.as_slice() else {
        return false;
    };
    let Some(presented) = authorization.strip_prefix("Bearer ") else {
        return false;
    };
    secret_eq(presented.as_bytes(), policy.token.expose().as_bytes())
}

fn is_api_path(path: &str) -> bool {
    path == "/api" || path.starts_with("/api/")
}

fn secret_eq(left: &[u8], right: &[u8]) -> bool {
    if left.len() != right.len() {
        return false;
    }
    left.iter()
        .zip(right)
        .fold(0_u8, |difference, (left, right)| {
            difference | (left ^ right)
        })
        == 0
}

fn route(
    state: &Arc<InspectState>,
    serve_static: bool,
    method: &str,
    path: &str,
    query: Option<&str>,
) -> Response {
    if method == "POST" {
        return match path {
            "/api/derived-access/cancel" => derived_access_control_response(state, false),
            "/api/derived-access/retry" => derived_access_control_response(state, true),
            _ => Response::text("405 Method Not Allowed", "method not allowed"),
        };
    }
    if method != "GET" {
        return Response::text("405 Method Not Allowed", "method not allowed");
    }

    if serve_static && let Some(response) = static_response(path) {
        return response;
    }
    if !is_api_path(path) {
        return Response::json_error("404 Not Found", "no such route");
    }

    let repo = state.repo.as_path();
    if path == "/api/v2/profile" {
        return api_response(api::change_v2_profile_json(
            repo,
            &state.change_reader_cache,
        ));
    }
    if path == "/api/v2/changes" {
        return change_v2_response(api::changes_v2_json(repo, &state.change_reader_cache));
    }
    if path == "/api/v2/attention" {
        return change_v2_response(api::change_attention_v2_json(
            repo,
            &state.change_reader_cache,
        ));
    }
    if path.starts_with("/api/v2/changes/") {
        return route_change_v2(state, path, query);
    }
    if is_legacy_semantic_path(path) {
        match pointbreak::session::activated_store_capability_for_repo(repo) {
            Ok(Some(capability)) => {
                if let Some(response) = legacy_semantic_gate(&capability) {
                    return response;
                }
            }
            Ok(None) => {}
            Err(error) => {
                return Response::json_error("500 Internal Server Error", &error.to_string());
            }
        }
    }
    match path {
        "/api/derived-access/status" => api_response(api::derived_access_status_json(
            &state.derived_history,
            state.authoritative_fallback.is_in_flight(),
        )),
        // The poll probe shares `/api/history` filtering but returns no entries.
        "/api/history/new-count" => match history_query(query) {
            Ok(request) => {
                let since_occurred_at = query_param(query, "sinceOccurredAt");
                let since_event_id = query_param(query, "sinceEventId");
                match (since_occurred_at, since_event_id) {
                    (None, None) => match requested_authoritative_access(query) {
                        Ok(true) => {
                            explicit_authoritative_response(state, api::zero_new_count_json)
                        }
                        Ok(false) => routed_api_response(api::routed_zero_new_count_json(
                            &state.derived_history,
                        )),
                        Err(message) => Response::json_error("400 Bad Request", &message),
                    },
                    (Some(occurred_at), Some(event_id)) => {
                        match requested_authoritative_access(query) {
                            Ok(true) => explicit_authoritative_response(state, || {
                                api::authoritative_new_count_json(
                                    repo,
                                    &request.query,
                                    &occurred_at,
                                    &event_id,
                                )
                            }),
                            Ok(false) => routed_api_response(api::routed_new_count_json(
                                repo,
                                &state.derived_history,
                                &state.history_cache,
                                &request.query,
                                &occurred_at,
                                &event_id,
                            )),
                            Err(message) => Response::json_error("400 Bad Request", &message),
                        }
                    }
                    _ => Response::json_error("400 Bad Request", "incomplete history cursor"),
                }
            }
            Err(message) => Response::json_error("400 Bad Request", &message),
        },
        "/api/history" => match history_query(query) {
            Ok(request) => match requested_authoritative_access(query) {
                Ok(true) => explicit_authoritative_response(state, || {
                    api::authoritative_history_json(repo, &request.query, &request.page)
                }),
                Ok(false) => routed_api_response(api::routed_history_json(
                    repo,
                    &state.derived_history,
                    &state.history_cache,
                    &request.query,
                    &request.page,
                )),
                Err(message) => Response::json_error("400 Bad Request", &message),
            },
            Err(message) => Response::json_error("400 Bad Request", &message),
        },
        "/api/revisions" => match revision_page_request(query) {
            Ok(request) => match requested_authoritative_access(query) {
                Ok(true) => explicit_authoritative_routed_response(state, || {
                    api::authoritative_revisions_json(repo, &request)
                }),
                Ok(false) => routed_api_response(api::routed_revisions_json(
                    repo,
                    &state.derived_history,
                    &state.snapshot_summaries,
                    &request,
                )),
                Err(message) => Response::json_error("400 Bad Request", &message),
            },
            Err(_) => Response::json_error("400 Bad Request", "invalid revision page request"),
        },
        "/api/threads" => match requested_authoritative_access(query) {
            Ok(true) => explicit_authoritative_response(state, || api::threads_json(repo)),
            Ok(false) => {
                routed_api_response(api::routed_threads_json(repo, &state.derived_history))
            }
            Err(message) => Response::json_error("400 Bad Request", &message),
        },
        "/api/attention" => {
            // An empty `revision=` is absent, matching the exact-match history
            // params (`track=`/`snapshot=`).
            let revision = query_param(query, "revision").filter(|value| !value.is_empty());
            match requested_authoritative_access(query) {
                Ok(true) => explicit_authoritative_response(state, || {
                    api::attention_json(repo, revision.as_deref())
                }),
                Ok(false) => routed_api_response(api::routed_attention_json(
                    repo,
                    &state.derived_history,
                    revision.as_deref(),
                )),
                Err(message) => Response::json_error("400 Bad Request", &message),
            }
        }
        "/api/freshness" => {
            // The freshness poll is the client's change detector; ride it to
            let stamp = api::freshness_commit_graph_stamp(repo);
            api_response(api::routed_freshness_json(
                repo,
                &state.derived_history,
                stamp,
            ))
        }
        "/api/version" => api_response(
            serde_json::to_string(&version_document()).map_err(|error| error.to_string()),
        ),
        "/api/identity" => api_response(api::identity_json(repo)),
        _ => route_member(state, path, query),
    }
}

fn is_legacy_semantic_path(path: &str) -> bool {
    matches!(
        path,
        "/api/history"
            | "/api/history/new-count"
            | "/api/revisions"
            | "/api/threads"
            | "/api/attention"
            | "/api/freshness"
    ) || path_member(path, "/api/revisions/").is_some_and(|member| !member.is_empty())
        || path_member(path, "/api/snapshots/").is_some_and(|member| !member.is_empty())
}

fn legacy_semantic_gate(
    capability: &pointbreak::session::StoreCapabilityInspection,
) -> Option<Response> {
    match &capability.status {
        pointbreak::session::StoreCapabilityStatus::Ready { .. } => {
            let document = ReaderUpgradeRequiredDocumentV1::new(
                "review_change_revision_v1",
                Some("legacy_revision_v2".to_owned()),
            );
            Some(match serde_json::to_string(&document) {
                Ok(body) => Response::new(
                    "426 Upgrade Required",
                    "application/json; charset=utf-8",
                    body.into_bytes(),
                ),
                Err(error) => Response::json_error("500 Internal Server Error", &error.to_string()),
            })
        }
        pointbreak::session::StoreCapabilityStatus::MigrationRequired => None,
        pointbreak::session::StoreCapabilityStatus::MigrationInProgress { .. } => {
            let document = ChangeQueryUnavailableDocumentV1::for_inspection(capability)
                .expect("non-ready capability has a typed unavailable document");
            Some(match serde_json::to_string(&document) {
                Ok(body) => Response::new(
                    "409 Conflict",
                    "application/json; charset=utf-8",
                    body.into_bytes(),
                ),
                Err(error) => Response::json_error("500 Internal Server Error", &error.to_string()),
            })
        }
    }
}

fn route_change_v2(state: &InspectState, path: &str, query: Option<&str>) -> Response {
    let Some(member_path) = path.strip_prefix("/api/v2/changes/") else {
        return Response::json_error("404 Not Found", "no such route");
    };
    let repo = state.repo.as_path();
    let cache = &state.change_reader_cache;
    let segments = member_path
        .split('/')
        .map(decode_member)
        .collect::<Option<Vec<_>>>();
    let Some(segments) = segments else {
        return Response::json_error("400 Bad Request", "invalid Change route identity");
    };
    match segments.as_slice() {
        [change_id] => change_v2_response(api::change_detail_v2_json(repo, cache, change_id)),
        [change_id, revisions, revision_id] if revisions == "revisions" => {
            let Some(artifact_hash) = query_param(query, "artifactHash") else {
                return Response::json_error("400 Bad Request", "missing artifactHash");
            };
            change_v2_response(api::change_revision_v2_json(
                repo,
                cache,
                change_id,
                revision_id,
                &artifact_hash,
                false,
            ))
        }
        [change_id, revisions, revision_id, resource]
            if revisions == "revisions" && resource == "resource" =>
        {
            let Some(artifact_hash) = query_param(query, "artifactHash") else {
                return Response::json_error("400 Bad Request", "missing artifactHash");
            };
            change_v2_response(api::change_revision_v2_json(
                repo,
                cache,
                change_id,
                revision_id,
                &artifact_hash,
                true,
            ))
        }
        [change_id, interdiff, from_revision_id, to_revision_id] if interdiff == "interdiff" => {
            let Some(from_hash) = query_param(query, "fromArtifactHash") else {
                return Response::json_error("400 Bad Request", "missing fromArtifactHash");
            };
            let Some(to_hash) = query_param(query, "toArtifactHash") else {
                return Response::json_error("400 Bad Request", "missing toArtifactHash");
            };
            change_v2_response(api::change_interdiff_v2_json(
                repo,
                cache,
                change_id,
                from_revision_id,
                &from_hash,
                to_revision_id,
                &to_hash,
            ))
        }
        _ => Response::json_error("404 Not Found", "no such route"),
    }
}

fn change_v2_response(result: Result<api::ChangeV2Json, String>) -> Response {
    match result {
        Ok(api::ChangeV2Json::Ok(body)) => Response::json_ok(body),
        Ok(api::ChangeV2Json::Unavailable(body)) => Response::new(
            "409 Conflict",
            "application/json; charset=utf-8",
            body.into_bytes(),
        ),
        Err(message) => Response::json_error("500 Internal Server Error", &message),
    }
}

fn requested_authoritative_access(query: Option<&str>) -> Result<bool, String> {
    match query_param(query, "access").as_deref() {
        None | Some("") | Some("derived") => Ok(false),
        Some("authoritative") => Ok(true),
        Some(_) => Err("invalid access mode".to_owned()),
    }
}

fn explicit_authoritative_response(
    state: &InspectState,
    build: impl FnOnce() -> Result<String, String>,
) -> Response {
    if !state.derived_history.is_active() {
        return api_response(build());
    }
    let Some(_permit) = state.authoritative_fallback.try_acquire() else {
        return Response::json_error(
            "429 Too Many Requests",
            "an authoritative fallback is already in progress",
        );
    };
    api_response(build()).with_header("X-Pointbreak-Access-Source", "authoritative-fallback")
}

fn explicit_authoritative_routed_response(
    state: &InspectState,
    build: impl FnOnce() -> Result<api::RoutedJson, String>,
) -> Response {
    if !state.derived_history.is_active() {
        return routed_api_response(build());
    }
    let Some(_permit) = state.authoritative_fallback.try_acquire() else {
        return Response::json_error(
            "429 Too Many Requests",
            "an authoritative fallback is already in progress",
        );
    };
    routed_api_response(build()).with_header("X-Pointbreak-Access-Source", "authoritative-fallback")
}

fn derived_access_control_response(state: &InspectState, retry: bool) -> Response {
    if !state.derived_history.is_active() {
        return Response::json_error("409 Conflict", "derived access is disabled");
    }
    let result = if retry {
        state.derived_history.restart_background_rebuild()
    } else {
        state.derived_history.cancel_background_rebuild()
    };
    match result {
        Ok(()) => api_response(api::derived_access_status_json(
            &state.derived_history,
            state.authoritative_fallback.is_in_flight(),
        )),
        Err(message) => Response::json_error("500 Internal Server Error", &message),
    }
}

fn static_response(path: &str) -> Option<Response> {
    Some(match path {
        "/" | "/index.html" => Response::shell(INDEX_HTML),
        "/tokens.css" => Response::asset("text/css; charset=utf-8", TOKENS_CSS),
        "/app.css" => Response::asset("text/css; charset=utf-8", APP_CSS),
        "/app.js" => Response::asset("application/javascript; charset=utf-8", APP_JS),
        "/pointbreak-logo-mono.svg" => {
            Response::asset_bytes("image/svg+xml; charset=utf-8", POINTBREAK_LOGO_MONO_SVG)
        }
        "/favicon.svg" => Response::asset_bytes("image/svg+xml; charset=utf-8", FAVICON_SVG),
        "/favicon.png" => Response::asset_bytes("image/png", FAVICON_PNG),
        "/favicon-dark.png" => Response::asset_bytes("image/png", FAVICON_DARK_PNG),
        "/favicon.ico" => Response::new("204 No Content", "image/x-icon", Vec::new()),
        _ => return None,
    })
}

/// Path-member routes: `/api/revisions/{id}` and `/api/snapshots/{id}`. An empty
/// member (a trailing slash with no id) is a `400`; anything else unmatched is a
/// `404`. The id segment arrives percent-encoded (the client encodes it with
/// `encodeURIComponent`) and is decoded here.
fn route_member(state: &Arc<InspectState>, path: &str, query: Option<&str>) -> Response {
    let repo = state.repo.as_path();
    if let Some(raw) = path_member(path, "/api/revisions/") {
        return match decode_member(raw) {
            Some(id) => match requested_authoritative_access(query) {
                Ok(true) => {
                    explicit_authoritative_response(state, || api::revision_json(repo, &id))
                }
                Ok(false) => routed_api_response(api::routed_revision_json(
                    repo,
                    &state.derived_history,
                    &id,
                )),
                Err(message) => Response::json_error("400 Bad Request", &message),
            },
            None => Response::json_error("400 Bad Request", "missing revision id"),
        };
    }
    if let Some(raw) = path_member(path, "/api/snapshots/") {
        return match decode_member(raw) {
            Some(id) => {
                let content_hash = query_param(query, "contentHash");
                api_response(api::snapshot_json(
                    repo,
                    &id,
                    content_hash.as_deref(),
                    Some(&state.highlight_cache),
                ))
            }
            None => Response::json_error("400 Bad Request", "missing snapshot id"),
        };
    }
    Response::json_error("404 Not Found", "no such route")
}

/// The single path segment after `prefix` (e.g. `/api/revisions/`), still
/// percent-encoded. `None` when `path` is not under `prefix` or the remainder
/// spans more than one segment (a literal `/`).
fn path_member<'a>(path: &'a str, prefix: &str) -> Option<&'a str> {
    let rest = path.strip_prefix(prefix)?;
    if rest.contains('/') {
        return None;
    }
    Some(rest)
}

/// Percent-decode a captured path member into the id, or `None` when it is empty.
fn decode_member(raw: &str) -> Option<String> {
    if raw.is_empty() {
        return None;
    }
    Some(percent_decode(raw))
}

fn split_target(target: &str) -> (&str, Option<&str>) {
    match target.split_once('?') {
        Some((path, query)) => (path, Some(query)),
        None => (target, None),
    }
}

/// The parsed `/api/history` request: the query model and the window spec.
struct HistoryRequest {
    query: HistoryQuery,
    page: HistoryPage,
}

/// Parse the `/api/history` query params into a `HistoryQuery` + `HistoryPage`.
/// `q` is free text; `track`/`snapshot`/`at` are exact (empty => absent); `type` is
/// a comma-separated enabled-type set (absent => all types); `order` is
/// `asc`/`desc` (absent/empty => asc). A non-numeric `limit`/`offset` or an unknown
/// `order` is a usage error the caller turns into a `400` without touching the
/// store. The `at` › `offset` precedence lives in `apply_history_query`; the parser
/// only collects the params. Paging is positional (`offset`/`at`); the opaque
/// forward cursor stays on the CLI path (`pointbreak history --cursor`). The
/// legacy `object=` param aliases to `snapshot=` for old bookmarks (#334).
fn history_query(query: Option<&str>) -> Result<HistoryRequest, String> {
    let q = query_param(query, "q").unwrap_or_default();
    // A known-but-unsupported qualifier or out-of-set value in `q` is a usage
    // error (400), never a silently-empty page; a deprecation hint is not fatal
    // and rides back on `queryNotices`.
    let parsed = parse_search_query_for(&q, QuerySurface::Event);
    if let Some(fatal) = parsed.diagnostics.iter().find(|d| {
        matches!(
            d.code,
            QueryDiagnosticCode::UnsupportedQualifier | QueryDiagnosticCode::UnsupportedValue
        )
    }) {
        return Err(fatal.message.clone());
    }
    let track = query_param(query, "track").filter(|value| !value.is_empty());
    let snapshot = query_param(query, "snapshot")
        .or_else(|| query_param(query, "object"))
        .filter(|value| !value.is_empty());
    let revision = query_param(query, "revision")
        .filter(|value| !value.is_empty())
        .map(pointbreak::model::RevisionId::new);
    let types = query_param(query, "type").map(|raw| {
        raw.split(',')
            .filter(|value| !value.is_empty())
            .map(str::to_owned)
            .collect::<BTreeSet<String>>()
    });
    let order = match query_param(query, "order").as_deref() {
        None | Some("") | Some("asc") => HistoryOrder::Asc,
        Some("desc") => HistoryOrder::Desc,
        Some(_) => return Err("invalid order".to_owned()),
    };
    let limit = parse_usize(query_param(query, "limit"), "invalid limit")?;
    let offset = parse_usize(query_param(query, "offset"), "invalid offset")?;
    let at = query_param(query, "at")
        .filter(|value| !value.is_empty())
        .map(EventId::new);
    Ok(HistoryRequest {
        query: HistoryQuery {
            q,
            track,
            snapshot,
            revision,
            revisions: None,
            types,
            order,
        },
        page: HistoryPage {
            limit,
            after: None,
            offset,
            at,
        },
    })
}

fn revision_page_request(
    query: Option<&str>,
) -> Result<pointbreak::session::RevisionPageRequest, pointbreak::session::RevisionPageRequestError>
{
    let limit = match query_param(query, "limit") {
        Some(raw) => Some(
            raw.parse::<usize>()
                .map_err(|_| pointbreak::session::RevisionPageRequestError::InvalidRequest)?,
        ),
        None => None,
    };
    let after = query_param(query, "after");
    pointbreak::session::RevisionPageRequest::new(limit, after.as_deref())
}

/// Parse an optional numeric query param; a present but non-numeric value is a
/// usage error (`message`), an absent one is `None`.
fn parse_usize(value: Option<String>, message: &'static str) -> Result<Option<usize>, String> {
    match value {
        Some(raw) => Ok(Some(raw.parse::<usize>().map_err(|_| message.to_owned())?)),
        None => Ok(None),
    }
}

fn query_param(query: Option<&str>, key: &str) -> Option<String> {
    let query = query?;
    for pair in query.split('&') {
        let mut kv = pair.splitn(2, '=');
        if kv.next() == Some(key) {
            return Some(percent_decode(kv.next().unwrap_or("")));
        }
    }
    None
}

fn percent_decode(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'%' if i + 2 < bytes.len() => {
                let hi = (bytes[i + 1] as char).to_digit(16);
                let lo = (bytes[i + 2] as char).to_digit(16);
                match (hi, lo) {
                    (Some(hi), Some(lo)) => {
                        out.push((hi * 16 + lo) as u8);
                        i += 3;
                    }
                    _ => {
                        out.push(bytes[i]);
                        i += 1;
                    }
                }
            }
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            byte => {
                out.push(byte);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn api_response(result: Result<String, String>) -> Response {
    match result {
        Ok(body) => Response::json_ok(body),
        Err(message) => Response::json_error("500 Internal Server Error", &message),
    }
}

fn routed_api_response(result: Result<api::RoutedJson, String>) -> Response {
    match result {
        Ok(api::RoutedJson::Ok(body)) => Response::json_ok(body),
        Ok(api::RoutedJson::Unavailable(status)) => match serde_json::to_string(&status) {
            Ok(body) => Response::new(
                "503 Service Unavailable",
                "application/json; charset=utf-8",
                body.into_bytes(),
            ),
            Err(error) => Response::json_error("500 Internal Server Error", &error.to_string()),
        },
        Ok(api::RoutedJson::RestartRequired) => {
            Response::json_error("409 Conflict", "restart_required")
        }
        Err(message) => Response::json_error("500 Internal Server Error", &message),
    }
}

fn write_response(stream: TcpStream, response: &Response) -> std::io::Result<()> {
    record_response_body(response);
    write_response_inner(stream, response, None)
}

fn record_response_body(response: &Response) {
    #[cfg(not(feature = "longitudinal-counting"))]
    let _ = response;
    #[cfg(feature = "longitudinal-counting")]
    pointbreak::bench_support::longitudinal::record_response_bytes(response.body.len());
}

fn write_response_inner(
    mut stream: TcpStream,
    response: &Response,
    extra_header: Option<(&str, &str)>,
) -> std::io::Result<()> {
    let mut header = format!(
        "HTTP/1.1 {}\r\nContent-Type: {}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nX-Content-Type-Options: nosniff\r\nReferrer-Policy: no-referrer\r\n",
        response.status,
        response.content_type,
        response.body.len(),
    );
    if response.content_security_policy {
        header.push_str(
            "Content-Security-Policy: default-src 'none'; script-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self'; connect-src 'self'; base-uri 'none'; form-action 'none'; frame-ancestors 'none'\r\n",
        );
    }
    for (name, value) in &response.headers {
        header.push_str(name);
        header.push_str(": ");
        header.push_str(value);
        header.push_str("\r\n");
    }
    if let Some((name, value)) = extra_header {
        header.push_str(name);
        header.push_str(": ");
        header.push_str(value);
        header.push_str("\r\n");
    }
    header.push_str("Connection: close\r\n\r\n");
    stream.write_all(header.as_bytes())?;
    stream.write_all(&response.body)?;
    stream.flush()
}

fn open_browser(url: &str) {
    #[cfg(target_os = "macos")]
    let command = std::process::Command::new("open").arg(url).spawn();
    #[cfg(all(unix, not(target_os = "macos")))]
    let command = std::process::Command::new("xdg-open").arg(url).spawn();
    #[cfg(target_os = "windows")]
    let command = std::process::Command::new("cmd")
        .args(["/C", "start", "", url])
        .spawn();

    if let Err(error) = command {
        tracing::debug!(error = %error, "inspect_open_browser_failed");
    }
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use super::*;

    fn route_for(method: &str, path: &str) -> Response {
        // The active default resolves the repository before serving even a
        // store-independent route. Use one real empty repository so these
        // router tests exercise the production constructor honestly.
        let repo = tempfile::tempdir().expect("routing test repository");
        let initialized = std::process::Command::new("git")
            .args(["init", "--quiet"])
            .current_dir(repo.path())
            .status()
            .expect("initialize routing test repository");
        assert!(initialized.success());
        let state = Arc::new(
            InspectState::new_with_background_rebuild(repo.path().to_path_buf(), false).unwrap(),
        );
        route(&state, true, method, path, None)
    }

    fn capability(
        status: pointbreak::session::StoreCapabilityStatus,
    ) -> pointbreak::session::StoreCapabilityInspection {
        pointbreak::session::StoreCapabilityInspection {
            status,
            cursor: pointbreak::session::AuthorityCursorV2 {
                schema: "pointbreak.authority-cursor.v2".to_owned(),
                journal_record_count: 1,
                event_count: 1,
                journal_record_set_hash: format!("sha256:{}", "2".repeat(64)),
                event_set_hash: format!("sha256:{}", "3".repeat(64)),
                capability_set_hash: format!("sha256:{}", "4".repeat(64)),
            },
            minimum_reader_profile: None,
        }
    }

    fn parse(raw: impl AsRef<[u8]>) -> Result<Option<RequestHead>, RequestParseError> {
        parse_request_head(&mut Cursor::new(raw.as_ref()))
    }

    #[test]
    fn request_parser_collects_only_authentication_headers() {
        let request = parse(
            b"GET /api/version HTTP/1.1\r\nHost: 127.0.0.1:1234\r\nX-Ignored: value\r\nAuthorization: Bearer opaque\r\n\r\n",
        )
        .expect("valid request")
        .expect("request head");

        assert_eq!(request.method, "GET");
        assert_eq!(request.target, "/api/version");
        assert_eq!(request.hosts, ["127.0.0.1:1234"]);
        assert_eq!(request.authorizations, ["Bearer opaque"]);
    }

    #[cfg(feature = "longitudinal-counting")]
    #[test]
    fn counting_transport_is_authenticated_request_local_and_counts_only_body_bytes() {
        let encoded = URL_SAFE_NO_PAD.encode(
            serde_json::to_vec(&serde_json::json!({
                "runIdentity": "a".repeat(64),
                "context": {
                    "rootIdentity": "b".repeat(64),
                    "operation": "WARM_HEAD",
                    "phase": "warm",
                    "baseExecutionIdentitySha256": "c".repeat(64),
                    "derivativeExecutionIdentitySha256": "d".repeat(64),
                    "manifestSha256": "e".repeat(64),
                    "scheduleSha256": "f".repeat(64),
                    "success": false,
                    "semanticResultSha256": "1".repeat(64),
                    "includeCapacityOwnership": true
                }
            }))
            .expect("request JSON"),
        );
        let request = parse(format!(
            "GET /api/version HTTP/1.1\r\nHost: 127.0.0.1:1234\r\nAuthorization: Bearer opaque\r\nX-Pointbreak-Longitudinal-Counting: {encoded}\r\n\r\n"
        ))
        .expect("valid request")
        .expect("request head");
        let counting = longitudinal_counting_request(&request)
            .expect("valid counting transport")
            .expect("counting requested");
        let scope = pointbreak::bench_support::longitudinal::LongitudinalCountingScopeV1::new(
            counting.run_identity,
        )
        .expect("valid scope");
        let _guard = scope.enter();
        let response = Response::json_ok("{\"ok\":true}".to_owned());

        record_response_body(&response);

        assert_eq!(
            scope.snapshot().counters.response_bytes,
            response.body.len() as u64
        );
        let encoded = longitudinal_receipt_header(&scope, counting.context, &response)
            .expect("receipt transport");
        let receipt: pointbreak::bench_support::longitudinal::LongitudinalCounterReceiptV1 =
            serde_json::from_slice(&URL_SAFE_NO_PAD.decode(encoded).expect("receipt base64"))
                .expect("receipt JSON");
        assert_eq!(receipt.operation, "WARM_HEAD");
        assert!(receipt.success);
        assert_eq!(receipt.counters.response_bytes, response.body.len() as u64);
    }

    #[test]
    fn request_parser_rejects_excess_header_count() {
        let mut raw = String::from("GET / HTTP/1.1\r\n");
        for _ in 0..=MAX_HEADER_COUNT {
            raw.push_str("X-Test: value\r\n");
        }
        raw.push_str("\r\n");

        assert!(matches!(parse(raw), Err(RequestParseError::HeaderTooLarge)));
    }

    #[test]
    fn request_parser_rejects_excess_header_bytes() {
        let raw = format!(
            "GET / HTTP/1.1\r\nX-Test: {}\r\n\r\n",
            "a".repeat(MAX_HEADER_BYTES)
        );

        assert!(matches!(parse(raw), Err(RequestParseError::HeaderTooLarge)));
    }

    #[test]
    fn request_parser_rejects_excess_request_line_bytes() {
        let raw = format!(
            "GET /{} HTTP/1.1\r\nHost: localhost\r\n\r\n",
            "a".repeat(MAX_REQUEST_LINE_BYTES)
        );

        assert!(matches!(parse(raw), Err(RequestParseError::HeaderTooLarge)));
    }

    #[test]
    fn secret_token_debug_and_display_are_redacted() {
        let token = SecretToken::generate().expect("generate token");
        assert!(!format!("{token:?}").contains(token.expose()));
        assert!(!format!("{token}").contains(token.expose()));
    }

    #[test]
    fn snapshot_cache_returns_identical_bytes_on_hit() {
        let mut cache = HighlightCache::new(8);
        assert!(cache.get("sha256:abc").is_none()); // miss
        let body = "{\"snapshot\":1}".to_owned();
        cache.put("sha256:abc", body.clone());
        assert_eq!(cache.get("sha256:abc").as_deref(), Some(body.as_str())); // hit
    }

    #[test]
    fn highlight_cache_evicts_oldest_past_capacity() {
        let mut cache = HighlightCache::new(2);
        cache.put("a", "1".to_owned());
        cache.put("b", "2".to_owned());
        cache.put("c", "3".to_owned()); // evicts the oldest entry, "a"
        assert!(cache.get("a").is_none());
        assert_eq!(cache.get("b").as_deref(), Some("2"));
        assert_eq!(cache.get("c").as_deref(), Some("3"));
    }

    #[cfg(feature = "longitudinal-counting")]
    #[test]
    fn counting_calibrates_bounded_highlight_cache_ownership() {
        let scope = pointbreak::bench_support::longitudinal::LongitudinalCountingScopeV1::new(
            "d".repeat(64),
        )
        .expect("valid scope");
        let _guard = scope.enter();
        let mut cache = HighlightCache::new(2);

        cache.put("a", "one".to_owned());
        cache.put("b", "second".to_owned());
        assert_eq!(cache.get("a").as_deref(), Some("one"));

        let ownership = scope.snapshot().capacity_ownership;
        assert_eq!(ownership.retained_snapshot_highlight_entries, 2);
        assert_eq!(ownership.retained_snapshot_highlight_bytes, 9);
    }

    #[test]
    fn history_query_reads_snapshot_param_and_aliases_legacy_object() {
        let from_new = history_query(Some("snapshot=obj-1")).unwrap();
        assert_eq!(from_new.query.snapshot.as_deref(), Some("obj-1"));
        // A stale bookmark's legacy `object=` param still resolves to snapshot (#334).
        let from_legacy = history_query(Some("object=obj-1")).unwrap();
        assert_eq!(from_legacy.query.snapshot.as_deref(), Some("obj-1"));
        // Absent => no snapshot constraint.
        let absent = history_query(Some("q=hello")).unwrap();
        assert_eq!(absent.query.snapshot, None);
    }

    #[test]
    fn history_query_reads_exact_revision_and_treats_empty_as_absent() {
        let selected = history_query(Some("revision=rev%3Asha256%3Aabc")).unwrap();
        assert_eq!(
            selected.query.revision.as_ref().map(|id| id.as_str()),
            Some("rev:sha256:abc")
        );
        assert!(
            history_query(Some("revision="))
                .unwrap()
                .query
                .revision
                .is_none()
        );
    }

    #[test]
    fn revision_page_query_enforces_the_frozen_default_and_maximum() {
        assert_eq!(revision_page_request(None).unwrap().limit(), 100);
        assert_eq!(
            revision_page_request(Some("limit=500")).unwrap().limit(),
            500
        );
        assert!(revision_page_request(Some("limit=0")).is_err());
        assert!(revision_page_request(Some("limit=501")).is_err());
        assert!(revision_page_request(Some("limit=lots")).is_err());
        assert!(revision_page_request(Some("after=not-base64!!")).is_err());
    }

    #[test]
    fn root_serves_index_html() {
        let response = route_for("GET", "/");
        assert_eq!(response.status, "200 OK");
        assert_eq!(response.content_type, "text/html; charset=utf-8");
        assert!(!response.body.is_empty());
    }

    #[test]
    fn app_css_styles_verification_and_endorsement_readback() {
        let response = route_for("GET", "/app.css");
        let body = String::from_utf8(response.body).expect("app.css is utf-8");
        assert!(
            body.contains(".verify") && body.contains(".endorsements"),
            "app.css carries the verification chip and endorsement block styles"
        );
    }

    #[test]
    fn static_assets_carry_expected_content_types() {
        assert_eq!(
            route_for("GET", "/tokens.css").content_type,
            "text/css; charset=utf-8"
        );
        assert_eq!(
            route_for("GET", "/app.css").content_type,
            "text/css; charset=utf-8"
        );
        assert_eq!(
            route_for("GET", "/app.js").content_type,
            "application/javascript; charset=utf-8"
        );
    }

    #[test]
    fn identity_route_is_registered() {
        // The path is routed to the identity builder (not a 404). Against the unused
        // test path the store resolve fails, so it is a JSON 500 — but crucially NOT
        // "404 Not Found", which is what an unrouted path returns.
        let response = route_for("GET", "/api/identity");
        assert_ne!(response.status, "404 Not Found");
        assert!(response.content_type.starts_with("application/json"));
    }

    #[test]
    fn unknown_route_is_json_not_found() {
        let response = route_for("GET", "/does-not-exist");
        assert_eq!(response.status, "404 Not Found");
        assert!(response.content_type.starts_with("application/json"));
    }

    #[test]
    fn v2_profile_is_the_only_semantic_bootstrap_on_an_l0_root() {
        let response = route_for("GET", "/api/v2/profile");
        assert_eq!(response.status, "200 OK");
        let value: serde_json::Value = serde_json::from_slice(&response.body).unwrap();
        assert_eq!(value["schema"], "pointbreak.inspect-reader-profile");
        assert_eq!(value["availability"], "migration_required");
        assert!(value["commitGraphStamp"].is_string());
    }

    #[test]
    fn doubled_change_route_prefix_is_not_reinterpreted_as_an_identity() {
        let response = route_for("GET", "/api/v2/changes//api/v2/changes/example");
        assert_eq!(response.status, "400 Bad Request");
    }

    #[test]
    fn change_reader_cache_reuses_and_invalidates_one_complete_generation() {
        let repo = tempfile::tempdir().expect("cache test repository");
        for args in [
            vec!["init", "--quiet"],
            vec!["config", "user.name", "Pointbreak Test"],
            vec!["config", "user.email", "pointbreak@example.test"],
            vec!["config", "commit.gpgsign", "false"],
        ] {
            assert!(
                std::process::Command::new("git")
                    .args(args)
                    .current_dir(repo.path())
                    .status()
                    .expect("run git")
                    .success()
            );
        }
        std::fs::write(repo.path().join("sample.txt"), "before\n").unwrap();
        assert!(
            std::process::Command::new("git")
                .args(["add", "sample.txt"])
                .current_dir(repo.path())
                .status()
                .unwrap()
                .success()
        );
        assert!(
            std::process::Command::new("git")
                .args(["commit", "--quiet", "-m", "base"])
                .current_dir(repo.path())
                .status()
                .unwrap()
                .success()
        );

        let cache = ChangeReaderCache::new();
        let first = cache.load(repo.path()).unwrap();
        let hit = cache.load(repo.path()).unwrap();
        assert!(Arc::ptr_eq(&first, &hit));

        std::fs::write(repo.path().join("sample.txt"), "after\n").unwrap();
        pointbreak::session::capture_worktree_review(pointbreak::session::CaptureOptions::new(
            repo.path(),
        ))
        .unwrap();
        let refreshed = cache.load(repo.path()).unwrap();
        assert!(!Arc::ptr_eq(&first, &refreshed));
        assert!(
            refreshed.capability.cursor.journal_record_count
                > first.capability.cursor.journal_record_count
        );
    }

    #[test]
    fn legacy_semantic_routes_remain_available_on_untouched_l0() {
        let capability = capability(pointbreak::session::StoreCapabilityStatus::MigrationRequired);
        assert!(legacy_semantic_gate(&capability).is_none());
    }

    #[test]
    fn legacy_semantic_routes_refuse_m1_before_partial_payload() {
        let mut capability = capability(
            pointbreak::session::StoreCapabilityStatus::MigrationInProgress {
                activation_id: "activation:sha256:test".to_owned(),
                manifest_hash: format!("sha256:{}", "1".repeat(64)),
            },
        );
        capability.minimum_reader_profile = Some("review_change_revision_v1".to_owned());
        let response = legacy_semantic_gate(&capability).unwrap();
        assert_eq!(response.status, "409 Conflict");
        let value: serde_json::Value = serde_json::from_slice(&response.body).unwrap();
        assert_eq!(value["schema"], "pointbreak.store-migration-in-progress");
        assert_eq!(value["state"], "migration_in_progress");
    }

    #[test]
    fn legacy_semantic_routes_return_typed_426_for_l2() {
        let mut capability = capability(pointbreak::session::StoreCapabilityStatus::Ready {
            activation_id: "activation:sha256:test".to_owned(),
            manifest_hash: format!("sha256:{}", "1".repeat(64)),
            completion_id: "completion:sha256:test".to_owned(),
        });
        capability.minimum_reader_profile = Some("review_change_revision_v1".to_owned());

        let response = legacy_semantic_gate(&capability).unwrap();
        assert_eq!(response.status, "426 Upgrade Required");
        let value: serde_json::Value = serde_json::from_slice(&response.body).unwrap();
        assert_eq!(value["schema"], "pointbreak.reader-upgrade-required");
        assert_eq!(value["code"], "reader_upgrade_required");
    }

    #[test]
    fn non_get_methods_are_rejected() {
        assert_eq!(
            route_for("POST", "/api/history").status,
            "405 Method Not Allowed"
        );
    }

    #[test]
    fn authoritative_fallback_gate_is_single_flight_and_reusable() {
        let gate = AuthoritativeFallbackGate::new();
        for _ in 0..100 {
            let permit = gate.try_acquire().expect("first fallback acquires permit");
            assert!(gate.is_in_flight());
            assert!(
                gate.try_acquire().is_none(),
                "concurrent fallback is rejected"
            );
            drop(permit);
            assert!(!gate.is_in_flight());
        }
    }

    #[test]
    fn lifecycle_post_surface_is_exact() {
        assert_eq!(
            route_for("POST", "/api/history").status,
            "405 Method Not Allowed"
        );
        assert_eq!(
            route_for("POST", "/api/derived-access/retry").status,
            "200 OK"
        );
        assert_eq!(
            route_for("POST", "/api/derived-access/cancel").status,
            "200 OK"
        );
    }

    #[test]
    fn path_member_extracts_single_segment() {
        assert_eq!(
            path_member("/api/revisions/abc", "/api/revisions/"),
            Some("abc")
        );
        assert_eq!(
            path_member("/api/snapshots/x%3Ay", "/api/snapshots/"),
            Some("x%3Ay")
        );
        // A deeper path is not a single member.
        assert_eq!(path_member("/api/revisions/a/b", "/api/revisions/"), None);
        // No trailing slash: the collection, not a member.
        assert_eq!(path_member("/api/revisions", "/api/revisions/"), None);
        // Trailing slash, empty member.
        assert_eq!(path_member("/api/revisions/", "/api/revisions/"), Some(""));
    }

    #[test]
    fn decode_member_percent_decodes_nonempty() {
        assert_eq!(
            decode_member("snap%3Agit%3Asha256%3Aabc").as_deref(),
            Some("snap:git:sha256:abc")
        );
        assert_eq!(decode_member(""), None);
    }

    #[test]
    fn revisions_member_without_id_is_bad_request() {
        assert_eq!(
            route_for("GET", "/api/revisions/").status,
            "400 Bad Request"
        );
    }

    #[test]
    fn snapshots_member_without_id_is_bad_request() {
        assert_eq!(
            route_for("GET", "/api/snapshots/").status,
            "400 Bad Request"
        );
    }

    #[test]
    fn bare_snapshots_collection_is_not_found() {
        // There is no snapshot-list endpoint; only `/api/snapshots/{id}` exists.
        assert_eq!(route_for("GET", "/api/snapshots").status, "404 Not Found");
    }

    #[test]
    fn deeper_member_paths_are_not_found() {
        assert_eq!(
            route_for("GET", "/api/revisions/a/b").status,
            "404 Not Found"
        );
        assert_eq!(
            route_for("GET", "/api/threads/anything").status,
            "404 Not Found"
        );
    }

    #[test]
    fn retired_routes_are_not_found() {
        // The pre-reshape object/singular routes and the older lineage routes.
        for path in [
            "/api/objects",
            "/api/object",
            "/api/revision",
            "/api/lineages",
            "/api/lineage",
        ] {
            assert_eq!(
                route_for("GET", path).status,
                "404 Not Found",
                "{path} is retired"
            );
        }
    }

    #[test]
    fn query_param_reads_and_percent_decodes_values() {
        let query = Some("contentHash=snap%3Agit%3Asha256%3Aabc&other=1");
        assert_eq!(
            query_param(query, "contentHash").as_deref(),
            Some("snap:git:sha256:abc")
        );
        assert_eq!(query_param(query, "missing"), None);
        assert_eq!(query_param(None, "contentHash"), None);
    }

    #[test]
    fn split_target_separates_path_and_query() {
        assert_eq!(
            split_target("/api/snapshots/x?contentHash=y"),
            ("/api/snapshots/x", Some("contentHash=y"))
        );
        assert_eq!(split_target("/"), ("/", None));
    }
}
