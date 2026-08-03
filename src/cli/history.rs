use std::collections::BTreeSet;
use std::io::Write;
use std::path::PathBuf;
use std::time::Duration;

use clap::{Args, ValueEnum};
use pointbreak::documents::{derived_history_document, history_document};
use pointbreak::model::RevisionId;
use pointbreak::session::event::EventType;
use pointbreak::session::{
    BaseProjectionConfig, DerivedHistoryAccess, DerivedHistoryRoute, EventVerificationPolicy,
    HistoryCursor, HistoryOrder, HistoryPage, HistoryQuery, LivenessToken, RefFilterMode,
    ReviewHistoryEntry, ReviewHistoryFilters, ReviewHistoryOptions, ReviewHistoryResult,
    ReviewHistorySummary, read_events_for_display, redact_history_bodies, review_history,
    validated_track_id,
};

use crate::cli::common::{clamp_title, count_label, endpoint_label, wire_label};
use crate::cli::output;

/// List the durable review event history, optionally filtered.
#[derive(Debug, Args)]
pub(super) struct HistoryArgs {
    /// Repository root or a path inside the repository.
    #[arg(long, default_value = ".")]
    repo: PathBuf,

    /// Filter to one captured Revision by id.
    #[arg(long)]
    revision: Option<String>,

    /// Filter to one review track, such as agent:codex.
    #[arg(long)]
    track: Option<String>,

    /// Filter with the review filter grammar (e.g. `type:assessment tag:issue:191`).
    /// Applies before --limit/--cursor windowing and composes with the typed flags.
    #[arg(long)]
    filter: Option<String>,

    /// Filter to one or more durable event types.
    #[arg(long = "event-type")]
    event_types: Vec<HistoryEventTypeArg>,

    /// Filter to events of revisions associated with this ref; a short branch name is
    /// normalized to its full ref before matching.
    #[arg(long = "ref", alias = "branch")]
    ref_name: Option<String>,

    /// How `--ref` matches: by the recorded label (offline) or by reachability
    /// from the ref's live tip.
    #[arg(long, value_enum, default_value = "label")]
    by: HistoryRefByArg,

    /// Hydrate body-like text from inline payloads or body artifacts.
    #[arg(long)]
    include_body: bool,

    /// Return at most N entries (a forward page from the start, or from --cursor);
    /// omit for the full history. For the last N, use `--tail N`. The response
    /// carries a `nextCursor` to continue. With --watch, the same page is
    /// re-rendered on each liveness change.
    #[arg(long)]
    limit: Option<usize>,

    /// Print the newest N entries, ordered oldest to newest for append-friendly output.
    /// Composes with all filters and with --watch; cannot be used with --limit/--cursor.
    #[arg(long, value_name = "N", conflicts_with_all = ["limit", "cursor"])]
    tail: Option<usize>,

    /// Continue from a previous response's opaque `nextCursor`. Omit to start from
    /// the beginning.
    #[arg(long)]
    cursor: Option<String>,

    /// Re-render whenever the store's liveness changes, polling client-side.
    /// Pull-only: no daemon and no filesystem watch. Cancel with Ctrl-C.
    #[arg(long)]
    watch: bool,

    /// Poll interval in milliseconds for `--watch`.
    #[arg(long, default_value_t = 3000)]
    poll_ms: u64,

    #[command(flatten)]
    format_args: output::FormatArgs,
}

#[derive(Clone, Copy, Debug, ValueEnum)]
#[value(rename_all = "kebab-case")]
enum HistoryEventTypeArg {
    ReviewInitialized,
    RevisionCaptured,
    ReviewObservationRecorded,
    ReviewAssessmentRecorded,
    ValidationCheckRecorded,
    InputRequestOpened,
    InputRequestResponded,
    RevisionRefAssociated,
    RevisionRefWithdrawn,
    RevisionCommitAssociated,
    RevisionCommitWithdrawn,
}

#[derive(Clone, Copy, Debug, Default, ValueEnum)]
#[value(rename_all = "kebab-case")]
enum HistoryRefByArg {
    #[default]
    Label,
    Liveness,
}

