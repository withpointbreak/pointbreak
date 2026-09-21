use std::io::Write;
use std::path::PathBuf;

use clap::{Args, Subcommand, ValueEnum};
use pointbreak::documents::{
    derived_review_summary_document, review_summary_check_document, review_summary_document,
};
use pointbreak::session::{
    ReceiptCheckOptions, ReviewSummaryOptions, RoutedReviewSummary, check_counted_input_receipt,
    now_rfc3339_utc, review_summary_routed,
};
use serde_json::Value;

use crate::cli::common::discover_trust_set;
use crate::cli::output;

#[derive(Debug, Args)]
pub(super) struct SummaryArgs {
    #[command(subcommand)]
    command: SummaryCommand,
}

#[derive(Debug, Subcommand)]
enum SummaryCommand {
    Show(SummaryShowArgs),
    Check(SummaryCheckArgs),
}

/// Show review rounds per change with the evidence basis of every number.
#[derive(Debug, Args)]
struct SummaryShowArgs {
    #[arg(long, default_value = ".")]
    repo: PathBuf,

    /// How much of the counted-input receipt to include: the digest and counts, or every entry.
    #[arg(long, value_enum, default_value_t = ReceiptDetail::Digest)]
    receipt: ReceiptDetail,

    #[command(flatten)]
    format_args: output::FormatArgs,
}

/// Re-check the receipt in a saved summary against the store as it is now.
///
/// Reports, for each fact the receipt lists, whether the store still holds it
/// unchanged. The saved summary must have been written with `--receipt entries`.
/// Differences are reported, not treated as failures.
#[derive(Debug, Args)]
struct SummaryCheckArgs {
    /// A summary saved from `summary show --receipt entries`.
    file: PathBuf,

    #[arg(long, default_value = ".")]
    repo: PathBuf,

    #[command(flatten)]
    format_args: output::FormatArgs,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, ValueEnum)]
enum ReceiptDetail {
    #[default]
    Digest,
    Entries,
}

pub(super) fn run(
    args: SummaryArgs,
    stdout: &mut dyn Write,
) -> Result<(), Box<dyn std::error::Error>> {
    match args.command {
        SummaryCommand::Show(args) => {
            let span = tracing::info_span!("shore.summary.show");
            let _entered = span.enter();
            tracing::debug!(command = "summary.show", "command_start");
            summary_show(args, stdout)
        }
        SummaryCommand::Check(args) => {
            let span = tracing::info_span!("shore.summary.check");
            let _entered = span.enter();
            tracing::debug!(command = "summary.check", "command_start");
            summary_check(args, stdout)
        }
    }
}

fn summary_show(
    args: SummaryShowArgs,
    stdout: &mut dyn Write,
) -> Result<(), Box<dyn std::error::Error>> {
    let format = output::resolve_format(args.format_args.explicit(), output::OutputFormat::Json)?;
    let options =
        ReviewSummaryOptions::new(&args.repo).with_trust_set(discover_trust_set(&args.repo));
    let include_entries = args.receipt == ReceiptDetail::Entries;
    let document = match review_summary_routed(options)? {
        RoutedReviewSummary::Authoritative {
            result,
            fallback_hint,
        } => {
            crate::cli::derived_read::emit_claimed_authoritative_fallback_hint(fallback_hint);
            review_summary_document(&result, include_entries, now_rfc3339_utc())
        }
        RoutedReviewSummary::Derived {
            result,
            projection_stamp,
        } => derived_review_summary_document(
            &result,
            projection_stamp,
            include_entries,
            now_rfc3339_utc(),
        ),
    };
    output::write_document(stdout, format, &document, || {
        serde_json::to_value(&document)
            .map(|value| render_summary_text(&value))
            .unwrap_or_default()
    })
}

fn summary_check(
    args: SummaryCheckArgs,
    stdout: &mut dyn Write,
) -> Result<(), Box<dyn std::error::Error>> {
    let format = output::resolve_format(args.format_args.explicit(), output::OutputFormat::Json)?;
    let result = check_counted_input_receipt(ReceiptCheckOptions::new(&args.repo, &args.file))?;
    let document = review_summary_check_document(&result);
    output::write_document(stdout, format, &document, || {
        serde_json::to_value(&document)
            .map(|value| render_check_text(&value))
            .unwrap_or_default()
    })
}

