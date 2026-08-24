//! Disposable correctness contract for seven representative interaction routes.
//!
//! This fixture is intentionally small. It proves semantic/output parity and
//! receipt truth only; it cannot supply a performance result. Representative-
//! scale readiness and evidence require a separately authorized disposable
//! L100 clone, never this repository or an owner store.

#![cfg(feature = "longitudinal-counting")]

mod support;

use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use pointbreak::bench_support::longitudinal::{
    INTERACTION_FACT_CURRENT_FORBIDDEN_PHASES_V1, INTERACTION_FACT_CURRENT_REQUIRED_PHASES_V1,
    InteractionActorV1 as Actor, InteractionChildScopeFactV1, InteractionExecutionIdentityV1,
    InteractionObservedRouteStateV1 as ObservedState,
    InteractionPerformanceExpectedContextV1 as ExpectedContext,
    InteractionPerformanceReceiptV1 as Receipt, InteractionPerformanceRoleV1 as PerformanceRole,
    InteractionRouteV1 as Route, InteractionScopeCoverageV1 as Coverage,
    InteractionSetupExpectationV1 as Setup, LongitudinalCounterReceiptV1,
    LongitudinalCountingScopeV1, LongitudinalDerivedAccessPhaseV1 as Phase,
    interaction_route_state_contract_v1,
};
use sha2::{Digest, Sha256};
use support::git_repo::GitRepo;
use support::{common_dir_store, pointbreak_env};

const TRACK: &str = "agent:interaction-fixture-reviewer";
const DOMAIN_ACTOR: &str = "actor:agent:claude-code";
const OFF_ENV: &[(&str, &str)] = &[
    ("POINTBREAK_DERIVED_ACCESS", "off"),
    ("POINTBREAK_ACTOR_ID", DOMAIN_ACTOR),
];
const ACTIVE_ENV: &[(&str, &str)] = &[
    ("POINTBREAK_DERIVED_ACCESS", "sqlite-wal-bodyless-v1"),
    ("POINTBREAK_ACTOR_ID", DOMAIN_ACTOR),
];

const ROUTE_IDS: [&str; 7] = [
    "assessment_current_result",
    "assessment_current_summary",
    "input_request_open_all_tracks",
    "observation_reviewer_list",
    "validation_reviewer_list",
    "version_json",
    "attention_current_or_fallback",
];

const AUTHORITATIVE_REQUIRED: [Phase; 5] = [
    Phase::WorkflowChangeReaderReplayH3,
    Phase::GitContextResolution,
    Phase::RouteRevisionSelection,
    Phase::RouteProjectionFold,
    Phase::SerializationAndOutput,
];

const AUTHORITATIVE_FORBIDDEN: [Phase; 11] = [
    Phase::CliCapabilityPreflightH1,
    Phase::WorkflowActivatedCapabilityProbe,
    Phase::WorkflowChangeStoreReopenInspection,
    Phase::OrdinaryReadStoreResolutionH2,
    Phase::SqliteSelection,
    Phase::CacheAndFallback,
    Phase::ReadTransaction,
    Phase::CheckpointAndWal,
    Phase::GenerationLeaseAndRetention,
    Phase::RouteBodyHydration,
    Phase::CarrierValidation,
];

struct Fixture {
    repo: GitRepo,
    revision: String,
    summary_content_hash: String,
    fixture_identity_sha256: String,
    journal_record_count: u64,
    event_count: u64,
    _manifest_dir: tempfile::TempDir,
}

struct RouteCase {
    id: &'static str,
    route: Route,
    arguments: Vec<String>,
    includes_body: bool,
}

struct ReceiptExpectation<'a> {
    expected: &'a ExpectedContext,
    stdout: &'a [u8],
    required_phases: &'a [Phase],
    forbidden_phases: &'a [Phase],
    exhaustive: bool,
}

fn fact_route_cases(repo: &str, revision: &str) -> [RouteCase; 5] {
    [
        RouteCase {
            id: ROUTE_IDS[0],
            route: Route::AssessmentCurrentResult,
            arguments: strings(&[
                "assessment",
                "show",
                "--repo",
                repo,
                "--exact-revision",
                revision,
                "--track",
                TRACK,
                "--format",
                "json",
            ]),
            includes_body: false,
        },
        RouteCase {
            id: ROUTE_IDS[1],
            route: Route::AssessmentCurrentSummary,
            arguments: strings(&[
                "assessment",
                "show",
                "--repo",
                repo,
                "--exact-revision",
                revision,
                "--track",
                TRACK,
                "--include-summary",
                "--format",
                "json",
            ]),
            includes_body: true,
        },
        RouteCase {
            id: ROUTE_IDS[2],
            route: Route::InputRequestOpenAllTracks,
            arguments: strings(&[
                "input-request",
                "list",
                "--repo",
                repo,
                "--exact-revision",
                revision,
                "--status",
                "open",
                "--format",
                "json",
            ]),
            includes_body: false,
        },
        RouteCase {
            id: ROUTE_IDS[3],
            route: Route::ObservationReviewerList,
            arguments: strings(&[
                "observation",
                "list",
                "--repo",
                repo,
                "--exact-revision",
                revision,
                "--track",
                TRACK,
                "--format",
                "json",
            ]),
            includes_body: false,
        },
        RouteCase {
            id: ROUTE_IDS[4],
            route: Route::ValidationReviewerList,
            arguments: strings(&[
                "validation",
                "list",
                "--repo",
                repo,
                "--exact-revision",
                revision,
                "--track",
                TRACK,
                "--format",
                "json",
            ]),
            includes_body: false,
        },
    ]
}

#[test]
fn cli_interaction_fact_matrix_freezes_only_the_five_catalog_routes() {
    let revision = format!("rev:sha256:{}", "1".repeat(64));
    assert_eq!(
        fact_route_cases("/tmp/pointbreak-fact-contract", &revision).map(|case| case.route),
        Route::FACT_READS,
    );

    for route in Route::FACT_READS {
        let current = interaction_route_state_contract_v1(route, Setup::FactActiveCurrent)
            .expect("current fact contract");
        assert_eq!(
            current.performance_role,
            PerformanceRole::ProvisionalTarget {
                sample_count: 5,
                strict_upper_bound_millis: 2_000,
            }
        );
        assert!(current.historical_evidence_unchanged);
        for setup in [Setup::FactExplicitOff, Setup::FactActiveUnavailable] {
            assert_eq!(
                interaction_route_state_contract_v1(route, setup)
                    .expect("compatibility fact contract")
                    .performance_role,
                PerformanceRole::CompatibilityCharacterization,
            );
        }
    }
}

