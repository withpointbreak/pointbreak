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

impl AttentionArgs {
    pub(super) fn qualified_invocation_read_v1(
        &self,
    ) -> Option<super::QualifiedInvocationReadV1<'_>> {
        let AttentionCommand::List(args) = &self.command;
        args.revision
            .is_some()
            .then_some(super::QualifiedInvocationReadV1 {
                route: super::InvocationReadRouteV1::AttentionCurrentOrFallback,
                repo: &args.repo,
                revision: args.revision.as_deref(),
                track: None,
                explicit_format: args.format_args.explicit(),
            })
    }
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
    #[cfg(feature = "longitudinal-counting")]
    routed.record_observed_state();
    let result = routed.result();
    // The text lane reads the same result the document consumes; clone it only
    // when that lane will render (eager-clone rule).
    let text_source = matches!(format.format, output::OutputFormat::Text).then(|| result.clone());
    match routed {
        RoutedAttention::Authoritative { result, .. } => {
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
    Authoritative {
        result: AttentionListResult,
        #[cfg_attr(not(feature = "longitudinal-counting"), allow(dead_code))]
        labeled_fallback: bool,
    },
    Derived {
        result: AttentionListResult,
        projection_stamp: String,
    },
}

impl RoutedAttention {
    fn result(&self) -> &AttentionListResult {
        match self {
            Self::Authoritative { result, .. } | Self::Derived { result, .. } => result,
        }
    }

    #[cfg(feature = "longitudinal-counting")]
    fn observed_state(
        &self,
    ) -> pointbreak::bench_support::longitudinal::InteractionObservedRouteStateV1 {
        use pointbreak::bench_support::longitudinal::InteractionObservedRouteStateV1 as State;

        match self {
            Self::Derived { .. } => State::DerivedCurrent,
            Self::Authoritative {
                labeled_fallback: false,
                ..
            } => State::AuthoritativeReplay,
            Self::Authoritative {
                labeled_fallback: true,
                ..
            } => State::LabeledFallbackToAuthoritative,
        }
    }

    #[cfg(feature = "longitudinal-counting")]
    fn record_observed_state(&self) {
        if let Some(counting) =
            pointbreak::bench_support::longitudinal::LongitudinalCountingScopeV1::current()
        {
            counting.record_observed_route_state_once(self.observed_state());
        }
    }
}

fn read_attention(
    repo: &std::path::Path,
    revision: Option<RevisionId>,
) -> Result<RoutedAttention, Box<dyn std::error::Error>> {
    let authoritative = |labeled_fallback| {
        let mut options = AttentionListOptions::new(repo);
        if let Some(revision) = &revision {
            options = options.with_revision(revision.clone());
        }
        list_attention(options)
            .map(|result| RoutedAttention::Authoritative {
                result,
                labeled_fallback,
            })
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
        DerivedAttentionRoute::Off if !access.is_active() => authoritative(false),
        DerivedAttentionRoute::Off | DerivedAttentionRoute::Unavailable(_) => {
            #[cfg(feature = "longitudinal-counting")]
            let _fallback_phase =
                pointbreak::bench_support::longitudinal::enter_derived_access_phase_v1(
                    pointbreak::bench_support::longitudinal::LongitudinalDerivedAccessPhaseV1::CacheAndFallback,
                );
            #[cfg(feature = "longitudinal-counting")]
            {
                pointbreak::bench_support::longitudinal::record_authoritative_fallback();
                pointbreak::bench_support::longitudinal::record_full_history_fallback();
            }
            crate::cli::derived_read::emit_authoritative_fallback_hint(&access);
            authoritative(true)
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

#[cfg(all(test, feature = "longitudinal-counting"))]
mod tests {
    use pointbreak::bench_support::longitudinal::InteractionObservedRouteStateV1;

    use super::*;

    #[test]
    fn attention_route_state_is_owned_by_the_selected_live_route() {
        for (routed, expected) in [
            (
                RoutedAttention::Derived {
                    result: empty_result(),
                    projection_stamp: "sha256:derived".to_owned(),
                },
                InteractionObservedRouteStateV1::DerivedCurrent,
            ),
            (
                RoutedAttention::Authoritative {
                    result: empty_result(),
                    labeled_fallback: false,
                },
                InteractionObservedRouteStateV1::AuthoritativeReplay,
            ),
            (
                RoutedAttention::Authoritative {
                    result: empty_result(),
                    labeled_fallback: true,
                },
                InteractionObservedRouteStateV1::LabeledFallbackToAuthoritative,
            ),
        ] {
            assert_eq!(routed.observed_state(), expected);
        }
    }

    fn empty_result() -> AttentionListResult {
        AttentionListResult {
            event_set_hash: String::new(),
            event_count: 0,
            revision: None,
            items: Vec::new(),
            diagnostics: Vec::new(),
        }
    }
}
