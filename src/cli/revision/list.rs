use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use clap::{Args, ValueEnum};
use pointbreak::documents::{
    derived_revision_list_page_document, revision_list_document, revision_list_page_document,
};
use pointbreak::session::{
    AUTHORITATIVE_REVISION_PAGE_PROFILE, DerivedHistoryAccess, DerivedRevisionPageRoute,
    QueryDiagnosticCode, QuerySurface, RefFilterMode, RevisionListOptions,
    RevisionOverviewsOptions, RevisionPageCursor, RevisionPageRequest, RevisionPageRequestError,
    RevisionRecordInputs, SnapshotSummaryCache, SupersessionView, UnreachableVisibility,
    build_revision_search_record, list_revisions, matches_query, parse_event_instant,
    parse_search_query_for, read_events_for_display, revision_supersession_classification,
    show_revision_overviews,
};
use sha2::{Digest, Sha256};

use crate::cli::common::{count_label, endpoint_label};
use crate::cli::output;

/// List captured revisions.
#[derive(Debug, Args)]
pub(super) struct RevisionListArgs {
    /// Repository root or a path inside the repository.
    #[arg(long, default_value = ".")]
    repo: PathBuf,

    /// List only the revisions sharing this content object id (a listing lens —
    /// coincident content may span threads; never a head-selector).
    #[arg(long)]
    object: Option<String>,

    /// Filter with the review filter grammar (e.g. `is:superseded tag:state-change`).
    /// When set, builds per-revision overviews + supersession classification for the
    /// listed revisions; the flagless listing pays no such cost.
    #[arg(long)]
    filter: Option<String>,

    /// Filter to revisions associated with this ref; a short branch name is
    /// normalized to its full ref before matching.
    #[arg(long = "ref", alias = "branch")]
    ref_name: Option<String>,

    /// How `--ref` matches: by the recorded label (offline) or by reachability
    /// from the ref's live tip.
    #[arg(long, value_enum, default_value = "label")]
    by: RefFilterByArg,

    /// Show every recorded revision. This is the default; the flag is retained
    /// for command compatibility.
    #[arg(long = "all")]
    _all: bool,

    /// Show only unreachable revisions (no live ref reaches any anchored
    /// commit; missing objects count). Takes precedence over `--all`.
    /// `--orphans` is a deprecated alias.
    #[arg(long, alias = "orphans")]
    unreachable: bool,

    /// Reachability target for the "merged" status: a revision is merged only when
    /// an ancestor of this ref (equality counts). Defaults to the repository's
    /// detected default branch (`origin/HEAD`, else local `main`/`master`), so the
    /// status answers "did this land on the default branch?"; when no default
    /// branch is detected it falls back to broad reachability (any live tip).
    #[arg(long = "integration-ref")]
    integration_ref: Option<String>,

    /// Scope the listing to captures belonging to the worktree at this path.
    #[arg(long)]
    worktree: Option<PathBuf>,

    /// Return at most N captured revisions in newest-first order. This explicit
    /// bounded mode emits a nextCursor when another page remains.
    #[arg(long, value_name = "N")]
    limit: Option<usize>,

    /// Continue a bounded listing from an opaque nextCursor. If the selected
    /// projection changes, retry without this cursor.
    #[arg(long, requires = "limit")]
    cursor: Option<String>,

    #[command(flatten)]
    format_args: output::FormatArgs,
}

#[derive(Clone, Copy, Debug, Default, ValueEnum)]
#[value(rename_all = "kebab-case")]
enum RefFilterByArg {
    #[default]
    Label,
    Liveness,
}

impl From<RefFilterByArg> for RefFilterMode {
    fn from(by: RefFilterByArg) -> Self {
        match by {
            RefFilterByArg::Label => RefFilterMode::Label,
            RefFilterByArg::Liveness => RefFilterMode::Liveness,
        }
    }
}

pub(super) fn run(
    args: RevisionListArgs,
    stdout: &mut dyn Write,
) -> Result<(), Box<dyn std::error::Error>> {
    let span = tracing::info_span!("shore.revision.list");
    let _entered = span.enter();
    tracing::debug!(command = "revision.list", "command_start");

    if args.limit.is_some() {
        return run_bounded(args, stdout);
    }
    let result = authoritative_result(&args, true)?;

    let format = output::resolve_format(args.format_args.explicit(), output::OutputFormat::Json)?;
    // `revision_list_document` consumes the result by value; render the digest
    // up front on the text lane only, so the machine lanes pay nothing extra.
    let text = matches!(format.format, output::OutputFormat::Text)
        .then(|| render_revision_list_text(&result, None));
    let document = revision_list_document(result);
    output::write_document(stdout, format, &document, || {
        text.expect("text lane resolves the digest source")
    })
}

