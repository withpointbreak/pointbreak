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

// === Derived revision show route: contract-freeze families ===
//
// These families pin the qualified `revision show <FULL_REV>` contract: the
// D20-A identity substitution with per-lane byte parity, the D22-A store-audit
// diagnostics matrix, head-seed and error-order preservation, and the
// zero-Change counter/phase contract. The derived route does not exist yet, so
// the derived-current assertions fail until the routing tasks take them Green;
// the legacy-lane assertions are pins and must stay Green throughout.

/// One rich fixture store for the qualified `revision show` contract:
///
/// - component alpha: A -> B (replace), a parallel sibling C, and a
///   provenance-free consolidation head F superseding {B, C} that re-binds B's
///   object artifact (forked component, single current head F);
/// - a synthetic competing-heads component: Q and R both supersede P;
/// - ordinary unrelated history: a second CLI component D -> E plus a parallel
///   member G;
/// - facts on F: an inline observation, a second observation whose body
///   exceeds the 4096-byte inline limit and externalizes as a note artifact,
///   an answered and an open input request, a cross-track assessment
///   replacement, validation with a log artifact reference plus a removal of
///   that log hash, commit/ref associations plus one withdrawal;
/// - store-audit carriers: an unrelated revision H with duplicate assessment
///   semantics, a removal of a never-referenced hash, a possession-operative
///   removal re-bound in a different component, an ingested removal of E's
///   artifact whose operativity turns entirely on its sole trusted detached
///   endorsement, an ingested never-endorsed removal of C's artifact whose
///   re-binding must NOT produce a reuse diagnostic, and an ingested removal
///   claiming the externalized note-body hash, whose only reference is that
///   body's observation carrier — so it must never surface as target-missing.
struct RevisionShowFixture {
    repo: GitRepo,
    home: tempfile::TempDir,
    addressed: String,
    superseded_seed: String,
    cli_chained_seed: String,
    competing_seed: String,
    competing_heads: (String, String),
    trusted_reused_hash: String,
    possession_reused_hash: String,
    unsigned_reused_hash: String,
    missing_target_hash: String,
    externalized_body_hash: String,
}

const REVISION_SHOW_ACTOR: &str = "actor:git-email:kevin@swiber.dev";

impl RevisionShowFixture {
    fn repo_arg(&self) -> String {
        self.repo.path().to_string_lossy().into_owned()
    }

    fn env<'a>(&'a self, base: &[(&'a str, &'a str)]) -> Vec<(&'a str, &'a str)> {
        let mut env = base.to_vec();
        env.push((
            "POINTBREAK_HOME",
            self.home.path().to_str().expect("fixture home is UTF-8"),
        ));
        env
    }

    fn off_env(&self) -> Vec<(&str, &str)> {
        self.env(OFF_ENV)
    }

    fn active_env(&self) -> Vec<(&str, &str)> {
        self.env(ACTIVE_ENV)
    }
}

fn revision_show_cli(fixture: &RevisionShowFixture, args: &[&str]) -> Vec<u8> {
    // Fixture writes go through the prepared helper so the resolved store
    // carries the ordinary ready-Change capability fixture; the assertion runs
    // below use the byte-precise `run_binary` on the already-prepared store.
    let output = pointbreak_env(args, &fixture.off_env());
    assert_success(&format!("fixture command {args:?}"), &output);
    output.stdout
}

fn append_raw_fixture_event(
    repo_root: &Path,
    event: &pointbreak::session::event::ShoreEvent,
    idempotency_key: &str,
) {
    let events_dir = common_dir_store(repo_root).join("events");
    fs::create_dir_all(&events_dir).expect("create fixture events directory");
    let stem = sha256(idempotency_key.as_bytes());
    fs::write(
        events_dir.join(format!("{stem}.json")),
        serde_json::to_vec(event).expect("serialize fixture event"),
    )
    .expect("write fixture event");
}

fn fixture_revision_target(
    revision_id: &pointbreak::model::RevisionId,
) -> pointbreak::session::event::EventTarget {
    pointbreak::session::event::EventTarget::for_revision(
        pointbreak::model::JournalId::new("journal:default"),
        revision_id.clone(),
        Some(pointbreak::model::TrackId::new(TRACK)),
    )
    .expect("build fixture revision target")
}

fn fixture_proposal_event(
    revision_id: &str,
    object_id: &str,
    object_artifact_content_hash: &str,
    supersedes: &[&str],
    idempotency_key: &str,
    occurred_at: &str,
) -> pointbreak::session::event::ShoreEvent {
    use pointbreak::model::{EngagementId, ObjectId, RevisionId};
    use pointbreak::session::event::{
        EventType, Revision, ShoreEvent, WorkObjectProposal, WorkObjectProposedPayload, Writer,
    };

    let revision_id = RevisionId::new(revision_id);
    ShoreEvent::new(
        EventType::WorkObjectProposed,
        idempotency_key,
        fixture_revision_target(&revision_id),
        Writer::shore_local("revision-show-fixture"),
        WorkObjectProposedPayload {
            engagement_id: EngagementId::new(format!("engagement:sha256:{}", "b".repeat(64))),
            work_object: WorkObjectProposal::Revision {
                revision: Revision {
                    id: revision_id.clone(),
                    object_id: ObjectId::new(object_id),
                    git_provenance: None,
                },
                summary: Some("fixture revision".to_owned()),
                object_artifact_content_hash: object_artifact_content_hash.to_owned(),
                supersedes: supersedes.iter().map(|id| RevisionId::new(*id)).collect(),
            },
        },
        occurred_at,
    )
    .expect("build fixture proposal")
}

fn fixture_assessment_event(
    revision_id: &str,
    assessment_id: &str,
    idempotency_key: &str,
    occurred_at: &str,
) -> pointbreak::session::event::ShoreEvent {
    use pointbreak::model::{AssessmentId, ReviewTargetRef, RevisionId};
    use pointbreak::session::event::{
        BodyContentType, EventType, ReviewAssessment, ReviewAssessmentRecordedPayload, ShoreEvent,
        Writer,
    };

    let revision_id = RevisionId::new(revision_id);
    ShoreEvent::new(
        EventType::ReviewAssessmentRecorded,
        idempotency_key,
        fixture_revision_target(&revision_id),
        Writer::shore_local("revision-show-fixture"),
        ReviewAssessmentRecordedPayload {
            assessment_id: AssessmentId::new(assessment_id),
            target: ReviewTargetRef::Revision {
                revision_id: revision_id.clone(),
            },
            assessment: ReviewAssessment::NeedsChanges,
            summary: Some("duplicate-bearing fixture assessment".to_owned()),
            summary_content_type: BodyContentType::TextMarkdown,
            summary_artifact_path: None,
            summary_byte_size: None,
            summary_content_hash: None,
            replaces_assessment_ids: Vec::new(),
            related_observation_ids: Vec::new(),
            related_input_request_ids: Vec::new(),
        },
        occurred_at,
    )
    .expect("build fixture assessment")
}

/// Build an `ArtifactRemoved` carrier. A locally-authored removal is operative
/// by possession under the default policy; an ingested one is operative only
/// through a valid signature or a trusted detached endorsement, which is
/// exactly the seam the removal-audit cosignature obligations exercise.
fn fixture_removal_event(
    content_hash: &str,
    idempotency_key: &str,
    occurred_at: &str,
    ingested: bool,
) -> pointbreak::session::event::ShoreEvent {
    use pointbreak::model::JournalId;
    use pointbreak::session::event::{
        ArtifactRemovedPayload, EventTarget, EventType, IngestProvenance, IngestVia, ShoreEvent,
        Writer,
    };

    let mut event = ShoreEvent::new(
        EventType::ArtifactRemoved,
        idempotency_key,
        EventTarget::for_journal(JournalId::new("journal:default")),
        Writer::shore_local("revision-show-fixture"),
        ArtifactRemovedPayload {
            content_hash: content_hash.to_owned(),
        },
        occurred_at,
    )
    .expect("build fixture removal");
    if ingested {
        event.ingest = Some(IngestProvenance {
            via: IngestVia::IngestEvents,
            received_at: "2027-01-01T01:00:00Z".to_owned(),
        });
    }
    event
}