#[test]
fn cli_interaction_performance_cases_preserve_semantics() {
    let fixture = fixture();
    let execution = execution_identity();
    let receipt_dir = tempfile::tempdir().expect("temporary receipt directory");
    let repo = fixture.repo.path().to_string_lossy().into_owned();

    let primary_cases = fact_route_cases(&repo, &fixture.revision);

    let mut observed_route_ids = BTreeSet::new();
    let mut representative_receipts = Vec::new();
    for case in primary_cases {
        assert!(observed_route_ids.insert(case.id), "duplicate route id");
        let expected = expected_context(
            execution.clone(),
            case.route,
            case.arguments.clone(),
            Setup::FactExplicitOff,
            Some(&fixture),
            route_track(case.route),
        );
        let mut required = AUTHORITATIVE_REQUIRED.to_vec();
        let mut forbidden = AUTHORITATIVE_FORBIDDEN.to_vec();
        if case.includes_body {
            required.extend([Phase::RouteBodyHydration, Phase::CarrierValidation]);
            forbidden.retain(|phase| {
                !matches!(phase, Phase::RouteBodyHydration | Phase::CarrierValidation)
            });
        }
        let receipt = run_success_case(
            case.id,
            &case.arguments,
            OFF_ENV,
            &expected,
            &receipt_dir,
            &required,
            &forbidden,
            true,
        );
        assert_eq!(
            receipt.counters.body_artifact_reads > 0,
            case.includes_body,
            "{} body-read applicability drifted",
            case.id
        );
        assert_eq!(
            receipt.observed.route_state,
            ObservedState::AuthoritativeReplay
        );
        assert_eq!(receipt.counters.fact_sqlite_rows_selected, 0);
        assert_eq!(receipt.counters.authoritative_fallbacks, 0);
        assert_eq!(receipt.counters.full_history_fallbacks, 0);
        assert!(receipt.children.is_empty());
        assert_eq!(
            receipt.counters.directory_entries_walked, fixture.journal_record_count,
            "{} must perform one strict Journal inspection",
            case.id
        );
        assert_eq!(
            receipt.counters.carrier_opens,
            fixture.journal_record_count + 2,
            "{} must add only the bounded capability pair",
            case.id
        );
        assert_eq!(receipt.counters.change_capability_carriers_opened, 2);
        assert_eq!(receipt.counters.event_decodes, fixture.event_count);
        assert_eq!(receipt.counters.event_validations, fixture.event_count);
        let json = run_binary(&case.arguments, OFF_ENV);
        let pretty_arguments = arguments_with_format(&case.arguments, "json-pretty");
        let pretty = run_binary(&pretty_arguments, OFF_ENV);
        assert_success(&format!("{} json-pretty", case.id), &pretty);
        assert_eq!(
            pretty.stderr, json.stderr,
            "{} pretty stderr drift",
            case.id
        );
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(&pretty.stdout).unwrap(),
            serde_json::from_slice::<serde_json::Value>(&json.stdout).unwrap(),
            "{} JSON lane semantics drifted",
            case.id
        );
        let text_arguments = arguments_with_format(&case.arguments, "text");
        let text = run_binary(&text_arguments, OFF_ENV);
        let repeated_text = run_binary(&text_arguments, OFF_ENV);
        assert_success(&format!("{} text", case.id), &text);
        assert_eq!(text.stderr, json.stderr, "{} text stderr drift", case.id);
        assert_eq!(
            text.stdout, repeated_text.stdout,
            "{} text bytes drifted",
            case.id
        );
        assert!(!text.stdout.is_empty(), "{} text lane is empty", case.id);
        representative_receipts.push(receipt);
    }

    for case in fact_route_cases(&repo, &fixture.revision) {
        let authoritative = run_binary(&case.arguments, OFF_ENV);
        let unavailable = run_binary(&case.arguments, ACTIVE_ENV);
        assert_eq!(
            unavailable.status.code(),
            authoritative.status.code(),
            "{} active-unavailable exit parity",
            case.id
        );
        assert_eq!(
            unavailable.stdout, authoritative.stdout,
            "{} active-unavailable stdout must stay unlabeled",
            case.id
        );
        assert_eq!(
            unavailable.stderr, authoritative.stderr,
            "{} active-unavailable stderr must stay unlabeled",
            case.id
        );
        let expected = expected_context(
            execution.clone(),
            case.route,
            case.arguments.clone(),
            Setup::FactActiveUnavailable,
            Some(&fixture),
            route_track(case.route),
        );
        let mut required = AUTHORITATIVE_REQUIRED.to_vec();
        let mut forbidden = AUTHORITATIVE_FORBIDDEN.to_vec();
        if case.includes_body {
            required.extend([Phase::RouteBodyHydration, Phase::CarrierValidation]);
            forbidden.retain(|phase| {
                !matches!(phase, Phase::RouteBodyHydration | Phase::CarrierValidation)
            });
        }
        let receipt = run_success_case(
            &format!("{}-active-unavailable", case.id),
            &case.arguments,
            ACTIVE_ENV,
            &expected,
            &receipt_dir,
            &required,
            &forbidden,
            true,
        );
        assert_eq!(
            receipt.observed.route_state,
            ObservedState::UnlabeledFallbackToAuthoritative
        );
        assert_eq!(receipt.counters.fact_sqlite_rows_selected, 0);
        assert_eq!(receipt.counters.authoritative_fallbacks, 1);
        assert_eq!(receipt.counters.full_history_fallbacks, 1);
        assert_eq!(receipt.children.len(), 1);
        assert_eq!(receipt.children[0].actor, Actor::BackgroundMaintenance);
        assert_eq!(receipt.children[0].coverage, Coverage::Complete);
        assert_eq!(
            derived_publication_count(fixture.repo.path()),
            0,
            "{} must not publish a request-owned generation",
            case.id
        );
        representative_receipts.push(receipt);
    }

    assert!(observed_route_ids.insert(ROUTE_IDS[5]));
    let version_arguments = strings(&["version", "--format", "json"]);
    let version_expected = expected_context(
        execution.clone(),
        Route::VersionJson,
        version_arguments.clone(),
        Setup::NotApplicable,
        None,
        None,
    );
    let version_receipt = run_success_case(
        ROUTE_IDS[5],
        &version_arguments,
        &[],
        &version_expected,
        &receipt_dir,
        &[Phase::SerializationAndOutput],
        &[
            Phase::CliCapabilityPreflightH1,
            Phase::WorkflowActivatedCapabilityProbe,
            Phase::OrdinaryReadStoreResolutionH2,
            Phase::WorkflowChangeReaderReplayH3,
            Phase::WorkflowChangeStoreReopenInspection,
            Phase::SqliteSelection,
            Phase::CarrierValidation,
            Phase::CacheAndFallback,
            Phase::ReadTransaction,
            Phase::CheckpointAndWal,
            Phase::GenerationLeaseAndRetention,
        ],
        false,
    );
    assert_eq!(version_receipt.counters.event_decodes, 0);
    assert_eq!(version_receipt.counters.event_folds, 0);

    assert!(observed_route_ids.insert(ROUTE_IDS[6]));
    let attention_arguments = strings(&[
        "attention",
        "list",
        "--repo",
        &repo,
        "--revision",
        &fixture.revision,
        "--format",
        "json",
    ]);
    let attention_scenarios = [
        (
            "cold-inactive",
            Setup::AttentionColdInactive,
            ObservedState::AuthoritativeReplay,
            OFF_ENV,
            vec![
                Phase::WorkflowChangeReaderReplayH3,
                Phase::RouteProjectionFold,
                Phase::SerializationAndOutput,
            ],
            vec![
                Phase::CliCapabilityPreflightH1,
                Phase::WorkflowActivatedCapabilityProbe,
                Phase::WorkflowChangeStoreReopenInspection,
                Phase::OrdinaryReadStoreResolutionH2,
                Phase::CacheAndFallback,
                Phase::ReadTransaction,
                Phase::SqliteSelection,
                Phase::GenerationLeaseAndRetention,
                Phase::RouteBodyHydration,
                Phase::CarrierValidation,
            ],
            true,
        ),
        (
            "active-unavailable",
            Setup::AttentionActiveUnavailable,
            ObservedState::LabeledFallbackToAuthoritative,
            ACTIVE_ENV,
            vec![
                Phase::WorkflowChangeReaderReplayH3,
                Phase::CacheAndFallback,
                Phase::RouteProjectionFold,
                Phase::SerializationAndOutput,
            ],
            vec![
                Phase::CliCapabilityPreflightH1,
                Phase::WorkflowActivatedCapabilityProbe,
                Phase::WorkflowChangeStoreReopenInspection,
                Phase::OrdinaryReadStoreResolutionH2,
                Phase::SqliteSelection,
                Phase::ReadTransaction,
                Phase::RouteBodyHydration,
                Phase::CarrierValidation,
            ],
            true,
        ),
    ];
    for (name, setup, state, env, required, forbidden, exhaustive) in attention_scenarios {
        let expected = expected_context(
            execution.clone(),
            Route::AttentionCurrentOrFallback,
            attention_arguments.clone(),
            setup,
            Some(&fixture),
            None,
        );
        let receipt = run_success_case(
            &format!("{}-{name}", ROUTE_IDS[6]),
            &attention_arguments,
            env,
            &expected,
            &receipt_dir,
            &required,
            &forbidden,
            exhaustive,
        );
        assert_eq!(receipt.observed.route_state, state);
        assert_eq!(
            receipt.counters.directory_entries_walked, fixture.journal_record_count,
            "{name} must perform one strict Journal inspection"
        );
        assert_eq!(
            receipt.counters.carrier_opens,
            fixture.journal_record_count + 2,
            "{name} must add only the bounded capability pair"
        );
        assert_eq!(receipt.counters.change_capability_carriers_opened, 2);
        assert_eq!(receipt.counters.event_decodes, fixture.event_count);
        assert_eq!(receipt.counters.event_validations, fixture.event_count);
        assert_eq!(receipt.counters.body_artifact_reads, 0);
        assert_eq!(
            receipt.counters.authoritative_fallbacks,
            u64::from(name == "active-unavailable")
        );
        assert_eq!(
            receipt.counters.full_history_fallbacks,
            u64::from(name == "active-unavailable")
        );
        let document = assert_attention_output_lanes(&attention_arguments, env, name);
        assert!(document["eventSetHash"].as_str().is_some());
        assert!(document.get("projectionStamp").is_none());
        assert_eq!(
            derived_publication_count(fixture.repo.path()),
            0,
            "{name} must not publish a request-owned replacement"
        );
        representative_receipts.push(receipt);
    }

    build_derived(&fixture.repo);
    for case in fact_route_cases(&repo, &fixture.revision) {
        let authoritative = run_binary(&case.arguments, OFF_ENV);
        let current = run_binary(&case.arguments, ACTIVE_ENV);
        assert_eq!(
            current.status.code(),
            authoritative.status.code(),
            "{} active-current exit parity",
            case.id
        );
        assert_eq!(
            current.stdout, authoritative.stdout,
            "{} active-current stdout parity",
            case.id
        );
        assert_eq!(
            current.stderr, authoritative.stderr,
            "{} active-current stderr parity",
            case.id
        );
        let expected = expected_context(
            execution.clone(),
            case.route,
            case.arguments.clone(),
            Setup::FactActiveCurrent,
            Some(&fixture),
            route_track(case.route),
        );
        let mut required = INTERACTION_FACT_CURRENT_REQUIRED_PHASES_V1.to_vec();
        required.extend([
            Phase::GitContextResolution,
            Phase::RouteRevisionSelection,
            Phase::RouteProjectionFold,
            Phase::SerializationAndOutput,
        ]);
        if case.includes_body {
            required.extend([Phase::RouteBodyHydration, Phase::CarrierValidation]);
        }
        let receipt = run_success_case(
            &format!("{}-active-current", case.id),
            &case.arguments,
            ACTIVE_ENV,
            &expected,
            &receipt_dir,
            &required,
            &INTERACTION_FACT_CURRENT_FORBIDDEN_PHASES_V1,
            false,
        );
        assert_eq!(receipt.observed.route_state, ObservedState::DerivedCurrent);
        assert_eq!(receipt.counters.directory_entries_walked, 0);
        assert_eq!(receipt.counters.strict_journal_inspections, 0);
        assert_eq!(receipt.counters.change_semantic_constructions, 0);
        assert_eq!(receipt.counters.change_projection_constructions, 0);
        assert!(receipt.counters.fact_sqlite_rows_selected > 0);
        assert_eq!(receipt.counters.authoritative_fallbacks, 0);
        assert_eq!(receipt.counters.full_history_fallbacks, 0);
        assert!(receipt.children.is_empty());
        assert_eq!(receipt.counters.body_artifact_reads > 0, case.includes_body);
        assert_eq!(derived_publication_count(fixture.repo.path()), 1);
        representative_receipts.push(receipt);
    }
    let derived_expected = expected_context(
        execution,
        Route::AttentionCurrentOrFallback,
        attention_arguments.clone(),
        Setup::AttentionDerivedCurrent,
        Some(&fixture),
        None,
    );
    let derived_receipt = run_success_case(
        &format!("{}-derived-current", ROUTE_IDS[6]),
        &attention_arguments,
        ACTIVE_ENV,
        &derived_expected,
        &receipt_dir,
        &[
            Phase::GenerationLeaseAndRetention,
            Phase::ReadTransaction,
            Phase::SqliteSelection,
            Phase::SerializationAndOutput,
        ],
        &[
            Phase::CliCapabilityPreflightH1,
            Phase::OrdinaryReadStoreResolutionH2,
            Phase::WorkflowChangeReaderReplayH3,
            Phase::RouteBodyHydration,
            Phase::CarrierValidation,
            Phase::CacheAndFallback,
        ],
        false,
    );
    assert_eq!(
        derived_receipt.observed.route_state,
        ObservedState::DerivedCurrent
    );
    assert_eq!(derived_receipt.counters.directory_entries_walked, 0);
    assert_eq!(derived_receipt.counters.event_decodes, 0);
    assert_eq!(derived_receipt.counters.event_validations, 0);
    assert_eq!(derived_receipt.counters.authoritative_fallbacks, 0);
    assert_eq!(derived_receipt.counters.full_history_fallbacks, 0);
    let derived_document =
        assert_attention_output_lanes(&attention_arguments, ACTIVE_ENV, "derived-current");
    assert!(derived_document["projectionStamp"].as_str().is_some());
    assert!(derived_document.get("eventSetHash").is_none());
    representative_receipts.push(derived_receipt);

    let drift = pointbreak_env(
        [
            "observation",
            "add",
            "--repo",
            &repo,
            "--revision",
            &fixture.revision,
            "--track",
            "agent:interaction-fixture-drift",
            "--title",
            "post-build authority drift",
        ],
        OFF_ENV,
    );
    assert_success("append post-build authority drift", &drift);
    let catching_up = fact_route_cases(&repo, &fixture.revision)
        .into_iter()
        .next()
        .expect("representative catching-up fact case");
    let authoritative = run_binary(&catching_up.arguments, OFF_ENV);
    assert_success("catching-up authoritative reference", &authoritative);
    let catching_up_expected = expected_context(
        derived_expected.execution.clone(),
        catching_up.route,
        catching_up.arguments.clone(),
        Setup::FactActiveUnavailable,
        Some(&fixture),
        route_track(catching_up.route),
    );
    let mut catching_up_required = AUTHORITATIVE_REQUIRED.to_vec();
    catching_up_required.push(Phase::CheckpointAndWal);
    let catching_up_forbidden = AUTHORITATIVE_FORBIDDEN
        .into_iter()
        .filter(|phase| *phase != Phase::CheckpointAndWal)
        .collect::<Vec<_>>();
    let catching_up_receipt = run_counted_success_case_against(
        "assessment-current-result-catching-up",
        &catching_up.arguments,
        ACTIVE_ENV,
        &authoritative,
        &catching_up_expected,
        &receipt_dir,
        &catching_up_required,
        &catching_up_forbidden,
        true,
    );
    assert_eq!(
        catching_up_receipt.observed.route_state,
        ObservedState::UnlabeledFallbackToAuthoritative
    );
    assert_eq!(catching_up_receipt.counters.fact_sqlite_rows_selected, 0);
    assert_eq!(catching_up_receipt.counters.authoritative_fallbacks, 1);
    assert_eq!(catching_up_receipt.counters.full_history_fallbacks, 1);
    assert_eq!(catching_up_receipt.children.len(), 1);
    assert_eq!(
        catching_up_receipt.children[0].actor,
        Actor::BackgroundMaintenance
    );
    assert_eq!(catching_up_receipt.children[0].coverage, Coverage::Complete);
    assert_eq!(derived_publication_count(fixture.repo.path()), 1);
    build_derived(&fixture.repo);
    assert_eq!(derived_publication_count(fixture.repo.path()), 1);

    assert_eq!(
        observed_route_ids,
        ROUTE_IDS.into_iter().collect(),
        "the matrix must contain seven and only seven route ids"
    );

    assert_receipt_negatives(&version_receipt, &version_expected);
    assert_selected_carrier_corruption_fails_closed(
        &fixture,
        &representative_receipts[1].expected,
        &receipt_dir,
    );
    assert_attention_hint_precedes_strict_replay_error(&fixture, &attention_arguments);
}