impl From<HistoryRefByArg> for RefFilterMode {
    fn from(by: HistoryRefByArg) -> Self {
        match by {
            HistoryRefByArg::Label => RefFilterMode::Label,
            HistoryRefByArg::Liveness => RefFilterMode::Liveness,
        }
    }
}

pub(super) fn run(
    args: HistoryArgs,
    stdout: &mut dyn Write,
) -> Result<(), Box<dyn std::error::Error>> {
    let span = tracing::info_span!("shore.review.history");
    let _entered = span.enter();
    tracing::debug!(command = "review.history", "command_start");
    if args.watch {
        return watch(&args, stdout);
    }
    render_once(&args, stdout)
}

fn render_once(
    args: &HistoryArgs,
    stdout: &mut dyn Write,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut routed = read_history(args)?;
    let result = routed.result_mut();
    for notice in &result.query_notices {
        eprintln!("hint: {}", notice.message);
    }
    if present_reverse(args) {
        result.entries.reverse();
    }
    let format = output::resolve_format(args.format_args.explicit(), output::OutputFormat::Json)?;
    // `history_document` consumes the result by value; render the digest up
    // front on the text lane only, so the machine lanes pay nothing extra.
    let text =
        matches!(format.format, output::OutputFormat::Text).then(|| render_history_text(result));
    match routed {
        RoutedHistory::Authoritative(result) => {
            let document = history_document(result);
            output::write_document(stdout, format, &document, || {
                text.expect("text lane resolves the digest source")
            })
        }
        RoutedHistory::Derived {
            result,
            projection_stamp,
        } => {
            let document = derived_history_document(result, projection_stamp);
            output::write_document(stdout, format, &document, || {
                text.expect("text lane resolves the digest source")
            })
        }
    }
}

enum RoutedHistory {
    Authoritative(ReviewHistoryResult),
    Derived {
        result: ReviewHistoryResult,
        projection_stamp: String,
    },
}

impl RoutedHistory {
    fn result_mut(&mut self) -> &mut ReviewHistoryResult {
        match self {
            Self::Authoritative(result) | Self::Derived { result, .. } => result,
        }
    }
}

fn read_history(args: &HistoryArgs) -> Result<RoutedHistory, Box<dyn std::error::Error>> {
    if !eligible_for_derived_history(args) {
        return Ok(RoutedHistory::Authoritative(review_history(
            history_options(args)?,
        )?));
    }

    let revision_id = args
        .revision
        .as_deref()
        .map(|revision| {
            crate::cli::id_resolver::IdResolver::new(&args.repo)
                .rev(revision)
                .map(RevisionId::new)
        })
        .transpose()?;
    let track_id = args.track.as_deref().map(validated_track_id).transpose()?;
    let event_types = args
        .event_types
        .iter()
        .copied()
        .map(EventType::from)
        .collect::<Vec<_>>();
    let query = HistoryQuery {
        q: String::new(),
        track: track_id.as_ref().map(|track| track.as_str().to_owned()),
        snapshot: None,
        revision: revision_id.clone(),
        revisions: None,
        types: (!event_types.is_empty()).then(|| {
            event_types
                .iter()
                .map(|event_type| event_type.as_str().to_owned())
                .collect::<BTreeSet<_>>()
        }),
        order: effective_order(args),
    };
    let page = HistoryPage {
        limit: effective_limit(args),
        after: args
            .cursor
            .as_deref()
            .map(HistoryCursor::decode)
            .transpose()
            .map_err(|_| "invalid --cursor: pass an opaque nextCursor from a prior response")?,
        offset: None,
        at: None,
    };
    let config = history_projection_config(&args.repo);
    let access = DerivedHistoryAccess::resolve(&args.repo).map_err(std::io::Error::other)?;
    match access
        .history(&query, &page, &config)
        .map_err(std::io::Error::other)?
    {
        DerivedHistoryRoute::Ready(mut derived) => {
            let next_cursor = (query.order == HistoryOrder::Asc
                && !derived.entries.is_empty()
                && derived.offset.saturating_add(derived.entries.len()) < derived.match_count)
                .then(|| {
                    let last = derived.entries.last().expect("nonempty page");
                    HistoryCursor {
                        occurred_at: last.occurred_at.clone(),
                        event_id: last.event_id.clone(),
                    }
                    .encode()
                });
            if !args.include_body {
                redact_history_bodies(&mut derived.entries);
            }
            Ok(RoutedHistory::Derived {
                result: ReviewHistoryResult {
                    event_set_hash: String::new(),
                    event_count: derived.event_count,
                    filters: ReviewHistoryFilters {
                        revision_id,
                        track_id,
                        event_types,
                        include_body: args.include_body,
                    },
                    entries: derived.entries,
                    next_cursor,
                    query_notices: derived.query_notices,
                    diagnostics: derived.diagnostics,
                },
                projection_stamp: derived.projection_stamp,
            })
        }
        DerivedHistoryRoute::Off if !access.is_active() => Ok(RoutedHistory::Authoritative(
            review_history(history_options(args)?)?,
        )),
        DerivedHistoryRoute::Off | DerivedHistoryRoute::Unavailable(_) => {
            crate::cli::derived_read::emit_authoritative_fallback_hint(&args.repo);
            Ok(RoutedHistory::Authoritative(review_history(
                history_options(args)?,
            )?))
        }
        DerivedHistoryRoute::ExhaustiveSearchFallback => Ok(RoutedHistory::Authoritative(
            review_history(history_options(args)?)?,
        )),
    }
}