#[allow(clippy::too_many_lines)]
fn revision_show_fixture() -> RevisionShowFixture {
    let home = tempfile::tempdir().expect("temporary signing home");
    let home_arg = home.path().to_str().expect("home path is UTF-8").to_owned();
    let key_env: Vec<(&str, &str)> = vec![("POINTBREAK_HOME", &home_arg)];
    let key_init = pointbreak_env(["key", "init", "--name", "default"], &key_env);
    assert_success("initialize fixture signing key", &key_init);

    let repo = GitRepo::new();
    repo.write("src/lib.rs", "pub fn value() -> u32 { 1 }\n");
    repo.commit_all("base");
    repo.write("src/lib.rs", "pub fn value() -> u32 { 2 }\n");
    let repo_arg = repo.path().to_string_lossy().into_owned();

    let enroll = pointbreak_env(
        [
            "key",
            "enroll",
            "default",
            "--actor",
            REVISION_SHOW_ACTOR,
            "--repo",
            &repo_arg,
        ],
        &key_env,
    );
    assert_success("enroll fixture signing key", &enroll);
    let attest = pointbreak_env(
        [
            "identity",
            "attest",
            REVISION_SHOW_ACTOR,
            "--kind",
            "human",
            "--role",
            "reviewer",
            "--repo",
            &repo_arg,
        ],
        &[],
    );
    assert_success("attest fixture actor", &attest);

    let fixture_stub = RevisionShowFixture {
        repo,
        home,
        addressed: String::new(),
        superseded_seed: String::new(),
        cli_chained_seed: String::new(),
        competing_seed: String::new(),
        competing_heads: (String::new(), String::new()),
        trusted_reused_hash: String::new(),
        possession_reused_hash: String::new(),
        unsigned_reused_hash: String::new(),
        missing_target_hash: String::new(),
        externalized_body_hash: String::new(),
    };

    let capture = |args: &[&str]| -> serde_json::Value {
        serde_json::from_slice(&revision_show_cli(&fixture_stub, args)).expect("capture JSON")
    };

    // Component alpha: A -> B (replace) plus a parallel sibling C. Parallel
    // capture records no supersession edge, so C starts as its own root; the
    // consolidation head F below joins B and C into one forked component.
    let a = capture(&["capture", "--repo", &repo_arg]);
    let a_id = a["revision"]["revisionId"].as_str().unwrap().to_owned();
    let a_cursor = a["reviewCursor"]["token"].as_str().unwrap().to_owned();
    fixture_stub
        .repo
        .write("src/lib.rs", "pub fn value() -> u32 { 3 }\n");
    let b = capture(&[
        "capture",
        "--repo",
        &repo_arg,
        "--review-cursor",
        &a_cursor,
        "--advance",
        "replace",
    ]);
    let b_id = b["revision"]["revisionId"].as_str().unwrap().to_owned();
    let b_cursor = b["reviewCursor"]["token"].as_str().unwrap().to_owned();
    let b_object = b["revision"]["objectId"].as_str().unwrap().to_owned();
    let b_hash = b["revision"]["objectArtifactContentHash"]
        .as_str()
        .unwrap()
        .to_owned();
    fixture_stub
        .repo
        .write("src/lib.rs", "pub fn value() -> u32 { 4 }\n");
    let c = capture(&[
        "capture",
        "--repo",
        &repo_arg,
        "--review-cursor",
        &b_cursor,
        "--advance",
        "parallel",
    ]);
    let c_id = c["revision"]["revisionId"].as_str().unwrap().to_owned();
    let c_object = c["revision"]["objectId"].as_str().unwrap().to_owned();
    let c_hash = c["revision"]["objectArtifactContentHash"]
        .as_str()
        .unwrap()
        .to_owned();

    // The consolidation head F supersedes {B, C} and re-binds B's stored
    // object artifact, so the forked component has one current head whose
    // snapshot bytes are present.
    let f_id = format!("rev:sha256:{}", "f0".repeat(32));
    append_raw_fixture_event(
        fixture_stub.repo.path(),
        &fixture_proposal_event(
            &f_id,
            &b_object,
            &b_hash,
            &[&b_id, &c_id],
            "revision-show-fixture:consolidation-head",
            "2027-01-01T00:00:01Z",
        ),
        "revision-show-fixture:consolidation-head",
    );

    // Facts on F through the ordinary CLI writers.
    revision_show_cli(
        &fixture_stub,
        &[
            "observation",
            "add",
            "--repo",
            &repo_arg,
            "--revision",
            &f_id,
            "--track",
            TRACK,
            "--title",
            "Fixture observation",
            "--body",
            "fixture observation body",
        ],
    );
    // A second observation whose >4096-byte body externalizes as a note
    // artifact; a later ingested removal claims exactly that body hash, so
    // this carrier is the claimed hash's only reference in the store.
    let externalized_body = "x".repeat(5000);
    let externalized_observation: serde_json::Value = serde_json::from_slice(&revision_show_cli(
        &fixture_stub,
        &[
            "observation",
            "add",
            "--repo",
            &repo_arg,
            "--revision",
            &f_id,
            "--track",
            TRACK,
            "--title",
            "Fixture externalized observation",
            "--body",
            &externalized_body,
        ],
    ))
    .expect("externalized observation JSON");
    let externalized_body_hash = externalized_observation["bodyContentHash"]
        .as_str()
        .expect("a >4096-byte body is stored as a note artifact")
        .to_owned();
    let answered_request: serde_json::Value = serde_json::from_slice(&revision_show_cli(
        &fixture_stub,
        &[
            "input-request",
            "open",
            "--repo",
            &repo_arg,
            "--revision",
            &f_id,
            "--track",
            "agent:interaction-fixture-author",
            "--title",
            "Need an answer",
            "--reason",
            "insufficient-evidence",
        ],
    ))
    .expect("input request JSON");
    let answered_request_id = answered_request["inputRequestId"]
        .as_str()
        .expect("opened request id")
        .to_owned();
    revision_show_cli(
        &fixture_stub,
        &[
            "input-request",
            "respond",
            &answered_request_id,
            "--repo",
            &repo_arg,
            "--outcome",
            "approved",
            "--reason",
            "fixture response",
        ],
    );
    revision_show_cli(
        &fixture_stub,
        &[
            "input-request",
            "open",
            "--repo",
            &repo_arg,
            "--revision",
            &f_id,
            "--track",
            TRACK,
            "--title",
            "Still open",
            "--reason",
            "manual-decision-required",
        ],
    );
    let first_assessment: serde_json::Value = serde_json::from_slice(&revision_show_cli(
        &fixture_stub,
        &[
            "assessment",
            "add",
            "--repo",
            &repo_arg,
            "--revision",
            &f_id,
            "--track",
            TRACK,
            "--assessment",
            "needs-changes",
            "--summary",
            "first-track call",
        ],
    ))
    .expect("assessment JSON");
    let first_assessment_id = first_assessment["assessmentId"]
        .as_str()
        .expect("first assessment id")
        .to_owned();
    revision_show_cli(
        &fixture_stub,
        &[
            "assessment",
            "add",
            "--repo",
            &repo_arg,
            "--revision",
            &f_id,
            "--track",
            "agent:interaction-fixture-author",
            "--assessment",
            "accepted",
            "--summary",
            "cross-track replacement call",
            "--replaces",
            &first_assessment_id,
        ],
    );
    let log_hash = format!("sha256:{}", "1a".repeat(32));
    revision_show_cli(
        &fixture_stub,
        &[
            "validation",
            "add",
            "--repo",
            &repo_arg,
            "--revision",
            &f_id,
            "--track",
            TRACK,
            "--check-name",
            "fixture check",
            "--status",
            "passed",
            "--summary",
            "fixture validation summary",
            "--log-content-hash",
            &log_hash,
        ],
    );
    let head_oid = fixture_stub
        .repo
        .git(["rev-parse", "HEAD"])
        .stdout
        .trim()
        .to_owned();
    revision_show_cli(
        &fixture_stub,
        &[
            "association",
            "record",
            "--repo",
            &repo_arg,
            "--revision",
            &f_id,
            "--track",
            TRACK,
            "--commit",
            &head_oid,
        ],
    );
    revision_show_cli(
        &fixture_stub,
        &[
            "association",
            "record",
            "--repo",
            &repo_arg,
            "--revision",
            &f_id,
            "--track",
            TRACK,
            "--ref",
            "refs/heads/main",
            "--head",
            &head_oid,
        ],
    );
    let withdrawable: serde_json::Value = serde_json::from_slice(&revision_show_cli(
        &fixture_stub,
        &[
            "association",
            "record",
            "--repo",
            &repo_arg,
            "--revision",
            &b_id,
            "--track",
            TRACK,
            "--commit",
            &head_oid,
        ],
    ))
    .expect("association JSON");
    let withdrawable_id = withdrawable["commitAssociationId"]
        .as_str()
        .expect("association id")
        .to_owned();
    revision_show_cli(
        &fixture_stub,
        &[
            "association",
            "withdraw",
            &withdrawable_id,
            "--repo",
            &repo_arg,
            "--revision",
            &b_id,
            "--track",
            TRACK,
        ],
    );

    // Ordinary unrelated history: a second CLI component D -> E plus a
    // parallel member G.
    fixture_stub
        .repo
        .write("src/lib.rs", "pub fn value() -> u32 { 5 }\n");
    let d = capture(&["capture", "--repo", &repo_arg]);
    let d_id = d["revision"]["revisionId"].as_str().unwrap().to_owned();
    let d_cursor = d["reviewCursor"]["token"].as_str().unwrap().to_owned();
    fixture_stub
        .repo
        .write("src/lib.rs", "pub fn value() -> u32 { 6 }\n");
    let e = capture(&[
        "capture",
        "--repo",
        &repo_arg,
        "--review-cursor",
        &d_cursor,
        "--advance",
        "replace",
    ]);
    let e_id = e["revision"]["revisionId"].as_str().unwrap().to_owned();
    let e_cursor = e["reviewCursor"]["token"].as_str().unwrap().to_owned();
    let e_object = e["revision"]["objectId"].as_str().unwrap().to_owned();
    let e_hash = e["revision"]["objectArtifactContentHash"]
        .as_str()
        .unwrap()
        .to_owned();
    fixture_stub
        .repo
        .write("src/lib.rs", "pub fn value() -> u32 { 7 }\n");
    let g = capture(&[
        "capture",
        "--repo",
        &repo_arg,
        "--review-cursor",
        &e_cursor,
        "--advance",
        "parallel",
    ]);
    let g_id = g["revision"]["revisionId"].as_str().unwrap().to_owned();

    // Store-audit carriers, appended directly so their shapes are exact.
    let repo_root = fixture_stub.repo.path().to_path_buf();
    let h_id = format!("rev:sha256:{}", "d1".repeat(32));
    append_raw_fixture_event(
        &repo_root,
        &fixture_proposal_event(
            &h_id,
            &format!("obj:sha256:{}", "d2".repeat(32)),
            &format!("sha256:{}", "d3".repeat(32)),
            &[],
            "revision-show-fixture:duplicate-revision",
            "2027-01-01T00:00:02Z",
        ),
        "revision-show-fixture:duplicate-revision",
    );
    let duplicate_assessment_id = format!("assess:sha256:{}", "d4".repeat(32));
    append_raw_fixture_event(
        &repo_root,
        &fixture_assessment_event(
            &h_id,
            &duplicate_assessment_id,
            "revision-show-fixture:duplicate-assessment-one",
            "2027-01-01T00:00:03Z",
        ),
        "revision-show-fixture:duplicate-assessment-one",
    );
    append_raw_fixture_event(
        &repo_root,
        &fixture_assessment_event(
            &h_id,
            &duplicate_assessment_id,
            "revision-show-fixture:duplicate-assessment-two",
            "2027-01-01T00:00:04Z",
        ),
        "revision-show-fixture:duplicate-assessment-two",
    );

    // A removal targeting a hash no event references.
    let missing_target_hash = format!("sha256:{}", "e1".repeat(32));
    append_raw_fixture_event(
        &repo_root,
        &fixture_removal_event(
            &missing_target_hash,
            "revision-show-fixture:removal-target-missing",
            "2027-01-01T00:00:05Z",
            false,
        ),
        "revision-show-fixture:removal-target-missing",
    );
    // A removal of the fixture validation's log hash: a support-closure removal
    // carrier for the addressed component.
    append_raw_fixture_event(
        &repo_root,
        &fixture_removal_event(
            &log_hash,
            "revision-show-fixture:removal-validation-log",
            "2027-01-01T00:00:06Z",
            false,
        ),
        "revision-show-fixture:removal-validation-log",
    );
    // An ingested (non-operative) removal claiming the externalized
    // observation body hash: the body carrier is that hash's sole reference,
    // so target-missing must NOT name it on either lane, and the
    // non-operative claim leaves `--include-body` rendering untouched.
    append_raw_fixture_event(
        &repo_root,
        &fixture_removal_event(
            &externalized_body_hash,
            "revision-show-fixture:removal-observation-body",
            "2027-01-01T00:00:19Z",
            true,
        ),
        "revision-show-fixture:removal-observation-body",
    );

    // Possession-operative reuse: a proposal binds a hash with no stored
    // bytes, a locally-authored removal (operative by possession) targets it,
    // and a later proposal in a different component re-binds it under the same
    // object identity.
    let possession_reused_hash = format!("sha256:{}", "e2".repeat(32));
    let possession_object = format!("obj:sha256:{}", "e4".repeat(32));
    append_raw_fixture_event(
        &repo_root,
        &fixture_proposal_event(
            &format!("rev:sha256:{}", "e3".repeat(32)),
            &possession_object,
            &possession_reused_hash,
            &[],
            "revision-show-fixture:possession-binder",
            "2027-01-01T00:00:07Z",
        ),
        "revision-show-fixture:possession-binder",
    );
    append_raw_fixture_event(
        &repo_root,
        &fixture_removal_event(
            &possession_reused_hash,
            "revision-show-fixture:possession-removal",
            "2027-01-01T00:00:08Z",
            false,
        ),
        "revision-show-fixture:possession-removal",
    );
    append_raw_fixture_event(
        &repo_root,
        &fixture_proposal_event(
            &format!("rev:sha256:{}", "e5".repeat(32)),
            &possession_object,
            &possession_reused_hash,
            &[],
            "revision-show-fixture:possession-rebinder",
            "2027-01-01T00:00:09Z",
        ),
        "revision-show-fixture:possession-rebinder",
    );

    // Trusted-endorsed reuse OUTSIDE the addressed component: an INGESTED
    // removal of E's artifact hash is not operative by possession, so its
    // operative status turns entirely on its sole endorsing detached
    // cosignature from the enrolled trusted key. A different component
    // re-binds the hash under E's object identity. For the addressed
    // component this removal, its endorsement, and the binding proposals are
    // pure removal-audit carriers.
    let trusted_removal = fixture_removal_event(
        &e_hash,
        "revision-show-fixture:trusted-removal",
        "2027-01-01T00:00:10Z",
        true,
    );
    let trusted_removal_event_id = trusted_removal.event_id.as_str().to_owned();
    append_raw_fixture_event(
        &repo_root,
        &trusted_removal,
        "revision-show-fixture:trusted-removal",
    );
    let endorse = pointbreak_env(
        ["endorse", &trusted_removal_event_id, "--repo", &repo_arg],
        &[
            ("POINTBREAK_HOME", &home_arg),
            ("POINTBREAK_ACTOR_ID", REVISION_SHOW_ACTOR),
        ],
    );
    assert_success("endorse fixture removal", &endorse);
    append_raw_fixture_event(
        &repo_root,
        &fixture_proposal_event(
            &format!("rev:sha256:{}", "e7".repeat(32)),
            &e_object,
            &e_hash,
            &[],
            "revision-show-fixture:trusted-rebinder",
            "2027-01-01T00:00:11Z",
        ),
        "revision-show-fixture:trusted-rebinder",
    );

    // Unsigned non-operative claim: an INGESTED, never-endorsed removal of C's
    // artifact hash is not operative under the default policy, so a re-binding
    // proposal must NOT produce a reuse diagnostic.
    append_raw_fixture_event(
        &repo_root,
        &fixture_removal_event(
            &c_hash,
            "revision-show-fixture:unsigned-removal",
            "2027-01-01T00:00:12Z",
            true,
        ),
        "revision-show-fixture:unsigned-removal",
    );
    append_raw_fixture_event(
        &repo_root,
        &fixture_proposal_event(
            &format!("rev:sha256:{}", "e9".repeat(32)),
            &c_object,
            &c_hash,
            &[],
            "revision-show-fixture:unsigned-rebinder",
            "2027-01-01T00:00:13Z",
        ),
        "revision-show-fixture:unsigned-rebinder",
    );

    // A competing-heads component. `--advance parallel` records no
    // supersession edge (parallel members are sibling roots inside the
    // Change), so edge-based competing heads are constructed through the
    // model's event-borne supersedes directly: Q and R both supersede P.
    let p_id = format!("rev:sha256:{}", "b1".repeat(32));
    let q_id = format!("rev:sha256:{}", "b2".repeat(32));
    let r_id = format!("rev:sha256:{}", "b3".repeat(32));
    let competing_object = format!("obj:sha256:{}", "b4".repeat(32));
    let competing_hash = format!("sha256:{}", "b5".repeat(32));
    for (revision, supersedes, key, occurred_at) in [
        (
            &p_id,
            vec![],
            "revision-show-fixture:competing-root",
            "2027-01-01T00:00:14Z",
        ),
        (
            &q_id,
            vec![p_id.as_str()],
            "revision-show-fixture:competing-head-one",
            "2027-01-01T00:00:15Z",
        ),
        (
            &r_id,
            vec![p_id.as_str()],
            "revision-show-fixture:competing-head-two",
            "2027-01-01T00:00:16Z",
        ),
    ] {
        append_raw_fixture_event(
            &repo_root,
            &fixture_proposal_event(
                revision,
                &competing_object,
                &competing_hash,
                &supersedes,
                key,
                occurred_at,
            ),
            key,
        );
    }
    // Two unrelated singleton revisions whose digests share an eight-hex
    // prefix, so an abbreviated selector can be proven ambiguous
    // deterministically.
    for (suffix, key, occurred_at) in [
        (
            "0",
            "revision-show-fixture:ambiguous-one",
            "2027-01-01T00:00:17Z",
        ),
        (
            "1",
            "revision-show-fixture:ambiguous-two",
            "2027-01-01T00:00:18Z",
        ),
    ] {
        append_raw_fixture_event(
            &repo_root,
            &fixture_proposal_event(
                &format!("rev:sha256:abcd1234{}", suffix.repeat(56)),
                &format!("obj:sha256:ab{}", suffix.repeat(62)),
                &format!("sha256:ac{}", suffix.repeat(62)),
                &[],
                key,
                occurred_at,
            ),
            key,
        );
    }

    // D/E/G remain in the store as ordinary second-component history.
    let _ = (d_id, e_id, g_id);

    RevisionShowFixture {
        addressed: f_id,
        // B is superseded through the model's event-borne (proposal-payload)
        // edge F -> B, so the head seed forward-resolves. A's supersession by
        // B is Change-borne only: the legacy head-seed resolver does not see
        // it, so A stays its own head and is pinned as the CLI-chained seed.
        superseded_seed: b_id,
        cli_chained_seed: a_id,
        competing_seed: p_id,
        competing_heads: (q_id, r_id),
        trusted_reused_hash: e_hash,
        possession_reused_hash,
        unsigned_reused_hash: c_hash,
        missing_target_hash,
        externalized_body_hash,
        ..fixture_stub
    }
}