#[test]
fn abbreviated_revision_selectors_preserve_legacy_counted_paths() {
    let fixture = fixture();
    let receipt_dir = tempfile::tempdir().expect("temporary receipt directory");
    let repo = fixture.repo.path().to_string_lossy().into_owned();
    let digest = fixture
        .revision
        .rsplit_once("sha256:")
        .expect("Revision id has a sha256 digest")
        .1;
    let fragment = &digest[..8];

    for case in fact_route_cases(&repo, &fixture.revision) {
        let case_name = format!("fragment-{}", case.id);
        let fragment_arguments = arguments_with_revision_selector(&case.arguments, fragment);
        run_legacy_fragment_case(
            &case_name,
            &case.arguments,
            &fragment_arguments,
            OFF_ENV,
            &receipt_dir,
            &fixture,
            5,
        );
    }

    let attention_full_arguments = strings(&[
        "attention",
        "list",
        "--repo",
        &repo,
        "--revision",
        &fixture.revision,
        "--format",
        "json",
    ]);
    let attention_fragment_arguments =
        arguments_with_revision_selector(&attention_full_arguments, fragment);
    run_legacy_fragment_case(
        "fragment-attention-off",
        &attention_full_arguments,
        &attention_fragment_arguments,
        OFF_ENV,
        &receipt_dir,
        &fixture,
        2,
    );
    run_legacy_fragment_case(
        "fragment-attention-active-unavailable",
        &attention_full_arguments,
        &attention_fragment_arguments,
        ACTIVE_ENV,
        &receipt_dir,
        &fixture,
        2,
    );

    build_derived(&fixture.repo);
    run_legacy_fragment_case(
        "fragment-attention-current",
        &attention_full_arguments,
        &attention_fragment_arguments,
        ACTIVE_ENV,
        &receipt_dir,
        &fixture,
        1,
    );
}