fn eligible_for_derived_history(args: &HistoryArgs) -> bool {
    effective_limit(args).is_some()
        && !args.watch
        && args.ref_name.is_none()
        && args
            .filter
            .as_deref()
            .is_none_or(|filter| filter.trim().is_empty())
}

fn history_projection_config(repo: &std::path::Path) -> BaseProjectionConfig {
    BaseProjectionConfig {
        verification_policy: Some(EventVerificationPolicy::advisory()),
        trust_set: crate::cli::common::discover_trust_set(repo),
        actor_attributes: crate::cli::common::discover_actor_attributes(repo),
        delegation_map: crate::cli::common::discover_delegation_map(repo),
        removal_policy: pointbreak::session::RemovalPolicy::default(),
    }
}

/// Bespoke text lane for `history`: a count headline naming the active filters,
/// then one scannable line per event — timestamp, event label, track, and the
/// fact headline — plus a continuation line when a window left more behind. An
/// empty listing renders a `no events` line, never silence.
fn render_history_text(result: &ReviewHistoryResult) -> String {
    let filters = &result.filters;
    let mut scope = String::new();
    if let Some(revision_id) = &filters.revision_id {
        scope.push_str(&format!(
            " · revision {}",
            output::short_ref(revision_id.as_str())
        ));
    }
    if let Some(track_id) = &filters.track_id {
        scope.push_str(&format!(" · track {}", track_id.as_str()));
    }
    if !filters.event_types.is_empty() {
        scope.push_str(&format!(
            " · {}",
            count_label(
                filters.event_types.len(),
                "event-type filter",
                "event-type filters"
            )
        ));
    }
    if result.entries.is_empty() {
        return format!("no events{scope}");
    }
    let mut lines = vec![format!(
        "{}{scope}:",
        count_label(result.entries.len(), "event", "events")
    )];
    for entry in &result.entries {
        lines.push(format!(
            "  {} · {}",
            entry.occurred_at,
            history_entry_label(entry)
        ));
    }
    if result.next_cursor.is_some() {
        lines.push("… more events remain (continue with --cursor)".to_owned());
    }
    lines.join("\n")
}