fn revision_show_arguments(fixture: &RevisionShowFixture, operand: &str) -> Vec<String> {
    strings(&[
        "revision",
        "show",
        operand,
        "--repo",
        &fixture.repo_arg(),
        "--format",
        "json",
    ])
}

/// Re-serialize the off-lane document with the derived identity substituted:
/// `eventSetHash` removed, `projectionStamp` inserted. The result must equal
/// the derived-lane bytes exactly — the identity block is the only permitted
/// difference (D20-A).
fn with_derived_identity(off_document: &serde_json::Value, projection_stamp: &str) -> Vec<u8> {
    let mut substituted = off_document.clone();
    let object = substituted
        .as_object_mut()
        .expect("revision show document is an object");
    assert!(
        object.remove("eventSetHash").is_some(),
        "the off lane must carry eventSetHash"
    );
    object.insert(
        "projectionStamp".to_owned(),
        serde_json::Value::String(projection_stamp.to_owned()),
    );
    let mut bytes = serde_json::to_vec(&substituted).expect("serialize substituted document");
    bytes.push(b'\n');
    bytes
}

fn diagnostic_codes(document: &serde_json::Value) -> Vec<String> {
    document["diagnostics"]
        .as_array()
        .expect("diagnostics array")
        .iter()
        .map(|entry| entry["code"].as_str().expect("diagnostic code").to_owned())
        .collect()
}