enum RoutedRevisionPage {
    Authoritative {
        result: pointbreak::session::RevisionListResult,
        next: Option<String>,
    },
    Derived {
        result: pointbreak::session::RevisionListResult,
        projection_stamp: String,
        next: Option<String>,
    },
}

impl RoutedRevisionPage {
    fn result(&self) -> &pointbreak::session::RevisionListResult {
        match self {
            Self::Authoritative { result, .. } | Self::Derived { result, .. } => result,
        }
    }

    fn next(&self) -> Option<&str> {
        match self {
            Self::Authoritative { next, .. } | Self::Derived { next, .. } => next.as_deref(),
        }
    }
}

fn eligible_for_bounded_route(args: &RevisionListArgs) -> bool {
    args.limit.is_some()
        && args.object.is_none()
        && args.ref_name.is_none()
        && args
            .filter
            .as_deref()
            .is_none_or(|filter| filter.trim().is_empty())
        && !args.unreachable
        && args.integration_ref.is_none()
        && args.worktree.is_none()
}

fn run_bounded(
    args: RevisionListArgs,
    stdout: &mut dyn Write,
) -> Result<(), Box<dyn std::error::Error>> {
    let limit = args.limit.expect("bounded route has a limit");
    if !(1..=500).contains(&limit) {
        return Err("--limit must be between 1 and 500".into());
    }
    let request = RevisionPageRequest::new(Some(limit), args.cursor.as_deref())
        .map_err(|_| "invalid --cursor: pass an opaque nextCursor from a prior response")?;
    let routed = read_bounded_page(&args, &request)?;
    let format = output::resolve_format(args.format_args.explicit(), output::OutputFormat::Json)?;
    let text = matches!(format.format, output::OutputFormat::Text)
        .then(|| render_revision_list_text(routed.result(), routed.next()));
    match routed {
        RoutedRevisionPage::Authoritative { result, next } => {
            let document = revision_list_page_document(result, next);
            output::write_document(stdout, format, &document, || {
                text.expect("text lane resolves the digest source")
            })
        }
        RoutedRevisionPage::Derived {
            result,
            projection_stamp,
            next,
        } => {
            let document = derived_revision_list_page_document(result, projection_stamp, next);
            output::write_document(stdout, format, &document, || {
                text.expect("text lane resolves the digest source")
            })
        }
    }
}

fn read_bounded_page(
    args: &RevisionListArgs,
    request: &RevisionPageRequest,
) -> Result<RoutedRevisionPage, Box<dyn std::error::Error>> {
    if !eligible_for_bounded_route(args) {
        return authoritative_page(args, request);
    }
    let repo = &args.repo;
    let access = DerivedHistoryAccess::resolve(repo).map_err(std::io::Error::other)?;
    let trust_set = crate::cli::common::discover_trust_set(repo);
    match access
        .revisions_page(
            repo,
            trust_set,
            Arc::new(SnapshotSummaryCache::new()),
            request,
        )
        .map_err(std::io::Error::other)?
    {
        DerivedRevisionPageRoute::Ready(page) => Ok(RoutedRevisionPage::Derived {
            result: page.result,
            projection_stamp: page.projection_stamp,
            next: page.next,
        }),
        DerivedRevisionPageRoute::RestartRequired => Err(revision_page_restart_error()),
        DerivedRevisionPageRoute::Off if !access.is_active() => authoritative_page(args, request),
        DerivedRevisionPageRoute::Off | DerivedRevisionPageRoute::Unavailable(_) => {
            crate::cli::derived_read::emit_authoritative_fallback_hint(repo);
            authoritative_page(args, request)
        }
    }
}

