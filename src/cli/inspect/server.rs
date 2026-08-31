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
    ChangeQueryUnavailableDocumentV1, EventHistoryDocumentV1, InspectStartupDocument,
    ReaderUpgradeRequiredDocumentV1, version_document,
};
use pointbreak::model::EventId;
use pointbreak::session::{
    ChangeReaderPresentationV1, HistoryOrder, HistoryPage, HistoryQuery, QueryDiagnosticCode,
    QuerySurface, SnapshotSummaryCache, TrustSet, parse_search_query_for,
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
    #[serde(default)]
    timeline_post_pin_barrier: Option<
        pointbreak::bench_support::longitudinal::LongitudinalTimelinePostPinBarrierRequestV1,
    >,
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
    pub derived_changes: pointbreak::session::DerivedChangeAccess,
    pub strict_change_stamp: pointbreak::session::StrictChangeStampBinder,
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
    /// One complete strict Change reader snapshot keyed by the append-only
    /// Journal marker. Only Timeline and exact/detail/resource/interdiff routes
    /// may enter it; Profile, Changes, and Attention use `derived_changes`.
    pub change_reader_cache: ChangeReaderCache,
    /// Ephemeral continuation-token authority, deliberately independent of the
    /// browser bearer secret and never persisted beyond this Inspector process.
    pub page_token_signer: super::page_token::PageTokenSigner,
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
        let derived_changes =
            pointbreak::session::DerivedChangeAccess::resolve_for_inspector(&repo)
                .map_err(|error| error.to_string())?;
        let strict_change_stamp = derived_changes.strict_stamp_binder();
        let derived_history = derived_changes.recovery_access();
        if start_background_rebuild && let Err(error) = derived_history.start_background_rebuild() {
            tracing::warn!(error = %error, "derived_access_background_rebuild_start_failed");
        }
        Ok(Self {
            repo,
            derived_changes,
            strict_change_stamp,
            derived_history,
            highlight_cache: RwLock::new(HighlightCache::new(HIGHLIGHT_CACHE_CAPACITY)),
            history_cache: super::cache::HistoryProjectionCache::new(),
            snapshot_summaries: Arc::new(SnapshotSummaryCache::new()),
            change_reader_cache: ChangeReaderCache::new(),
            page_token_signer: super::page_token::PageTokenSigner::generate()
                .map_err(|error| error.to_string())?,
            authoritative_fallback: AuthoritativeFallbackGate::new(),
        })
    }
}

struct CachedChangeReaderState<Presentation> {
    marker: u64,
    state: Arc<pointbreak::session::ChangeReaderStateV1>,
    presentation_loaded: bool,
    presentation: Option<Arc<Presentation>>,
}

struct CachedTimelineState<Timeline> {
    marker: u64,
    trust_set: TrustSet,
    timeline: Option<Arc<Timeline>>,
}

struct ChangeReaderBaseGeneration<Presentation> {
    marker: u64,
    state: Arc<pointbreak::session::ChangeReaderStateV1>,
    presentation: Option<Arc<Presentation>>,
}

pub(super) struct ChangeReaderGeneration<
    Presentation = ChangeReaderPresentationV1,
    Timeline = EventHistoryDocumentV1,
> {
    pub(super) state: Arc<pointbreak::session::ChangeReaderStateV1>,
    pub(super) presentation: Option<Arc<Presentation>>,
    pub(super) timeline: Option<Arc<Timeline>>,
}

#[derive(Clone, Copy)]
enum ChangeReaderLoad<'a> {
    State,
    Changes,
    Timeline(&'a TrustSet),
}

#[derive(Debug)]
pub(super) enum ChangeReaderLoadError {
    MovingJournal,
    Other(String),
}

impl fmt::Display for ChangeReaderLoadError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MovingJournal => formatter
                .write_str("Journal changed while the Change reader generation was loading; retry"),
            Self::Other(message) => formatter.write_str(message),
        }
    }
}

/// Single-generation cache for the warm Change reader.
///
/// The cheap marker is only an invalidation detector. A miss always performs a
/// complete capability-validated fold, and the marker is re-read afterward so
/// a moving Journal can never publish a mixed generation. Timeline projection
/// has an independent single-flight slot so it cannot hold up already-warm
/// Change readers. Cold CLI commands do not use this process-local cache.
pub(super) struct ChangeReaderCache<
    Presentation = ChangeReaderPresentationV1,
    Timeline = EventHistoryDocumentV1,
> {
    slot: Mutex<Option<CachedChangeReaderState<Presentation>>>,
    timeline_slot: Mutex<Option<CachedTimelineState<Timeline>>>,
}

impl<Presentation, Timeline> ChangeReaderCache<Presentation, Timeline> {
    fn new() -> Self {
        Self {
            slot: Mutex::new(None),
            timeline_slot: Mutex::new(None),
        }
    }
}

impl ChangeReaderCache<ChangeReaderPresentationV1, EventHistoryDocumentV1> {
    pub(super) fn load_state(
        &self,
        repo: &std::path::Path,
    ) -> Result<ChangeReaderGeneration, ChangeReaderLoadError> {
        self.load(repo, ChangeReaderLoad::State)
    }

    pub(super) fn load_changes(
        &self,
        repo: &std::path::Path,
    ) -> Result<ChangeReaderGeneration, ChangeReaderLoadError> {
        self.load(repo, ChangeReaderLoad::Changes)
    }

    pub(super) fn load_timeline(
        &self,
        repo: &std::path::Path,
        trust_set: &TrustSet,
    ) -> Result<ChangeReaderGeneration, ChangeReaderLoadError> {
        self.load(repo, ChangeReaderLoad::Timeline(trust_set))
    }

    fn load(
        &self,
        repo: &std::path::Path,
        scope: ChangeReaderLoad<'_>,
    ) -> Result<ChangeReaderGeneration, ChangeReaderLoadError> {
        self.load_with(
            scope,
            || {
                pointbreak::session::change_reader_head_marker_for_repo(repo)
                    .map_err(|error| ChangeReaderLoadError::Other(error.to_string()))
            },
            || {
                pointbreak::session::change_reader_state_for_repo(repo)
                    .map(Arc::new)
                    .map_err(|error| ChangeReaderLoadError::Other(error.to_string()))
            },
            |state| {
                state
                    .ready()
                    .map(|ready| ready.presentation().map(Arc::new))
                    .transpose()
                    .map_err(|error| ChangeReaderLoadError::Other(error.to_string()))
            },
            |state, presentation, trust_set| {
                state
                    .ready()
                    .map(|ready| {
                        ready
                            .event_history_document(trust_set, presentation)
                            .map(Arc::new)
                    })
                    .transpose()
                    .map_err(|error| ChangeReaderLoadError::Other(error.to_string()))
            },
        )
    }
}

impl<Presentation, Timeline> ChangeReaderCache<Presentation, Timeline> {
    fn load_with(
        &self,
        scope: ChangeReaderLoad<'_>,
        mut marker: impl FnMut() -> Result<u64, ChangeReaderLoadError>,
        mut build: impl FnMut() -> Result<
            Arc<pointbreak::session::ChangeReaderStateV1>,
            ChangeReaderLoadError,
        >,
        mut build_presentation: impl FnMut(
            &pointbreak::session::ChangeReaderStateV1,
        )
            -> Result<Option<Arc<Presentation>>, ChangeReaderLoadError>,
        mut build_timeline: impl FnMut(
            &pointbreak::session::ChangeReaderStateV1,
            &Presentation,
            &TrustSet,
        ) -> Result<Option<Arc<Timeline>>, ChangeReaderLoadError>,
    ) -> Result<ChangeReaderGeneration<Presentation, Timeline>, ChangeReaderLoadError> {
        match scope {
            ChangeReaderLoad::State | ChangeReaderLoad::Changes => {
                let generation =
                    self.load_base_with(scope, &mut marker, &mut build, &mut build_presentation)?;
                Ok(ChangeReaderGeneration {
                    state: generation.state,
                    presentation: generation.presentation,
                    timeline: None,
                })
            }
            ChangeReaderLoad::Timeline(trust_set) => self.load_timeline_with(
                trust_set,
                &mut marker,
                &mut build,
                &mut build_presentation,
                &mut build_timeline,
            ),
        }
    }

    fn load_base_with(
        &self,
        scope: ChangeReaderLoad<'_>,
        marker: &mut impl FnMut() -> Result<u64, ChangeReaderLoadError>,
        build: &mut impl FnMut() -> Result<
            Arc<pointbreak::session::ChangeReaderStateV1>,
            ChangeReaderLoadError,
        >,
        build_presentation: &mut impl FnMut(
            &pointbreak::session::ChangeReaderStateV1,
        )
            -> Result<Option<Arc<Presentation>>, ChangeReaderLoadError>,
    ) -> Result<ChangeReaderBaseGeneration<Presentation>, ChangeReaderLoadError> {
        debug_assert!(!matches!(scope, ChangeReaderLoad::Timeline(_)));
        let mut slot = self.slot.lock().map_err(|_| {
            ChangeReaderLoadError::Other("Change reader cache lock is poisoned".to_owned())
        })?;
        for _ in 0..2 {
            let before = marker()?;
            if let Some(cached) = slot.as_mut()
                && cached.marker == before
            {
                match scope {
                    ChangeReaderLoad::State => {
                        return Ok(ChangeReaderBaseGeneration {
                            marker: cached.marker,
                            state: Arc::clone(&cached.state),
                            presentation: None,
                        });
                    }
                    ChangeReaderLoad::Changes if cached.presentation_loaded => {
                        return Ok(ChangeReaderBaseGeneration {
                            marker: cached.marker,
                            state: Arc::clone(&cached.state),
                            presentation: cached.presentation.clone(),
                        });
                    }
                    ChangeReaderLoad::Changes => {}
                    ChangeReaderLoad::Timeline(_) => {
                        unreachable!("Timeline loads use their own slot")
                    }
                }

                let state = Arc::clone(&cached.state);
                let presentation = build_presentation(&state)?;
                let after = marker()?;
                if before == after {
                    cached.presentation_loaded = true;
                    cached.presentation = presentation.clone();
                    return Ok(ChangeReaderBaseGeneration {
                        marker: after,
                        state,
                        presentation,
                    });
                }
                continue;
            }
            let state = build()?;
            let presentation_loaded = !matches!(scope, ChangeReaderLoad::State);
            let presentation = presentation_loaded
                .then(|| build_presentation(&state))
                .transpose()?
                .flatten();
            let after = marker()?;
            if before == after {
                *slot = Some(CachedChangeReaderState {
                    marker: after,
                    state: Arc::clone(&state),
                    presentation_loaded,
                    presentation: presentation.clone(),
                });
                return Ok(ChangeReaderBaseGeneration {
                    marker: after,
                    state,
                    presentation,
                });
            }
        }
        Err(ChangeReaderLoadError::MovingJournal)
    }