fn diagnostic_messages(document: &serde_json::Value) -> Vec<String> {
    document["diagnostics"]
        .as_array()
        .expect("diagnostics array")
        .iter()
        .map(|entry| {
            entry["message"]
                .as_str()
                .expect("diagnostic message")
                .to_owned()
        })
        .collect()
}

#[test]
fn revision_show_cell_catalog_freezes_the_two_approved_cells() {
    use pointbreak::bench_support::longitudinal::INTERACTION_REVISION_SHOW_CELLS_V1;

    assert_eq!(
        INTERACTION_REVISION_SHOW_CELLS_V1.map(|(cell, _, _)| cell),
        ["revision_show_current_exact", "revision_show_explicit_off"],
    );
    for (_, route, _) in INTERACTION_REVISION_SHOW_CELLS_V1 {
        assert_eq!(route, Route::RevisionShowDetail);
    }

    let target =
        interaction_route_state_contract_v1(Route::RevisionShowDetail, Setup::FactActiveCurrent)
            .expect("revision_show_current_exact cell");
    assert_eq!(target.observed, ObservedState::DerivedCurrent);
    assert_eq!(
        target.performance_role,
        PerformanceRole::ProvisionalTarget {
            sample_count: 5,
            strict_upper_bound_millis: 2_000,
        }
    );
    assert!(target.historical_evidence_unchanged);

    let characterization =
        interaction_route_state_contract_v1(Route::RevisionShowDetail, Setup::FactExplicitOff)
            .expect("revision_show_explicit_off cell");
    assert_eq!(
        characterization.observed,
        ObservedState::AuthoritativeReplay
    );
    assert_eq!(
        characterization.performance_role,
        PerformanceRole::CompatibilityCharacterization
    );

    // Active-unavailable revision show is characterization-only prose (D23-A):
    // no other setup is a catalog cell.
    for setup in [
        Setup::NotApplicable,
        Setup::AuthoritativeReplay,
        Setup::FactActiveUnavailable,
        Setup::FactPostSelectionFailure,
        Setup::AttentionDerivedCurrent,
        Setup::AttentionColdInactive,
        Setup::AttentionActiveUnavailable,
    ] {
        assert!(
            interaction_route_state_contract_v1(Route::RevisionShowDetail, setup).is_none(),
            "{setup:?} must not be a revision-show catalog cell"
        );
    }
}

#[test]
fn change_read_cell_catalog_freezes_the_four_approved_cells() {
    use pointbreak::bench_support::longitudinal::INTERACTION_CHANGE_READ_CELLS_V1;

    assert_eq!(
        INTERACTION_CHANGE_READ_CELLS_V1.map(|(cell, _, _)| cell),
        [
            "change_profile_current",
            "change_list_current",
            "change_attention_current",
            "change_list_explicit_off",
        ],
    );
    assert_eq!(
        INTERACTION_CHANGE_READ_CELLS_V1.map(|(_, route, _)| route),
        [
            Route::ChangeProfileRead,
            Route::ChangeListRead,
            Route::ChangeAttentionRead,
            Route::ChangeListRead,
        ],
    );

    for route in [
        Route::ChangeProfileRead,
        Route::ChangeListRead,
        Route::ChangeAttentionRead,
    ] {
        let target = interaction_route_state_contract_v1(route, Setup::FactActiveCurrent)
            .expect("active-current change-read cell");
        assert_eq!(target.observed, ObservedState::DerivedCurrent);
        assert_eq!(
            target.performance_role,
            PerformanceRole::ProvisionalTarget {
                sample_count: 5,
                strict_upper_bound_millis: 2_000,
            }
        );
        assert!(target.historical_evidence_unchanged);
    }

    let characterization =
        interaction_route_state_contract_v1(Route::ChangeListRead, Setup::FactExplicitOff)
            .expect("change_list_explicit_off cell");
    assert_eq!(
        characterization.observed,
        ObservedState::AuthoritativeReplay
    );
    assert_eq!(
        characterization.performance_role,
        PerformanceRole::CompatibilityCharacterization
    );
    assert_eq!(characterization.strict_authoritative_snapshots, 0);

    // Only the change list has an explicit-off characterization cell, and no
    // change read has an unavailable or terminal catalog cell.
    for route in [
        Route::ChangeProfileRead,
        Route::ChangeListRead,
        Route::ChangeAttentionRead,
    ] {
        for setup in [
            Setup::NotApplicable,
            Setup::AuthoritativeReplay,
            Setup::FactActiveUnavailable,
            Setup::FactPostSelectionFailure,
            Setup::AttentionDerivedCurrent,
            Setup::AttentionColdInactive,
            Setup::AttentionActiveUnavailable,
        ] {
            assert!(
                interaction_route_state_contract_v1(route, setup).is_none(),
                "{setup:?} must not be a change-read catalog cell"
            );
        }
    }
    for route in [Route::ChangeProfileRead, Route::ChangeAttentionRead] {
        assert!(
            interaction_route_state_contract_v1(route, Setup::FactExplicitOff).is_none(),
            "only the change list carries the explicit-off characterization cell"
        );
    }
}