fn authoritative_page(
    args: &RevisionListArgs,
    request: &RevisionPageRequest,
) -> Result<RoutedRevisionPage, Box<dyn std::error::Error>> {
    let mut result = authoritative_result(args, false)?;
    let normalized = result
        .entries
        .iter()
        .map(|entry| {
            parse_event_instant(&entry.captured_at)
                .map(|instant| (entry.revision_id.clone(), instant))
                .ok_or_else(|| {
                    format!(
                        "captured revision {} has an invalid instant",
                        entry.revision_id.as_str()
                    )
                })
        })
        .collect::<Result<std::collections::BTreeMap<_, _>, _>>()?;
    result.entries.sort_by(|left, right| {
        normalized[&right.revision_id]
            .cmp(&normalized[&left.revision_id])
            .then_with(|| right.revision_id.as_str().cmp(left.revision_id.as_str()))
    });
    let snapshot = authoritative_cursor_snapshot(args, &result.event_set_hash);
    let cursor = match request.cursor(AUTHORITATIVE_REVISION_PAGE_PROFILE, &snapshot) {
        Ok(cursor) => cursor,
        Err(RevisionPageRequestError::RestartRequired) => {
            return Err(revision_page_restart_error());
        }
        Err(RevisionPageRequestError::InvalidRequest) => {
            return Err("invalid revision page cursor".into());
        }
    };
    let start = cursor.as_ref().map_or(0, |cursor| {
        result.entries.partition_point(|entry| {
            let captured_at = normalized[&entry.revision_id];
            captured_at > cursor.captured_at_millis
                || (captured_at == cursor.captured_at_millis
                    && entry.revision_id.as_str() >= cursor.revision_id.as_str())
        })
    });
    let mut entries = result.entries.split_off(start.min(result.entries.len()));
    let has_more = entries.len() > request.limit();
    entries.truncate(request.limit());
    let next = if has_more {
        let entry = entries.last().expect("nonempty bounded page");
        Some(request.next(
            AUTHORITATIVE_REVISION_PAGE_PROFILE,
            &snapshot,
            &RevisionPageCursor {
                captured_at_millis: normalized[&entry.revision_id],
                revision_id: entry.revision_id.clone(),
            },
        ))
    } else {
        None
    };
    result.entries = entries;
    Ok(RoutedRevisionPage::Authoritative { result, next })
}

fn authoritative_cursor_snapshot(args: &RevisionListArgs, event_set_hash: &str) -> String {
    // The token must not be reusable with a different authoritative selector:
    // event-set freshness alone cannot distinguish two filters over the same
    // journal. Hash the local request shape so potentially sensitive filter or
    // path text never appears in the otherwise-decodable opaque token.
    let selector = serde_json::json!({
        "eventSetHash": event_set_hash,
        "object": args.object,
        "filter": args.filter,
        "ref": args.ref_name,
        "by": match args.by {
            RefFilterByArg::Label => "label",
            RefFilterByArg::Liveness => "liveness",
        },
        "unreachable": args.unreachable,
        "integrationRef": args.integration_ref,
        "worktree": args.worktree.as_ref().map(|path| path.to_string_lossy()),
    });
    let digest = Sha256::digest(
        serde_json::to_vec(&selector)
            .expect("selector JSON")
            .as_slice(),
    );
    let mut encoded = String::with_capacity(64);
    for byte in digest {
        use std::fmt::Write as _;
        write!(encoded, "{byte:02x}").expect("write to string");
    }
    format!("sha256:{encoded}")
}

fn authoritative_result(
    args: &RevisionListArgs,
    group_shared_commits: bool,
) -> Result<pointbreak::session::RevisionListResult, Box<dyn std::error::Error>> {
    let ids = crate::cli::id_resolver::IdResolver::new(&args.repo);
    let object = args
        .object
        .as_deref()
        .map(|object| ids.object(object))
        .transpose()?;
    let mut options = RevisionListOptions::new(&args.repo)
        .with_read_for_display(true)
        .with_group_shared_commits(group_shared_commits);
    if let Some(ref_name) = &args.ref_name {
        options = options.with_ref_filter(ref_name.clone(), args.by.into());
    }
    // `--all` is the default and remains accepted for compatibility.
    // `--unreachable` (deprecated alias `--orphans`) is the only CLI
    // reachability filter and keeps precedence when both flags are supplied.
    let visibility = if args.unreachable {
        UnreachableVisibility::UnreachableOnly
    } else {
        UnreachableVisibility::All
    };
    options = options.with_unreachable_visibility(visibility);
    if let Some(integration_ref) = &args.integration_ref {
        options = options.with_integration_ref(integration_ref.clone());
    }
    if let Some(worktree) = &args.worktree {
        options = options.with_worktree_scope(worktree);
    }
    let mut result = list_revisions(options)?;
    // `--object` is a listing lens: filter to revisions over the same content
    // object id (coincident content, which may span threads). It never resolves a
    // head and never force-disambiguates.
    if let Some(object) = object.as_deref() {
        result
            .entries
            .retain(|entry| entry.object_id.as_str() == object);
        result.revision_count = result.entries.len();
    }
    apply_revision_filter(&args.repo, args.filter.as_deref(), &mut result)?;
    Ok(result)
}