/// Render the check document as plain lines: the receipt, the tally, each
/// difference, and what the check does and does not speak for.
fn render_check_text(document: &Value) -> String {
    let receipt = &document["receipt"];
    let mut lines = vec![
        "Receipt re-check".to_owned(),
        format!(
            "  receipt: {} · digest {} ({})",
            plural(&receipt["count"], "entry", "entries"),
            text(&receipt["digest"]),
            text(&receipt["algorithm"]),
        ),
        format!(
            "  listed entries reproduce the receipt digest: {}",
            if document["digestMatchesEntries"] == Value::Bool(true) {
                "yes"
            } else {
                "no"
            },
        ),
        format!(
            "  matched: {} · changed: {} · missing: {}",
            number(&document["matched"]),
            number(&document["changed"]),
            number(&document["missing"]),
        ),
    ];
    let differing = array(&document["differing"]);
    if differing.is_empty() {
        lines.push("Differences: none".to_owned());
    } else {
        lines.push("Differences".to_owned());
    }
    for difference in &differing {
        let recorded = format!(
            "recorded payload {} · record {}",
            text(&difference["recordedPayloadHash"]),
            text(&difference["recordedEventRecordHash"]),
        );
        match text(&difference["outcome"]).as_str() {
            "missing" => lines.push(format!(
                "  missing {} ({recorded})",
                text(&difference["eventId"]),
            )),
            _ => lines.push(format!(
                "  changed {} ({recorded}; now payload {} · record {})",
                text(&difference["eventId"]),
                text(&difference["currentPayloadHash"]),
                text(&difference["currentEventRecordHash"]),
            )),
        }
    }
    lines.push(
        "Checks the facts this receipt lists. Facts added or removed elsewhere in the store are \
         out of its reach."
            .to_owned(),
    );
    lines.join("\n")
}

/// Render the summary document as plain lines, in a fixed order: population,
/// exclusions, review rounds, first capture, record-internal measures,
/// landed-work coverage, provenance and receipt. Every count prints with its
/// denominator; an unavailable measure prints its reasons, never a zero.
fn render_summary_text(document: &Value) -> String {
    let population = &document["population"];
    let rounds = &document["reviewRounds"];
    let measures = &document["recordInternalMeasures"];
    let coverage = &document["landedWorkCoverage"];
    let provenance = &document["provenance"];
    let counted = &provenance["countedInputs"];
    let mut lines = Vec::new();

    lines.push(
        "Population (review changes; describes this store, not the people who wrote to it)"
            .to_owned(),
    );
    lines.push(format!(
        "  changes counted: {} of {} seen",
        number(&population["changesCounted"]),
        number(&population["changesSeen"]),
    ));
    lines.push(format!(
        "  memberships: {} current · {} historical",
        number(&population["memberships"]["current"]),
        number(&population["memberships"]["historical"]),
    ));
    lines.push(format!(
        "  captured revisions: {} current · {} historical",
        number(&population["capturedRevisions"]["current"]),
        number(&population["capturedRevisions"]["historical"]),
    ));
    if let Some(window) = population.get("observedWindow") {
        lines.push(format!(
            "  observed captures: {} to {}",
            text(&window["from"]),
            text(&window["to"]),
        ));
    }

    lines.push("Exclusions".to_owned());
    for exclusion in array(&population["exclusions"]) {
        match text(&exclusion["reason"]).as_str() {
            "migrationBackfill" => {
                let basis = match exclusion.get("manifestHash") {
                    Some(hash) => format!("store activation manifest {}", text(hash)),
                    None => "no activation manifest".to_owned(),
                };
                lines.push(format!(
                    "  migration backfill: {} · {} ({basis})",
                    plural(&exclusion["changes"], "change", "changes"),
                    plural(&exclusion["memberships"], "membership", "memberships"),
                ));
            }
            _ => lines.push(format!(
                "  no current members: {}",
                plural(&exclusion["changes"], "change", "changes"),
            )),
        }
    }

    lines.push("Review rounds (current-member revisions per change)".to_owned());
    match unavailable(&rounds["profile"]) {
        Some(reasons) => lines.push(format!("  {reasons}")),
        None => {
            for row in array(&rounds["profile"]["distribution"]) {
                lines.push(format!(
                    "  {}: {} of {}",
                    plural(&row["rounds"], "round", "rounds"),
                    number(&row["changes"]),
                    plural(&rounds["profile"]["of"], "change", "changes"),
                ));
            }
        }
    }

    lines.push("First capture".to_owned());
    let first = &rounds["firstCapture"];
    match unavailable(first) {
        Some(reasons) => lines.push(format!("  {reasons}")),
        None => lines.push(format!(
            "  accepted: {} · needs changes: {} · other verdict: {} · unassessed: {} (of {})",
            number(&first["accepted"]),
            number(&first["needsChanges"]),
            number(&first["otherVerdict"]),
            number(&first["unassessed"]),
            plural(&first["of"], "change", "changes"),
        )),
    }

    lines.push("Record-internal measures".to_owned());
    lines.push("  Measured against the record itself; says nothing about landed work.".to_owned());
    lines.push(share_line(
        "assessed captures",
        &measures["assessedCaptureShare"],
        "assessed",
        ("captured revision", "captured revisions"),
    ));
    lines.push(share_line(
        "commit-associated captures",
        &measures["commitAssociatedCaptureShare"],
        "associated",
        ("captured revision", "captured revisions"),
    ));
    lines.push(share_line(
        "first-capture acceptance",
        &measures["firstCaptureAcceptance"],
        "accepting",
        ("assessed first capture", "assessed first captures"),
    ));
    let actors = &measures["actorDistinctAssessmentShare"];
    match unavailable(actors) {
        Some(reasons) => lines.push(format!("  actor relation: {reasons}")),
        None => {
            lines.push(format!(
                "  assessed by a different actor id: {} · same actor id: {} · undetermined: {} (of {})",
                number(&actors["actorDistinct"]),
                number(&actors["actorSame"]),
                number(&actors["undetermined"]),
                plural(&actors["of"], "captured revision", "captured revisions"),
            ));
            lines.push(
                "  Actor ids are not verified identity; signing keys were not compared.".to_owned(),
            );
        }
    }

    lines.push("Landed-work coverage".to_owned());
    for rung in [
        "tipExact",
        "associatedRange",
        "assessedRange",
        "acceptingVerdictRange",
        "distinctIdentity",
        "provedLanding",
    ] {
        let state = unavailable(&coverage[rung]).unwrap_or_else(|| "computed".to_owned());
        lines.push(format!("  {rung}: {state}"));
    }
    lines.push("  Not computed by this read: needs the integration branch history.".to_owned());

    lines.push("Provenance".to_owned());
    match text(&provenance["basis"]).as_str() {
        "projection" => lines.push(format!(
            "  basis: projection {} (identifies a local index snapshot, not the facts counted) · {}",
            text(&provenance["projectionStamp"]),
            plural(&provenance["eventCount"], "event", "events"),
        )),
        _ => lines.push(format!(
            "  basis: fact set {} · {}",
            text(&provenance["eventSetHash"]),
            plural(&provenance["eventCount"], "event", "events"),
        )),
    }
    lines.push(format!(
        "  definitions: {} · computed at {}",
        text(&provenance["metricDefinitions"]),
        text(&provenance["computedAt"]),
    ));
    let by_kind = &counted["byKind"];
    let verification = &counted["verification"];
    lines.push(format!(
        "  counted inputs: {} · digest {} ({})",
        plural(&counted["count"], "event", "events"),
        text(&counted["digest"]),
        text(&counted["algorithm"]),
    ));
    lines.push(format!(
        "    membership claims: {} · membership withdrawals: {} · captures: {} · assessments: {} · commit associations: {}",
        number(&by_kind["membershipClaims"]),
        number(&by_kind["membershipWithdrawals"]),
        number(&by_kind["captures"]),
        number(&by_kind["assessments"]),
        number(&by_kind["commitAssociations"]),
    ));
    lines.push(format!(
        "    signatures: valid {} · untrusted key {} · invalid {} · unsigned {} (allowed signers {})",
        number(&verification["valid"]),
        number(&verification["untrustedKey"]),
        number(&verification["invalid"]),
        number(&verification["unsigned"]),
        if verification["allowedSignersConfigured"] == Value::Bool(true) {
            "configured"
        } else {
            "not configured"
        },
    ));
    for entry in array(&counted["entries"]) {
        lines.push(format!(
            "    {} {} {} {}",
            text(&entry["eventId"]),
            text(&entry["payloadHash"]),
            text(&entry["eventRecordHash"]),
            text(&entry["verificationStatus"]),
        ));
    }
    lines.push("  Proves which facts were counted, not that the store is complete.".to_owned());
    lines.join("\n")
}