#[test]
fn change_read_cells_run_counted_with_plain_run_parity() {
    let fixture = fixture();
    let repo = fixture.repo.path().to_string_lossy().into_owned();
    let execution = execution_identity();
    let receipt_dir = tempfile::tempdir().expect("temporary change-read receipt directory");

    let list_arguments = strings(&["change", "list", "--repo", &repo, "--format", "json"]);
    let off_expected = expected_context(
        execution.clone(),
        Route::ChangeListRead,
        list_arguments.clone(),
        Setup::FactExplicitOff,
        Some(&fixture),
        None,
    );
    let off_receipt = run_success_case(
        "change-list-explicit-off",
        &list_arguments,
        OFF_ENV,
        &off_expected,
        &receipt_dir,
        &[Phase::SerializationAndOutput],
        &[
            Phase::CliCapabilityPreflightH1,
            Phase::ChangePageSnapshotAcquisition,
            Phase::ChangePageBodylessSelection,
            Phase::ChangePagePresentationProjection,
        ],
        true,
    );
    assert_eq!(
        off_receipt.observed.route_state,
        ObservedState::AuthoritativeReplay
    );
    assert_eq!(off_receipt.counters.strict_journal_inspections, 0);
    assert_eq!(off_receipt.counters.body_artifact_reads, 0);
    assert_eq!(off_receipt.counters.object_artifact_reads, 0);
    assert!(off_receipt.counters.change_semantic_constructions > 0);

    build_derived(&fixture.repo);
    for (case_name, subcommand, route, forbidden) in [
        (
            "change-profile-current",
            "profile",
            Route::ChangeProfileRead,
            [
                Phase::CliCapabilityPreflightH1,
                Phase::WorkflowChangeReaderReplayH3,
                Phase::ChangePageSnapshotAcquisition,
                Phase::RouteBodyHydration,
            ]
            .as_slice(),
        ),
        (
            "change-list-current",
            "list",
            Route::ChangeListRead,
            [
                Phase::CliCapabilityPreflightH1,
                Phase::WorkflowChangeReaderReplayH3,
                Phase::RouteBodyHydration,
            ]
            .as_slice(),
        ),
        (
            "change-attention-current",
            "attention",
            Route::ChangeAttentionRead,
            [
                Phase::CliCapabilityPreflightH1,
                Phase::WorkflowChangeReaderReplayH3,
                Phase::RouteBodyHydration,
            ]
            .as_slice(),
        ),
    ] {
        let arguments = strings(&["change", subcommand, "--repo", &repo, "--format", "json"]);
        let expected = expected_context(
            execution.clone(),
            route,
            arguments.clone(),
            Setup::FactActiveCurrent,
            Some(&fixture),
            None,
        );
        let mut required = vec![Phase::SerializationAndOutput];
        if route != Route::ChangeProfileRead {
            required.extend([
                Phase::ChangePageSnapshotAcquisition,
                Phase::ChangePageBodylessSelection,
                Phase::ChangePageProposalLocatorExpansion,
                Phase::ChangePageCarrierHydrationValidation,
                Phase::ChangePageSupportExpansion,
                Phase::ChangePagePresentationProjection,
            ]);
        }
        let receipt = run_success_case(
            case_name,
            &arguments,
            ACTIVE_ENV,
            &expected,
            &receipt_dir,
            &required,
            forbidden,
            false,
        );
        assert_eq!(receipt.observed.route_state, ObservedState::DerivedCurrent);
        assert_eq!(receipt.counters.strict_journal_inspections, 0);
        assert_eq!(receipt.counters.body_artifact_reads, 0);
        assert_eq!(receipt.counters.object_artifact_reads, 0);
        assert_eq!(receipt.counters.authoritative_fallbacks, 0);
        assert_eq!(receipt.counters.full_history_fallbacks, 0);
        if route == Route::ChangeProfileRead {
            assert_eq!(receipt.counters.event_decodes, 0);
            assert_eq!(receipt.counters.change_proposal_carriers_opened, 0);
            assert_eq!(receipt.counters.change_support_carriers_opened, 0);
        } else {
            assert!(receipt.counters.change_candidates > 0);
            assert!(receipt.counters.change_proposal_carriers_opened > 0);
            assert!(receipt.counters.change_rows_emitted > 0);
        }
        assert!(receipt.children.is_empty());
    }
}

#[test]
fn revision_show_derived_current_substitutes_projection_stamp_with_lane_parity() {
    let fixture = revision_show_fixture();
    build_derived(&fixture.repo);
    let arguments = revision_show_arguments(&fixture, &fixture.addressed);

    let off = run_binary(&arguments, &fixture.off_env());
    assert_success("addressed revision show explicit-off", &off);
    let off_document: serde_json::Value =
        serde_json::from_slice(&off.stdout).expect("off-lane JSON");
    assert!(
        off_document["eventSetHash"].as_str().is_some(),
        "the authoritative lane keeps eventSetHash"
    );
    assert!(off_document.get("projectionStamp").is_none());
    assert_eq!(
        off_document["revision"]["revisionId"].as_str().unwrap(),
        fixture.addressed,
        "the consolidation head is the addressed revision"
    );

    let active = run_binary(&arguments, &fixture.active_env());
    assert_eq!(active.status.code(), off.status.code(), "exit parity");
    assert_eq!(active.stderr, off.stderr, "stderr parity");
    let active_document: serde_json::Value =
        serde_json::from_slice(&active.stdout).expect("active-lane JSON");
    let projection_stamp = active_document["projectionStamp"]
        .as_str()
        .unwrap_or_else(|| {
            panic!(
                "derived-current revision show must serialize projectionStamp; got {active_document}"
            )
        })
        .to_owned();
    assert!(
        active_document.get("eventSetHash").is_none(),
        "derived-current revision show must omit eventSetHash"
    );
    assert_eq!(
        active_document["eventCount"], off_document["eventCount"],
        "eventCount stays the exact full authoritative count"
    );
    assert_eq!(
        active.stdout,
        with_derived_identity(&off_document, &projection_stamp),
        "derived-current bytes must equal explicit-off bytes except the identity block"
    );

    // json-pretty lane: the same substitution parity.
    let pretty_arguments = arguments_with_format(&arguments, "json-pretty");
    let off_pretty = run_binary(&pretty_arguments, &fixture.off_env());
    assert_success("addressed json-pretty explicit-off", &off_pretty);
    let active_pretty = run_binary(&pretty_arguments, &fixture.active_env());
    assert_eq!(active_pretty.status.code(), off_pretty.status.code());
    let active_pretty_document: serde_json::Value =
        serde_json::from_slice(&active_pretty.stdout).expect("active json-pretty JSON");
    assert!(active_pretty_document["projectionStamp"].as_str().is_some());
    assert!(active_pretty_document.get("eventSetHash").is_none());
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&active_pretty.stdout).unwrap(),
        active_document,
        "json-pretty semantics match the json lane"
    );

    // text lane: byte-identical between the lanes (the digest reads the same
    // result and renders no identity block).
    let text_arguments = arguments_with_format(&arguments, "text");
    let off_text = run_binary(&text_arguments, &fixture.off_env());
    assert_success("addressed text explicit-off", &off_text);
    let active_text = run_binary(&text_arguments, &fixture.active_env());
    assert_eq!(active_text.status.code(), off_text.status.code());
    assert_eq!(
        active_text.stdout, off_text.stdout,
        "text digest bytes are lane-identical"
    );
    assert_eq!(active_text.stderr, off_text.stderr);

    // --include-body lane parity under the same substitution.
    let mut body_arguments = arguments.clone();
    body_arguments.push("--include-body".to_owned());
    let off_body = run_binary(&body_arguments, &fixture.off_env());
    assert_success("addressed include-body explicit-off", &off_body);
    let off_body_document: serde_json::Value =
        serde_json::from_slice(&off_body.stdout).expect("off include-body JSON");
    let active_body = run_binary(&body_arguments, &fixture.active_env());
    assert_eq!(active_body.status.code(), off_body.status.code());
    let active_body_document: serde_json::Value =
        serde_json::from_slice(&active_body.stdout).expect("active include-body JSON");
    let body_stamp = active_body_document["projectionStamp"]
        .as_str()
        .expect("include-body derived lane carries projectionStamp")
        .to_owned();
    assert_eq!(
        active_body.stdout,
        with_derived_identity(&off_body_document, &body_stamp),
        "include-body parity except the identity block"
    );

    // Head-seed preservation: the event-borne superseded seed
    // forward-resolves to the consolidation head with the same substitution
    // parity.
    let seed_arguments = revision_show_arguments(&fixture, &fixture.superseded_seed);
    let off_seed = run_binary(&seed_arguments, &fixture.off_env());
    assert_success("superseded seed explicit-off", &off_seed);
    let off_seed_document: serde_json::Value =
        serde_json::from_slice(&off_seed.stdout).expect("off seed JSON");
    assert_eq!(
        off_seed_document["revision"]["revisionId"]
            .as_str()
            .unwrap(),
        fixture.addressed,
        "the superseded seed resolves to the thread head"
    );
    let active_seed = run_binary(&seed_arguments, &fixture.active_env());
    assert_eq!(active_seed.status.code(), off_seed.status.code());
    let active_seed_document: serde_json::Value =
        serde_json::from_slice(&active_seed.stdout).expect("active seed JSON");
    let seed_stamp = active_seed_document["projectionStamp"]
        .as_str()
        .expect("seed derived lane carries projectionStamp")
        .to_owned();
    assert_eq!(
        active_seed.stdout,
        with_derived_identity(&off_seed_document, &seed_stamp),
        "head-seed parity except the identity block"
    );

    // A Change-chained seed: its supersession is Change-borne only, so the
    // legacy resolver keeps it as its own head; the derived lane reproduces
    // exactly that resolution.
    let chained_arguments = revision_show_arguments(&fixture, &fixture.cli_chained_seed);
    let off_chained = run_binary(&chained_arguments, &fixture.off_env());
    assert_success("chained seed explicit-off", &off_chained);
    let off_chained_document: serde_json::Value =
        serde_json::from_slice(&off_chained.stdout).expect("off chained JSON");
    assert_eq!(
        off_chained_document["revision"]["revisionId"]
            .as_str()
            .unwrap(),
        fixture.cli_chained_seed,
        "a Change-borne-only supersession does not forward-resolve the legacy head seed"
    );
    let active_chained = run_binary(&chained_arguments, &fixture.active_env());
    assert_eq!(active_chained.status.code(), off_chained.status.code());
    let active_chained_document: serde_json::Value =
        serde_json::from_slice(&active_chained.stdout).expect("active chained JSON");
    let chained_stamp = active_chained_document["projectionStamp"]
        .as_str()
        .expect("chained derived lane carries projectionStamp")
        .to_owned();
    assert_eq!(
        active_chained.stdout,
        with_derived_identity(&off_chained_document, &chained_stamp),
        "Change-chained seed parity except the identity block"
    );
}