fn revision_page_restart_error() -> Box<dyn std::error::Error> {
    "revision page changed; retry without --cursor".into()
}

/// Bespoke text lane for `revision list`: a count headline, then one scannable
/// line per listed row — short id, endpoints, merge status, capture time, and
/// the member count for a grouped row. Bounded pages distinguish the displayed
/// row count from the total and name the cursor needed to continue. An empty
/// listing renders a `no revisions` line, never silence.
fn render_revision_list_text(
    result: &pointbreak::session::RevisionListResult,
    next_cursor: Option<&str>,
) -> String {
    if result.entries.is_empty() {
        return "no revisions".to_owned();
    }
    let headline = if result.entries.len() == result.revision_count {
        count_label(result.revision_count, "revision", "revisions")
    } else {
        format!(
            "{} of {}",
            result.entries.len(),
            count_label(result.revision_count, "revision", "revisions")
        )
    };
    let mut lines = vec![format!("{}:", headline)];
    for entry in &result.entries {
        let captured_from = entry.git_provenance.as_ref().map_or_else(
            || "non-git revision".to_owned(),
            |provenance| {
                format!(
                    "base {} → {}",
                    endpoint_label(&provenance.base),
                    endpoint_label(&provenance.target)
                )
            },
        );
        let mut line = format!(
            "  {}{} · {} · {} · {}",
            output::short_ref(entry.revision_id.as_str()),
            entry
                .summary
                .as_deref()
                .map(crate::cli::common::clamp_title)
                .map(|summary| format!(" · \"{summary}\""))
                .unwrap_or_default(),
            captured_from,
            entry.merge_status,
            entry.captured_at,
        );
        let members = entry.grouped_revision_ids.len();
        if members > 1 {
            line.push_str(&format!(
                " · stands for {}",
                count_label(members, "revision", "revisions")
            ));
        }
        lines.push(line);
    }
    if next_cursor.is_some() {
        lines.push("… more revisions remain (continue with --cursor)".to_owned());
    }
    lines.join("\n")
}

/// Apply the review filter grammar on the revision surface, in place. A no-op
/// unless `--filter` is set, so the flagless listing pays no new cost and stays
/// byte-identical: the per-revision overviews and supersession classification are
/// built only inside this branch. Overviews are keyed by the representative
/// `revision_id`, so a grouped row filters on its representative's
/// overview/classification (the row stands for the group).
fn apply_revision_filter(
    repo: &Path,
    filter: Option<&str>,
    result: &mut pointbreak::session::RevisionListResult,
) -> Result<(), Box<dyn std::error::Error>> {
    let Some(filter) = filter else {
        return Ok(());
    };

    let overviews = show_revision_overviews(
        RevisionOverviewsOptions::new(repo)
            .with_revisions(result.entries.iter().map(|e| e.revision_id.clone()))
            .with_trust_set(crate::cli::common::discover_trust_set(repo))
            .with_read_for_display(true),
    )?;
    let (events, _) = read_events_for_display(repo)?;
    let classification =
        revision_supersession_classification(&SupersessionView::from_events(&events)?);

    // Parse on the revision surface. A known-but-unsupported qualifier or value is
    // a usage error (non-zero exit carrying the message); a deprecated qualifier
    // keeps running behind a stderr hint.
    let parsed = parse_search_query_for(filter, QuerySurface::Revision);
    for diagnostic in &parsed.diagnostics {
        match diagnostic.code {
            QueryDiagnosticCode::UnsupportedQualifier | QueryDiagnosticCode::UnsupportedValue => {
                return Err(diagnostic.message.clone().into());
            }
            QueryDiagnosticCode::DeprecatedQualifier => eprintln!("hint: {}", diagnostic.message),
        }
    }

    result.entries.retain(|entry| {
        let Some(overview) = overviews.get(&entry.revision_id) else {
            return false;
        };
        let facet = classification.get(&entry.revision_id);
        let record = build_revision_search_record(RevisionRecordInputs {
            entry,
            overview,
            classification_state: facet.map(|f| f.state).unwrap_or("isolated"),
            competing: facet.is_some_and(|f| f.competing),
        });
        matches_query(&record.0, &parsed.clauses)
    });
    result.revision_count = result.entries.len();
    Ok(())
}