/// `<label>: N of M <unit> (P%)`, the percentage text-only and only when the
/// denominator is nonzero.
fn share_line(label: &str, measure: &Value, count_key: &str, unit: (&str, &str)) -> String {
    if let Some(reasons) = unavailable(measure) {
        return format!("  {label}: {reasons}");
    }
    let count = number(&measure[count_key]);
    let of = number(&measure["of"]);
    let percent = (count * 100 + of / 2)
        .checked_div(of)
        .map(|percent| format!(" ({percent}%)"))
        .unwrap_or_default();
    format!(
        "  {label}: {count} of {}{percent}",
        plural(&measure["of"], unit.0, unit.1)
    )
}

fn unavailable(measure: &Value) -> Option<String> {
    (measure["state"] == "unavailable").then(|| {
        let reasons: Vec<String> = array(&measure["reasons"]).iter().map(text).collect();
        format!("unavailable ({})", reasons.join(", "))
    })
}

fn array(value: &Value) -> Vec<Value> {
    value.as_array().cloned().unwrap_or_default()
}

fn number(value: &Value) -> u64 {
    value.as_u64().unwrap_or_default()
}

fn text(value: &Value) -> String {
    value.as_str().unwrap_or_default().to_owned()
}

fn plural(value: &Value, singular: &str, plural: &str) -> String {
    let count = number(value);
    format!("{count} {}", if count == 1 { singular } else { plural })
}