/// One-line label for a history entry: the event kind, the writing track when
/// present, and the entry's own headline (title, check name, call, or ids).
fn history_entry_label(entry: &ReviewHistoryEntry) -> String {
    let track = entry
        .track_id
        .as_ref()
        .map(|track_id| format!(" · {}", track_id.as_str()))
        .unwrap_or_default();
    match &entry.summary {
        ReviewHistorySummary::ReviewInitialized {} => format!("review initialized{track}"),
        ReviewHistorySummary::RevisionCaptured {
            revision_id,
            summary,
            base,
            target,
            ..
        } => {
            let mut label = format!("captured {}", output::short_ref(revision_id.as_str()));
            if let Some(summary) = summary {
                label.push_str(&format!(" · \"{}\"", clamp_title(summary)));
            }
            if let (Some(base), Some(target)) = (base, target) {
                label.push_str(&format!(
                    " · base {} → {}",
                    endpoint_label(base),
                    endpoint_label(target)
                ));
            }
            label
        }
        ReviewHistorySummary::ReviewObservationRecorded {
            observation_id,
            title,
            ..
        } => format!(
            "observation {}{track} · \"{}\"",
            output::short_ref(observation_id.as_str()),
            clamp_title(title)
        ),
        ReviewHistorySummary::InputRequestOpened {
            input_request_id,
            mode,
            title,
            ..
        } => format!(
            "input request {} ({}){track} · \"{}\"",
            output::short_ref(input_request_id.as_str()),
            wire_label(mode),
            clamp_title(title)
        ),
        ReviewHistorySummary::InputRequestResponded {
            input_request_id,
            outcome,
            ..
        } => format!(
            "response {} to {}{track}",
            wire_label(outcome),
            output::short_ref(input_request_id.as_str())
        ),
        ReviewHistorySummary::ReviewAssessmentRecorded {
            assessment_id,
            assessment,
            ..
        } => format!(
            "assessment {}{track} · {}",
            output::short_ref(assessment_id.as_str()),
            wire_label(assessment)
        ),
        ReviewHistorySummary::ReviewNoteImported {} => format!("note imported (retired){track}"),
        ReviewHistorySummary::ValidationCheckRecorded {
            validation_check_id,
            check_name,
            status,
            ..
        } => format!(
            "validation {}{track} · {} {}",
            output::short_ref(validation_check_id.as_str()),
            check_name,
            wire_label(status)
        ),
        ReviewHistorySummary::RevisionRefAssociated {
            ref_name, head_oid, ..
        } => format!(
            "ref associated{track} · {} @ {}",
            ref_name,
            output::short_ref(head_oid)
        ),
        ReviewHistorySummary::RevisionRefWithdrawn {
            ref_association_id, ..
        } => format!(
            "ref withdrawn{track} · {}",
            output::short_ref(ref_association_id.as_str())
        ),
        ReviewHistorySummary::RevisionCommitAssociated { commit_oid, .. } => format!(
            "commit associated{track} · {}",
            output::short_ref(commit_oid)
        ),
        ReviewHistorySummary::RevisionCommitWithdrawn {
            commit_association_id,
            ..
        } => format!(
            "commit withdrawn{track} · {}",
            output::short_ref(commit_association_id.as_str())
        ),
    }
}

/// Client-side liveness poll: re-render only when the store's liveness moves —
/// either its decodable `event_set_hash` changes or the number of skip
/// diagnostics changes — never on a bare tick. The second trigger matters because
/// a lone retired event is skipped from the event set, so it would leave the hash
/// untouched; folding the diagnostic count in surfaces it. The core emits the
/// change fact but never delivers it, so this loop owns delivery — a pure pull
/// with no daemon and no filesystem watch. Cancel with Ctrl-C.
fn watch(args: &HistoryArgs, stdout: &mut dyn Write) -> Result<(), Box<dyn std::error::Error>> {
    let interval = Duration::from_millis(args.poll_ms);
    let mut last_seen: Option<(String, usize)> = None;
    loop {
        let fingerprint = watch_fingerprint(&args.repo)?;
        if last_seen.as_ref() != Some(&fingerprint) {
            render_once(args, stdout)?;
            stdout.flush()?;
            last_seen = Some(fingerprint);
        }
        std::thread::sleep(interval);
    }
}

/// The poll fingerprint: the decodable-event-set hash paired with the
/// skip-diagnostic count. Either component moving means the rendered history
/// would change, so `--watch` re-renders when the pair does.
fn watch_fingerprint(
    repo: &std::path::Path,
) -> Result<(String, usize), Box<dyn std::error::Error>> {
    let (events, diagnostics) = read_events_for_display(repo)?;
    let token = LivenessToken::for_journal(&events)?;
    Ok((token.event_set_hash, diagnostics.len()))
}