#[test]
fn selector_errors_and_excluded_fact_shapes_stay_legacy_and_byte_equal() {
    let fixture = fixture();
    let repo = fixture.repo.path().to_string_lossy().into_owned();
    build_derived(&fixture.repo);

    let base = fact_route_cases(&repo, &fixture.revision);
    let excluded_cases = [
        (
            "assessment-all",
            arguments_with_extra(&base[0].arguments, &["--all"]),
        ),
        (
            "assessment-all-tracks",
            arguments_without_option_value(&base[0].arguments, "--track"),
        ),
        (
            "input-request-track",
            arguments_with_extra(&base[2].arguments, &["--track", TRACK]),
        ),
        (
            "input-request-mode",
            arguments_with_extra(&base[2].arguments, &["--mode", "operative"]),
        ),
        (
            "input-request-file",
            arguments_with_extra(&base[2].arguments, &["--file", "src/lib.rs"]),
        ),
        (
            "input-request-status-all",
            arguments_with_option_value(&base[2].arguments, "--status", "all"),
        ),
        (
            "input-request-body",
            arguments_with_extra(&base[2].arguments, &["--include-body"]),
        ),
        (
            "observation-all-tracks",
            arguments_without_option_value(&base[3].arguments, "--track"),
        ),
        (
            "observation-file",
            arguments_with_extra(&base[3].arguments, &["--file", "src/lib.rs"]),
        ),
        (
            "observation-tag",
            arguments_with_extra(&base[3].arguments, &["--tag", "security"]),
        ),
        (
            "observation-body",
            arguments_with_extra(&base[3].arguments, &["--include-body"]),
        ),
        (
            "validation-all-tracks",
            arguments_without_option_value(&base[4].arguments, "--track"),
        ),
        (
            "validation-status",
            arguments_with_extra(&base[4].arguments, &["--status", "passed"]),
        ),
        (
            "validation-body",
            arguments_with_extra(&base[4].arguments, &["--include-body"]),
        ),
    ];

    for (name, arguments) in excluded_cases {
        let off = run_binary(&arguments, OFF_ENV);
        assert_success(&format!("{name} explicit-off"), &off);
        let active = run_binary(&arguments, ACTIVE_ENV);
        assert_eq!(active.status.code(), off.status.code(), "{name} status");
        assert_eq!(active.stdout, off.stdout, "{name} stdout");
        assert_eq!(active.stderr, off.stderr, "{name} stderr");
    }

    for selector in ["abc", "not-hex", "deadbeef", "obj:11111111"] {
        for case in fact_route_cases(&repo, &fixture.revision) {
            let arguments = arguments_with_revision_selector(&case.arguments, selector);
            let off = run_binary(&arguments, OFF_ENV);
            assert!(!off.status.success(), "{} {selector} must fail", case.id);
            let active = run_binary(&arguments, ACTIVE_ENV);
            assert_eq!(
                active.status.code(),
                off.status.code(),
                "{} {selector} status",
                case.id
            );
            assert_eq!(active.stdout, off.stdout, "{} {selector} stdout", case.id);
            assert_eq!(active.stderr, off.stderr, "{} {selector} stderr", case.id);
        }
    }
}

