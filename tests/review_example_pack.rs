use std::fs;
use std::path::PathBuf;
use std::process::Command;

use pointbreak::session::{
    ImportArtifactOptions, IngestEventsOptions, import_artifact, ingest_events,
    referenced_artifacts,
};
use serde_json::Value;
use sha2::{Digest, Sha256};
use tempfile::tempdir;

#[allow(dead_code)]
#[path = "../examples/support/review_example_pack.rs"]
mod pack_support;

fn pack_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("examples/review/checkout-refactor")
}

#[test]
fn canonical_review_example_pack_exists() {
    let manifest = pack_root().join("manifest.json");
    assert!(
        manifest.is_file(),
        "canonical Review example manifest is missing: {}",
        manifest.display()
    );
}

#[test]
fn current_exporter_and_materialize_hint_use_pointbreak() {
    let exporter = include_str!("../examples/support/review_example_pack.rs");
    let command = include_str!("../examples/review_example_pack.rs");

    assert!(exporter.contains("name: \"pointbreak\".to_owned()"));
    assert!(command.contains("pointbreak inspect --repo"));
    assert!(!command.contains("shore inspect --repo"));
}

#[test]
fn synthetic_decision_matrix_materializer_uses_only_isolated_pointbreak_surfaces() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let script_path = root.join("scripts/materialize-inspector-decision-matrix.sh");
    assert!(
        script_path.is_file(),
        "synthetic decision matrix materializer is missing: {}",
        script_path.display()
    );

    let script = fs::read_to_string(&script_path).expect("read decision matrix materializer");
    assert!(script.contains("POINTBREAK_BINARY"));
    assert!(
        script
            .contains("pointbreak_home=\"${POINTBREAK_HOME:-$destination/.git/pointbreak-home}\"")
    );
    assert!(script.contains("POINTBREAK_HOME=\"$pointbreak_home\""));
    assert!(script.contains("--format json"));
    assert!(script.contains("\"$cygpath_program\" -u \"$native_path\""));
    assert!(!script.contains("~/.pointbreak"));
    assert!(!script.contains("shore"));
    assert!(!script.contains("rev:sha256:"));
    assert!(!script.contains("evt:sha256:"));
    assert!(!script.contains("assoc-commit:sha256:"));
    assert!(
        script.contains("rm -f -- \"$missing_object_path\"")
            && script.contains("cat-file -e \"$missing_commit^{commit}\""),
        "the intentionally removed loose object must be retry-safe and proven unreadable"
    );

    let justfile = fs::read_to_string(root.join("Justfile")).expect("read Justfile");
    assert!(justfile.contains("review-decision-matrix-materialize output:"));
    assert!(justfile.contains("scripts/materialize-inspector-decision-matrix.sh"));
}