fn history_options(args: &HistoryArgs) -> Result<ReviewHistoryOptions, Box<dyn std::error::Error>> {
    let mut options = ReviewHistoryOptions::new(&args.repo)
        .with_include_body(args.include_body)
        .with_read_for_display(true)
        .with_order(effective_order(args));
    if let Some(limit) = effective_limit(args) {
        options = options.with_limit(limit);
    }
    if let Some(token) = &args.cursor {
        let cursor = HistoryCursor::decode(token)
            .map_err(|_| "invalid --cursor: pass an opaque nextCursor from a prior response")?;
        options = options.with_cursor(cursor);
    }
    if let Some(filter) = &args.filter {
        options = options.with_filter(filter.clone());
    }
    if let Some(revision) = &args.revision {
        let ids = crate::cli::id_resolver::IdResolver::new(&args.repo);
        options = options.with_revision_id(RevisionId::new(ids.rev(revision)?));
    }
    if let Some(track) = &args.track {
        options = options.with_track(track.clone());
    }
    for event_type in args.event_types.iter().copied() {
        options = options.with_event_type(event_type.into());
    }
    if let Some(ref_name) = &args.ref_name {
        options = options.with_ref_filter(ref_name.clone(), args.by.into());
    }
    if let Some(map) = crate::cli::common::discover_delegation_map(&args.repo) {
        options = options.with_delegation_map(map);
    }
    // Advisory policy: presence enables the verificationStatus render, never gates a write.
    options = options.with_trust_set(crate::cli::common::discover_trust_set(&args.repo));
    options = options.with_verification_policy(EventVerificationPolicy::advisory());
    // Sibling enrichment for endorsement readbacks (endorser kind/roles), reader-relative.
    options =
        options.with_actor_attributes(crate::cli::common::discover_actor_attributes(&args.repo));
    Ok(options)
}

fn effective_order(args: &HistoryArgs) -> HistoryOrder {
    if args.tail.is_some() {
        HistoryOrder::Desc
    } else {
        HistoryOrder::Asc
    }
}

fn effective_limit(args: &HistoryArgs) -> Option<usize> {
    args.tail.or(args.limit)
}

fn present_reverse(args: &HistoryArgs) -> bool {
    args.tail.is_some()
}

impl From<HistoryEventTypeArg> for EventType {
    fn from(value: HistoryEventTypeArg) -> Self {
        match value {
            HistoryEventTypeArg::ReviewInitialized => Self::ReviewInitialized,
            HistoryEventTypeArg::RevisionCaptured => Self::WorkObjectProposed,
            HistoryEventTypeArg::ReviewObservationRecorded => Self::ReviewObservationRecorded,
            HistoryEventTypeArg::ReviewAssessmentRecorded => Self::ReviewAssessmentRecorded,
            HistoryEventTypeArg::ValidationCheckRecorded => Self::ValidationCheckRecorded,
            HistoryEventTypeArg::InputRequestOpened => Self::InputRequestOpened,
            HistoryEventTypeArg::InputRequestResponded => Self::InputRequestResponded,
            HistoryEventTypeArg::RevisionRefAssociated => Self::RevisionRefAssociated,
            HistoryEventTypeArg::RevisionRefWithdrawn => Self::RevisionRefWithdrawn,
            HistoryEventTypeArg::RevisionCommitAssociated => Self::RevisionCommitAssociated,
            HistoryEventTypeArg::RevisionCommitWithdrawn => Self::RevisionCommitWithdrawn,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn watch_fingerprint_changes_when_only_a_retired_event_appears() {
        let repo = tempfile::tempdir().unwrap();
        std::process::Command::new("git")
            .args(["init"])
            .current_dir(repo.path())
            .output()
            .unwrap();

        let before = watch_fingerprint(repo.path()).unwrap();

        // Drop a raw retired-type event into the resolved store; no valid-event
        // change, so only the skip-diagnostic count moves.
        let events_dir = pointbreak::session::store_dir_for_repo(repo.path())
            .unwrap()
            .join("events");
        std::fs::create_dir_all(&events_dir).unwrap();
        std::fs::write(
            events_dir.join(format!("{}.json", "a".repeat(64))),
            br#"{"eventType":"review_disposition_recorded"}"#,
        )
        .unwrap();

        let after = watch_fingerprint(repo.path()).unwrap();

        assert_ne!(
            before, after,
            "a new retired event must move the watch fingerprint"
        );
        assert_eq!(
            after.1,
            before.1 + 1,
            "the skip-diagnostic count increments"
        );
    }
}