fn fixture() -> Fixture {
    let repo = GitRepo::new();
    repo.write("src/lib.rs", "pub fn value() -> u32 { 1 }\n");
    repo.commit_all("base");
    repo.write("src/lib.rs", "pub fn value() -> u32 { 2 }\n");
    let repo_arg = repo.path().to_string_lossy().into_owned();

    let capture = pointbreak_env(["capture", "--repo", &repo_arg], OFF_ENV);
    assert_success("capture fixture Revision", &capture);
    let capture: serde_json::Value = serde_json::from_slice(&capture.stdout).expect("capture JSON");
    let revision = capture["revision"]["revisionId"]
        .as_str()
        .expect("captured Revision id")
        .to_owned();

    let summary = format!("selected summary carrier:{}", "s".repeat(5_000));
    let assessment = pointbreak_env(
        [
            "assessment",
            "add",
            "--repo",
            &repo_arg,
            "--revision",
            &revision,
            "--track",
            TRACK,
            "--assessment",
            "accepted",
            "--summary",
            &summary,
        ],
        OFF_ENV,
    );
    assert_success("add fixture assessment", &assessment);
    let assessment: serde_json::Value =
        serde_json::from_slice(&assessment.stdout).expect("assessment JSON");
    let summary_content_hash = assessment["summaryContentHash"]
        .as_str()
        .expect("externalized summary content hash")
        .to_owned();

    for (label, output) in [
        (
            "add fixture observation",
            pointbreak_env(
                [
                    "observation",
                    "add",
                    "--repo",
                    &repo_arg,
                    "--revision",
                    &revision,
                    "--track",
                    TRACK,
                    "--title",
                    "Observed behavior",
                    "--body",
                    "fixture observation body",
                ],
                OFF_ENV,
            ),
        ),
        (
            "add fixture validation",
            pointbreak_env(
                [
                    "validation",
                    "add",
                    "--repo",
                    &repo_arg,
                    "--revision",
                    &revision,
                    "--track",
                    TRACK,
                    "--check-name",
                    "fixture check",
                    "--status",
                    "passed",
                    "--summary",
                    "fixture validation summary",
                ],
                OFF_ENV,
            ),
        ),
        (
            "open all-tracks fixture request",
            pointbreak_env(
                [
                    "input-request",
                    "open",
                    "--repo",
                    &repo_arg,
                    "--revision",
                    &revision,
                    "--track",
                    "agent:another-fixture-track",
                    "--title",
                    "Need fixture input",
                    "--reason",
                    "insufficient-evidence",
                ],
                OFF_ENV,
            ),
        ),
    ] {
        assert_success(label, &output);
    }

    let manifest_dir = tempfile::tempdir().expect("temporary fixture manifest directory");
    let manifest = serde_json::json!({
        "schema": "pointbreak.interaction-performance-test-fixture.v1",
        "repository": repo_arg,
        "revision": revision,
        "reviewerTrack": TRACK,
        "domainActor": DOMAIN_ACTOR,
        "assessmentSummaryContentHash": summary_content_hash,
    });
    let manifest_bytes = serde_json::to_vec(&manifest).expect("fixture manifest JSON");
    fs::write(
        manifest_dir.path().join("fixture-manifest.json"),
        &manifest_bytes,
    )
    .expect("write fixture manifest");
    let fixture_identity_sha256 = sha256(&manifest_bytes);
    let authority = pointbreak::session::store_capability_for_repo(repo.path())
        .expect("inspect fixture authority")
        .cursor;

    Fixture {
        repo,
        revision,
        summary_content_hash,
        fixture_identity_sha256,
        journal_record_count: authority.journal_record_count,
        event_count: authority.event_count,
        _manifest_dir: manifest_dir,
    }
}

fn execution_identity() -> InteractionExecutionIdentityV1 {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let binary_path = PathBuf::from(env!("CARGO_BIN_EXE_pointbreak"))
        .canonicalize()
        .expect("canonical pointbreak test binary");
    InteractionExecutionIdentityV1 {
        source_commit: command_stdout(root, "git", &["rev-parse", "HEAD"]),
        source_tree: command_stdout(root, "git", &["rev-parse", "HEAD^{tree}"]),
        cargo_lock_sha256: sha256(&fs::read(root.join("Cargo.lock")).expect("read Cargo.lock")),
        binary_path: binary_path.to_string_lossy().into_owned(),
        binary_sha256: sha256(&fs::read(&binary_path).expect("read pointbreak test binary")),
        build_profile: "test".to_owned(),
        rustc_version: command_stdout(root, "rustc", &["--version"]),
        features: strings(&["bench", "gix", "longitudinal-counting"]),
    }
}

fn expected_context(
    execution: InteractionExecutionIdentityV1,
    route: Route,
    arguments: Vec<String>,
    setup_expectation: Setup,
    fixture: Option<&Fixture>,
    track: Option<&str>,
) -> ExpectedContext {
    let expected_child_actors = if matches!(
        &setup_expectation,
        Setup::FactActiveUnavailable
            | Setup::FactPostSelectionFailure
            | Setup::AttentionActiveUnavailable
    ) {
        BTreeMap::from([(Actor::BackgroundMaintenance, 1)])
    } else {
        BTreeMap::new()
    };
    ExpectedContext {
        execution,
        route,
        arguments,
        setup_expectation,
        fixture_identity_sha256: fixture.map(|fixture| fixture.fixture_identity_sha256.clone()),
        revision: fixture.map(|fixture| fixture.revision.clone()),
        track: track.map(str::to_owned),
        domain_actor: fixture.map(|_| DOMAIN_ACTOR.to_owned()),
        expected_child_actors,
    }
}

fn route_track(route: Route) -> Option<&'static str> {
    matches!(
        route,
        Route::AssessmentCurrentResult
            | Route::AssessmentCurrentSummary
            | Route::ObservationReviewerList
            | Route::ValidationReviewerList
    )
    .then_some(TRACK)
}

#[allow(clippy::too_many_arguments)]
fn run_success_case(
    case_name: &str,
    arguments: &[String],
    env: &[(&str, &str)],
    expected: &ExpectedContext,
    receipt_dir: &tempfile::TempDir,
    required_phases: &[Phase],
    forbidden_phases: &[Phase],
    exhaustive: bool,
) -> Receipt {
    let ordinary = run_binary(arguments, env);
    assert_success(case_name, &ordinary);

    run_counted_success_case_against(
        case_name,
        arguments,
        env,
        &ordinary,
        expected,
        receipt_dir,
        required_phases,
        forbidden_phases,
        exhaustive,
    )
}

#[allow(clippy::too_many_arguments)]
fn run_counted_success_case_against(
    case_name: &str,
    arguments: &[String],
    env: &[(&str, &str)],
    reference: &Output,
    expected: &ExpectedContext,
    receipt_dir: &tempfile::TempDir,
    required_phases: &[Phase],
    forbidden_phases: &[Phase],
    exhaustive: bool,
) -> Receipt {
    assert_success(case_name, reference);

    let receipt_path = receipt_dir.path().join(format!("{case_name}.json"));
    let encoded = encode_request(case_name, expected, &receipt_path);
    let counted_arguments = [
        vec!["--longitudinal-counting".to_owned(), encoded],
        arguments.to_vec(),
    ]
    .concat();
    let counted = run_binary(&counted_arguments, env);

    assert_eq!(
        counted.status.code(),
        reference.status.code(),
        "{case_name} exit parity\nordinary stderr:\n{}\ncounted stderr:\n{}",
        String::from_utf8_lossy(&reference.stderr),
        String::from_utf8_lossy(&counted.stderr)
    );
    assert_eq!(
        counted.stdout, reference.stdout,
        "{case_name} stdout parity"
    );
    assert_eq!(
        counted.stderr, reference.stderr,
        "{case_name} stderr parity"
    );

    let receipt: Receipt = serde_json::from_slice(
        &fs::read(&receipt_path)
            .unwrap_or_else(|error| panic!("read {case_name} receipt: {error}")),
    )
    .unwrap_or_else(|error| panic!("parse {case_name} receipt: {error}"));
    check_case_receipt(
        &receipt,
        ReceiptExpectation {
            expected,
            stdout: &reference.stdout,
            required_phases,
            forbidden_phases,
            exhaustive,
        },
    )
    .unwrap_or_else(|error| panic!("{case_name} receipt drift: {error}"));
    receipt
}