#[test]
fn inspector_decision_continuity_browser_gate_uses_isolated_pointbreak_surfaces() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let script_path = root.join("scripts/verify-inspector-decision-continuity.sh");
    assert!(
        script_path.is_file(),
        "Inspector decision-continuity browser gate is missing: {}",
        script_path.display()
    );

    let script = fs::read_to_string(&script_path).expect("read Inspector browser gate");
    let browser_program =
        fs::read_to_string(root.join("scripts/verify-inspector-decision-continuity.mjs"))
            .expect("read Inspector browser program");
    let gate = format!("{script}\n{browser_program}");
    for required in [
        "POINTBREAK_BINARY",
        "POINTBREAK_HOME",
        "--format json",
        "review-example-materialize",
        "review-decision-matrix-materialize",
        "playwright-cli",
        "1440",
        "1000",
        "900",
        "506",
        "390",
        "844",
        "Decision context",
    ] {
        assert!(
            gate.contains(required),
            "missing browser gate term: {required}"
        );
    }
    for excluded in [
        "cargo publish",
        "gh release",
        "npm publish",
        "vsce package",
        "capture-marketing-review-screenshots",
    ] {
        assert!(
            !gate.contains(excluded),
            "browser gate includes excluded command: {excluded}"
        );
    }

    let justfile = fs::read_to_string(root.join("Justfile")).expect("read Justfile");
    assert!(justfile.contains("review-decision-browser-verify"));
    assert!(justfile.contains("scripts/verify-inspector-decision-continuity.sh"));
    assert!(justfile.contains(r#"if [ -n "${POINTBREAK_BINARY:-}" ]"#));
    assert!(script.contains(r#"POINTBREAK_BINARY="$pointbreak_binary""#));
    assert!(script.contains("[A-Za-z]:"));
    let materializer =
        fs::read_to_string(root.join("scripts/materialize-inspector-decision-matrix.sh"))
            .expect("read decision matrix materializer");
    assert!(materializer.contains("[A-Za-z]:"));
    assert!(script.contains("review-decision-matrix-materialize"));
}

#[test]
fn change_inspector_browser_gate_compares_canonical_current_revision_refs() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let script = fs::read_to_string(root.join("scripts/change-inspector-browser-verify.sh"))
        .expect("read Change Inspector browser gate")
        .replace("\r\n", "\n");
    let materializer =
        fs::read_to_string(root.join("scripts/materialize-inspector-decision-matrix.sh"))
            .expect("read decision matrix materializer");
    let browser_program =
        fs::read_to_string(root.join("scripts/change-inspector-browser-verify.mjs"))
            .expect("read Change Inspector browser program")
            .replace("\r\n", "\n");
    let browser_diagnostics =
        fs::read_to_string(root.join("scripts/change-inspector-browser-diagnostics.mjs"))
            .expect("read Change Inspector browser diagnostics");
    let manifest_publisher =
        fs::read_to_string(root.join("scripts/change-inspector-browser-manifest.mjs"))
            .expect("read Change Inspector browser manifest publisher");

    assert!(
        script.contains("sort_by(.revisionId, .objectArtifactContentHash)"),
        "fixture construction order must be normalized to the Change document order"
    );
    assert!(
        script.contains(".currentRevisionRefs == $current"),
        "the product document must still match the complete canonical array exactly"
    );
    for line in script
        .lines()
        .filter(|line| line.contains("--operation-id"))
    {
        assert!(
            line.contains("--operation-id \"change-operation:"),
            "browser fixture operation ID is outside the CLI namespace: {line}"
        );
    }
    assert!(
        !browser_program.contains("new URL("),
        "the Playwright run-code sandbox does not expose the Web URL constructor"
    );
    assert!(
        !browser_program.contains("new URLSearchParams((await hash())"),
        "route query parsing must stay in page context because the Playwright run-code sandbox does not expose URLSearchParams"
    );
    assert!(
        !browser_program.contains("#detail-body > p.mono")
            && browser_program.contains("#detail-body [data-event-id]")
            && browser_program.contains(".dataset.eventId === expectedEventId"),
        "exact Timeline event readiness must compare the detail's full data-event-id, not compact identity prose"
    );
    assert!(
        !browser_program.contains("const filteredQuery = new URLSearchParams(")
            && !browser_program.contains("const clearedQuery = new URLSearchParams(")
            && browser_program.contains("const routeParameters = (names) =>")
            && browser_program
                .matches("await routeParameters([\"limit\", \"order\"])")
                .count()
                >= 2,
        "the outer Playwright runner must read filtered and cleared route parameters through one page-context helper"
    );
    assert!(
        !browser_program.contains("initialTimelineText.includes(\"Newest first\")")
            && !browser_program.contains("ascendingText.includes(\"Oldest first\")")
            && browser_program.contains("const defaultChronologyDeclared = initialTimelineText")
            && browser_program.contains("const ascendingChronologyDeclared = ascendingText")
            && browser_program.matches(".toLowerCase()").count() >= 2,
        "Timeline chronology checks must normalize rendered case before comparing their labels"
    );
    assert!(
        browser_program
            .contains("const semanticRouteMatchesInPage = ({ expectedHash, source }) =>")
            && browser_program.contains("page.url().startsWith(`${config.server.baseUrl}/`)")
            && browser_program.contains("(await currentRouteMatches(targetHash))")
            && browser_program.contains("await waitForCurrentRoute(targetHash);")
            && browser_program.contains("const companionTimelineHash =")
            && browser_program
                .contains("await waitForCurrentRoute(companionTimelineHash, \"timeline\");")
            && browser_program.contains("dataset.timelineRoute")
            && browser_program.contains("reload || readingKey !== priorKeys.reading"),
        "browser readiness must compare semantic routes while preserving exact-reading replacement for different identities"
    );
    let annotated_diff_round_trip = browser_program
        .split_once("// An annotated diff is a first-class full-frame exact route")
        .and_then(|(_, tail)| tail.split_once("await open(exact, layouts[1]"))
        .map(|(round_trip, _)| round_trip)
        .expect("browser gate retains the annotated-diff round-trip section");
    assert!(
        annotated_diff_round_trip.contains("const canonicalRevisionRoute = await hash();")
            && !annotated_diff_round_trip
                .contains("const focusedRevisionRoute = await page.evaluate((diffRoute) => {")
            && !annotated_diff_round_trip.contains("params.delete(\"fq\");")
            && annotated_diff_round_trip
                .matches("canonicalRevisionRoute")
                .count()
                >= 2,
        "annotated-diff Close and Forward waits must restore the captured canonical Revision entry route"
    );
    let timeline_search = browser_program
        .split_once("const timelineSearch = \"Browser correction replacement\";")
        .and_then(|(_, tail)| tail.split_once("// Drive all typed filters"))
        .map(|(search, _)| search)
        .expect("browser gate retains the Timeline plain-search section");
    assert!(
        !timeline_search.contains("Remove search filter:")
            && timeline_search.contains("getByRole(\"button\", { name: \"Clear all\" })")
            && timeline_search.contains("const correctionEventIds =")
            && timeline_search.contains("const expectedCorrectionEventIds = [")
            && timeline_search.contains("config.fixture.correction.eventId"),
        "plain Timeline q must clear through Clear all instead of an invented removable chip"
    );
    assert!(
        !browser_program.contains("throw new BrowserDiagnosticFailure(report);")
            && browser_program.contains(
                "const completion = diagnostics.result({ screenshotCount: screenshots });"
            )
            && browser_program.contains("return completion;")
            && script.contains("line == \"### Result\"")
            && script.contains("browser_gate_status=$?"),
        "browser assertion failures must return a structured Playwright result while a nonzero runner remains infrastructure-fatal"
    );
    let first_response = materializer
        .find("actor:agent:pointbreak-matrix-response-one")
        .expect("matrix records the first response");
    let fact_port = materializer
        .find("fact port --repo")
        .expect("matrix records the fact port through the public CLI");
    let conflicting_response = materializer
        .find("actor:agent:pointbreak-matrix-response-two")
        .expect("matrix records the conflicting response");
    assert!(
        first_response < fact_port && fact_port < conflicting_response,
        "the matrix must port the fact while the target Change is still resolvable"
    );
    assert!(
        script.contains(".fact_port.event_id")
            && script.contains(".fact_port.port_id")
            && !script.contains("fact-port-target-cursor"),
        "the browser gate must consume the pre-ambiguity fact port instead of bypassing selection"
    );
    assert!(
        !browser_program.contains("controlCount === 1")
            && browser_program.contains("controlCount === 2")
            && browser_program.contains("controls.nth(1)")
            && browser_program.contains("instanceof HTMLAnchorElement")
            && browser_program.contains("href: control.getAttribute(\"href\")")
            && browser_program.contains("accessibleName: control.getAttribute(\"aria-label\")")
            && browser_program.contains("topologyFixture.initial.current.revision")
            && browser_program.contains("topologyFixture.initial.current.artifact"),
        "an ordinary Change card must retain its primary action plus a native exact Revision link"
    );
    assert!(
        browser_program.contains("const changeGraphMaxScroll = Math.max(")
            && browser_program.contains("changeGraphEnd === changeGraphMaxScroll")
            && browser_program.contains("changeGraphHome === 0"),
        "a narrow Change graph may fit its viewport, but Home and End must still stay within its scroll bounds"
    );
    assert!(
        annotated_diff_round_trip.contains("const canonicalReadingIdentity =")
            && annotated_diff_round_trip.contains("await canonicalReadingIdentityLocator")
            && annotated_diff_round_trip
                .contains("const waitForCanonicalRevisionSurface = async (phase) =>")
            && annotated_diff_round_trip.contains("diff.classList.contains(\"hidden\")")
            && annotated_diff_round_trip.contains("!split.classList.contains(\"hidden\")")
            && annotated_diff_round_trip.contains("identity.dataset.revisionId")
            && annotated_diff_round_trip.contains("identity.dataset.artifactHash")
            && annotated_diff_round_trip
                .matches("await waitForCanonicalRevisionSurface(")
                .count()
                >= 2,
        "annotated-diff Close and Forward must prove the canonical exact Revision surface is visible and bound"
    );

    let explicit_cleanup = script
        .rfind("\ncleanup strict || die \"browser session did not close cleanly\"\n")
        .expect("browser gate explicitly completes cleanup");
    let installed_trap = script
        .find("\ntrap cleanup EXIT\n")
        .expect("browser gate installs its cleanup trap");
    let deterministic_equal_timestamp = script
        .find("\nequal_timestamp_pair=\"$(jq -s -ce '\n")
        .expect("browser gate binds one deterministic capture event pair");
    let disarmed_trap = script
        .rfind("\ntrap - EXIT\n")
        .expect("browser gate disarms its cleanup trap");
    for section in [
        "Reader readiness",
        "Timeline overview and chronology",
        "Timeline search and correlation",
        "Timeline preferences",
        "Timeline keyboard and exact detail",
        "Timeline follow and stale continuation",
        "Changes and Attention paging",
        "Attention guidance",
        "Changes keyboard and filters",
        "Change topology cards",
        "Change relationship graph",
        "Exact Revision selection and history",
        "Shared Revision membership",
        "Split, preferences, and dialogs",
        "Fact relationship graph",
        "Annotated diff",
        "Exact detail and reading",
        "Exact resource availability",
        "Polling retention and reduced motion",
        "Browser runtime",
    ] {
        assert!(
            browser_program.contains(&format!("diagnostics.section(\"{section}\"")),
            "browser diagnostics must retain the independently recoverable {section:?} section"
        );
    }
    assert!(
        browser_diagnostics.contains("status: stopped ? \"stopped\"")
            && browser_diagnostics.contains("failures.map((failure)")
            && browser_diagnostics.contains("throw new BrowserDiagnosticFailure(")
            && browser_diagnostics.contains("const result = ({ screenshotCount }) =>")
            && browser_diagnostics.contains("browserDiagnosticRecorded")
            && browser_diagnostics.contains("requireCondition")
            && browser_diagnostics.contains("condition: label")
            && browser_diagnostics.contains("outcome: \"satisfied\"")
            && !browser_diagnostics.contains("comparison.expected ?? true")
            && !browser_diagnostics.contains("comparison.actual ?? Boolean(condition)"),
        "browser diagnostics must distinguish section stops, aggregate failures, and refuse completion"
    );
    assert!(
        browser_program.matches("compare(").count() >= 100
            && browser_program.contains("\"1..79\"")
            && browser_program.contains("defaultTimeline.liveEvents")
            && browser_program.contains("expectedParallelPeers")
            && browser_program.contains("diffMetrics"),
        "browser checks must preserve explicit expected and observed values for stable product contracts"
    );
    assert!(
        browser_program.matches("requireCondition(").count() >= 12
            && browser_program
                .contains("const actualExactRevisionRoute = await page.evaluate(() =>")
            && !browser_program.contains("exactRevisionRouteMatches"),
        "authority-bearing browser prerequisites must stop their section and exact routes must be sampled once"
    );
    assert!(
        script.contains(".status == \"passed\" and .globalInvalid == false")
            && script.contains("(.failures | length == 0)")
            && script.contains("(.sections | all(.status == \"passed\"")
            && script.contains("browser-result.json"),
        "browser orchestration must preserve and reject the structured aggregate report"
    );
    for source in [
        "change-inspector-browser-verify.sh",
        "change-inspector-browser-verify.mjs",
        "change-inspector-browser-diagnostics.mjs",
        "change-inspector-browser-manifest.mjs",
        "materialize-inspector-decision-matrix.sh",
    ] {
        assert!(
            script.contains(&format!("$source_commit:scripts/{source}")),
            "browser gate must snapshot committed harness source {source:?} before qualification"
        );
    }
    for fixture in [
        "5a1f8bbdea0db6199064bb2b75dfa89382b23398c71c640f7ca3268e48e3afaf.json",
        "f31956c2b820926adc74d4d03cb03820d13c9ed2739b5f7ada81611a6f8bcff1.json",
    ] {
        assert!(
            script.contains(fixture),
            "browser gate must snapshot committed activation fixture {fixture:?} for the isolated materializer"
        );
    }
    assert!(
        script
            .contains("$source_commit:tests/support/assets/change-ready-store/$activation_fixture")
            && script.contains(
                "$source_commit:tests/support/assets/change-ready-store/$completion_fixture",
            )
            && script.contains("ready_store=\"$snapshot_ready_store\""),
        "browser gate must execute both materializer and reader fixtures from the committed snapshot"
    );
    assert!(
        script.contains("binary_snapshot=")
            && script.contains("pointbreak_binary=\"$binary_snapshot\"")
            && script.contains("harness-digests.json")
            && script.contains("harness_record_sha256=")
            && script.contains("--slurpfile harness"),
        "browser gate must execute source-bound harness and binary snapshots and bind them in evidence"
    );
    assert!(
        manifest_publisher.contains("browserResult.failures.length > 0")
            && manifest_publisher.contains("await link(publisherPath, manifestPath)")
            && !manifest_publisher.contains("await rename("),
        "manifest publication must reject recorded failures before atomic no-replace publication"
    );

    let manifest_publish = script
        .rfind(
            "node \"$browser_manifest_publisher\" \"$manifest_tmp\" \"$root/manifest.json\" \"$browser_result\"",
        )
        .expect("browser gate publishes its completion marker");
    assert!(
        installed_trap < deterministic_equal_timestamp,
        "failure cleanup must be installed before deterministic fixture selection"
    );
    assert!(
        script.contains(".changeEvents | map(select(.outcome == \"created\"))")
            && script.contains("eventType == \"change_declared\"")
            && script.contains("eventType == \"change_membership_asserted\"")
            && script.contains("changeId: $capture.changeId")
            && !script.contains("equal_timestamp_pids")
            && !script.contains("browser-equal-time")
            && browser_program
                .contains("change=${encodeURIComponent(config.fixture.equalTimestamp.changeId)}",)
            && browser_program.contains("new Set(equalTimestampOccurredAt).size === 1")
            && !browser_program.contains("track=agent%3Abrowser-equal-time"),
        "Timeline tie evidence must come from one supported multi-event Change capture, not clock luck"
    );
    for registration in [
        "reader_state_started_pid=$!\n  register_background_process \"$reader_state_started_pid\"",
        "server_pid=$!\nregister_background_process \"$server_pid\"",
        "timeline_append_pid=$!\nregister_background_process \"$timeline_append_pid\"",
    ] {
        assert!(
            script.contains(registration),
            "browser gate does not immediately register background child: {registration}"
        );
    }
    assert!(
        explicit_cleanup < disarmed_trap && disarmed_trap < manifest_publish,
        "browser shutdown and child reaping must precede completion-last manifest publication"
    );
}

#[test]
fn change_inspector_browser_gate_rejects_frozen_exact_identity_false_negatives() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let browser_program =
        fs::read_to_string(root.join("scripts/change-inspector-browser-verify.mjs"))
            .expect("read Change Inspector browser program")
            .replace("\r\n", "\n");

    let exact_timeline_details = browser_program
        .split_once("const inspectExactTimelineEvent = async (")
        .and_then(|(_, tail)| tail.split_once("// The original proposal remains correlated"))
        .map(|(details, _)| details)
        .expect("browser gate retains exact Timeline detail checks");
    assert!(
        browser_program.contains("const readTimelineEventIdentity = async (label) =>")
            && browser_program.contains("timelineDetailIdentityCount === 1")
            && browser_program
                .contains("eventId: await detailIdentity.getAttribute(\"data-event-id\"),")
            && browser_program.contains("title: await detailIdentity.getAttribute(\"title\"),")
            && browser_program.contains("name: await detailIdentity.getAttribute(\"aria-label\"),")
            && browser_program.contains("const compareTimelineEventIdentity =")
            && browser_program.contains("title: eventId,")
            && browser_program.contains("name: `event ${eventId}`,")
            && exact_timeline_details
                .contains("const detailIdentity = await waitForExactTimelineEvent(eventId);")
            && exact_timeline_details.contains("await readExactDetailIdentitySources()")
            && !exact_timeline_details.contains("detail.includes(identity)"),
        "exact Timeline detail checks must prove the full opaque event identity through data-event-id, title, and aria-label rather than rendered text"
    );

    let exact_event_lifecycle = browser_program
        .split_once("// Open one exact event from the Timeline.")
        .and_then(|(_, tail)| {
            tail.split_once("await open(\n\t\t\t\t\"timeline?limit=100&order=desc\",")
        })
        .map(|(lifecycle, _)| lifecycle)
        .expect("browser gate retains click, history, and reload event checks");
    assert!(
        exact_event_lifecycle
            .matches("waitForExactTimelineEvent(selectedEventId)")
            .count()
            >= 3
            && exact_event_lifecycle.contains("const expectedExactEventRoute =")
            && exact_event_lifecycle.contains("exactEventRouteFromTimelineRoute(")
            && exact_event_lifecycle
                .contains("const exactEventRouteMatches = await currentRouteMatches(")
            && exact_event_lifecycle
                .matches("await currentRouteMatches(expectedExactEventRoute)")
                .count()
                >= 2
            && !exact_event_lifecycle.contains("const exactEventRoute = await hash();")
            && exact_event_lifecycle.contains("Timeline event Forward route")
            && exact_event_lifecycle.contains("Timeline event reload route")
            && !exact_event_lifecycle
                .contains("innerText()).includes(\n\t\t\t\t\tselectedEventId,")
            && !exact_event_lifecycle.contains("textContent?.includes(selectedEventId)"),
        "exact event checks after click and reload must retain the semantic route and data-event-id identity, not infer either from visible detail text"
    );

    let exact_event_wait = browser_program
        .split_once("const waitForExactTimelineEvent = async (eventId) =>")
        .and_then(|(_, tail)| tail.split_once("const readExactDetailIdentitySources = () =>"))
        .map(|(wait, _)| wait)
        .expect("browser gate retains exact Timeline event readiness");
    assert!(
        !exact_event_wait.contains("detailIdentity.title === expectedEventId")
            && !exact_event_wait.contains("detailIdentity.getAttribute(\"aria-label\")")
            && browser_program.contains("compareTimelineEventIdentity("),
        "event readiness must settle on route and data identity before recording title and accessible-name mismatches as structured comparisons"
    );

    assert!(
        browser_program.contains("titleTokens: identityTokens(title)")
            && browser_program.contains("nameTokens: identityTokens(name)")
            && browser_program.contains("source.titleTokens.includes(identity)")
            && browser_program.contains("source.nameTokens.includes(identity)")
            && !browser_program.contains("source.title.includes(identity)")
            && !browser_program.contains("source.name.includes(identity)"),
        "correlated opaque identities must compare complete structured tokens instead of accepting substrings"
    );

    let historical_proposal = browser_program
        .split_once("const historical = config.fixture.historicalMembership;")
        .and_then(|(_, tail)| {
            tail.split_once(
                "await open(\n\t\t\t\t`timeline?limit=100&order=asc&change=${encodeURIComponent(config.fixture.equalTimestamp.changeId)}`",
            )
        })
        .map(|(proposal, _)| proposal)
        .expect("browser gate retains the withdrawn historical proposal check");
    assert!(
        historical_proposal.contains("historicalEventIdentity = await waitForExactTimelineEvent(")
            && historical_proposal.contains("historicalProposalEventId,")
            && historical_proposal.contains("await readExactDetailIdentitySources()")
            && historical_proposal.contains("containsExactDetailIdentity(")
            && !historical_proposal.contains("textContent?.includes(eventId)"),
        "historical proposal readiness must compare its exact data-event-id instead of matching opaque identity text"
    );

    let annotated_diff_round_trip = browser_program
        .split_once("// An annotated diff is a first-class full-frame exact route")
        .and_then(|(_, tail)| tail.split_once("await open(exact, layouts[1]"))
        .map(|(round_trip, _)| round_trip)
        .expect("browser gate retains the annotated-diff round-trip section");
    assert!(
        annotated_diff_round_trip
            .matches("await waitForCurrentRoute(focusedDiffRoute);")
            .count()
            >= 2
            && annotated_diff_round_trip.contains("canonicalReadingIdentityCount === 1")
            && annotated_diff_round_trip
                .contains("await waitForCanonicalRevisionSurface(\"Close\");")
            && annotated_diff_round_trip
                .contains("await waitForCanonicalRevisionSurface(\"Forward\");")
            && !annotated_diff_round_trip.contains("location.hash === expectedRoute")
            && !annotated_diff_round_trip
                .contains("detail.dataset.changeReadingKey === expectedReadingKey"),
        "annotated diff reload, Close, and Forward must use semantic route matching with separately labeled exits, not raw fragment order or a prior detail generation key"
    );
    assert!(
        annotated_diff_round_trip.contains("const expectedAnnotatedDiffRoute =")
            && annotated_diff_round_trip.contains("canonicalRevisionPath}/diff")
            && annotated_diff_round_trip
                .contains("const annotatedDiffEntryRouteMatches = await currentRouteMatches(")
            && annotated_diff_round_trip.contains("expectedAnnotatedDiffRoute,")
            && annotated_diff_round_trip.contains("annotated diff entry route"),
        "annotated diff entry must prove the complete semantic route derived from the accepted exact Revision instead of preserving an observed route as its oracle"
    );

    let narrow_exact_detail = browser_program
        .split_once("await diagnostics.section(\"Exact detail and reading\"")
        .and_then(|(_, tail)| {
            tail.split_once("await diagnostics.section(\"Exact resource availability\"")
        })
        .map(|(detail, _)| detail)
        .expect("browser gate retains the narrow exact-detail section");
    assert!(
        narrow_exact_detail.contains("[data-exact-diff-activation]")
            && narrow_exact_detail.contains("narrowExactActivationCount === 1")
            && narrow_exact_detail.contains("await currentRouteMatches(`#/${exact}`)")
            && !narrow_exact_detail.contains("document.activeElement?.id === \"detail-back\"")
            && !narrow_exact_detail.contains("narrowExactRoute === `#/${exact}`"),
        "narrow exact detail must focus the served exact-diff activation and compare the exact route semantically"
    );
}

#[test]
fn change_inspector_browser_gate_binds_retained_bytes_and_instruments_initial_navigation() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let script = fs::read_to_string(root.join("scripts/change-inspector-browser-verify.sh"))
        .expect("read Change Inspector browser gate");
    let browser_program =
        fs::read_to_string(root.join("scripts/change-inspector-browser-verify.mjs"))
            .expect("read Change Inspector browser program");
    let manifest_publisher =
        fs::read_to_string(root.join("scripts/change-inspector-browser-manifest.mjs"))
            .expect("read Change Inspector browser manifest publisher");

    assert!(
        script.contains("evidenceInventory")
            && script.contains("browser-artifacts")
            && script.contains("logs/browser-result.json")
            && script.contains("logs/browser-gate.log")
            && script.contains("logs/browser-program.mjs"),
        "the completion candidate must retain a named inventory for every PNG and browser result, log, and rendered program"
    );
    assert!(
        script.contains("sort") && script.contains("shasum -a 256"),
        "the completion candidate must publish SHA-256 evidence entries in a deterministic order"
    );
    assert!(
        manifest_publisher.contains("evidenceRoot")
            && manifest_publisher.contains("evidenceInventory")
            && manifest_publisher.contains("sha256")
            && manifest_publisher.contains("await link(")
            && !manifest_publisher.contains("await rename("),
        "manifest publication must verify retained bytes and atomically refuse replacement"
    );

    assert!(
        !script.contains("run_pw open \"$browser_url\""),
        "the shell must not navigate before browser diagnostics are installed"
    );
    let first_navigation = browser_program
        .find("await page.goto(")
        .expect("browser program performs its initial navigation");
    for observer in [
        "page.on(\"console\"",
        "page.on(\"pageerror\"",
        "page.on(\"requestfailed\"",
    ] {
        let observer_offset = browser_program
            .find(observer)
            .unwrap_or_else(|| panic!("browser program is missing {observer} observer"));
        assert!(
            observer_offset < first_navigation,
            "{observer} must be installed before the program's first page navigation"
        );
    }
}