    fn load_timeline_with(
        &self,
        trust_set: &TrustSet,
        marker: &mut impl FnMut() -> Result<u64, ChangeReaderLoadError>,
        build: &mut impl FnMut() -> Result<
            Arc<pointbreak::session::ChangeReaderStateV1>,
            ChangeReaderLoadError,
        >,
        build_presentation: &mut impl FnMut(
            &pointbreak::session::ChangeReaderStateV1,
        )
            -> Result<Option<Arc<Presentation>>, ChangeReaderLoadError>,
        build_timeline: &mut impl FnMut(
            &pointbreak::session::ChangeReaderStateV1,
            &Presentation,
            &TrustSet,
        ) -> Result<Option<Arc<Timeline>>, ChangeReaderLoadError>,
    ) -> Result<ChangeReaderGeneration<Presentation, Timeline>, ChangeReaderLoadError> {
        let generation =
            self.load_base_with(ChangeReaderLoad::Changes, marker, build, build_presentation)?;

        // Timeline construction can be expensive, but it is a pure projection
        // over this already-stable immutable Change generation. Serialize only
        // competing Timeline builders; warm State and Changes readers must not
        // wait for it. An append during this projection does not invalidate the
        // coherent snapshot: this result remains generation N, while the next
        // request observes the new marker and loads generation N+1.
        let mut timeline_slot = self.timeline_slot.lock().map_err(|_| {
            ChangeReaderLoadError::Other("Change reader Timeline cache lock is poisoned".to_owned())
        })?;
        if let Some(cached) = timeline_slot.as_ref()
            && cached.marker == generation.marker
            && cached.trust_set == *trust_set
        {
            return Ok(ChangeReaderGeneration {
                state: generation.state,
                presentation: generation.presentation,
                timeline: cached.timeline.clone(),
            });
        }

        let timeline = generation
            .presentation
            .as_deref()
            .map(|presentation| build_timeline(&generation.state, presentation, trust_set))
            .transpose()?
            .flatten();
        let generation_is_current = self
            .slot
            .lock()
            .map_err(|_| {
                ChangeReaderLoadError::Other("Change reader cache lock is poisoned".to_owned())
            })?
            .as_ref()
            .is_some_and(|cached| cached.marker == generation.marker);
        if generation_is_current {
            *timeline_slot = Some(CachedTimelineState {
                marker: generation.marker,
                trust_set: trust_set.clone(),
                timeline: timeline.clone(),
            });
        }
        Ok(ChangeReaderGeneration {
            state: generation.state,
            presentation: generation.presentation,
            timeline,
        })
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
    let capability_url = format!("{url}#/changes?token={}", policy.token.expose());
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
    #[cfg(feature = "longitudinal-counting")]
    if let Some(counting_request) = longitudinal_counting_request(&request)? {
        let LongitudinalCountingRequest {
            run_identity,
            context,
            timeline_post_pin_barrier,
        } = counting_request;
        let mut scope =
            pointbreak::bench_support::longitudinal::LongitudinalCountingScopeV1::new(run_identity)
                .map_err(std::io::Error::other)?;
        if let Some(barrier) = timeline_post_pin_barrier {
            // A failed-closed barrier must still answer with a typed response:
            // a dropped connection leaves the controller unable to attribute
            // the refusal, while nothing here has touched request state yet.
            if request.method != "GET" || path != "/api/v2/history" {
                return write_response(
                    stream,
                    &Response::text(
                        "400 Bad Request",
                        "Timeline post-pin barrier is valid only for one derived Timeline request",
                    ),
                );
            }
            if context.manifest_sha256 != barrier.barrier_identity_sha256 {
                return write_response(
                    stream,
                    &Response::text(
                        "400 Bad Request",
                        "Timeline post-pin barrier identity must bind the counter manifest",
                    ),
                );
            }
            let Some(root) = std::env::var_os(
                pointbreak::bench_support::longitudinal::LONGITUDINAL_TIMELINE_POST_PIN_BARRIER_ROOT_ENV_V1,
            ) else {
                return write_response(
                    stream,
                    &Response::text(
                        "400 Bad Request",
                        "Timeline post-pin barrier request omitted its explicit child environment root",
                    ),
                );
            };
            scope = match scope.with_timeline_post_pin_barrier(PathBuf::from(root), barrier) {
                Ok(scope) => scope,
                Err(error) => {
                    return write_response(
                        stream,
                        &Response::text(
                            "400 Bad Request",
                            &format!("Timeline post-pin barrier arming failed: {error}"),
                        ),
                    );
                }
            };
        }
        let _guard = scope.enter();
        let response = route(state, policy.serve_static, &request.method, path, query);
        record_response_body(&response);
        let receipt = longitudinal_receipt_header(&scope, context, &response)?;
        // An armed barrier the route never consumed must answer with the
        // route's own outcome attached: dropping the connection here hides
        // which early response prevented the pin from being reached.
        let barrier_receipt = match longitudinal_barrier_receipt_header(&scope) {
            Ok(barrier_receipt) => barrier_receipt,
            Err(error) => {
                let body_prefix: String = String::from_utf8_lossy(&response.body)
                    .chars()
                    .take(400)
                    .collect();
                return write_response(
                    stream,
                    &Response::text(
                        "400 Bad Request",
                        &format!(
                            "Timeline post-pin barrier failed: {error}; \
                             route response was {} with body {body_prefix}",
                            response.status
                        ),
                    ),
                );
            }
        };
        let mut headers = vec![(
            pointbreak::bench_support::longitudinal::LONGITUDINAL_COUNTER_RECEIPT_HEADER_V1,
            receipt.as_str(),
        )];
        if let Some(barrier_receipt) = &barrier_receipt {
            headers.push((
                pointbreak::bench_support::longitudinal::LONGITUDINAL_TIMELINE_POST_PIN_BARRIER_RECEIPT_HEADER_V1,
                barrier_receipt.as_str(),
            ));
        }
        return write_response_inner(stream, &response, &headers);
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
    if scope.has_timeline_post_pin_barrier() {
        context.semantic_result_sha256 =
            pointbreak::bench_support::longitudinal::canonical_longitudinal_response_semantic_sha256_v1(
                &response.body,
            )
            .map_err(std::io::Error::other)?;
    }
    let receipt = scope
        .receipt(context)
        .map_err(|error| std::io::Error::other(error.to_string()))?;
    let receipt =
        serde_json::to_vec(&receipt).map_err(|error| std::io::Error::other(error.to_string()))?;
    Ok(URL_SAFE_NO_PAD.encode(receipt))
}

#[cfg(feature = "longitudinal-counting")]
fn longitudinal_barrier_receipt_header(
    scope: &pointbreak::bench_support::longitudinal::LongitudinalCountingScopeV1,
) -> std::io::Result<Option<String>> {
    scope
        .timeline_post_pin_barrier_receipt()
        .map_err(std::io::Error::other)?
        .map(|receipt| {
            serde_json::to_vec(&receipt)
                .map(|bytes| URL_SAFE_NO_PAD.encode(bytes))
                .map_err(|error| std::io::Error::other(error.to_string()))
        })
        .transpose()
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
        if !state.derived_changes.is_active() {
            return authoritative_change_v2_profile_response(state);
        }
        return change_v2_response(api::change_v2_profile_json(repo, &state.derived_changes));
    }
    if path == "/api/v2/history" {
        if !state.derived_changes.is_active() {
            return change_v2_response(api::authoritative_event_history_v2_json(
                repo,
                &state.change_reader_cache,
                &state.strict_change_stamp,
                query,
                &state.page_token_signer,
            ));
        }
        return change_v2_response(api::event_history_v2_json(
            repo,
            &state.derived_changes,
            query,
            &state.page_token_signer,
        ));
    }
    if path == "/api/v2/changes" {
        if !state.derived_changes.is_active() {
            return authoritative_changes_v2_response(state, query);
        }
        return change_v2_response(api::changes_v2_json(
            &state.derived_changes,
            query,
            &state.page_token_signer,
        ));
    }
    if path == "/api/v2/attention" {
        if !state.derived_changes.is_active() {
            return authoritative_change_attention_v2_response(state, query);
        }
        return change_v2_response(api::change_attention_v2_json(
            &state.derived_changes,
            query,
            &state.page_token_signer,
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
        pointbreak::session::StoreCapabilityStatus::MigrationRequired
        | pointbreak::session::StoreCapabilityStatus::MigrationInProgress { .. } => {
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
    let stamp_binder = &state.strict_change_stamp;
    let segments = member_path
        .split('/')
        .map(|raw| {
            (!raw.is_empty())
                .then(|| strict_percent_decode(raw).ok())
                .flatten()
        })
        .collect::<Option<Vec<_>>>();
    let Some(segments) = segments else {
        return exact_selection_error_response("invalid Change route identity");
    };
    match segments.as_slice() {
        [change_id] => {
            if state.derived_changes.is_active() {
                change_v2_response(api::derived_change_detail_v2_json(
                    &state.derived_changes,
                    change_id,
                ))
            } else {
                change_v2_response(api::change_detail_v2_json(
                    repo,
                    cache,
                    stamp_binder,
                    change_id,
                ))
            }
        }
        [change_id, revisions, revision_id] if revisions == "revisions" => {
            let artifact_hash = match exact_selector_values(query, &["artifactHash"]) {
                Ok(mut values) => values.remove(0),
                Err(message) => return exact_selection_error_response(&message),
            };
            if state.derived_changes.is_active() {
                change_v2_response(api::derived_change_revision_v2_json(
                    &state.derived_changes,
                    change_id,
                    revision_id,
                    &artifact_hash,
                    false,
                ))
            } else {
                change_v2_response(api::change_revision_v2_json(
                    repo,
                    cache,
                    stamp_binder,
                    change_id,
                    revision_id,
                    &artifact_hash,
                    false,
                ))
            }
        }
        [change_id, revisions, revision_id, resource]
            if revisions == "revisions" && resource == "resource" =>
        {
            let artifact_hash = match exact_selector_values(query, &["artifactHash"]) {
                Ok(mut values) => values.remove(0),
                Err(message) => return exact_selection_error_response(&message),
            };
            if state.derived_changes.is_active() {
                change_v2_response(api::derived_change_revision_v2_json(
                    &state.derived_changes,
                    change_id,
                    revision_id,
                    &artifact_hash,
                    true,
                ))
            } else {
                change_v2_response(api::change_revision_v2_json(
                    repo,
                    cache,
                    stamp_binder,
                    change_id,
                    revision_id,
                    &artifact_hash,
                    true,
                ))
            }
        }
        [change_id, interdiff, from_revision_id, to_revision_id] if interdiff == "interdiff" => {
            let values = match exact_selector_values(query, &["fromArtifactHash", "toArtifactHash"])
            {
                Ok(values) => values,
                Err(message) => return exact_selection_error_response(&message),
            };
            if state.derived_changes.is_active() {
                change_v2_response(api::derived_change_interdiff_v2_json(
                    &state.derived_changes,
                    change_id,
                    from_revision_id,
                    &values[0],
                    to_revision_id,
                    &values[1],
                ))
            } else {
                change_v2_response(api::change_interdiff_v2_json(
                    repo,
                    cache,
                    stamp_binder,
                    change_id,
                    from_revision_id,
                    &values[0],
                    to_revision_id,
                    &values[1],
                ))
            }
        }
        _ => Response::json_error("404 Not Found", "no such route"),
    }
}

/// Parse the complete query grammar for an exact Change-reader surface.
///
/// These selectors authorize immutable captured bytes or ordered comparison
/// endpoints, so accepting a first duplicate, ignoring an unknown member, or
/// repairing malformed encoding would make the URL's identity ambiguous. Keep
/// this stricter than the legacy convenience parser used by unrelated routes.
fn exact_selector_values(query: Option<&str>, expected: &[&str]) -> Result<Vec<String>, String> {
    let query = query.ok_or_else(|| format!("missing {}", expected[0]))?;
    if query.is_empty() {
        return Err(format!("missing {}", expected[0]));
    }

    let mut values = vec![None; expected.len()];
    for pair in query.split('&') {
        let (key, raw_value) = pair
            .split_once('=')
            .ok_or_else(|| "exact selector query members require values".to_owned())?;
        let Some(index) = expected
            .iter()
            .position(|expected_key| *expected_key == key)
        else {
            return Err(format!("unknown exact selector query member: {key}"));
        };
        if values[index].is_some() {
            return Err(format!("duplicate exact selector query member: {key}"));
        }
        let value = strict_percent_decode(raw_value)?;
        if value.is_empty() {
            return Err(format!("empty exact selector query member: {key}"));
        }
        values[index] = Some(value);
    }

    expected
        .iter()
        .enumerate()
        .map(|(index, key)| values[index].take().ok_or_else(|| format!("missing {key}")))
        .collect()
}

fn exact_selection_error_response(message: &str) -> Response {
    Response::new(
        "400 Bad Request",
        "application/json; charset=utf-8",
        api::exact_selection_error_json(message).into_bytes(),
    )
}

fn change_v2_response(result: Result<api::ChangeV2Json, String>) -> Response {
    match result {
        Ok(api::ChangeV2Json::Ok(body)) => Response::json_ok(body),
        Ok(api::ChangeV2Json::Unavailable(body)) => Response::new(
            "409 Conflict",
            "application/json; charset=utf-8",
            body.into_bytes(),
        ),
        Ok(api::ChangeV2Json::UpgradeRequired(body)) => Response::new(
            "426 Upgrade Required",
            "application/json; charset=utf-8",
            body.into_bytes(),
        ),
        Ok(api::ChangeV2Json::Invalid(body)) => Response::new(
            "400 Bad Request",
            "application/json; charset=utf-8",
            body.into_bytes(),
        ),
        Ok(api::ChangeV2Json::Stale(body)) => Response::new(
            "409 Conflict",
            "application/json; charset=utf-8",
            body.into_bytes(),
        ),
        Ok(api::ChangeV2Json::Retryable(body)) => Response::new(
            "503 Service Unavailable",
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

fn authoritative_change_v2_profile_response(state: &InspectState) -> Response {
    api_response(api::authoritative_change_v2_profile_json(
        state.repo.as_path(),
        &state.change_reader_cache,
    ))
}

fn authoritative_changes_v2_response(state: &InspectState, query: Option<&str>) -> Response {
    change_v2_response(api::authoritative_changes_v2_json(
        state.repo.as_path(),
        &state.change_reader_cache,
        query,
        &state.page_token_signer,
    ))
}

fn authoritative_change_attention_v2_response(
    state: &InspectState,
    query: Option<&str>,
) -> Response {
    change_v2_response(api::authoritative_change_attention_v2_json(
        state.repo.as_path(),
        &state.change_reader_cache,
        query,
        &state.page_token_signer,
    ))
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

fn strict_percent_decode(input: &str) -> Result<String, String> {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'%' => {
                if index + 2 >= bytes.len() {
                    return Err("truncated percent encoding in exact selector".to_owned());
                }
                let Some(high) = (bytes[index + 1] as char).to_digit(16) else {
                    return Err("malformed percent encoding in exact selector".to_owned());
                };
                let Some(low) = (bytes[index + 2] as char).to_digit(16) else {
                    return Err("malformed percent encoding in exact selector".to_owned());
                };
                out.push((high * 16 + low) as u8);
                index += 3;
            }
            b'+' => {
                out.push(b' ');
                index += 1;
            }
            byte => {
                out.push(byte);
                index += 1;
            }
        }
    }
    String::from_utf8(out).map_err(|_| "invalid UTF-8 in exact selector".to_owned())
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
    write_response_inner(stream, response, &[])
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
    extra_headers: &[(&str, &str)],
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
    for (name, value) in extra_headers {
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

    #[test]
    fn qualification_cli_control_binary_attests_clean_source() {
        if let Ok(expected_commit) =
            std::env::var("POINTBREAK_QUALIFICATION_EXPECTED_CONTROL_COMMIT")
        {
            assert_eq!(env!("POINTBREAK_BUILD_SOURCE"), "git");
            assert_eq!(env!("POINTBREAK_BUILD_COMMIT"), expected_commit);
            assert_eq!(env!("POINTBREAK_BUILD_DIRTY"), "false");
            let build_configuration = format!(
                "debug={},gix={},bench={},longitudinal-counting={},lmdb-proof={},gix-parity={}",
                cfg!(debug_assertions),
                cfg!(feature = "gix"),
                cfg!(feature = "bench"),
                cfg!(feature = "longitudinal-counting"),
                cfg!(feature = "lmdb-proof"),
                cfg!(feature = "gix-parity"),
            );
            assert_eq!(
                build_configuration,
                "debug=true,gix=true,bench=true,longitudinal-counting=true,lmdb-proof=false,gix-parity=false"
            );
        }
        println!(
            "pointbreak-control-source={} commit={} dirty={} longitudinal-counting={}",
            env!("POINTBREAK_BUILD_SOURCE"),
            env!("POINTBREAK_BUILD_COMMIT"),
            env!("POINTBREAK_BUILD_DIRTY"),
            cfg!(feature = "longitudinal-counting"),
        );
    }

    const SERVER_SOURCE: &str = include_str!("server.rs");
    const API_SOURCE: &str = include_str!("api.rs");

    fn source_between<'a>(source: &'a str, start: &str, end: &str) -> &'a str {
        let start = source
            .find(start)
            .unwrap_or_else(|| panic!("missing source start marker: {start}"));
        let tail = &source[start..];
        let end = tail
            .find(end)
            .unwrap_or_else(|| panic!("missing source end marker after {start}: {end}"));
        &tail[..end]
    }

    fn assert_source_order(source: &str, earlier: &str, later: &str) {
        let earlier = source
            .find(earlier)
            .unwrap_or_else(|| panic!("missing earlier source marker: {earlier}"));
        let later = source
            .find(later)
            .unwrap_or_else(|| panic!("missing later source marker: {later}"));
        assert!(
            earlier < later,
            "expected `{earlier}` before `{later}` in production source"
        );
    }

    #[test]
    fn inspector_state_resolves_one_runtime_for_change_reads_and_recovery_controls() {
        let state = source_between(
            SERVER_SOURCE,
            "pub(super) struct InspectState {",
            "impl InspectState {",
        );
        assert!(
            state.contains("derived_changes: pointbreak::session::DerivedChangeAccess"),
            "Inspector state must own the derived Change product facade"
        );
        assert!(
            state.contains("derived_history: pointbreak::session::DerivedHistoryAccess"),
            "the supported recovery facade remains explicit"
        );
        assert!(
            state.contains("strict_change_stamp: pointbreak::session::StrictChangeStampBinder"),
            "strict documents must own the product runtime's metadata-only stamp binder"
        );

        let constructor = source_between(
            SERVER_SOURCE,
            "fn new_with_background_rebuild(",
            "struct CachedChangeReaderState",
        );
        assert!(
            constructor.contains("DerivedChangeAccess::resolve_for_inspector"),
            "the product facade must be the only Inspector derived-runtime resolver"
        );
        assert!(
            !constructor.contains("DerivedHistoryAccess::resolve_for_inspector"),
            "recovery must wrap the product facade's runtime, not resolve a parallel runtime"
        );
        assert!(
            constructor.contains("let derived_history = derived_changes."),
            "the recovery facade must be obtained from the resolved product facade"
        );
        assert!(
            constructor.contains("let strict_change_stamp = derived_changes.strict_stamp_binder()"),
            "strict stamp binding must reuse the resolved product runtime"
        );
        assert!(constructor.contains("derived_changes,"));
        assert!(constructor.contains("strict_change_stamp,"));
        assert!(constructor.contains("derived_history,"));

        let route = source_between(SERVER_SOURCE, "fn route(", "fn route_change_v2(");
        let controls = source_between(
            SERVER_SOURCE,
            "fn derived_access_control_response(",
            "fn static_response(",
        );
        assert!(
            route.contains("api::derived_access_status_json(\n            &state.derived_history"),
            "status must use the recovery facade derived from the product runtime"
        );
        assert!(controls.contains("state.derived_history.restart_background_rebuild()"));
        assert!(controls.contains("state.derived_history.cancel_background_rebuild()"));
    }

    /// Post-Green source-shape verification for the route ownership split.
    #[test]
    fn routes_split_derived_collections_and_timeline_from_explicit_off_and_exact_reads() {
        let route = source_between(SERVER_SOURCE, "fn route(", "fn route_change_v2(");
        let profile = source_between(
            route,
            "if path == \"/api/v2/profile\"",
            "if path == \"/api/v2/history\"",
        );
        let timeline = source_between(
            route,
            "if path == \"/api/v2/history\"",
            "if path == \"/api/v2/changes\"",
        );
        let changes = source_between(
            route,
            "if path == \"/api/v2/changes\"",
            "if path == \"/api/v2/attention\"",
        );
        let attention = source_between(
            route,
            "if path == \"/api/v2/attention\"",
            "if path.starts_with(\"/api/v2/changes/\")",
        );
        for (name, derived_route, authoritative_helper, derived_helper) in [
            (
                "Profile",
                profile,
                "authoritative_change_v2_profile_response",
                "api::change_v2_profile_json",
            ),
            (
                "Changes",
                changes,
                "authoritative_changes_v2_response",
                "api::changes_v2_json",
            ),
            (
                "Attention",
                attention,
                "authoritative_change_attention_v2_response",
                "api::change_attention_v2_json",
            ),
        ] {
            assert!(
                derived_route.contains("state.derived_changes"),
                "{name} must dispatch through DerivedChangeAccess"
            );
            assert!(
                !derived_route.contains("state.change_reader_cache"),
                "{name} must not enter the strict Change reader cache"
            );
            assert_source_order(
                derived_route,
                "!state.derived_changes.is_active()",
                authoritative_helper,
            );
            assert_source_order(derived_route, authoritative_helper, derived_helper);
        }
        let explicit_off_helpers = source_between(
            SERVER_SOURCE,
            "fn authoritative_change_v2_profile_response(",
            "fn explicit_authoritative_response(",
        );
        assert_eq!(
            explicit_off_helpers
                .matches("&state.change_reader_cache")
                .count(),
            3,
            "every explicit-off Change collection must use the strict reader cache"
        );
        for forbidden in [
            "state.derived_changes",
            "read_change_semantics_for_qualification",
            "DerivedChangeAccess",
        ] {
            assert!(
                !explicit_off_helpers.contains(forbidden),
                "explicit-off Change routing must not enter {forbidden}"
            );
        }
        assert!(timeline.contains("state.derived_changes"));
        assert!(timeline.contains("state.change_reader_cache"));
        assert!(timeline.contains("state.strict_change_stamp"));
        assert_source_order(
            timeline,
            "!state.derived_changes.is_active()",
            "api::authoritative_event_history_v2_json",
        );
        assert_source_order(
            timeline,
            "api::authoritative_event_history_v2_json",
            "api::event_history_v2_json",
        );

        let exact = source_between(
            SERVER_SOURCE,
            "fn route_change_v2(",
            "fn exact_selector_values(",
        );
        assert!(exact.contains("let cache = &state.change_reader_cache"));
        assert!(exact.contains("let stamp_binder = &state.strict_change_stamp"));
        let detail = source_between(
            exact,
            "[change_id] => {",
            "[change_id, revisions, revision_id] if revisions == \"revisions\" => {",
        );
        let revision = source_between(
            exact,
            "[change_id, revisions, revision_id] if revisions == \"revisions\" => {",
            "[change_id, revisions, revision_id, resource]",
        );
        let resource = source_between(
            exact,
            "[change_id, revisions, revision_id, resource]",
            "[change_id, interdiff, from_revision_id, to_revision_id]",
        );
        let interdiff = source_between(
            exact,
            "[change_id, interdiff, from_revision_id, to_revision_id]",
            "_ => Response::json_error",
        );
        for (name, derived_route, producer) in [
            ("detail", detail, "api::derived_change_detail_v2_json"),
            ("revision", revision, "api::derived_change_revision_v2_json"),
            ("resource", resource, "api::derived_change_revision_v2_json"),
            (
                "interdiff",
                interdiff,
                "api::derived_change_interdiff_v2_json",
            ),
        ] {
            assert!(
                derived_route.contains("state.derived_changes.is_active()"),
                "{name} must gate the derived producer on active mode"
            );
            assert!(
                derived_route.contains(producer),
                "{name} must dispatch to its derived producer"
            );
            assert!(
                !derived_route.contains("change_reader_cache"),
                "{name} derived dispatch must not name the replay cache"
            );
            assert!(
                derived_route.contains("cache,\n                    stamp_binder,"),
                "{name} inactive dispatch must retain the cache and strict stamp binder pair"
            );
        }

        let profile_api = source_between(
            API_SOURCE,
            "pub(super) fn change_v2_profile_json(",
            "pub(super) fn changes_v2_json(",
        );
        let changes_api = source_between(
            API_SOURCE,
            "pub(super) fn changes_v2_json(",
            "pub(super) fn event_history_v2_json(",
        );
        let timeline_api = source_between(
            API_SOURCE,
            "pub(super) fn event_history_v2_json(",
            "pub(super) fn authoritative_event_history_v2_json(",
        );
        let authoritative_timeline_api = source_between(
            API_SOURCE,
            "pub(super) fn authoritative_event_history_v2_json(",
            "pub(super) fn authoritative_event_history_v2_from_loaded(",
        );
        let attention_api = source_between(
            API_SOURCE,
            "pub(super) fn change_attention_v2_json(",
            "pub(super) fn exact_selection_error_json(",
        );
        for (name, derived_api) in [
            ("Profile", profile_api),
            ("Changes", changes_api),
            ("Attention", attention_api),
        ] {
            assert!(
                derived_api.contains("DerivedChangeAccess"),
                "{name} API must accept the product facade"
            );
            assert!(
                !derived_api.contains("ChangeReaderCache"),
                "{name} API must not accept the strict reader cache"
            );
        }
        assert!(timeline_api.contains("DerivedChangeAccess"));
        assert!(!timeline_api.contains("ChangeReaderCache"));
        assert!(!timeline_api.contains("StrictChangeStampBinder"));
        assert!(authoritative_timeline_api.contains("ChangeReaderCache"));
        assert!(authoritative_timeline_api.contains("StrictChangeStampBinder"));
        assert!(!authoritative_timeline_api.contains("DerivedChangeAccess"));
    }

    /// Post-Green source-shape verification for the frozen exact-read helpers.
    #[test]
    fn derived_exact_producers_use_projection_native_builders() {
        let detail = source_between(
            API_SOURCE,
            "pub(super) fn derived_change_detail_v2_json(",
            "pub(super) fn change_revision_v2_json(",
        );
        let revision = source_between(
            API_SOURCE,
            "pub(super) fn derived_change_revision_v2_json(",
            "pub(super) fn change_interdiff_v2_json(",
        );
        let interdiff = source_between(
            API_SOURCE,
            "pub(super) fn derived_change_interdiff_v2_json(",
            "fn with_change_v2(",
        );

        assert!(detail.contains("review_generation_detail_document"));
        for required in [
            "review_generation",
            "exact_ref_from_projections",
            "exact_revision_session",
            "exact_read_from_shown",
            "contextual_exact_read_from_derived",
        ] {
            assert!(
                revision.contains(required),
                "exact Revision producer must reference `{required}`"
            );
        }
        assert!(interdiff.contains("exact_ref_from_projections"));
        assert!(interdiff.contains("build_interdiff_from_projections"));
        for (name, producer) in [("detail", detail), ("interdiff", interdiff)] {
            for forbidden in ["build_exact_read", "build_contextual_exact_read"] {
                assert!(
                    !producer.contains(forbidden),
                    "{name} must not reference frozen helper `{forbidden}`"
                );
            }
        }
        for forbidden in [
            "build_exact_read",
            "build_contextual_exact_read",
            "ChangeReaderCache",
            "StrictChangeStampBinder",
        ] {
            assert!(
                !revision.contains(forbidden),
                "exact Revision producer must not reference `{forbidden}`"
            );
        }
    }

    #[test]
    fn change_query_validation_precedes_derived_generation_access() {
        let changes = source_between(
            API_SOURCE,
            "pub(super) fn changes_v2_json(",
            "pub(super) fn event_history_v2_json(",
        );
        let attention = source_between(
            API_SOURCE,
            "pub(super) fn change_attention_v2_json(",
            "pub(super) fn exact_selection_error_json(",
        );
        let timeline = source_between(
            API_SOURCE,
            "pub(super) fn event_history_v2_json(",
            "pub(super) fn authoritative_event_history_v2_json(",
        );
        for (name, helper, parser_call, facade_call) in [
            (
                "Changes",
                changes,
                "super::change_page::parse_signed",
                ".changes(",
            ),
            (
                "Attention",
                attention,
                "super::change_page::parse_signed",
                ".attention(",
            ),
            (
                "Timeline",
                timeline,
                "super::event_history_page::parse_signed",
                ".timeline(",
            ),
        ] {
            assert!(helper.contains(parser_call));
            assert!(
                helper.contains("DerivedChangeAccess"),
                "{name} must receive the already-resolved product facade"
            );
            assert_source_order(helper, parser_call, facade_call);
            for forbidden in [
                "ChangeReaderCache",
                "change_reader_state_for_repo",
                "change_reader_head_marker_for_repo",
                "with_change_v2",
                "DerivedHistoryRoute",
                "ExhaustiveSearchFallback",
                "event_history_query::apply",
                "authoritative_event_history_v2",
                "resolve_for_inspector",
            ] {
                assert!(
                    !helper.contains(forbidden),
                    "{name} enters forbidden `{forbidden}` after the product-route split"
                );
            }
        }
    }

    #[test]
    fn authenticated_requests_have_no_eager_cache_warm() {
        let connection = source_between(
            SERVER_SOURCE,
            "fn handle_connection(",
            "fn longitudinal_receipt_header(",
        );
        assert!(
            !connection.contains("change_reader_cache"),
            "connection admission must not populate exact or Timeline state"
        );
        assert!(
            !connection.contains("load_state"),
            "Profile-first must not hide a complete strict fold"
        );
    }

    #[test]
    fn static_and_unauthenticated_requests_reach_no_cache_warm() {
        let connection = source_between(
            SERVER_SOURCE,
            "fn handle_connection(",
            "fn longitudinal_receipt_header(",
        );
        assert_source_order(connection, "!has_exact_host", "let response = route(");
        assert_source_order(connection, "!has_exact_bearer", "let response = route(");
        assert!(
            !connection.contains("warm_caches_after_auth"),
            "static and unauthenticated requests must have no cache-warm seam"
        );

        let route = source_between(SERVER_SOURCE, "fn route(", "fn route_change_v2(");
        assert_source_order(route, "static_response(path)", "if !is_api_path(path)");
    }

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

    fn route_for_query(path: &str, query: &str) -> Response {
        let repo = tempfile::tempdir().expect("routing test repository");
        assert!(
            std::process::Command::new("git")
                .args(["init", "--quiet"])
                .current_dir(repo.path())
                .status()
                .unwrap()
                .success()
        );
        let state = Arc::new(
            InspectState::new_with_background_rebuild(repo.path().to_path_buf(), false).unwrap(),
        );
        route(&state, true, "GET", path, Some(query))
    }

    struct ExactChangeFixture {
        _repo: tempfile::TempDir,
        state: InspectState,
        change_id: String,
        revision_id: String,
        artifact_hash: String,
    }

    fn exact_change_fixture(rebuild: bool) -> ExactChangeFixture {
        let repo = tempfile::tempdir().expect("exact Change repository");
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
        std::fs::write(repo.path().join("sample.txt"), "after\n").unwrap();
        let events = repo.path().join(".git/pointbreak/events");
        std::fs::create_dir_all(&events).expect("create exact Change fixture authority");
        for (name, bytes) in [
            (
                "5a1f8bbdea0db6199064bb2b75dfa89382b23398c71c640f7ca3268e48e3afaf.json",
                include_bytes!(
                    "../../../tests/support/assets/change-ready-store/5a1f8bbdea0db6199064bb2b75dfa89382b23398c71c640f7ca3268e48e3afaf.json"
                )
                .as_slice(),
            ),
            (
                "f31956c2b820926adc74d4d03cb03820d13c9ed2739b5f7ada81611a6f8bcff1.json",
                include_bytes!(
                    "../../../tests/support/assets/change-ready-store/f31956c2b820926adc74d4d03cb03820d13c9ed2739b5f7ada81611a6f8bcff1.json"
                )
                .as_slice(),
            ),
        ] {
            std::fs::write(events.join(name), bytes)
                .expect("write exact Change fixture authority record");
        }
        let capture = pointbreak::session::capture_change_revision(
            pointbreak::session::ChangeCaptureOptions::initial(
                "change-operation:exact-change-route-fixture",
                pointbreak::session::CaptureOptions::new(repo.path()),
                pointbreak::model::ChangeIdentityDescriptorV1::opaque_nonce([0x84; 32]),
            ),
        )
        .expect("capture exact Change fixture");

        let state =
            InspectState::new_with_background_rebuild(repo.path().to_path_buf(), false).unwrap();
        if rebuild {
            state
                .derived_changes
                .recovery_access()
                .rebuild(|_| pointbreak::session::DerivedHistoryControl::Continue)
                .expect("publish the fixture's derived generation through the public recovery API");
            assert!(
                state.derived_changes.is_active(),
                "the rebuilt product access must be active"
            );
            let pointbreak::session::DerivedChangeOutcomeV1::Ready(current) = state
                .derived_changes
                .review_generation()
                .expect("select the rebuilt generation through the product access")
            else {
                panic!("the rebuilt product access must select a current generation");
            };
            assert!(
                !current.stamp().is_empty(),
                "the selected current generation must carry its stamp"
            );
        }

        let change_id = capture.change_id.as_str().to_owned();
        let revision_id = capture.revision.revision_id.as_str().to_owned();
        let artifact_hash = capture.revision.object_artifact_content_hash;

        ExactChangeFixture {
            _repo: repo,
            state,
            change_id,
            revision_id,
            artifact_hash,
        }
    }

    fn exact_change_fixture_with_fact_port() -> ExactChangeFixture {
        let mut fixture = exact_change_fixture(false);
        let repo = fixture._repo.path();
        let change_id = pointbreak::model::ChangeId::new(fixture.change_id.clone());
        let origin_revision = pointbreak::model::RevisionId::new(fixture.revision_id.clone());
        let origin = pointbreak::model::RevisionRefV1::new(
            origin_revision.clone(),
            fixture.artifact_hash.clone(),
        )
        .unwrap();
        let observation = pointbreak::session::record_observation(
            pointbreak::session::ObservationAddOptions::new(repo)
                .with_exact_revision_id(origin_revision.clone())
                .with_track("agent:route-parity")
                .with_title("ported route parity observation")
                .with_actor_id(pointbreak::model::ActorId::new("actor:agent:route-parity")),
        )
        .expect("record the ported fixture fact");
        let state = pointbreak::session::change_reader_state_for_repo(repo)
            .expect("read ported fixture Change state");
        let ready = state.ready().expect("ported fixture Change store is ready");
        let review_cursor = pointbreak::session::select_review_cursor(
            &ready.projection.changes[&change_id],
            &ready.document_projection,
            Some(&origin_revision),
            false,
            pointbreak::session::ReviewSourceBindingV1::Captured,
        )
        .expect("select the ported fixture Review cursor")
        .token;

        std::fs::write(repo.join("sample.txt"), "after replacement\n").unwrap();
        let replacement = pointbreak::session::capture_change_revision(
            pointbreak::session::ChangeCaptureOptions::advance(
                "change-operation:exact-change-route-port",
                pointbreak::session::CaptureOptions::new(repo),
                review_cursor,
                pointbreak::session::ChangeAdvanceV1::Replace,
            ),
        )
        .expect("capture the ported fixture replacement");
        pointbreak::session::port_review_fact(
            pointbreak::session::FactPortOptions::new(
                repo,
                origin,
                pointbreak::session::event::FactRefV1::Observation {
                    observation_id: observation.observation_id,
                },
                replacement.review_cursor.token.clone(),
                pointbreak::session::event::FactPortRelationV1::ContextOnly,
                "agent:route-parity",
            )
            .with_actor_id(pointbreak::model::ActorId::new("actor:agent:route-parity")),
        )
        .expect("port the fixture fact into the replacement");
        fixture
            .state
            .derived_changes
            .recovery_access()
            .rebuild(|_| pointbreak::session::DerivedHistoryControl::Continue)
            .expect("publish the ported fixture's derived generation");
        let pointbreak::session::DerivedChangeOutcomeV1::Ready(_) = fixture
            .state
            .derived_changes
            .review_generation()
            .expect("select the ported fixture's derived generation")
        else {
            panic!("the ported fixture must select a current generation");
        };

        fixture.change_id = replacement.change_id.as_str().to_owned();
        fixture.revision_id = replacement.revision.revision_id.as_str().to_owned();
        fixture.artifact_hash = replacement.revision.object_artifact_content_hash;
        fixture
    }

    fn change_v2_json_parts(
        result: Result<api::ChangeV2Json, String>,
    ) -> Result<(&'static str, String), String> {
        result.map(|outcome| match outcome {
            api::ChangeV2Json::Ok(body) => ("ok", body),
            api::ChangeV2Json::Unavailable(body) => ("unavailable", body),
            api::ChangeV2Json::UpgradeRequired(body) => ("upgrade-required", body),
            api::ChangeV2Json::Invalid(body) => ("invalid", body),
            api::ChangeV2Json::Stale(body) => ("stale", body),
            api::ChangeV2Json::Retryable(body) => ("retryable", body),
        })
    }

    fn assert_active_but_unready_exact_read_posture(
        fixture: &ExactChangeFixture,
        requests: &[(String, Option<String>)],
    ) {
        assert!(fixture.state.derived_changes.is_active());
        assert!(
            !matches!(
                fixture.state.derived_changes.review_generation(),
                Ok(pointbreak::session::DerivedChangeOutcomeV1::Ready(_))
            ),
            "the refusal fixture must not carry a current generation"
        );
        let mut exact_responses = Vec::with_capacity(requests.len());
        for (path, query) in requests {
            let response = route_change_v2(&fixture.state, path, query.as_deref());
            assert!(
                matches!(response.status, "409 Conflict" | "503 Service Unavailable"),
                "{path} silently fell back instead of following the derived lens posture: {}",
                response.status
            );
            let body: serde_json::Value = serde_json::from_slice(&response.body).unwrap();
            assert!(
                matches!(
                    body["schema"].as_str(),
                    Some("pointbreak.store-migration-required")
                        | Some("pointbreak.store-migration-in-progress")
                        | Some("pointbreak.inspect-change-authority-error")
                        | Some("pointbreak.inspect-change-projection-error")
                ),
                "{path}: {body}"
            );
            exact_responses.push((path.as_str(), response.status, response.body));
        }
        let [detail, revision, resource, interdiff] = exact_responses.as_slice() else {
            panic!("the posture comparison requires all four exact-route responses");
        };
        for response in [revision, resource, interdiff] {
            assert_eq!(
                (detail.1, detail.2.as_slice()),
                (response.1, response.2.as_slice()),
                "{} and {} must follow the same refusal posture",
                detail.0,
                response.0
            );
        }
    }

    #[test]
    fn bounded_change_routes_share_typed_query_errors() {
        for path in ["/api/v2/changes", "/api/v2/attention"] {
            let response = route_for_query(path, "order=activity_desc");
            assert_eq!(response.status, "400 Bad Request");
            let body: serde_json::Value = serde_json::from_slice(&response.body).unwrap();
            assert_eq!(body["schema"], "pointbreak.inspect-change-page-error");
            assert_eq!(body["code"], "invalid_query");
        }
    }

    #[test]
    fn timeline_route_uses_its_closed_typed_query_contract_before_store_access() {
        let response = route_for_query("/api/v2/history", "unknown=value");
        assert_eq!(response.status, "400 Bad Request");
        let body: serde_json::Value = serde_json::from_slice(&response.body).unwrap();
        assert_eq!(body["schema"], "pointbreak.inspect-event-history-error");
        assert_eq!(body["code"], "invalid_query");
    }

    #[test]
    fn exact_selector_query_is_closed_strict_and_order_independent() {
        assert_eq!(
            exact_selector_values(
                Some("toArtifactHash=sha256%3Atwo&fromArtifactHash=sha256%3Aone"),
                &["fromArtifactHash", "toArtifactHash"],
            )
            .unwrap(),
            vec!["sha256:one", "sha256:two"]
        );

        for (query, expected) in [
            (
                Some("artifactHash=one&artifactHash=two"),
                &["artifactHash"][..],
            ),
            (Some("artifactHash="), &["artifactHash"][..]),
            (Some("artifactHash=%"), &["artifactHash"][..]),
            (Some("artifactHash=%GG"), &["artifactHash"][..]),
            (Some("artifactHash=%FF"), &["artifactHash"][..]),
            (Some("artifactHash=one&extra=two"), &["artifactHash"][..]),
            (Some("artifactHash"), &["artifactHash"][..]),
            (None, &["artifactHash"][..]),
        ] {
            assert!(
                exact_selector_values(query, expected).is_err(),
                "query unexpectedly accepted: {query:?}"
            );
        }
    }

    #[test]
    fn exact_routes_return_typed_bad_request_before_store_access() {
        let cases = [
            (
                "/api/v2/changes/change/revisions/revision",
                "artifactHash=one&artifactHash=two",
            ),
            (
                "/api/v2/changes/change/revisions/revision/resource",
                "artifactHash=one&unknown=two",
            ),
            (
                "/api/v2/changes/change/interdiff/from/to",
                "fromArtifactHash=one&toArtifactHash=%GG",
            ),
            (
                "/api/v2/changes/change%GG/revisions/revision",
                "artifactHash=one",
            ),
        ];
        for (path, query) in cases {
            let response = route_for_query(path, query);
            assert_eq!(response.status, "400 Bad Request", "{path}?{query}");
            let body: serde_json::Value = serde_json::from_slice(&response.body).unwrap();
            assert_eq!(
                body["schema"], "pointbreak.inspect-change-selection-error",
                "{path}?{query}: {body:#}"
            );
            assert_eq!(body["code"], "invalid_exact_selection");
        }
    }

    #[test]
    fn typed_page_outcomes_map_to_distinct_http_statuses() {
        let unavailable = change_v2_response(Ok(api::ChangeV2Json::Unavailable(
            "{\"code\":\"migration_required\"}".to_owned(),
        )));
        let upgrade_required = change_v2_response(Ok(api::ChangeV2Json::UpgradeRequired(
            "{\"code\":\"reader_upgrade_required\"}".to_owned(),
        )));
        let invalid = change_v2_response(Ok(api::ChangeV2Json::Invalid(
            "{\"code\":\"invalid_query\"}".to_owned(),
        )));
        let stale = change_v2_response(Ok(api::ChangeV2Json::Stale(
            "{\"code\":\"stale_projection\"}".to_owned(),
        )));
        let moving = change_v2_response(Ok(api::ChangeV2Json::Retryable(
            "{\"code\":\"moving_journal\"}".to_owned(),
        )));
        assert_eq!(unavailable.status, "409 Conflict");
        assert_eq!(upgrade_required.status, "426 Upgrade Required");
        assert_eq!(invalid.status, "400 Bad Request");
        assert_eq!(stale.status, "409 Conflict");
        assert_eq!(moving.status, "503 Service Unavailable");
    }

    #[test]
    fn moving_journal_during_exact_load_is_a_typed_retryable_503() {
        let exact = api::with_change_v2_outcome_from_loaded(
            Err(ChangeReaderLoadError::MovingJournal),
            None,
            |_, _| unreachable!("a moving load cannot compose an exact response"),
        )
        .expect("a moving exact load has a typed response");
        let api::ChangeV2Json::Retryable(exact_body) = exact else {
            panic!("a moving exact load must remain retryable");
        };

        let repo = tempfile::tempdir().expect("moving-load comparison repository");
        let signer = super::super::page_token::PageTokenSigner::from_seed([9_u8; 32]);
        let request = super::super::event_history_page::parse_signed(None, &signer)
            .expect("bare Timeline request is valid");
        let timeline = api::authoritative_event_history_v2_from_loaded(
            repo.path(),
            request,
            &signer,
            None,
            &pointbreak::session::TrustSet::default(),
            Err(ChangeReaderLoadError::MovingJournal),
        )
        .expect("a moving Timeline load has a typed response");
        let api::ChangeV2Json::Retryable(timeline_body) = timeline else {
            panic!("a moving Timeline load must remain retryable");
        };

        assert_eq!(exact_body, timeline_body);
        let document: serde_json::Value = serde_json::from_str(&exact_body).unwrap();
        assert_eq!(document["code"], "moving_journal");
        assert_eq!(document["retryable"], true);
        assert_eq!(
            change_v2_response(Ok(api::ChangeV2Json::Retryable(exact_body))).status,
            "503 Service Unavailable"
        );
    }

    #[test]
    fn other_exact_load_errors_keep_the_500_class() {
        let message = "exact Change load failed";
        let Err(error) = api::with_change_v2_outcome_from_loaded(
            Err(ChangeReaderLoadError::Other(message.to_owned())),
            None,
            |_, _| unreachable!("a failed load cannot compose an exact response"),
        ) else {
            panic!("other load failures keep the server error path");
        };
        assert_eq!(error, message);
    }

    #[test]
    fn derived_change_detail_bytes_equal_the_authoritative_arm() {
        let fixture = exact_change_fixture(true);
        let authoritative = change_v2_json_parts(api::change_detail_v2_json(
            &fixture.state.repo,
            &fixture.state.change_reader_cache,
            &fixture.state.strict_change_stamp,
            &fixture.change_id,
        ));
        let derived = change_v2_json_parts(api::derived_change_detail_v2_json(
            &fixture.state.derived_changes,
            &fixture.change_id,
        ));
        assert_eq!(derived, authoritative, "success bytes including stamps");

        let missing = format!("change:sha256:{}", "f".repeat(64));
        let authoritative = change_v2_json_parts(api::change_detail_v2_json(
            &fixture.state.repo,
            &fixture.state.change_reader_cache,
            &fixture.state.strict_change_stamp,
            &missing,
        ));
        let derived = change_v2_json_parts(api::derived_change_detail_v2_json(
            &fixture.state.derived_changes,
            &missing,
        ));
        assert_eq!(derived, authoritative, "unknown-Change outcome parity");
    }

    #[test]
    fn derived_change_interdiff_bytes_equal_the_authoritative_arm() {
        let fixture = exact_change_fixture(true);
        let missing_from = format!("rev:sha256:{}", "e".repeat(64));
        let missing_to = format!("rev:sha256:{}", "d".repeat(64));
        for (case, from_revision_id, to_revision_id) in [
            (
                "success",
                fixture.revision_id.as_str(),
                fixture.revision_id.as_str(),
            ),
            ("bad from wins", missing_from.as_str(), missing_to.as_str()),
            (
                "bad to follows valid from",
                fixture.revision_id.as_str(),
                missing_to.as_str(),
            ),
        ] {
            let authoritative = change_v2_json_parts(api::change_interdiff_v2_json(
                &fixture.state.repo,
                &fixture.state.change_reader_cache,
                &fixture.state.strict_change_stamp,
                &fixture.change_id,
                from_revision_id,
                &fixture.artifact_hash,
                to_revision_id,
                &fixture.artifact_hash,
            ));
            let derived = change_v2_json_parts(api::derived_change_interdiff_v2_json(
                &fixture.state.derived_changes,
                &fixture.change_id,
                from_revision_id,
                &fixture.artifact_hash,
                to_revision_id,
                &fixture.artifact_hash,
            ));
            assert_eq!(derived, authoritative, "{case} byte parity");
        }
    }

    #[test]
    fn derived_change_revision_bytes_equal_the_authoritative_arm() {
        let fixture = exact_change_fixture(true);
        let missing_change = format!("change:sha256:{}", "f".repeat(64));
        let missing_revision = format!("rev:sha256:{}", "e".repeat(64));
        let mismatched_hash = format!("sha256:{}", "d".repeat(64));
        for (case, change_id, revision_id, artifact_hash) in [
            (
                "success",
                fixture.change_id.as_str(),
                fixture.revision_id.as_str(),
                fixture.artifact_hash.as_str(),
            ),
            (
                "unknown Change",
                missing_change.as_str(),
                fixture.revision_id.as_str(),
                fixture.artifact_hash.as_str(),
            ),
            (
                "nonmember Revision",
                fixture.change_id.as_str(),
                missing_revision.as_str(),
                fixture.artifact_hash.as_str(),
            ),
            (
                "malformed artifact hash",
                fixture.change_id.as_str(),
                fixture.revision_id.as_str(),
                "not-an-artifact-hash",
            ),
            (
                "mismatched artifact hash",
                fixture.change_id.as_str(),
                fixture.revision_id.as_str(),
                mismatched_hash.as_str(),
            ),
        ] {
            let authoritative = change_v2_json_parts(api::change_revision_v2_json(
                &fixture.state.repo,
                &fixture.state.change_reader_cache,
                &fixture.state.strict_change_stamp,
                change_id,
                revision_id,
                artifact_hash,
                true,
            ));
            let derived = change_v2_json_parts(api::derived_change_revision_v2_json(
                &fixture.state.derived_changes,
                change_id,
                revision_id,
                artifact_hash,
                true,
            ));
            assert_eq!(derived, authoritative, "{case} resource byte parity");
        }

        let fixture = exact_change_fixture_with_fact_port();
        let missing_change = format!("change:sha256:{}", "f".repeat(64));
        let missing_revision = format!("rev:sha256:{}", "e".repeat(64));
        let mismatched_hash = format!("sha256:{}", "d".repeat(64));
        for (case, change_id, revision_id, artifact_hash) in [
            (
                "success",
                fixture.change_id.as_str(),
                fixture.revision_id.as_str(),
                fixture.artifact_hash.as_str(),
            ),
            (
                "unknown Change",
                missing_change.as_str(),
                fixture.revision_id.as_str(),
                fixture.artifact_hash.as_str(),
            ),
            (
                "nonmember Revision",
                fixture.change_id.as_str(),
                missing_revision.as_str(),
                fixture.artifact_hash.as_str(),
            ),
            (
                "malformed artifact hash",
                fixture.change_id.as_str(),
                fixture.revision_id.as_str(),
                "not-an-artifact-hash",
            ),
            (
                "mismatched artifact hash",
                fixture.change_id.as_str(),
                fixture.revision_id.as_str(),
                mismatched_hash.as_str(),
            ),
        ] {
            let authoritative = change_v2_json_parts(api::change_revision_v2_json(
                &fixture.state.repo,
                &fixture.state.change_reader_cache,
                &fixture.state.strict_change_stamp,
                change_id,
                revision_id,
                artifact_hash,
                false,
            ));
            if case == "success" {
                let (_, body) = authoritative.as_ref().expect("authoritative success");
                let document: serde_json::Value = serde_json::from_str(body).unwrap();
                assert_eq!(
                    document["factPorts"].as_array().map(Vec::len),
                    Some(1),
                    "contextual parity must include the applicable fact port"
                );
            }
            let derived = change_v2_json_parts(api::derived_change_revision_v2_json(
                &fixture.state.derived_changes,
                change_id,
                revision_id,
                artifact_hash,
                false,
            ));
            assert_eq!(derived, authoritative, "{case} contextual byte parity");
        }
    }

    /// Post-Green verification that every derived-gated exact route shares the
    /// same fail-closed response while the active runtime has no current generation.
    #[test]
    fn active_but_unready_exact_reads_follow_the_owner_posture() {
        let fixture = exact_change_fixture(false);
        assert_active_but_unready_exact_read_posture(
            &fixture,
            &[
                (format!("/api/v2/changes/{}", fixture.change_id), None),
                (
                    format!(
                        "/api/v2/changes/{}/revisions/{}",
                        fixture.change_id, fixture.revision_id
                    ),
                    Some(format!("artifactHash={}", fixture.artifact_hash)),
                ),
                (
                    format!(
                        "/api/v2/changes/{}/revisions/{}/resource",
                        fixture.change_id, fixture.revision_id
                    ),
                    Some(format!("artifactHash={}", fixture.artifact_hash)),
                ),
                (
                    format!(
                        "/api/v2/changes/{}/interdiff/{}/{}",
                        fixture.change_id, fixture.revision_id, fixture.revision_id
                    ),
                    Some(format!(
                        "fromArtifactHash={}&toArtifactHash={}",
                        fixture.artifact_hash, fixture.artifact_hash
                    )),
                ),
            ],
        );
    }

    /// Post-Green in-process verification that the Inspector exact producer
    /// enters bounded revision-detail selection and never the removal-audit phase.
    #[cfg(feature = "longitudinal-counting")]
    #[test]
    fn derived_exact_revision_producer_skips_audit_hydration() {
        use pointbreak::bench_support::longitudinal::{
            LongitudinalCountingScopeV1, LongitudinalDerivedAccessPhaseV1 as Phase,
        };

        let fixture = exact_change_fixture(true);
        let scope = LongitudinalCountingScopeV1::new("b".repeat(64)).expect("counting scope");
        let _guard = scope.enter();
        let (outcome, _) = change_v2_json_parts(api::derived_change_revision_v2_json(
            &fixture.state.derived_changes,
            &fixture.change_id,
            &fixture.revision_id,
            &fixture.artifact_hash,
            false,
        ))
        .expect("derived exact Revision response");
        assert_eq!(outcome, "ok");

        let phases = scope.snapshot().derived_access_phases;
        assert!(
            phases
                .iter()
                .any(|sample| sample.phase == Phase::RevisionDetailSqlSelection),
            "the exact producer must enter revision-detail SQL selection: {phases:#?}"
        );
        assert!(
            phases.iter().all(|sample| {
                sample.phase != Phase::RevisionDetailAuditCarrierHydrationValidation
            }),
            "the exact producer must never enter removal-audit hydration: {phases:#?}"
        );
    }

    #[test]
    fn timeline_route_preserves_typed_migration_state() {
        let response = route_for("GET", "/api/v2/history");
        assert_eq!(response.status, "409 Conflict");
        let body: serde_json::Value = serde_json::from_slice(&response.body).unwrap();
        assert_eq!(body["schema"], "pointbreak.store-migration-required");
        assert_eq!(body["state"], "migration_required");
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
        assert_eq!(receipt.semantic_result_sha256, "1".repeat(64));
        assert_eq!(receipt.counters.response_bytes, response.body.len() as u64);
    }

    #[cfg(feature = "longitudinal-counting")]
    #[test]
    fn barrier_counting_binds_the_actual_canonical_response_semantic() {
        use pointbreak::bench_support::longitudinal::{
            LONGITUDINAL_TIMELINE_POST_PIN_BARRIER_REQUEST_SCHEMA_V1,
            LongitudinalCounterReceiptContextV1, LongitudinalCountingScopeV1,
            LongitudinalTimelinePostPinBarrierRequestV1,
            canonical_longitudinal_response_semantic_sha256_v1,
        };

        let root = tempfile::tempdir().expect("barrier root");
        let barrier = LongitudinalTimelinePostPinBarrierRequestV1 {
            schema: LONGITUDINAL_TIMELINE_POST_PIN_BARRIER_REQUEST_SCHEMA_V1.to_owned(),
            barrier_identity_sha256: "b".repeat(64),
            expected_carrier_key_digest: "c".repeat(64),
            clean_carrier_sha256: "d".repeat(64),
            mutated_carrier_sha256: "e".repeat(64),
            mutation_recipe_sha256: "f".repeat(64),
        };
        let scope = LongitudinalCountingScopeV1::new("a".repeat(64))
            .expect("valid scope")
            .with_timeline_post_pin_barrier(root.path(), barrier)
            .expect("armed barrier");
        let response = Response::json_error("503 Service Unavailable", "projection_invalid");
        let supplied_semantic = "1".repeat(64);
        let encoded = longitudinal_receipt_header(
            &scope,
            LongitudinalCounterReceiptContextV1 {
                root_identity: "2".repeat(64),
                operation: "timeline_invalid_signature_fault".to_owned(),
                phase: "trust_suite".to_owned(),
                base_execution_identity_sha256: "3".repeat(64),
                derivative_execution_identity_sha256: "4".repeat(64),
                manifest_sha256: "b".repeat(64),
                schedule_sha256: "5".repeat(64),
                success: true,
                semantic_result_sha256: supplied_semantic.clone(),
                include_capacity_ownership: false,
            },
            &response,
        )
        .expect("receipt transport");
        let receipt: pointbreak::bench_support::longitudinal::LongitudinalCounterReceiptV1 =
            serde_json::from_slice(&URL_SAFE_NO_PAD.decode(encoded).expect("receipt base64"))
                .expect("receipt JSON");

        assert!(!receipt.success);
        assert_ne!(receipt.semantic_result_sha256, supplied_semantic);
        assert_eq!(
            receipt.semantic_result_sha256,
            canonical_longitudinal_response_semantic_sha256_v1(&response.body)
                .expect("canonical response semantic")
        );
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
    fn l0_v2_routes_survive_the_automatic_preactivation_generation() {
        let repo = tempfile::tempdir().expect("routing test repository");
        assert!(
            std::process::Command::new("git")
                .args(["init", "--quiet"])
                .current_dir(repo.path())
                .status()
                .expect("initialize routing test repository")
                .success()
        );
        let state = Arc::new(InspectState::new(repo.path().to_path_buf()).unwrap());
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        loop {
            let status = route(&state, true, "GET", "/api/derived-access/status", None);
            assert_eq!(status.status, "200 OK");
            let status: serde_json::Value = serde_json::from_slice(&status.body).unwrap();
            if status["availability"] == "current" && status["rebuildInFlight"] == false {
                break;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "the L0 background worker did not publish its preactivation generation: {status}"
            );
            std::thread::sleep(std::time::Duration::from_millis(10));
        }

        let profile = route(&state, true, "GET", "/api/v2/profile", None);
        assert_eq!(profile.status, "200 OK");
        let profile: serde_json::Value = serde_json::from_slice(&profile.body).unwrap();
        assert_eq!(profile["schema"], "pointbreak.inspect-reader-profile");
        assert_eq!(profile["availability"], "migration_required");
        assert!(profile["commitGraphStamp"].is_string());

        for path in ["/api/v2/changes", "/api/v2/attention"] {
            let response = route(&state, true, "GET", path, None);
            assert_eq!(response.status, "409 Conflict", "{path}");
            assert!(
                !response
                    .headers
                    .iter()
                    .any(|(name, _)| *name == "X-Pointbreak-Access-Source"),
                "{path} must not activate an authoritative fallback"
            );
            let body: serde_json::Value = serde_json::from_slice(&response.body).unwrap();
            assert_eq!(
                body["schema"], "pointbreak.store-migration-required",
                "{path}"
            );
            assert_eq!(body["state"], "migration_required", "{path}");
        }
    }

    #[test]
    fn doubled_change_route_prefix_is_not_reinterpreted_as_an_identity() {
        let response = route_for("GET", "/api/v2/changes//api/v2/changes/example");
        assert_eq!(response.status, "400 Bad Request");
    }

    #[test]
    fn change_reader_cache_reuses_and_invalidates_one_complete_generation() {
        use std::cell::Cell;

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
        let first = cache.load_state(repo.path()).unwrap();
        let hit = cache.load_state(repo.path()).unwrap();
        assert!(Arc::ptr_eq(&first.state, &hit.state));

        std::fs::write(repo.path().join("sample.txt"), "after\n").unwrap();
        pointbreak::session::capture_worktree_review(pointbreak::session::CaptureOptions::new(
            repo.path(),
        ))
        .unwrap();
        let refreshed = cache.load_state(repo.path()).unwrap();
        assert!(!Arc::ptr_eq(&first.state, &refreshed.state));
        assert!(
            refreshed.state.capability.cursor.journal_record_count
                > first.state.capability.cursor.journal_record_count
        );

        let cache = ChangeReaderCache::<u8, u16>::new();
        let state_builds = Cell::new(0_usize);
        let presentation_builds = Cell::new(0_usize);
        let timeline_builds = Cell::new(0_usize);
        let trust_set = TrustSet::default();
        let presented = cache
            .load_with(
                ChangeReaderLoad::Changes,
                || {
                    pointbreak::session::change_reader_head_marker_for_repo(repo.path())
                        .map_err(|error| ChangeReaderLoadError::Other(error.to_string()))
                },
                || {
                    state_builds.set(state_builds.get() + 1);
                    pointbreak::session::change_reader_state_for_repo(repo.path())
                        .map(Arc::new)
                        .map_err(|error| ChangeReaderLoadError::Other(error.to_string()))
                },
                |_| {
                    presentation_builds.set(presentation_builds.get() + 1);
                    Ok(Some(Arc::new(presentation_builds.get() as u8)))
                },
                |_, _, _| panic!("a Changes load must not build Timeline"),
            )
            .expect("first Changes request builds one state and presentation");
        assert_eq!(state_builds.get(), 1);
        assert_eq!(presentation_builds.get(), 1);
        assert_eq!(timeline_builds.get(), 0);
        let first_presentation = presented
            .presentation
            .as_ref()
            .expect("captured Change state has a presentation");
        assert!(presented.timeline.is_none());

        let same_changes = cache
            .load_with(
                ChangeReaderLoad::Changes,
                || {
                    pointbreak::session::change_reader_head_marker_for_repo(repo.path())
                        .map_err(|error| ChangeReaderLoadError::Other(error.to_string()))
                },
                || panic!("same-marker Changes request must not rebuild state"),
                |_| panic!("same-marker Changes request must not rebuild presentation"),
                |_, _, _| panic!("a Changes load must not build Timeline"),
            )
            .expect("same-marker Changes cache hit");
        assert!(Arc::ptr_eq(
            first_presentation,
            same_changes
                .presentation
                .as_ref()
                .expect("Changes returns cached presentation")
        ));

        let profile = cache
            .load_with(
                ChangeReaderLoad::State,
                || {
                    pointbreak::session::change_reader_head_marker_for_repo(repo.path())
                        .map_err(|error| ChangeReaderLoadError::Other(error.to_string()))
                },
                || panic!("profile cache hit must not rebuild state"),
                |_| panic!("profile cache hit must not build presentation"),
                |_, _, _| panic!("profile cache hit must not build Timeline"),
            )
            .expect("profile cache hit");
        assert!(profile.presentation.is_none());
        assert!(profile.timeline.is_none());
        assert!(Arc::ptr_eq(&presented.state, &profile.state));

        let timeline = cache
            .load_with(
                ChangeReaderLoad::Timeline(&trust_set),
                || {
                    pointbreak::session::change_reader_head_marker_for_repo(repo.path())
                        .map_err(|error| ChangeReaderLoadError::Other(error.to_string()))
                },
                || panic!("Timeline must reuse the cached state"),
                |_| panic!("Timeline must reuse the cached Change presentation"),
                |_, presentation, _| {
                    assert_eq!(*presentation, 1);
                    timeline_builds.set(timeline_builds.get() + 1);
                    Ok(Some(Arc::new(timeline_builds.get() as u16)))
                },
            )
            .expect("first Timeline request builds only Timeline");
        assert_eq!(state_builds.get(), 1);
        assert_eq!(presentation_builds.get(), 1);
        assert_eq!(timeline_builds.get(), 1);
        assert!(Arc::ptr_eq(
            first_presentation,
            timeline
                .presentation
                .as_ref()
                .expect("Timeline reuses the Change presentation")
        ));
        let first_timeline = timeline.timeline.as_ref().expect("Timeline is available");

        let same_timeline = cache
            .load_with(
                ChangeReaderLoad::Timeline(&trust_set),
                || {
                    pointbreak::session::change_reader_head_marker_for_repo(repo.path())
                        .map_err(|error| ChangeReaderLoadError::Other(error.to_string()))
                },
                || panic!("same trust must not rebuild state"),
                |_| panic!("same trust must not rebuild Change presentation"),
                |_, _, _| panic!("same trust must not rebuild Timeline"),
            )
            .expect("same-trust Timeline cache hit");
        assert!(Arc::ptr_eq(
            first_timeline,
            same_timeline.timeline.as_ref().expect("cached Timeline")
        ));

        let trust_path = repo.path().join("different-trust.json");
        std::fs::write(
            &trust_path,
            r#"{"allowedSigners":{"actor:agent:codex":["did:key:z6MkehRgf7yJbgaGfYsdoAsKdBPE3dj2CYhowQdcjqSJgvVd"]}}"#,
        )
        .expect("write distinct trust fixture");
        let different_trust =
            TrustSet::from_allowed_signers_file(&trust_path).expect("parse distinct trust fixture");
        let different = cache
            .load_with(
                ChangeReaderLoad::Timeline(&different_trust),
                || {
                    pointbreak::session::change_reader_head_marker_for_repo(repo.path())
                        .map_err(|error| ChangeReaderLoadError::Other(error.to_string()))
                },
                || panic!("trust-only change must not rebuild state"),
                |_| panic!("trust-only change must not rebuild Change presentation"),
                |_, presentation, _| {
                    assert_eq!(*presentation, 1);
                    timeline_builds.set(timeline_builds.get() + 1);
                    Ok(Some(Arc::new(timeline_builds.get() as u16)))
                },
            )
            .expect("different trust rebuilds only Timeline");
        assert_eq!(state_builds.get(), 1);
        assert_eq!(presentation_builds.get(), 1);
        assert_eq!(timeline_builds.get(), 2);
        assert!(Arc::ptr_eq(&presented.state, &different.state));
        assert!(Arc::ptr_eq(
            first_presentation,
            different
                .presentation
                .as_ref()
                .expect("different trust reuses the Change presentation")
        ));
        assert!(!Arc::ptr_eq(
            first_timeline,
            different
                .timeline
                .as_ref()
                .expect("different trust has a distinct Timeline")
        ));

        std::fs::write(repo.path().join("sample.txt"), "after again\n").unwrap();
        pointbreak::session::capture_worktree_review(pointbreak::session::CaptureOptions::new(
            repo.path(),
        ))
        .unwrap();
        let advanced = cache
            .load_with(
                ChangeReaderLoad::Changes,
                || {
                    pointbreak::session::change_reader_head_marker_for_repo(repo.path())
                        .map_err(|error| ChangeReaderLoadError::Other(error.to_string()))
                },
                || {
                    state_builds.set(state_builds.get() + 1);
                    pointbreak::session::change_reader_state_for_repo(repo.path())
                        .map(Arc::new)
                        .map_err(|error| ChangeReaderLoadError::Other(error.to_string()))
                },
                |_| {
                    presentation_builds.set(presentation_builds.get() + 1);
                    Ok(Some(Arc::new(presentation_builds.get() as u8)))
                },
                |_, _, _| panic!("a post-append Changes load must not build Timeline"),
            )
            .expect("append advances state and Change presentation together");
        assert_eq!(state_builds.get(), 2);
        assert_eq!(presentation_builds.get(), 2);
        assert_eq!(timeline_builds.get(), 2);
        assert!(!Arc::ptr_eq(&different.state, &advanced.state));
        assert!(!Arc::ptr_eq(
            different
                .presentation
                .as_ref()
                .expect("pre-append presentation exists"),
            advanced
                .presentation
                .as_ref()
                .expect("post-append presentation exists")
        ));
        assert!(advanced.timeline.is_none());
    }

    #[test]
    fn change_reader_cache_serializes_concurrent_timeline_bootstrap() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        use std::sync::{Arc, Barrier};

        let repo = tempfile::tempdir().expect("concurrent cache test repository");
        assert!(
            std::process::Command::new("git")
                .args(["init", "--quiet"])
                .current_dir(repo.path())
                .status()
                .expect("initialize repository")
                .success()
        );
        let state = Arc::new(
            pointbreak::session::change_reader_state_for_repo(repo.path())
                .expect("build one reusable test state"),
        );
        let cache = Arc::new(ChangeReaderCache::<u8, u16>::new());
        let state_builds = Arc::new(AtomicUsize::new(0));
        let presentation_builds = Arc::new(AtomicUsize::new(0));
        let timeline_builds = Arc::new(AtomicUsize::new(0));
        let barrier = Arc::new(Barrier::new(9));
        let mut handles = Vec::new();

        for _ in 0..8 {
            let cache = Arc::clone(&cache);
            let state = Arc::clone(&state);
            let state_builds = Arc::clone(&state_builds);
            let presentation_builds = Arc::clone(&presentation_builds);
            let timeline_builds = Arc::clone(&timeline_builds);
            let barrier = Arc::clone(&barrier);
            handles.push(std::thread::spawn(move || {
                let trust_set = TrustSet::default();
                barrier.wait();
                cache
                    .load_with(
                        ChangeReaderLoad::Timeline(&trust_set),
                        || Ok(41),
                        || {
                            state_builds.fetch_add(1, Ordering::SeqCst);
                            Ok(Arc::clone(&state))
                        },
                        |_| {
                            presentation_builds.fetch_add(1, Ordering::SeqCst);
                            Ok(Some(Arc::new(7)))
                        },
                        |_, presentation, _| {
                            assert_eq!(*presentation, 7);
                            timeline_builds.fetch_add(1, Ordering::SeqCst);
                            Ok(Some(Arc::new(9)))
                        },
                    )
                    .expect("concurrent bootstrap reaches one cached generation")
            }));
        }
        barrier.wait();

        let generations = handles
            .into_iter()
            .map(|handle| handle.join().expect("cache bootstrap thread"))
            .collect::<Vec<_>>();
        assert_eq!(state_builds.load(Ordering::SeqCst), 1);
        assert_eq!(presentation_builds.load(Ordering::SeqCst), 1);
        assert_eq!(timeline_builds.load(Ordering::SeqCst), 1);
        let first_presentation = generations[0]
            .presentation
            .as_ref()
            .expect("presentation exists");
        let first_timeline = generations[0].timeline.as_ref().expect("Timeline exists");
        assert!(generations.iter().all(|generation| {
            Arc::ptr_eq(&state, &generation.state)
                && Arc::ptr_eq(
                    first_presentation,
                    generation
                        .presentation
                        .as_ref()
                        .expect("every caller receives the cached presentation"),
                )
                && Arc::ptr_eq(
                    first_timeline,
                    generation
                        .timeline
                        .as_ref()
                        .expect("every caller receives the cached Timeline"),
                )
        }));
    }

    #[test]
    fn change_reader_cache_slow_timeline_does_not_block_warm_changes() {
        use std::sync::{Arc, mpsc};
        use std::time::Duration;

        let repo = tempfile::tempdir().expect("mixed-scope cache test repository");
        assert!(
            std::process::Command::new("git")
                .args(["init", "--quiet"])
                .current_dir(repo.path())
                .status()
                .expect("initialize repository")
                .success()
        );
        let state = Arc::new(
            pointbreak::session::change_reader_state_for_repo(repo.path())
                .expect("build one reusable test state"),
        );
        let cache = Arc::new(ChangeReaderCache::<u8, u16>::new());
        let warmed = cache
            .load_with(
                ChangeReaderLoad::Changes,
                || Ok(41),
                || Ok(Arc::clone(&state)),
                |_| Ok(Some(Arc::new(7))),
                |_, _, _| panic!("warming Changes must not build Timeline"),
            )
            .expect("warm one marker-stable Changes generation");
        let warmed_presentation = Arc::clone(
            warmed
                .presentation
                .as_ref()
                .expect("warm Changes presentation exists"),
        );

        let (timeline_started_tx, timeline_started_rx) = mpsc::channel();
        let (timeline_release_tx, timeline_release_rx) = mpsc::channel();
        let timeline_cache = Arc::clone(&cache);
        let timeline_handle = std::thread::spawn(move || {
            let trust_set = TrustSet::default();
            timeline_cache
                .load_with(
                    ChangeReaderLoad::Timeline(&trust_set),
                    || Ok(41),
                    || panic!("warm Timeline must not rebuild state"),
                    |_| panic!("warm Timeline must not rebuild Change presentation"),
                    |_, presentation, _| {
                        assert_eq!(*presentation, 7);
                        timeline_started_tx
                            .send(())
                            .expect("announce blocked Timeline construction");
                        timeline_release_rx
                            .recv()
                            .expect("release blocked Timeline construction");
                        Ok(Some(Arc::new(9)))
                    },
                )
                .expect("blocked Timeline builds from the warm generation")
        });
        timeline_started_rx
            .recv_timeout(Duration::from_secs(10))
            .expect("Timeline construction reached its deliberate gate");

        let (changes_done_tx, changes_done_rx) = mpsc::channel();
        let changes_cache = Arc::clone(&cache);
        let changes_handle = std::thread::spawn(move || {
            let generation = changes_cache
                .load_with(
                    ChangeReaderLoad::Changes,
                    || Ok(41),
                    || panic!("warm Changes hit must not rebuild state"),
                    |_| panic!("warm Changes hit must not rebuild presentation"),
                    |_, _, _| panic!("Changes hit must not build Timeline"),
                )
                .expect("Changes remains available during Timeline construction");
            changes_done_tx
                .send(generation.presentation)
                .expect("report completed Changes hit");
        });
        let concurrent_presentation = changes_done_rx
            .recv_timeout(Duration::from_secs(10))
            .expect("warm Changes hit must finish before Timeline is released")
            .expect("concurrent Changes presentation exists");
        assert!(Arc::ptr_eq(&warmed_presentation, &concurrent_presentation));

        timeline_release_tx
            .send(())
            .expect("release Timeline construction");
        changes_handle.join().expect("Changes cache-hit thread");
        let timeline = timeline_handle.join().expect("Timeline builder thread");
        assert_eq!(
            timeline.timeline.as_deref().copied(),
            Some(9),
            "Timeline publishes after the independent Changes hit"
        );
    }

    #[test]
    fn change_reader_cache_keeps_snapshot_when_marker_advances_during_timeline_projection() {
        use std::cell::Cell;

        let repo = tempfile::tempdir().expect("Timeline snapshot cache test repository");
        assert!(
            std::process::Command::new("git")
                .args(["init", "--quiet"])
                .current_dir(repo.path())
                .status()
                .expect("initialize repository")
                .success()
        );
        let state = Arc::new(
            pointbreak::session::change_reader_state_for_repo(repo.path())
                .expect("build one reusable test state"),
        );
        let cache = ChangeReaderCache::<u8, u16>::new();
        let current_marker = Cell::new(41_u64);
        let state_builds = Cell::new(0_usize);
        let presentation_builds = Cell::new(0_usize);
        let timeline_builds = Cell::new(0_usize);
        let trust_set = TrustSet::default();

        let captured = cache
            .load_with(
                ChangeReaderLoad::Timeline(&trust_set),
                || Ok(current_marker.get()),
                || {
                    state_builds.set(state_builds.get() + 1);
                    Ok(Arc::clone(&state))
                },
                |_| {
                    presentation_builds.set(presentation_builds.get() + 1);
                    Ok(Some(Arc::new(presentation_builds.get() as u8)))
                },
                |_, _, _| {
                    timeline_builds.set(timeline_builds.get() + 1);
                    current_marker.set(42);
                    Ok(Some(Arc::new(timeline_builds.get() as u16)))
                },
            )
            .expect("pure Timeline projection returns its captured generation");
        assert_eq!(
            state_builds.get(),
            1,
            "Timeline did not retry the state fold"
        );
        assert_eq!(presentation_builds.get(), 1);
        assert_eq!(timeline_builds.get(), 1);
        assert_eq!(captured.timeline.as_deref().copied(), Some(1));
        {
            let cached = cache.timeline_slot.lock().expect("Timeline cache slot");
            let cached = cached.as_ref().expect("captured Timeline is cached");
            assert_eq!(cached.marker, 41);
            assert_eq!(cached.timeline.as_deref().copied(), Some(1));
        }

        let advanced = cache
            .load_with(
                ChangeReaderLoad::Timeline(&trust_set),
                || Ok(current_marker.get()),
                || {
                    state_builds.set(state_builds.get() + 1);
                    Ok(Arc::clone(&state))
                },
                |_| {
                    presentation_builds.set(presentation_builds.get() + 1);
                    Ok(Some(Arc::new(presentation_builds.get() as u8)))
                },
                |_, _, _| {
                    timeline_builds.set(timeline_builds.get() + 1);
                    Ok(Some(Arc::new(timeline_builds.get() as u16)))
                },
            )
            .expect("next Timeline request advances to the new marker");
        assert_eq!(state_builds.get(), 2);
        assert_eq!(presentation_builds.get(), 2);
        assert_eq!(timeline_builds.get(), 2);
        assert_eq!(advanced.timeline.as_deref().copied(), Some(2));
        assert_eq!(
            cache
                .timeline_slot
                .lock()
                .expect("Timeline cache slot")
                .as_ref()
                .expect("advanced Timeline is cached")
                .marker,
            42
        );
    }

    #[test]
    fn change_reader_cache_older_timeline_cannot_overwrite_newer_cached_generation() {
        use std::sync::atomic::{AtomicU64, Ordering};
        use std::sync::{Arc, mpsc};
        use std::time::Duration;

        let repo = tempfile::tempdir().expect("Timeline generation-order test repository");
        assert!(
            std::process::Command::new("git")
                .args(["init", "--quiet"])
                .current_dir(repo.path())
                .status()
                .expect("initialize repository")
                .success()
        );
        let state = Arc::new(
            pointbreak::session::change_reader_state_for_repo(repo.path())
                .expect("build one reusable test state"),
        );
        let cache = Arc::new(ChangeReaderCache::<u8, u16>::new());
        let marker = Arc::new(AtomicU64::new(41));
        cache
            .load_with(
                ChangeReaderLoad::Changes,
                || Ok(marker.load(Ordering::SeqCst)),
                || Ok(Arc::clone(&state)),
                |_| Ok(Some(Arc::new(7))),
                |_, _, _| panic!("warming Changes must not build Timeline"),
            )
            .expect("warm generation N");

        let mut timeline_slot = cache.timeline_slot.lock().expect("hold Timeline slot");
        let (old_base_tx, old_base_rx) = mpsc::channel();
        let old_cache = Arc::clone(&cache);
        let old_marker = Arc::clone(&marker);
        let old_handle = std::thread::spawn(move || {
            let trust_set = TrustSet::default();
            let mut old_base_tx = Some(old_base_tx);
            old_cache
                .load_with(
                    ChangeReaderLoad::Timeline(&trust_set),
                    || {
                        let observed = old_marker.load(Ordering::SeqCst);
                        if let Some(sender) = old_base_tx.take() {
                            sender.send(observed).expect("announce generation N base");
                        }
                        Ok(observed)
                    },
                    || panic!("generation N is already warm"),
                    |_| panic!("generation N presentation is already warm"),
                    |_, _, _| Ok(Some(Arc::new(9))),
                )
                .expect("older Timeline still returns its coherent generation")
        });
        assert_eq!(
            old_base_rx
                .recv_timeout(Duration::from_secs(10))
                .expect("older Timeline captured its base before advancing"),
            41
        );

        marker.store(42, Ordering::SeqCst);
        cache
            .load_with(
                ChangeReaderLoad::Changes,
                || Ok(marker.load(Ordering::SeqCst)),
                || Ok(Arc::clone(&state)),
                |_| Ok(Some(Arc::new(8))),
                |_, _, _| panic!("advancing Changes must not build Timeline"),
            )
            .expect("publish generation N+1 while older Timeline waits");
        *timeline_slot = Some(CachedTimelineState {
            marker: 42,
            trust_set: TrustSet::default(),
            timeline: Some(Arc::new(99)),
        });
        drop(timeline_slot);

        let old = old_handle.join().expect("older Timeline thread");
        assert_eq!(old.timeline.as_deref().copied(), Some(9));
        let cached = cache.timeline_slot.lock().expect("Timeline cache slot");
        let cached = cached.as_ref().expect("newer Timeline remains cached");
        assert_eq!(cached.marker, 42);
        assert_eq!(cached.timeline.as_deref().copied(), Some(99));
    }

    #[test]
    fn change_reader_cache_remembers_same_trust_unavailable_timeline() {
        use std::cell::Cell;

        let repo = tempfile::tempdir().expect("unavailable cache test repository");
        assert!(
            std::process::Command::new("git")
                .args(["init", "--quiet"])
                .current_dir(repo.path())
                .status()
                .expect("initialize repository")
                .success()
        );
        let state = Arc::new(
            pointbreak::session::change_reader_state_for_repo(repo.path())
                .expect("build unavailable test state"),
        );
        let cache = ChangeReaderCache::<u8, u16>::new();
        let state_builds = Cell::new(0_usize);
        let presentation_builds = Cell::new(0_usize);
        let timeline_builds = Cell::new(0_usize);
        let trust_set = TrustSet::default();
        let first = cache
            .load_with(
                ChangeReaderLoad::Timeline(&trust_set),
                || Ok(17),
                || {
                    state_builds.set(state_builds.get() + 1);
                    Ok(Arc::clone(&state))
                },
                |_| {
                    presentation_builds.set(presentation_builds.get() + 1);
                    Ok(Some(Arc::new(1)))
                },
                |_, _, _| {
                    timeline_builds.set(timeline_builds.get() + 1);
                    Ok(None)
                },
            )
            .expect("first unavailable Timeline attempt is cached");
        assert!(first.presentation.is_some());
        assert!(first.timeline.is_none());

        let second = cache
            .load_with(
                ChangeReaderLoad::Timeline(&trust_set),
                || Ok(17),
                || panic!("same marker and trust must not rebuild state"),
                |_| panic!("cached unavailable Timeline must not rebuild presentation"),
                |_, _, _| panic!("cached unavailable Timeline must not rebuild"),
            )
            .expect("same trust returns cached unavailability");
        assert!(second.presentation.is_some());
        assert!(second.timeline.is_none());
        assert!(Arc::ptr_eq(&first.state, &second.state));
        assert_eq!(state_builds.get(), 1);
        assert_eq!(presentation_builds.get(), 1);
        assert_eq!(timeline_builds.get(), 1);
    }

    #[test]
    fn change_reader_cache_refuses_a_generation_that_moves_during_both_folds() {
        use std::cell::{Cell, RefCell};
        use std::collections::VecDeque;

        let repo = tempfile::tempdir().expect("moving cache test repository");
        assert!(
            std::process::Command::new("git")
                .args(["init", "--quiet"])
                .current_dir(repo.path())
                .status()
                .expect("initialize repository")
                .success()
        );

        let cache = ChangeReaderCache::<ChangeReaderPresentationV1>::new();
        let markers = RefCell::new(VecDeque::from([1_u64, 2, 2, 3]));
        let builds = Cell::new(0_usize);
        let moving = cache.load_with(
            ChangeReaderLoad::State,
            || {
                markers
                    .borrow_mut()
                    .pop_front()
                    .ok_or_else(|| ChangeReaderLoadError::Other("missing test marker".to_owned()))
            },
            || {
                builds.set(builds.get() + 1);
                pointbreak::session::change_reader_state_for_repo(repo.path())
                    .map(Arc::new)
                    .map_err(|error| ChangeReaderLoadError::Other(error.to_string()))
            },
            |_| panic!("a profile-only load must not build presentation"),
            |_, _, _| panic!("a profile-only load must not build Timeline"),
        );
        assert!(matches!(&moving, Err(ChangeReaderLoadError::MovingJournal)));
        assert_eq!(builds.get(), 2, "both bounded retries folded once");

        let stable_markers = RefCell::new(VecDeque::from([4_u64, 4]));
        let stable = cache
            .load_with(
                ChangeReaderLoad::State,
                || {
                    stable_markers.borrow_mut().pop_front().ok_or_else(|| {
                        ChangeReaderLoadError::Other("missing stable test marker".to_owned())
                    })
                },
                || {
                    builds.set(builds.get() + 1);
                    pointbreak::session::change_reader_state_for_repo(repo.path())
                        .map(Arc::new)
                        .map_err(|error| ChangeReaderLoadError::Other(error.to_string()))
                },
                |_| panic!("a profile-only load must not build presentation"),
                |_, _, _| panic!("a profile-only load must not build Timeline"),
            )
            .expect("a later stable generation is cacheable");
        assert_eq!(builds.get(), 3);

        let hit = cache
            .load_with(
                ChangeReaderLoad::State,
                || Ok(4),
                || panic!("stable cached generation should not rebuild"),
                |_| panic!("stable profile cache hit should not build presentation"),
                |_, _, _| panic!("stable profile cache hit should not build Timeline"),
            )
            .unwrap();
        assert!(Arc::ptr_eq(&stable.state, &hit.state));
    }

    #[test]
    fn change_reader_cache_refuses_real_appends_between_fold_and_after_marker() {
        use std::cell::Cell;

        let repo = tempfile::tempdir().expect("real moving cache test repository");
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

        let cache = ChangeReaderCache::<ChangeReaderPresentationV1>::new();
        let append_count = Cell::new(0_usize);
        let moving = cache.load_with(
            ChangeReaderLoad::State,
            || {
                pointbreak::session::change_reader_head_marker_for_repo(repo.path())
                    .map_err(|error| ChangeReaderLoadError::Other(error.to_string()))
            },
            || {
                let state = pointbreak::session::change_reader_state_for_repo(repo.path())
                    .map(Arc::new)
                    .map_err(|error| ChangeReaderLoadError::Other(error.to_string()))?;
                let generation = append_count.get() + 1;
                append_count.set(generation);
                std::fs::write(
                    repo.path().join("sample.txt"),
                    format!("generation {generation}\n"),
                )
                .map_err(|error| ChangeReaderLoadError::Other(error.to_string()))?;
                pointbreak::session::capture_worktree_review(
                    pointbreak::session::CaptureOptions::new(repo.path()),
                )
                .map_err(|error| ChangeReaderLoadError::Other(error.to_string()))?;
                Ok(state)
            },
            |_| panic!("a profile-only load must not build presentation"),
            |_, _, _| panic!("a profile-only load must not build Timeline"),
        );

        assert!(matches!(moving, Err(ChangeReaderLoadError::MovingJournal)));
        assert_eq!(
            append_count.get(),
            2,
            "each bounded fold was invalidated by a real journal append"
        );

        let signer = super::super::page_token::PageTokenSigner::from_seed([7_u8; 32]);
        let request = super::super::event_history_page::parse_signed(None, &signer)
            .expect("bare Timeline request is valid");
        let retryable = api::authoritative_event_history_v2_from_loaded(
            repo.path(),
            request,
            &signer,
            None,
            &pointbreak::session::TrustSet::default(),
            moving,
        )
        .expect("moving journal is a typed Timeline response");
        let api::ChangeV2Json::Retryable(body) = retryable else {
            panic!("a real append race must remain retryable")
        };
        let body: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert_eq!(body["schema"], "pointbreak.inspect-event-history-error");
        assert_eq!(body["code"], "moving_journal");
        assert_eq!(body["retryable"], true);
        assert_eq!(
            change_v2_response(Ok(api::ChangeV2Json::Retryable(body.to_string()))).status,
            "503 Service Unavailable"
        );
    }

    #[test]
    fn legacy_semantic_routes_refuse_l0_before_partial_payload() {
        let capability = capability(pointbreak::session::StoreCapabilityStatus::MigrationRequired);
        let response = legacy_semantic_gate(&capability).unwrap();
        assert_eq!(response.status, "409 Conflict");
        let value: serde_json::Value = serde_json::from_slice(&response.body).unwrap();
        assert_eq!(value["schema"], "pointbreak.store-migration-required");
        assert_eq!(value["state"], "migration_required");
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