fn run_legacy_fragment_case(
    case_name: &str,
    full_arguments: &[String],
    fragment_arguments: &[String],
    env: &[(&str, &str)],
    receipt_dir: &tempfile::TempDir,
    fixture: &Fixture,
    expected_decode_passes: u64,
) {
    let full = run_binary(full_arguments, env);
    assert_success(&format!("{case_name} full selector"), &full);
    let fragment = run_binary(fragment_arguments, env);
    assert_success(case_name, &fragment);
    assert_eq!(fragment.status.code(), full.status.code(), "{case_name}");
    assert_eq!(fragment.stdout, full.stdout, "{case_name} stdout parity");
    assert_eq!(fragment.stderr, full.stderr, "{case_name} stderr parity");

    let receipt_path = receipt_dir.path().join(format!("{case_name}.json"));
    let encoded = encode_counter_request(case_name, &receipt_path);
    let counted_arguments = [
        vec!["--longitudinal-counting".to_owned(), encoded],
        fragment_arguments.to_vec(),
    ]
    .concat();
    let counted = run_binary(&counted_arguments, env);
    assert_eq!(counted.status.code(), fragment.status.code(), "{case_name}");
    assert_eq!(
        counted.stdout, fragment.stdout,
        "{case_name} counted stdout"
    );
    assert_eq!(
        counted.stderr, fragment.stderr,
        "{case_name} counted stderr"
    );

    let receipt: LongitudinalCounterReceiptV1 = serde_json::from_slice(
        &fs::read(&receipt_path)
            .unwrap_or_else(|error| panic!("read {case_name} receipt: {error}")),
    )
    .unwrap_or_else(|error| panic!("parse {case_name} receipt: {error}"));
    receipt
        .validate()
        .unwrap_or_else(|error| panic!("validate {case_name} receipt: {error}"));
    assert!(receipt.success, "{case_name} counted product result");
    assert_eq!(
        receipt.counters.event_decodes,
        fixture.event_count * expected_decode_passes,
        "{case_name} must retain its legacy complete-history decode multiplicity"
    );
    assert_eq!(
        receipt.counters.event_validations, receipt.counters.event_decodes,
        "{case_name} must validate every decoded event"
    );
    assert!(
        receipt.counters.directory_entries_walked
            >= fixture.journal_record_count * expected_decode_passes,
        "{case_name} must truthfully count every legacy complete-history walk"
    );
}

fn check_case_receipt(
    receipt: &Receipt,
    expectation: ReceiptExpectation<'_>,
) -> Result<(), String> {
    receipt.validate().map_err(|error| error.to_string())?;
    if &receipt.expected != expectation.expected {
        return Err("expected fixture/route context mismatch".to_owned());
    }
    if receipt.observed.execution_actor != Actor::RequestReader {
        return Err("root actor is not request_reader".to_owned());
    }
    if receipt.scope_coverage != Coverage::Complete {
        return Err("representative case has incomplete child coverage".to_owned());
    }
    let mut observed_child_actors = BTreeMap::new();
    for child in &receipt.children {
        if child.coverage != Coverage::Complete {
            return Err("representative case has an incomplete child terminal".to_owned());
        }
        *observed_child_actors.entry(child.actor).or_insert(0) += 1;
    }
    if observed_child_actors != expectation.expected.expected_child_actors {
        return Err("representative case child multiplicity drifted".to_owned());
    }
    if receipt.observed.semantic_result_sha256 != sha256(expectation.stdout) {
        return Err("accepted stdout semantic hash mismatch".to_owned());
    }
    if receipt.counters.response_bytes != expectation.stdout.len() as u64 {
        return Err("accepted stdout byte count mismatch".to_owned());
    }
    if receipt.phases.iter().any(|sample| sample.actor.is_none()) {
        return Err("phase is missing its execution actor".to_owned());
    }
    if receipt.phases.iter().any(|sample| {
        sample.actor.is_some_and(|actor| {
            actor != Actor::RequestReader
                && !expectation
                    .expected
                    .expected_child_actors
                    .contains_key(&actor)
        })
    }) {
        return Err("phase belongs to an unexpected execution actor".to_owned());
    }
    let request_phases = receipt
        .phases
        .iter()
        .filter(|sample| sample.actor == Some(Actor::RequestReader))
        .map(|sample| sample.phase)
        .collect::<Vec<_>>();
    for required in expectation.required_phases {
        if !request_phases.contains(required) {
            return Err(format!(
                "missing required request-reader phase {required:?}; observed {request_phases:?}"
            ));
        }
    }
    for forbidden in expectation.forbidden_phases {
        if request_phases.contains(forbidden) {
            return Err(format!(
                "observed forbidden request-reader phase {forbidden:?}; observed {request_phases:?}"
            ));
        }
    }
    if expectation.exhaustive
        && (receipt.counters.event_decodes == 0 || receipt.counters.event_folds == 0)
    {
        return Err("exhaustive route lost decode/fold counters".to_owned());
    }
    Ok(())
}