#[test]
fn canonical_review_example_manifest_pins_the_record_and_all_authoritative_files() {
    let manifest_path = pack_root().join("manifest.json");
    let manifest: Value = serde_json::from_slice(
        &fs::read(&manifest_path)
            .unwrap_or_else(|error| panic!("read {}: {error}", manifest_path.display())),
    )
    .expect("manifest is valid JSON");

    assert_eq!(manifest["schema"], "pointbreak.review-example-pack");
    assert_eq!(manifest["version"], 1);
    assert_eq!(manifest["name"], "checkout-refactor");
    assert_eq!(manifest["classification"], "reproducible_sample_record");
    assert_eq!(manifest["producer"]["name"], "shore");
    assert_eq!(manifest["producer"]["version"], "0.5.0");
    let producer_commit = manifest["producer"]["commit"]
        .as_str()
        .expect("producer commit");
    assert_eq!(producer_commit.len(), 40);
    assert!(producer_commit.bytes().all(|byte| byte.is_ascii_hexdigit()));

    assert_eq!(manifest["record"]["eventCount"], 13);
    assert_eq!(
        manifest["record"]["eventSetHash"],
        "sha256:cabdabbbdf88ab71b43faee14cc28bf8e407e5c2bfc18d07af4bba126da12243"
    );
    assert_eq!(
        manifest["record"]["revision"],
        "rev:sha256:fa6981d38de12a850da707b69657e7a9153120c92a0dd08f534fbb40394d885f"
    );
    assert_eq!(
        manifest["record"]["track"],
        "example:marketing-review-proof"
    );
    assert_eq!(manifest["record"]["selectedAssessment"], "accepted");
    assert_eq!(manifest["record"]["verificationStatus"], "unsigned");
    assert_eq!(
        manifest["record"]["writerActors"],
        serde_json::json!([
            "actor:agent:pointbreak-example-author",
            "actor:agent:pointbreak-example-reviewer"
        ])
    );

    assert_eq!(manifest["events"]["path"], "events.json");
    assert_eq!(manifest["events"]["count"], 13);
    assert_eq!(manifest["source"]["bundlePath"], "source.bundle");
    assert_eq!(manifest["source"]["bundleRef"], "refs/heads/main");
    assert_eq!(
        manifest["source"]["base"]["commitOid"],
        "f1a8ed1801f669b1b846e482d198092cd6e617df"
    );
    assert_eq!(
        manifest["source"]["target"]["commitOid"],
        "3e7b4b3e1e1e7cccfc14a4c724204ff381b315e4"
    );
    assert_eq!(
        manifest["source"]["response"]["commitOid"],
        "c4f50c2dc010f69f9080d0ad6b0999728568c3c1"
    );
    for pointer in [
        "/source/base/treeOid",
        "/source/target/treeOid",
        "/source/response/treeOid",
    ] {
        let oid = manifest.pointer(pointer).and_then(Value::as_str).unwrap();
        assert_eq!(oid.len(), 40, "manifest field {pointer} is not a Git OID");
        assert!(oid.bytes().all(|byte| byte.is_ascii_hexdigit()));
    }
    assert_eq!(
        manifest["documents"]["history"]["path"],
        "exports/history.json"
    );
    assert_eq!(
        manifest["documents"]["history"]["schema"],
        "pointbreak.review-history"
    );
    assert_eq!(manifest["documents"]["history"]["version"], 1);
    assert_eq!(
        manifest["documents"]["revision"]["path"],
        "exports/revision.json"
    );
    assert_eq!(
        manifest["documents"]["revision"]["schema"],
        "pointbreak.review-revision"
    );
    assert_eq!(manifest["documents"]["revision"]["version"], 2);

    for pointer in [
        "/events/sha256",
        "/source/bundleSha256",
        "/documents/history/sha256",
        "/documents/revision/sha256",
    ] {
        let digest = manifest
            .pointer(pointer)
            .and_then(Value::as_str)
            .unwrap_or_else(|| panic!("manifest field {pointer} is missing"));
        assert_eq!(
            digest.len(),
            64,
            "manifest field {pointer} is not a SHA-256"
        );
        assert!(digest.bytes().all(|byte| byte.is_ascii_hexdigit()));
    }

    let artifacts = manifest["artifacts"]
        .as_array()
        .expect("artifacts is an array");
    assert_eq!(artifacts.len(), 1);
    assert_eq!(artifacts[0]["kind"], "object");
    assert_eq!(
        artifacts[0]["contentHash"],
        "sha256:c366c7cb8d826536573781f9136d9ccbcebc17301cc6aaba0b8a4f1c2f641327"
    );
    assert_ne!(
        artifacts[0]["contentHash"]
            .as_str()
            .unwrap()
            .trim_start_matches("sha256:"),
        artifacts[0]["sha256"]
            .as_str()
            .expect("artifact byte digest")
    );
}

