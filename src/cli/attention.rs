use std::io::Write;
use std::path::PathBuf;

use clap::{Args, Subcommand};
use pointbreak::documents::{attention_list_document, derived_attention_list_document};
use pointbreak::model::RevisionId;
use pointbreak::session::{
    AttentionDetail, AttentionItem, AttentionListOptions, AttentionListResult, AttentionTier,
    DerivedAttentionRoute, DerivedHistoryAccess, list_attention,
};

use crate::cli::common::clamp_title;
use crate::cli::output;

#[derive(Debug, Args)]
pub(super) struct AttentionArgs {
    #[command(subcommand)]
    command: AttentionCommand,
}

#[derive(Debug, Subcommand)]
enum AttentionCommand {
    List(AttentionListArgs),
}

/// List open asks and unresolved review state that need an actor's judgment.
#[derive(Debug, Args)]
struct AttentionListArgs {
    #[arg(long, default_value = ".")]
    repo: PathBuf,

    /// Scope to one revision: its anchored items plus the thread that covers it.
    #[arg(long)]
    revision: Option<String>,

    #[command(flatten)]
    format_args: output::FormatArgs,
}

pub(super) fn run(
    args: AttentionArgs,
    stdout: &mut dyn Write,
) -> Result<(), Box<dyn std::error::Error>> {
    match args.command {
        AttentionCommand::List(args) => {
            let span = tracing::info_span!("shore.attention.list");
            let _entered = span.enter();
            tracing::debug!(command = "attention.list", "command_start");
            attention_list(args, stdout)
        }
    }
}

fn attention_list(
    args: AttentionListArgs,
    stdout: &mut dyn Write,
) -> Result<(), Box<dyn std::error::Error>> {
    let format_explicit = args.format_args.explicit();
    let format = output::resolve_format(format_explicit, output::OutputFormat::Json)?;

    let revision = args
        .revision
        .as_deref()
        .map(|revision| {
            crate::cli::id_resolver::IdResolver::new(&args.repo)
                .rev(revision)
                .map(RevisionId::new)
        })
        .transpose()?;
    let routed = read_attention(&args.repo, revision)?;
    let result = routed.result();
    // The text lane reads the same result the document consumes; clone it only
    // when that lane will render (eager-clone rule).
    let text_source = matches!(format.format, output::OutputFormat::Text).then(|| result.clone());
    match routed {
        RoutedAttention::Authoritative(result) => {
            let document = attention_list_document(result);
            output::write_document(stdout, format, &document, || {
                render_attention_list_text(
                    text_source
                        .as_ref()
                        .expect("text lane resolves the attention source"),
                )
            })
        }
        RoutedAttention::Derived {
            result,
            projection_stamp,
        } => {
            let document = derived_attention_list_document(result, projection_stamp);
            output::write_document(stdout, format, &document, || {
                render_attention_list_text(
                    text_source
                        .as_ref()
                        .expect("text lane resolves the attention source"),
                )
            })
        }
    }
}

enum RoutedAttention {
    Authoritative(AttentionListResult),
    Derived {
        result: AttentionListResult,
        projection_stamp: String,
    },
}

impl RoutedAttention {
    fn result(&self) -> &AttentionListResult {
        match self {
            Self::Authoritative(result) | Self::Derived { result, .. } => result,
        }
    }
}

fn read_attention(
    repo: &std::path::Path,
    revision: Option<RevisionId>,
) -> Result<RoutedAttention, Box<dyn std::error::Error>> {
    let authoritative = || {
        let mut options = AttentionListOptions::new(repo);
        if let Some(revision) = &revision {
            options = options.with_revision(revision.clone());
        }
        list_attention(options)
            .map(RoutedAttention::Authoritative)
            .map_err(Into::into)
    };
    let access = DerivedHistoryAccess::resolve(repo).map_err(std::io::Error::other)?;
    match access
        .attention(revision.as_ref())
        .map_err(std::io::Error::other)?
    {
        DerivedAttentionRoute::Ready(derived) => Ok(RoutedAttention::Derived {
            result: AttentionListResult {
                event_set_hash: String::new(),
                event_count: derived.event_count,
                revision,
                items: derived.items,
                diagnostics: derived.diagnostics,
            },
            projection_stamp: derived.projection_stamp,
        }),
        DerivedAttentionRoute::Off if !access.is_active() => authoritative(),
        DerivedAttentionRoute::Off | DerivedAttentionRoute::Unavailable(_) => {
            crate::cli::derived_read::emit_authoritative_fallback_hint(&access);
            authoritative()
        }
    }
}

/// Bespoke text lane for `attention list` (ADR-0029: text is disposable, never
/// byte-pinned). A count headline, then one scannable line per item — the tier
/// from the document's own field, the kebab kind label, and a shortened anchor
/// id. Items already sort primary-before-secondary, so the lines do too. An empty
/// projection renders a `nothing needs attention` line, never silence.
fn render_attention_list_text(result: &AttentionListResult) -> String {
    if result.items.is_empty() {
        return "nothing needs attention".to_owned();
    }
    let mut lines = vec![format!(
        "attention: {} item(s) need judgment:",
        result.items.len()
    )];
    for item in &result.items {
        lines.push(render_attention_item_line(item));
    }
    lines.join("\n")
}

fn render_attention_item_line(item: &AttentionItem) -> String {
    // The item id is `{kind}:{anchor}`; the kind already labels the line, so only
    // the anchor is shortened. The display kind is kebab (underscore -> hyphen).
    let (kind, anchor) = item.id.split_once(':').unwrap_or((item.id.as_str(), ""));
    let kind = kind.replace('_', "-");
    let anchor = output::short_ref(anchor);
    let tier = match item.tier {
        AttentionTier::Primary => "primary",
        AttentionTier::Secondary => "secondary",
    };
    let mut line = format!("  [{tier}] {kind}  {anchor}");
    if let AttentionDetail::OpenInputRequest { title, .. } = &item.detail {
        line.push_str("  ");
        line.push_str(&clamp_title(title));
    }
    line
}