#[test]
fn revision_show_derived_current_preserves_store_audit_diagnostics() {
    let fixture = revision_show_fixture();
    build_derived(&fixture.repo);
    let arguments = revision_show_arguments(&fixture, &fixture.addressed);

    let off = run_binary(&arguments, &fixture.off_env());
    assert_success("audit fixture explicit-off", &off);
    let off_document: serde_json::Value =
        serde_json::from_slice(&off.stdout).expect("off-lane JSON");
    let codes = diagnostic_codes(&off_document);
    let messages = diagnostic_messages(&off_document);

    // Fixture sanity on the legacy lane: the two store-wide removal-audit
    // classes are present, the duplicate-assessment class is present, and the
    // unsigned non-operative claim produces no reuse diagnostic.
    assert!(
        codes
            .iter()
            .any(|code| code == "snapshot_content_removed_target_missing"),
        "target-missing removal audit present: {codes:?}"
    );
    assert!(
        messages
            .iter()
            .any(|message| message.contains(&fixture.missing_target_hash)),
        "target-missing names the never-referenced hash"
    );
    assert!(
        !messages
            .iter()
            .any(|message| message.contains(&fixture.externalized_body_hash)),
        "a removed hash referenced by a note-body carrier is not target-missing"
    );
    let reuse_messages: Vec<&String> = messages
        .iter()
        .filter(|message| message.contains("re-binds removed snapshot content"))
        .collect();
    assert!(
        reuse_messages
            .iter()
            .any(|message| message.contains(&fixture.possession_reused_hash)),
        "possession-operative reuse present: {reuse_messages:?}"
    );
    assert!(
        reuse_messages
            .iter()
            .any(|message| message.contains(&fixture.trusted_reused_hash)),
        "trusted-endorsed reuse present (the sole endorsing cosignature is the \
         detached signature on the ingested removal): {reuse_messages:?}"
    );
    assert!(
        !reuse_messages
            .iter()
            .any(|message| message.contains(&fixture.unsigned_reused_hash)),
        "an ingested never-endorsed claim must not produce a reuse diagnostic"
    );
    assert!(
        codes
            .iter()
            .any(|code| code == "duplicate_semantic_assessment_event"),
        "global duplicate diagnostics present: {codes:?}"
    );

    // The derived-current lane must reproduce the complete public diagnostics
    // contract byte- and order-identically (D22-A) under the D20-A identity
    // substitution. Byte equality of the full document pins both content and
    // array positions, and simultaneously proves the removal-audit carriers
    // perturb no other public output.
    let active = run_binary(&arguments, &fixture.active_env());
    assert_eq!(active.status.code(), off.status.code());
    let active_document: serde_json::Value =
        serde_json::from_slice(&active.stdout).expect("active-lane JSON");
    let projection_stamp = active_document["projectionStamp"]
        .as_str()
        .expect("derived-current audit lane carries projectionStamp")
        .to_owned();
    assert_eq!(
        active.stdout,
        with_derived_identity(&off_document, &projection_stamp),
        "store-audit diagnostics parity except the identity block"
    );
}

#[test]
fn revision_show_error_ordering_is_frozen_per_lane() {
    let fixture = revision_show_fixture();
    build_derived(&fixture.repo);

    // Competing heads: the seed's thread has two heads, so the head-seed
    // resolution error is byte-identical on both lanes.
    let competing_arguments = revision_show_arguments(&fixture, &fixture.competing_seed);
    let off_competing = run_binary(&competing_arguments, &fixture.off_env());
    assert!(!off_competing.status.success());
    let stderr = String::from_utf8_lossy(&off_competing.stderr);
    assert!(
        stderr.contains("competing heads"),
        "competing-heads error names the condition: {stderr}"
    );
    assert!(
        stderr.contains(fixture.competing_heads.0.as_str())
            && stderr.contains(fixture.competing_heads.1.as_str()),
        "competing-heads error lists both heads: {stderr}"
    );
    let active_competing = run_binary(&competing_arguments, &fixture.active_env());
    assert_eq!(active_competing.status.code(), off_competing.status.code());
    assert_eq!(active_competing.stdout, off_competing.stdout);
    assert_eq!(active_competing.stderr, off_competing.stderr);

    // A full ID absent from the store reproduces the existing not-found bytes
    // on both lanes with no post-selection fallback.
    let absent = format!("rev:sha256:{}", "9".repeat(64));
    let absent_arguments = revision_show_arguments(&fixture, &absent);
    let off_absent = run_binary(&absent_arguments, &fixture.off_env());
    assert!(!off_absent.status.success());
    assert!(
        String::from_utf8_lossy(&off_absent.stderr).contains("unknown revision"),
        "absent full id keeps the unknown-revision error"
    );
    let active_absent = run_binary(&absent_arguments, &fixture.active_env());
    assert_eq!(active_absent.status.code(), off_absent.status.code());
    assert_eq!(active_absent.stdout, off_absent.stdout);
    assert_eq!(active_absent.stderr, off_absent.stderr);

    // Selector errors stay ahead of any route work on both lanes.
    for selector in ["not-hex", "abc", "obj:11111111"] {
        let selector_arguments = revision_show_arguments(&fixture, selector);
        let off_selector = run_binary(&selector_arguments, &fixture.off_env());
        assert!(!off_selector.status.success(), "{selector} must fail");
        let active_selector = run_binary(&selector_arguments, &fixture.active_env());
        assert_eq!(
            active_selector.status.code(),
            off_selector.status.code(),
            "{selector} exit parity"
        );
        assert_eq!(active_selector.stdout, off_selector.stdout);
        assert_eq!(active_selector.stderr, off_selector.stderr);
    }

    // A unique fragment resolves through the legacy index-backed lane with
    // identical bytes on both lanes.
    let digest = fixture
        .addressed
        .rsplit_once("sha256:")
        .expect("addressed id has a digest")
        .1;
    let fragment_arguments = revision_show_arguments(&fixture, &digest[..12]);
    let off_fragment = run_binary(&fragment_arguments, &fixture.off_env());
    assert_success("unique fragment explicit-off", &off_fragment);
    let active_fragment = run_binary(&fragment_arguments, &fixture.active_env());
    assert_eq!(active_fragment.status.code(), off_fragment.status.code());
    assert_eq!(active_fragment.stdout, off_fragment.stdout);
    assert_eq!(active_fragment.stderr, off_fragment.stderr);

    // An ambiguous fragment lists every candidate and never auto-picks, with
    // identical bytes on both lanes.
    let ambiguous_arguments = revision_show_arguments(&fixture, "abcd1234");
    let off_ambiguous = run_binary(&ambiguous_arguments, &fixture.off_env());
    assert!(!off_ambiguous.status.success());
    assert!(
        String::from_utf8_lossy(&off_ambiguous.stderr).contains("ambiguous"),
        "ambiguous fragment keeps its candidate-listing error"
    );
    let active_ambiguous = run_binary(&ambiguous_arguments, &fixture.active_env());
    assert_eq!(active_ambiguous.status.code(), off_ambiguous.status.code());
    assert_eq!(active_ambiguous.stdout, off_ambiguous.stdout);
    assert_eq!(active_ambiguous.stderr, off_ambiguous.stderr);

    // The omitted operand stays on the legacy current-capture path; on this
    // multi-revision store that is its existing ambiguity error, byte-equal
    // on both lanes.
    let omitted_arguments = strings(&[
        "revision",
        "show",
        "--repo",
        &fixture.repo_arg(),
        "--format",
        "json",
    ]);
    let off_omitted = run_binary(&omitted_arguments, &fixture.off_env());
    assert!(!off_omitted.status.success());
    assert!(
        String::from_utf8_lossy(&off_omitted.stderr).contains("multiple captured revisions"),
        "omitted operand keeps the current-capture ambiguity error"
    );
    let active_omitted = run_binary(&omitted_arguments, &fixture.active_env());
    assert_eq!(active_omitted.status.code(), off_omitted.status.code());
    assert_eq!(active_omitted.stdout, off_omitted.stdout);
    assert_eq!(active_omitted.stderr, off_omitted.stderr);
}