#[test]
fn canonical_review_example_has_the_complete_causal_record() {
    let events: Vec<pointbreak::session::event::ShoreEvent> = serde_json::from_slice(
        &fs::read(pack_root().join("events.json")).expect("read events.json"),
    )
    .expect("events.json is valid JSON");
    assert_eq!(events.len(), 13);

    let event_types = events
        .iter()
        .map(|event| event.event_type.as_str())
        .collect::<Vec<_>>();
    assert_eq!(
        event_types,
        [
            "work_object_proposed",
            "revision_ref_associated",
            "review_observation_recorded",
            "validation_check_recorded",
            "input_request_opened",
            "review_assessment_recorded",
            "review_observation_recorded",
            "review_observation_recorded",
            "validation_check_recorded",
            "input_request_responded",
            "validation_check_recorded",
            "review_assessment_recorded",
            "revision_commit_associated",
        ]
    );
    assert!(events.iter().all(|event| event.signature.is_none()));

    let revision: Value = serde_json::from_slice(
        &fs::read(pack_root().join("exports/revision.json")).expect("read revision document"),
    )
    .expect("revision document is valid JSON");
    assert_eq!(revision["schema"], "pointbreak.review-revision");
    assert_eq!(revision["version"], 2);
    assert_eq!(
        revision["eventSetHash"],
        "sha256:cabdabbbdf88ab71b43faee14cc28bf8e407e5c2bfc18d07af4bba126da12243"
    );
    assert_eq!(revision["eventCount"], 13);
    assert_eq!(revision["currentAssessment"]["assessment"], "accepted");

    let assessments = revision["assessments"].as_array().expect("assessments");
    assert_eq!(assessments.len(), 2);
    assert_eq!(assessments[0]["assessment"], "needs_changes");
    assert_eq!(assessments[0]["status"], "replaced");
    assert_eq!(assessments[1]["assessment"], "accepted");
    assert_eq!(assessments[1]["status"], "current");
    assert_eq!(
        assessments[1]["replaces"],
        serde_json::json!([assessments[0]["id"]])
    );
    assert!(assessments.iter().all(|assessment| {
        assessment["writer"]["actorId"] == "actor:agent:pointbreak-example-reviewer"
    }));

    let request = &revision["inputRequests"][0];
    assert_eq!(request["reasonCode"], "manual_decision_required");
    assert_eq!(request["status"], "responded");
    assert_eq!(request["responses"][0]["outcome"], "approved");
    assert_eq!(
        request["writer"]["actorId"],
        "actor:agent:pointbreak-example-reviewer"
    );
    assert_eq!(
        request["responses"][0]["writer"]["actorId"],
        "actor:agent:pointbreak-example-author"
    );
    assert!(request["responses"][0]["reason"].is_string());

    let observations = revision["observations"].as_array().expect("observations");
    assert_eq!(observations.len(), 3);
    assert!(observations.iter().all(|observation| {
        observation["writer"]["actorId"] == "actor:agent:pointbreak-example-author"
    }));

    let validations = revision["validationChecks"]
        .as_array()
        .expect("validations");
    assert_eq!(validations.len(), 3);
    assert_eq!(validations[0]["status"], "failed");
    assert_eq!(validations[1]["status"], "passed");
    assert_eq!(validations[2]["status"], "passed");
    assert_eq!(
        validations
            .iter()
            .map(|validation| validation["writer"]["actorId"].as_str().unwrap())
            .collect::<std::collections::BTreeSet<_>>(),
        std::collections::BTreeSet::from([
            "actor:agent:pointbreak-example-author",
            "actor:agent:pointbreak-example-reviewer",
        ])
    );

    let response_commit = &revision["commitRange"]["currentCommits"][1];
    assert_eq!(response_commit["source"], "association");
    assert_eq!(
        response_commit["commitOid"],
        "c4f50c2dc010f69f9080d0ad6b0999728568c3c1"
    );
    assert!(revision["commitRange"].get("liveness").is_none());
}