fn assert_receipt_negatives(valid: &Receipt, expected: &ExpectedContext) {
    for (label, state_count, actor_count, hash_count) in [
        ("missing state", 0, 1, 1),
        ("duplicate state", 2, 1, 1),
        ("missing actor", 1, 0, 1),
        ("duplicate actor", 1, 2, 1),
        ("missing hash", 1, 1, 0),
        ("duplicate hash", 1, 1, 2),
    ] {
        let scope = LongitudinalCountingScopeV1::new(sha256(label.as_bytes())).unwrap();
        scope.record_observed_route_once(Route::VersionJson);
        for _ in 0..state_count {
            scope.record_observed_route_state_once(ObservedState::NotApplicable);
        }
        for _ in 0..actor_count {
            scope.record_execution_actor_once(Actor::RequestReader);
        }
        scope.record_outcome_once(true, 0);
        for _ in 0..hash_count {
            scope.record_semantic_result_sha256_once(sha256(b"{}\n"));
        }
        assert!(
            scope.interaction_receipt(expected.clone()).is_err(),
            "{label} must fail receipt assembly"
        );
    }

    let mut state_drift = valid.clone();
    state_drift.observed.route_state = ObservedState::AuthoritativeReplay;
    resign(&mut state_drift);
    assert!(state_drift.validate().is_err(), "state drift must fail");

    let mut actor_drift = valid.clone();
    actor_drift.observed.execution_actor = Actor::ProductWriter;
    resign(&mut actor_drift);
    assert!(
        actor_drift.validate().is_err(),
        "root actor drift must fail"
    );

    let mut hash_drift = valid.clone();
    hash_drift.observed.semantic_result_sha256 = "0".repeat(64);
    resign(&mut hash_drift);
    assert!(
        check_case_receipt(
            &hash_drift,
            ReceiptExpectation {
                expected: &hash_drift.expected,
                stdout: b"not the recorded output",
                required_phases: &[Phase::SerializationAndOutput],
                forbidden_phases: &[],
                exhaustive: false,
            }
        )
        .is_err(),
        "an independently recomputed output hash must reject drift"
    );

    let mut missing_phase_actor = valid.clone();
    missing_phase_actor.phases[0].actor = None;
    resign(&mut missing_phase_actor);
    assert!(
        missing_phase_actor.validate().is_err(),
        "missing phase actor must fail"
    );

    let mut missing_terminal = valid.clone();
    missing_terminal
        .expected
        .expected_child_actors
        .insert(Actor::BackgroundMaintenance, 1);
    resign(&mut missing_terminal);
    assert!(
        missing_terminal.validate().is_err(),
        "a reserved/expected child without a terminal must fail"
    );

    let child = InteractionChildScopeFactV1 {
        ordinal: 0,
        actor: Actor::BackgroundMaintenance,
        coverage: Coverage::Complete,
    };
    let mut duplicate_terminal = valid.clone();
    duplicate_terminal
        .expected
        .expected_child_actors
        .insert(Actor::BackgroundMaintenance, 2);
    duplicate_terminal.children = vec![child.clone(), child.clone()];
    resign(&mut duplicate_terminal);
    assert!(
        duplicate_terminal.validate().is_err(),
        "duplicate child terminal must fail"
    );

    let mut ordinal_drift = valid.clone();
    ordinal_drift
        .expected
        .expected_child_actors
        .insert(Actor::BackgroundMaintenance, 1);
    ordinal_drift.children = vec![InteractionChildScopeFactV1 {
        ordinal: 1,
        ..child.clone()
    }];
    resign(&mut ordinal_drift);
    assert!(
        ordinal_drift.validate().is_err(),
        "child ordinal drift must fail"
    );

    let mut actor_drift = valid.clone();
    actor_drift
        .expected
        .expected_child_actors
        .insert(Actor::BackgroundMaintenance, 1);
    actor_drift.children = vec![InteractionChildScopeFactV1 {
        actor: Actor::BackgroundRebuild,
        ..child.clone()
    }];
    resign(&mut actor_drift);
    assert!(
        actor_drift.validate().is_err(),
        "child actor drift must fail"
    );

    let mut unexpected_child = valid.clone();
    unexpected_child.children = vec![child.clone()];
    resign(&mut unexpected_child);
    assert!(
        unexpected_child.validate().is_err(),
        "an unexpected source child must remain visible and fail"
    );

    let mut actor_count_drift = valid.clone();
    actor_count_drift
        .expected
        .expected_child_actors
        .insert(Actor::BackgroundMaintenance, 2);
    actor_count_drift.children = vec![child.clone()];
    resign(&mut actor_count_drift);
    assert!(
        actor_count_drift.validate().is_err(),
        "expected actor-count drift must fail"
    );

    let mut incomplete = valid.clone();
    incomplete
        .expected
        .expected_child_actors
        .insert(Actor::BackgroundMaintenance, 1);
    incomplete.children = vec![InteractionChildScopeFactV1 {
        coverage: Coverage::Incomplete {
            reason: "source-owned child failure".to_owned(),
        },
        ..child
    }];
    incomplete.scope_coverage = Coverage::Incomplete {
        reason: "source-owned child failure".to_owned(),
    };
    resign(&mut incomplete);
    incomplete
        .validate()
        .expect("truthful incomplete coverage is structurally valid");
    assert!(
        check_case_receipt(
            &incomplete,
            ReceiptExpectation {
                expected: &incomplete.expected,
                stdout: b"",
                required_phases: &[],
                forbidden_phases: &[],
                exhaustive: false,
            }
        )
        .is_err(),
        "an incomplete sample must be inadmissible to the representative matrix"
    );
}

fn assert_selected_carrier_corruption_fails_closed(
    fixture: &Fixture,
    expected: &ExpectedContext,
    receipt_dir: &tempfile::TempDir,
) {
    let summary_hash = fixture
        .summary_content_hash
        .strip_prefix("sha256:")
        .expect("prefixed summary hash");
    let artifact = common_dir_store(fixture.repo.path())
        .join("artifacts/notes")
        .join(format!("{summary_hash}.json"));
    fs::write(
        &artifact,
        br#"{"schema":"shore.note-body","version":1,"body":"corrupt"}"#,
    )
    .expect("corrupt selected fixture carrier");

    let mut expected = expected.clone();
    expected.setup_expectation = Setup::FactPostSelectionFailure;
    expected.expected_child_actors.clear();
    let ordinary = run_binary(&expected.arguments, ACTIVE_ENV);
    assert!(
        !ordinary.status.success(),
        "corrupt carrier must fail closed"
    );
    let receipt_path = receipt_dir.path().join("corrupt-selected-carrier.json");
    let encoded = encode_request("corrupt-selected-carrier", &expected, &receipt_path);
    let counted_arguments = [
        vec!["--longitudinal-counting".to_owned(), encoded],
        expected.arguments.clone(),
    ]
    .concat();
    let counted = run_binary(&counted_arguments, ACTIVE_ENV);

    assert!(
        !counted.status.success(),
        "diagnostics must preserve refusal"
    );
    assert_eq!(counted.stdout, ordinary.stdout);
    assert!(
        String::from_utf8_lossy(&ordinary.stderr).contains("content hash mismatch"),
        "unexpected corrupt-carrier error: {}",
        String::from_utf8_lossy(&ordinary.stderr)
    );
    assert_eq!(counted.stderr, ordinary.stderr, "counted refusal stderr");
    let receipt: Receipt = serde_json::from_slice(
        &fs::read(&receipt_path).expect("terminal diagnostic receipt must be published"),
    )
    .expect("terminal diagnostic receipt JSON");
    check_case_receipt(
        &receipt,
        ReceiptExpectation {
            expected: &expected,
            stdout: &ordinary.stdout,
            required_phases: &[
                Phase::FactSqliteSelection,
                Phase::FactSelectedCarrierHydrationValidation,
                Phase::FactSupportCarrierHydrationValidation,
                Phase::FactWorkflowProjection,
                Phase::GitContextResolution,
                Phase::RouteRevisionSelection,
                Phase::RouteProjectionFold,
                Phase::RouteBodyHydration,
                Phase::CarrierValidation,
            ],
            forbidden_phases: &INTERACTION_FACT_CURRENT_FORBIDDEN_PHASES_V1,
            exhaustive: false,
        },
    )
    .expect("terminal diagnostic receipt remains admissible");
    assert_eq!(
        receipt.observed.route_state,
        ObservedState::DerivedSelectionFailedClosed
    );
    assert!(!receipt.observed.success);
    assert_eq!(receipt.counters.authoritative_fallbacks, 0);
    assert_eq!(receipt.counters.full_history_fallbacks, 0);
    assert!(receipt.children.is_empty());
}