#[test]
fn revision_show_active_unavailable_stays_unlabeled_authoritative() {
    let fixture = revision_show_fixture();
    let arguments = revision_show_arguments(&fixture, &fixture.addressed);

    // No generation was ever built: the qualified lane discovers
    // unavailability and falls back to one unlabeled authoritative read with
    // byte-identical output and no derived identity.
    let off = run_binary(&arguments, &fixture.off_env());
    assert_success("active-unavailable explicit-off reference", &off);
    let active = run_binary(&arguments, &fixture.active_env());
    assert_eq!(active.status.code(), off.status.code());
    assert_eq!(
        active.stdout, off.stdout,
        "unlabeled fallback stdout parity"
    );
    assert_eq!(
        active.stderr, off.stderr,
        "unlabeled fallback stderr parity"
    );
    let document: serde_json::Value = serde_json::from_slice(&active.stdout).expect("JSON");
    assert!(document.get("projectionStamp").is_none());
    assert!(document["eventSetHash"].as_str().is_some());
    assert_eq!(
        derived_publication_count(fixture.repo.path()),
        0,
        "a short-lived request must not publish a request-owned generation"
    );

    // The counted run keeps truthful fallback counters and no derived rows.
    let receipt_dir = tempfile::tempdir().expect("temporary receipt directory");
    let receipt_path = receipt_dir.path().join("revision-show-unavailable.json");
    let encoded = encode_counter_request("revision-show-active-unavailable", &receipt_path);
    let counted_arguments = [
        vec!["--longitudinal-counting".to_owned(), encoded],
        arguments.clone(),
    ]
    .concat();
    let counted = run_binary(&counted_arguments, &fixture.active_env());
    assert_eq!(counted.status.code(), active.status.code());
    assert_eq!(counted.stdout, active.stdout);
    let receipt: LongitudinalCounterReceiptV1 =
        serde_json::from_slice(&fs::read(&receipt_path).expect("receipt bytes"))
            .expect("receipt JSON");
    receipt.validate().expect("valid counter receipt");
    assert!(receipt.success);
    assert_eq!(receipt.counters.authoritative_fallbacks, 1);
    assert_eq!(receipt.counters.full_history_fallbacks, 1);
    assert_eq!(receipt.counters.fact_sqlite_rows_selected, 0);
    assert_eq!(receipt.counters.strict_journal_inspections, 0);
    assert!(receipt.counters.event_decodes > 0);
}

#[test]
fn revision_show_omitted_and_fragment_selectors_stay_legacy() {
    // A single-revision store: the omitted operand and a unique fragment both
    // succeed through the legacy path in every derived state, byte-equal to
    // the explicit-off lane, with truthful legacy multiplicity counters.
    let repo = GitRepo::new();
    repo.write(
        "src/lib.rs",
        "pub fn value() -> u32 { 1 }
",
    );
    repo.commit_all("base");
    repo.write(
        "src/lib.rs",
        "pub fn value() -> u32 { 2 }
",
    );
    let repo_arg = repo.path().to_string_lossy().into_owned();
    let capture = pointbreak_env(["capture", "--repo", &repo_arg], OFF_ENV);
    assert_success("capture legacy-shape revision", &capture);
    let capture: serde_json::Value = serde_json::from_slice(&capture.stdout).expect("capture JSON");
    let revision = capture["revision"]["revisionId"]
        .as_str()
        .expect("captured revision id")
        .to_owned();
    let digest = revision
        .rsplit_once("sha256:")
        .expect("revision digest")
        .1
        .to_owned();

    let omitted = strings(&["revision", "show", "--repo", &repo_arg, "--format", "json"]);
    let fragment = strings(&[
        "revision",
        "show",
        &digest[..8],
        "--repo",
        &repo_arg,
        "--format",
        "json",
    ]);
    let full = strings(&[
        "revision", "show", &revision, "--repo", &repo_arg, "--format", "json",
    ]);

    let assert_legacy_parity = |label: &str| {
        for (name, arguments) in [("omitted", &omitted), ("fragment", &fragment)] {
            let off = run_binary(arguments, OFF_ENV);
            assert_success(&format!("{label} {name} explicit-off"), &off);
            let document: serde_json::Value =
                serde_json::from_slice(&off.stdout).expect("legacy JSON");
            assert!(
                document.get("projectionStamp").is_none(),
                "{label} {name} must never carry a derived identity"
            );
            let active = run_binary(arguments, ACTIVE_ENV);
            assert_eq!(active.status.code(), off.status.code(), "{label} {name}");
            assert_eq!(active.stdout, off.stdout, "{label} {name} stdout");
            assert_eq!(active.stderr, off.stderr, "{label} {name} stderr");
        }
    };
    assert_legacy_parity("unbuilt");
    build_derived(&repo);
    assert_legacy_parity("current");

    // One counted successful unique-fragment witness: the legacy lane keeps
    // its complete-history decode multiplicity even while a current derived
    // generation exists.
    // The full-ID shape is the qualified route: under active-current it goes
    // derived. The fragment stays legacy, so its bytes match the explicit-off
    // full-selector output.
    let full_output = run_binary(&full, OFF_ENV);
    assert_success("full selector reference", &full_output);
    let full_active = run_binary(&full, ACTIVE_ENV);
    assert_success("full selector active-current", &full_active);
    let full_active_document: serde_json::Value =
        serde_json::from_slice(&full_active.stdout).expect("active full JSON");
    assert!(
        full_active_document["projectionStamp"].as_str().is_some(),
        "the qualified full-ID shape serves the derived identity when current"
    );
    let authority = pointbreak::session::store_capability_for_repo(repo.path())
        .expect("inspect legacy-shape authority")
        .cursor;
    let receipt_dir = tempfile::tempdir().expect("temporary receipt directory");
    let receipt_path = receipt_dir.path().join("revision-show-fragment.json");
    let encoded = encode_counter_request("revision-show-unique-fragment", &receipt_path);
    let counted_arguments = [
        vec!["--longitudinal-counting".to_owned(), encoded],
        fragment.clone(),
    ]
    .concat();
    let counted = run_binary(&counted_arguments, ACTIVE_ENV);
    assert_eq!(counted.status.code(), full_output.status.code());
    assert_eq!(
        counted.stdout, full_output.stdout,
        "the unique fragment resolves to the full selector's bytes"
    );
    let receipt: LongitudinalCounterReceiptV1 =
        serde_json::from_slice(&fs::read(&receipt_path).expect("receipt bytes"))
            .expect("receipt JSON");
    receipt.validate().expect("valid fragment receipt");
    assert!(receipt.success);
    assert_eq!(receipt.counters.strict_journal_inspections, 0);
    assert_eq!(receipt.counters.fact_sqlite_rows_selected, 0);
    assert_eq!(
        receipt.counters.event_decodes % authority.event_count,
        0,
        "legacy decode multiplicity stays a whole number of complete passes"
    );
    assert!(
        receipt.counters.event_decodes >= authority.event_count * 2,
        "the fragment lane keeps its index build plus complete read"
    );
    assert!(receipt.counters.directory_entries_walked >= authority.journal_record_count);
}