#[test]
fn canonical_review_example_materializes_through_public_apis() {
    pack_support::verify_pack(&pack_root()).expect("verify canonical pack");
    let temp = tempdir().expect("temporary directory");
    let output = temp.path().join("checkout-refactor");
    pack_support::materialize_pack(&pack_root(), &output).expect("materialize canonical pack");

    let log = Command::new("git")
        .arg("-C")
        .arg(&output)
        .args(["log", "--format=%H", "--reverse"])
        .output()
        .expect("read materialized git log");
    assert!(log.status.success());
    assert_eq!(
        String::from_utf8(log.stdout)
            .unwrap()
            .lines()
            .collect::<Vec<_>>(),
        [
            "f1a8ed1801f669b1b846e482d198092cd6e617df",
            "3e7b4b3e1e1e7cccfc14a4c724204ff381b315e4",
            "c4f50c2dc010f69f9080d0ad6b0999728568c3c1",
        ]
    );

    let manifest: Value = serde_json::from_slice(
        &fs::read(pack_root().join("manifest.json")).expect("read manifest"),
    )
    .expect("manifest JSON");
    for name in ["base", "target", "response"] {
        let commit = manifest["source"][name]["commitOid"].as_str().unwrap();
        let expected_tree = manifest["source"][name]["treeOid"].as_str().unwrap();
        let tree = Command::new("git")
            .arg("-C")
            .arg(&output)
            .args(["rev-parse", &format!("{commit}^{{tree}}")])
            .output()
            .unwrap();
        assert!(tree.status.success());
        assert_eq!(
            String::from_utf8(tree.stdout).unwrap().trim(),
            expected_tree
        );
    }

    let test = Command::new("node")
        .arg("checkout.test.js")
        .current_dir(&output)
        .status()
        .expect("run materialized source tests");
    assert!(test.success());

    let events: Vec<pointbreak::session::event::ShoreEvent> =
        serde_json::from_slice(&fs::read(pack_root().join("events.json")).expect("read events"))
            .expect("deserialize events");
    let second_ingest = ingest_events(IngestEventsOptions::new(&output, events.clone()))
        .expect("idempotent event ingest");
    assert_eq!(second_ingest.events_created, 0);
    assert_eq!(second_ingest.events_existing, 13);

    for artifact in referenced_artifacts(&events).expect("artifact refs") {
        let entry = manifest["artifacts"]
            .as_array()
            .unwrap()
            .iter()
            .find(|entry| entry["contentHash"] == artifact.content_hash())
            .expect("artifact manifest entry");
        let result = import_artifact(ImportArtifactOptions::new(
            &output,
            artifact,
            fs::read(pack_root().join(entry["path"].as_str().unwrap())).unwrap(),
        ))
        .expect("idempotent artifact import");
        assert_eq!(
            result.outcome,
            pointbreak::session::ImportArtifactOutcome::Existing
        );
    }
}