fn assert_attention_output_lanes(
    arguments: &[String],
    env: &[(&str, &str)],
    case: &str,
) -> serde_json::Value {
    let json = run_binary(arguments, env);
    let json_value = serde_json::from_slice::<serde_json::Value>(&json.stdout).unwrap();
    let pretty = run_binary(arguments_with_format(arguments, "json-pretty"), env);
    assert_success(&format!("attention {case} json-pretty"), &pretty);
    assert_eq!(pretty.stderr, json.stderr, "attention {case} stderr drift");
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&pretty.stdout).unwrap(),
        json_value,
        "attention {case} JSON lane semantics drifted"
    );
    let text_arguments = arguments_with_format(arguments, "text");
    let text = run_binary(&text_arguments, env);
    let repeated_text = run_binary(&text_arguments, env);
    assert_success(&format!("attention {case} text"), &text);
    assert_eq!(
        text.stderr, json.stderr,
        "attention {case} text stderr drift"
    );
    assert_eq!(
        text.stdout, repeated_text.stdout,
        "attention {case} text drift"
    );
    assert!(!text.stdout.is_empty(), "attention {case} text is empty");
    json_value
}

fn assert_attention_hint_precedes_strict_replay_error(fixture: &Fixture, arguments: &[String]) {
    let idempotency_key = "unrelated-future-control";
    let stem = Sha256::digest(idempotency_key.as_bytes())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    fs::write(
        common_dir_store(fixture.repo.path())
            .join("events")
            .join(format!("{stem}.json")),
        br#"{"schema":"pointbreak.future-control","version":1}"#,
    )
    .expect("write unrelated future control");

    let output = run_binary(arguments, ACTIVE_ENV);

    assert!(!output.status.success(), "strict replay error must refuse");
    assert!(
        output.stdout.is_empty(),
        "strict replay error wrote product bytes"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    let hint = stderr
        .find("hint: derived access is unavailable")
        .expect("fallback hint");
    let refusal = stderr
        .find("unknown Journal record schema")
        .expect("strict replay refusal");
    assert!(
        hint < refusal,
        "fallback hint must precede replay refusal: {stderr}"
    );
}

fn build_derived(repo: &GitRepo) {
    let output = pointbreak_env(
        [
            "store",
            "derived",
            "build",
            "--repo",
            repo.path().to_str().expect("fixture repo path is UTF-8"),
        ],
        ACTIVE_ENV,
    );
    assert_success("build disposable derived generation", &output);
}

fn derived_publication_count(repo: &Path) -> usize {
    let publications = common_dir_store(repo).join("derived/publications");
    match fs::read_dir(publications) {
        Ok(entries) => entries.count(),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => 0,
        Err(error) => panic!("read derived publications: {error}"),
    }
}

fn encode_request(case_name: &str, expected: &ExpectedContext, receipt_path: &Path) -> String {
    let request = serde_json::json!({
        "runIdentity": sha256(case_name.as_bytes()),
        "context": {
            "rootIdentity": "1".repeat(64),
            "operation": "INTERACTION_PERFORMANCE_CORRECTNESS",
            "phase": case_name,
            "baseExecutionIdentitySha256": "2".repeat(64),
            "derivativeExecutionIdentitySha256": "3".repeat(64),
            "manifestSha256": "4".repeat(64),
            "scheduleSha256": "5".repeat(64),
            "success": false,
            "semanticResultSha256": "6".repeat(64),
            "includeCapacityOwnership": true
        },
        "interactionContext": expected,
        "receiptPath": receipt_path,
    });
    URL_SAFE_NO_PAD.encode(serde_json::to_vec(&request).expect("counting request JSON"))
}

fn encode_counter_request(case_name: &str, receipt_path: &Path) -> String {
    let request = serde_json::json!({
        "runIdentity": sha256(case_name.as_bytes()),
        "context": {
            "rootIdentity": "1".repeat(64),
            "operation": "FRAGMENT_SELECTOR_LEGACY_PATH",
            "phase": case_name,
            "baseExecutionIdentitySha256": "2".repeat(64),
            "derivativeExecutionIdentitySha256": "3".repeat(64),
            "manifestSha256": "4".repeat(64),
            "scheduleSha256": "5".repeat(64),
            "success": false,
            "semanticResultSha256": "6".repeat(64),
            "includeCapacityOwnership": false
        },
        "receiptPath": receipt_path,
    });
    URL_SAFE_NO_PAD.encode(serde_json::to_vec(&request).expect("counting request JSON"))
}

fn run_binary<I, S>(arguments: I, env: &[(&str, &str)]) -> Output
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let mut command = Command::new(env!("CARGO_BIN_EXE_pointbreak"));
    command
        .args(arguments)
        .env_remove("POINTBREAK_HOME")
        .env_remove("POINTBREAK_DERIVED_ACCESS")
        .env_remove("POINTBREAK_ACTOR_ID")
        .env_remove("POINTBREAK_GIT_BACKEND")
        .env_remove("POINTBREAK_LOG")
        .env_remove("RUST_LOG")
        .env_remove("POINTBREAK_FORMAT")
        .env_remove("POINTBREAK_THEME")
        .env_remove("BAT_THEME")
        .env_remove("NO_COLOR")
        .env_remove("CLICOLOR_FORCE");
    for (key, value) in env {
        command.env(key, value);
    }
    command.output().expect("run pointbreak binary")
}

fn command_stdout(root: &Path, program: &str, arguments: &[&str]) -> String {
    let output = Command::new(program)
        .args(arguments)
        .current_dir(root)
        .output()
        .unwrap_or_else(|error| panic!("run {program}: {error}"));
    assert_success(program, &output);
    String::from_utf8(output.stdout)
        .expect("command stdout is UTF-8")
        .trim()
        .to_owned()
}

fn resign(receipt: &mut Receipt) {
    receipt.receipt_sha256 = receipt.canonical_sha256().expect("canonical receipt hash");
}

fn strings(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| (*value).to_owned()).collect()
}

fn arguments_with_format(arguments: &[String], format: &str) -> Vec<String> {
    let mut arguments = arguments.to_vec();
    let index = arguments
        .iter()
        .position(|argument| argument == "--format")
        .expect("fixture format option");
    arguments[index + 1] = format.to_owned();
    arguments
}

fn arguments_with_revision_selector(arguments: &[String], selector: &str) -> Vec<String> {
    let mut arguments = arguments.to_vec();
    let index = arguments
        .iter()
        .position(|argument| matches!(argument.as_str(), "--exact-revision" | "--revision"))
        .expect("fixture Revision selector");
    arguments[index + 1] = selector.to_owned();
    arguments
}

fn arguments_with_extra(arguments: &[String], extra: &[&str]) -> Vec<String> {
    arguments
        .iter()
        .cloned()
        .chain(extra.iter().map(|value| (*value).to_owned()))
        .collect()
}

fn arguments_with_option_value(arguments: &[String], option: &str, value: &str) -> Vec<String> {
    let mut arguments = arguments.to_vec();
    let index = arguments
        .iter()
        .position(|argument| argument == option)
        .unwrap_or_else(|| panic!("fixture option {option}"));
    arguments[index + 1] = value.to_owned();
    arguments
}

fn arguments_without_option_value(arguments: &[String], option: &str) -> Vec<String> {
    let mut arguments = arguments.to_vec();
    let index = arguments
        .iter()
        .position(|argument| argument == option)
        .unwrap_or_else(|| panic!("fixture option {option}"));
    arguments.drain(index..=index + 1);
    arguments
}

fn sha256(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

#[track_caller]
fn assert_success(label: &str, output: &Output) {
    assert!(
        output.status.success(),
        "{label} failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