/// Clone one stored event under a fresh idempotency key with the retired
/// `writer.role` envelope field re-attached. A retired-TYPE record cannot pass
/// the activated-store Journal router at all (its event-type code is unknown
/// there), so the lenient-skippable class an activated store can still hold is
/// exactly this retired-ENVELOPE shape: the router accepts it (it probes
/// schema, key digest, event id, payload hash, and the type code, never the
/// writer envelope), the strict typed decode refuses it, and the lenient
/// decode skips it as `unsupported_event_envelope`.
fn write_retired_envelope_record(repo_root: &Path) -> (PathBuf, Vec<u8>) {
    let events_dir = common_dir_store(repo_root).join("events");
    let source = fs::read_dir(&events_dir)
        .expect("list events directory")
        .filter_map(Result::ok)
        .find_map(|entry| {
            let bytes = fs::read(entry.path()).ok()?;
            let value: serde_json::Value = serde_json::from_slice(&bytes).ok()?;
            (value["schema"] == "shore.event").then_some(value)
        })
        .expect("find a source event to clone");
    let mut retired = source;
    let fresh_key = "fixture:retired-envelope";
    let stem = sha256(fresh_key.as_bytes());
    retired["idempotencyKey"] = serde_json::json!(fresh_key);
    retired["eventId"] = serde_json::json!(format!("evt:sha256:{stem}"));
    retired["writer"]
        .as_object_mut()
        .expect("event writer object")
        .insert("role".to_owned(), serde_json::json!("author"));
    let bytes = serde_json::to_vec(&retired).expect("serialize retired record");
    let path = events_dir.join(format!("{stem}.json"));
    fs::write(&path, &bytes).expect("write retired-envelope record");
    (path, bytes)
}

#[test]
fn lenient_skippable_records_cannot_present_an_active_current_generation() {
    // Proof (i) of the D22-A lenient-skip falsifier: a store whose lenient
    // read would skip an event cannot present an active-current generation.
    //
    // Leg 1: the strict derived rebuild refuses the retired-envelope record,
    // so no generation is ever published over it.
    let repo = GitRepo::new();
    repo.write("src/lib.rs", "pub fn value() -> u32 { 1 }\n");
    repo.commit_all("base");
    repo.write("src/lib.rs", "pub fn value() -> u32 { 2 }\n");
    let repo_arg = repo.path().to_string_lossy().into_owned();
    let capture = pointbreak_env(["capture", "--repo", &repo_arg], OFF_ENV);
    assert_success("capture falsifier revision", &capture);
    let capture: serde_json::Value = serde_json::from_slice(&capture.stdout).expect("capture JSON");
    let revision = capture["revision"]["revisionId"]
        .as_str()
        .expect("captured revision id")
        .to_owned();

    let (retired_path, retired) = write_retired_envelope_record(repo.path());
    let build = pointbreak_env(
        ["store", "derived", "build", "--repo", &repo_arg],
        ACTIVE_ENV,
    );
    assert!(
        !build.status.success(),
        "the strict derived rebuild must refuse a lenient-skippable record; stdout:\n{}",
        String::from_utf8_lossy(&build.stdout)
    );
    assert_eq!(derived_publication_count(repo.path()), 0);

    // The display lane still serves the store leniently and surfaces the skip
    // diagnostic through the store-wide lenient read (D22-A class (d)).
    let arguments = strings(&[
        "revision", "show", &revision, "--repo", &repo_arg, "--format", "json",
    ]);
    let off = run_binary(&arguments, OFF_ENV);
    assert_success("lenient off-lane revision show", &off);
    let off_document: serde_json::Value =
        serde_json::from_slice(&off.stdout).expect("off-lane JSON");
    assert!(
        diagnostic_codes(&off_document)
            .iter()
            .any(|code| code == "unsupported_event_envelope"),
        "the off lane surfaces the lenient skip diagnostic: {:?}",
        diagnostic_codes(&off_document)
    );
    let active = run_binary(&arguments, ACTIVE_ENV);
    assert_eq!(active.status.code(), off.status.code());
    assert_eq!(active.stdout, off.stdout);
    assert_eq!(active.stderr, off.stderr);

    // Leg 2: a generation published before the record appeared is stale at the
    // request checkpoint, so the qualified lane cannot serve it as current.
    fs::remove_file(&retired_path).expect("remove retired-envelope record");
    let rebuild = pointbreak_env(
        ["store", "derived", "build", "--repo", &repo_arg],
        ACTIVE_ENV,
    );
    assert_success("build clean falsifier generation", &rebuild);
    fs::write(&retired_path, &retired).expect("re-append retired-envelope record");
    let stale_off = run_binary(&arguments, OFF_ENV);
    assert_success("stale off-lane revision show", &stale_off);
    let stale_active = run_binary(&arguments, ACTIVE_ENV);
    assert_eq!(stale_active.status.code(), stale_off.status.code());
    assert_eq!(
        stale_active.stdout, stale_off.stdout,
        "a stale generation must not serve derived-current output"
    );
    let stale_document: serde_json::Value =
        serde_json::from_slice(&stale_active.stdout).expect("stale active JSON");
    assert!(
        stale_document.get("projectionStamp").is_none(),
        "no derived identity may appear over a stale generation"
    );
}

#[test]
fn revision_show_counter_and_phase_contract() {
    let fixture = revision_show_fixture();
    let execution = execution_identity();
    let receipt_dir = tempfile::tempdir().expect("temporary receipt directory");
    let arguments = revision_show_arguments(&fixture, &fixture.addressed);
    let fixture_identity = sha256(fixture.addressed.as_bytes());

    let expected = |setup: Setup| ExpectedContext {
        execution: execution.clone(),
        route: Route::RevisionShowDetail,
        arguments: arguments.clone(),
        setup_expectation: setup,
        fixture_identity_sha256: Some(fixture_identity.clone()),
        revision: Some(fixture.addressed.clone()),
        track: None,
        domain_actor: Some(DOMAIN_ACTOR.to_owned()),
        expected_child_actors: BTreeMap::new(),
    };

    // Explicit off: the compatibility cell keeps its truthful full-replay
    // multiplicity — one complete lenient read, no zero-duplicate claim, no
    // derived work.
    let off_expected = expected(Setup::FactExplicitOff);
    let off_receipt = run_success_case(
        "revision_show_explicit_off",
        &arguments,
        &fixture.off_env(),
        &off_expected,
        &receipt_dir,
        &[Phase::SerializationAndOutput],
        &[
            Phase::CliCapabilityPreflightH1,
            Phase::SqliteSelection,
            Phase::ReadTransaction,
            Phase::CheckpointAndWal,
            Phase::GenerationLeaseAndRetention,
        ],
        true,
    );
    assert_eq!(
        off_receipt.observed.route_state,
        ObservedState::AuthoritativeReplay
    );
    assert_eq!(off_receipt.counters.fact_sqlite_rows_selected, 0);
    assert!(off_receipt.counters.event_decodes > 0);
    assert_eq!(
        off_receipt.counters.event_validations,
        off_receipt.counters.event_decodes
    );
    assert!(off_receipt.children.is_empty());

    // Derived current: the target cell performs zero Change construction and
    // zero strict Journal inspection, with truthful fact-row selection and the
    // four RevisionDetail phases, even with unrelated history present.
    build_derived(&fixture.repo);
    let current_expected = expected(Setup::FactActiveCurrent);
    let mut required =
        pointbreak::bench_support::longitudinal::INTERACTION_REVISION_DETAIL_CURRENT_REQUIRED_PHASES_V1
            .to_vec();
    required.extend([
        Phase::GitContextResolution,
        Phase::RouteRevisionSelection,
        Phase::RouteProjectionFold,
        Phase::SerializationAndOutput,
    ]);
    let current_receipt = run_success_case(
        "revision_show_current_exact",
        &arguments,
        &fixture.active_env(),
        &current_expected,
        &receipt_dir,
        &required,
        &INTERACTION_FACT_CURRENT_FORBIDDEN_PHASES_V1,
        false,
    );
    assert_eq!(
        current_receipt.observed.route_state,
        ObservedState::DerivedCurrent
    );
    assert_eq!(current_receipt.counters.strict_journal_inspections, 0);
    assert_eq!(current_receipt.counters.change_semantic_constructions, 0);
    assert_eq!(current_receipt.counters.change_projection_constructions, 0);
    assert!(current_receipt.counters.fact_sqlite_rows_selected > 0);
    assert_eq!(current_receipt.counters.directory_entries_walked, 0);
    assert_eq!(current_receipt.counters.authoritative_fallbacks, 0);
    assert_eq!(current_receipt.counters.full_history_fallbacks, 0);
    assert!(current_receipt.children.is_empty());
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
        revision: fixture.and_then(|fixture| {
            let carries_revision_selector = !matches!(
                route,
                Route::ChangeProfileRead | Route::ChangeListRead | Route::ChangeAttentionRead
            );
            carries_revision_selector.then(|| fixture.revision.clone())
        }),
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