#[test]
fn canonical_review_example_rejects_corruption_and_nonempty_destinations() {
    let temp = tempdir().expect("temporary directory");
    let corrupt = temp.path().join("corrupt-pack");
    copy_dir(&pack_root(), &corrupt);
    let artifact = fs::read_dir(corrupt.join("artifacts"))
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    fs::write(&artifact, b"corrupt").unwrap();
    let error = pack_support::verify_pack(&corrupt).unwrap_err().to_string();
    assert!(
        error.contains("digest mismatch"),
        "unexpected error: {error}"
    );

    let destination = temp.path().join("nonempty");
    fs::create_dir(&destination).unwrap();
    fs::write(destination.join("keep"), b"do not replace").unwrap();
    let error = pack_support::materialize_pack(&pack_root(), &destination)
        .unwrap_err()
        .to_string();
    assert!(error.contains("not empty"), "unexpected error: {error}");
    assert_eq!(
        fs::read(destination.join("keep")).unwrap(),
        b"do not replace"
    );
}

#[test]
fn canonical_review_example_rejects_unknown_schema_and_forged_relationships() {
    let temp = tempdir().expect("temporary directory");

    let unknown_schema = temp.path().join("unknown-schema");
    copy_dir(&pack_root(), &unknown_schema);
    let manifest_path = unknown_schema.join("manifest.json");
    let mut manifest: Value = serde_json::from_slice(&fs::read(&manifest_path).unwrap()).unwrap();
    manifest["schema"] = Value::String("pointbreak.unknown-pack".to_owned());
    write_json(&manifest_path, &manifest);
    let error = pack_support::verify_pack(&unknown_schema)
        .unwrap_err()
        .to_string();
    assert!(
        error.contains("manifest.schema"),
        "unexpected error: {error}"
    );

    let forged_relationship = temp.path().join("forged-relationship");
    copy_dir(&pack_root(), &forged_relationship);
    let revision_path = forged_relationship.join("exports/revision.json");
    let mut revision: Value = serde_json::from_slice(&fs::read(&revision_path).unwrap()).unwrap();
    revision["assessments"][1]["replaces"] = serde_json::json!(["assess:forged"]);
    write_json(&revision_path, &revision);

    let manifest_path = forged_relationship.join("manifest.json");
    let mut manifest: Value = serde_json::from_slice(&fs::read(&manifest_path).unwrap()).unwrap();
    manifest["documents"]["revision"]["sha256"] =
        Value::String(sha256(&fs::read(&revision_path).unwrap()));
    write_json(&manifest_path, &manifest);
    let error = pack_support::verify_pack(&forged_relationship)
        .unwrap_err()
        .to_string();
    assert!(
        error.contains("accepted replacement"),
        "unexpected error: {error}"
    );
}

fn copy_dir(source: &std::path::Path, destination: &std::path::Path) {
    fs::create_dir_all(destination).unwrap();
    for entry in fs::read_dir(source).unwrap() {
        let entry = entry.unwrap();
        let target = destination.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_dir(&entry.path(), &target);
        } else {
            fs::copy(entry.path(), target).unwrap();
        }
    }
}

fn write_json(path: &std::path::Path, value: &Value) {
    let mut bytes = serde_json::to_vec_pretty(value).unwrap();
    bytes.push(b'\n');
    fs::write(path, bytes).unwrap();
}

fn sha256(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
