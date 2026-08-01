#![cfg_attr(not(any(test, feature = "bench")), allow(dead_code))]

use std::ffi::OsStr;

use serde::{Deserialize, Serialize};

use crate::canonical_hash::{canonical_json_bytes, sha256_bytes_hex};

pub(crate) const DERIVED_ACCESS_PROFILE_ENV: &str = "POINTBREAK_DERIVED_ACCESS";
pub(crate) const PRODUCT_INTEGRATION_CONTRACT_SCHEMA_V1: &str =
    "pointbreak.derived-access-product-integration-contract.v1";
pub(crate) const PRODUCT_INTEGRATION_CONTRACT_PUBLICATION_SCHEMA_V1: &str =
    "pointbreak.derived-access-product-integration-contract-publication.v1";
pub(crate) const PRODUCT_INTEGRATION_CONTRACT_FIXTURE_SCHEMA_V1: &str =
    "pointbreak.derived-access-product-integration-contract-fixture.v1";
pub(crate) const PRODUCT_INTEGRATION_CONTRACT_SMOKE_SCHEMA_V1: &str =
    "pointbreak.derived-access-product-integration-contract-smoke.v1";
pub(crate) const PRODUCT_INTEGRATION_CONTRACT_SHA256_V1: &str =
    "3afe3a1fd65f0d5c58246dbe426c35a32325dc42bd9931d78f0e2354411dd00d";

const MIB: u64 = 1024 * 1024;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub(crate) enum DerivedAccessProfile {
    #[serde(rename = "off")]
    Off,
    #[serde(rename = "sqlite-wal-bodyless-v1")]
    SqliteWalBodylessV1,
}

impl DerivedAccessProfile {
    pub(crate) const ALL: [Self; 2] = [Self::Off, Self::SqliteWalBodylessV1];

    pub(crate) fn parse(value: Option<&OsStr>) -> Result<Self, DerivedAccessProfileError> {
        let Some(value) = value else {
            return Ok(Self::Off);
        };
        let value = value
            .to_str()
            .ok_or(DerivedAccessProfileError::NonUnicode)?;
        match value {
            "off" => Ok(Self::Off),
            "sqlite-wal-bodyless-v1" => Ok(Self::SqliteWalBodylessV1),
            _ => Err(DerivedAccessProfileError::Unsupported(value.to_owned())),
        }
    }

    #[allow(dead_code)]
    pub(crate) fn from_environment() -> Result<Self, DerivedAccessProfileError> {
        Self::parse(std::env::var_os(DERIVED_ACCESS_PROFILE_ENV).as_deref())
    }

    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::SqliteWalBodylessV1 => "sqlite-wal-bodyless-v1",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub(crate) enum DerivedAccessProfileError {
    #[error("{DERIVED_ACCESS_PROFILE_ENV} must be Unicode")]
    NonUnicode,
    #[error(
        "unsupported {DERIVED_ACCESS_PROFILE_ENV} value {0:?}; expected off or sqlite-wal-bodyless-v1"
    )]
    Unsupported(String),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum DerivedAccessAvailability {
    Absent,
    Bootstrapping,
    Current,
    CatchingUp,
    RebuildRequired,
    Quarantined,
    Unavailable,
}

impl DerivedAccessAvailability {
    pub(crate) const ALL: [Self; 7] = [
        Self::Absent,
        Self::Bootstrapping,
        Self::Current,
        Self::CatchingUp,
        Self::RebuildRequired,
        Self::Quarantined,
        Self::Unavailable,
    ];

    pub(crate) fn allowed_successors(self) -> Vec<Self> {
        match self {
            Self::Absent => vec![Self::Bootstrapping, Self::Unavailable],
            Self::Bootstrapping => vec![
                Self::Absent,
                Self::Current,
                Self::RebuildRequired,
                Self::Quarantined,
                Self::Unavailable,
            ],
            Self::Current => vec![
                Self::CatchingUp,
                Self::RebuildRequired,
                Self::Quarantined,
                Self::Unavailable,
            ],
            Self::CatchingUp => vec![
                Self::Current,
                Self::RebuildRequired,
                Self::Quarantined,
                Self::Unavailable,
            ],
            Self::RebuildRequired => {
                vec![Self::Bootstrapping, Self::Quarantined, Self::Unavailable]
            }
            Self::Quarantined => {
                vec![Self::Absent, Self::Bootstrapping, Self::Unavailable]
            }
            Self::Unavailable => vec![
                Self::Absent,
                Self::Bootstrapping,
                Self::RebuildRequired,
                Self::Quarantined,
            ],
        }
    }

    pub(crate) fn allows(self, successor: Self) -> bool {
        self.allowed_successors().contains(&successor)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ProductRouteId {
    Freshness,
    HistoryNewCount,
    HistoryChronological,
    HistoryBodylessFilter,
    HistoryBodySearch,
    Revisions,
    RevisionDetail,
    Threads,
    Attention,
}

impl ProductRouteId {
    pub(crate) const ALL: [Self; 9] = [
        Self::Freshness,
        Self::HistoryNewCount,
        Self::HistoryChronological,
        Self::HistoryBodylessFilter,
        Self::HistoryBodySearch,
        Self::Revisions,
        Self::RevisionDetail,
        Self::Threads,
        Self::Attention,
    ];
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ProductWorkClass {
    CheapAuthorityMetadata,
    BoundedSelected,
    OutputProportional,
    HistoryProportional,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum DerivedAccessFallback {
    CheapAuthoritativeDetector,
    TypedUnavailable,
    IntentionalExhaustiveBodySearch,
    InstrumentedExactDetail,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum RequestCounterId {
    DirectoryEntriesWalked,
    CarrierOpens,
    CarrierBytesRead,
    EventDecodes,
    EventValidations,
    EventFolds,
    ChronologicalSortItems,
    BodyArtifactReads,
    ObjectArtifactReads,
    ProjectionRebuilds,
    StateRebuilds,
    SelectedOutputRows,
    RetainedHydratedBytes,
    AuthoritativeFallbacks,
    FullHistoryFallbacks,
    MixedWriterAudits,
}

impl RequestCounterId {
    pub(crate) const ALL: [Self; 16] = [
        Self::DirectoryEntriesWalked,
        Self::CarrierOpens,
        Self::CarrierBytesRead,
        Self::EventDecodes,
        Self::EventValidations,
        Self::EventFolds,
        Self::ChronologicalSortItems,
        Self::BodyArtifactReads,
        Self::ObjectArtifactReads,
        Self::ProjectionRebuilds,
        Self::StateRebuilds,
        Self::SelectedOutputRows,
        Self::RetainedHydratedBytes,
        Self::AuthoritativeFallbacks,
        Self::FullHistoryFallbacks,
        Self::MixedWriterAudits,
    ];
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ProjectionVersionDecision {
    ProjectionStamp,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ProjectionStampComponent {
    StoreIdentity,
    Profile,
    SchemaVersion,
    Epoch,
    AppliedSequence,
}

impl ProjectionStampComponent {
    pub(crate) const ALL: [Self; 5] = [
        Self::StoreIdentity,
        Self::Profile,
        Self::SchemaVersion,
        Self::Epoch,
        Self::AppliedSequence,
    ];
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum WireProjectionVersion {
    None,
    EventSetHash,
    ProjectionStamp,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ProductWireSurface {
    Freshness,
    HistoryNewCount,
    History,
    Revisions,
    RevisionDetail,
    Threads,
    Attention,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ProductParityFixtureId {
    History,
    Revisions,
    RevisionDetail,
    Threads,
    Attention,
    Freshness,
    MixedWriters,
    Rollback,
}

impl ProductParityFixtureId {
    pub(crate) const ALL: [Self; 8] = [
        Self::History,
        Self::Revisions,
        Self::RevisionDetail,
        Self::Threads,
        Self::Attention,
        Self::Freshness,
        Self::MixedWriters,
        Self::Rollback,
    ];
}

impl ProductWireSurface {
    const ALL: [Self; 7] = [
        Self::Freshness,
        Self::HistoryNewCount,
        Self::History,
        Self::Revisions,
        Self::RevisionDetail,
        Self::Threads,
        Self::Attention,
    ];
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ClientVersionTransition {
    ProjectionStampOnly,
    ProjectionStampThenEventSetHash,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub(crate) enum MatchedOperationId {
    SemanticId,
    FreshNoChange,
    #[serde(rename = "NEWCOUNT_ZERO")]
    NewCountZero,
    WindowHead,
    WindowMiddle,
    WindowTail,
    RevisionDetailActive,
    RevisionDetailRemoved,
    AppendOne,
    PostOne,
    Restart,
}

impl MatchedOperationId {
    const ALL: [Self; 11] = [
        Self::SemanticId,
        Self::FreshNoChange,
        Self::NewCountZero,
        Self::WindowHead,
        Self::WindowMiddle,
        Self::WindowTail,
        Self::RevisionDetailActive,
        Self::RevisionDetailRemoved,
        Self::AppendOne,
        Self::PostOne,
        Self::Restart,
    ];
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ProductGateId {
    DefaultOffNoSidecarAction,
    OrdinaryRoutesAvoidFullHistory,
    SelectedCarriersAuthoritativelyValidated,
    BodySearchOnlyExhaustiveFallback,
    EveryFallbackEmitsReceipt,
    ProjectionStampAvoidsFullSetHash,
    CurrentRequiresExactCoverage,
    TruthSuccessDerivedDegradedIsExplicit,
    L100SteadyRssAtMost128Mib,
    ZeroFullHistoryHydratedCacheBytes,
    QualificationMatchedOperationCeilings,
    ShellAvailableDuringRebuild,
    MixedWriterNeverFalseCurrent,
    WireTransitionBackwardCompatible,
    NoProductionActivation,
}

impl ProductGateId {
    pub(crate) const ALL: [Self; 15] = [
        Self::DefaultOffNoSidecarAction,
        Self::OrdinaryRoutesAvoidFullHistory,
        Self::SelectedCarriersAuthoritativelyValidated,
        Self::BodySearchOnlyExhaustiveFallback,
        Self::EveryFallbackEmitsReceipt,
        Self::ProjectionStampAvoidsFullSetHash,
        Self::CurrentRequiresExactCoverage,
        Self::TruthSuccessDerivedDegradedIsExplicit,
        Self::L100SteadyRssAtMost128Mib,
        Self::ZeroFullHistoryHydratedCacheBytes,
        Self::QualificationMatchedOperationCeilings,
        Self::ShellAvailableDuringRebuild,
        Self::MixedWriterNeverFalseCurrent,
        Self::WireTransitionBackwardCompatible,
        Self::NoProductionActivation,
    ];
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct AvailabilityStateContractV1 {
    pub(crate) state: DerivedAccessAvailability,
    pub(crate) allowed_successors: Vec<DerivedAccessAvailability>,
    pub(crate) meaning: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ProjectionVersionContractV1 {
    pub(crate) decision: ProjectionVersionDecision,
    pub(crate) field: String,
    pub(crate) schema: String,
    pub(crate) stamp_components: Vec<ProjectionStampComponent>,
    pub(crate) event_set_hash_required_on_loose_responses: bool,
    pub(crate) event_set_hash_optional_deprecated_on_active_responses: bool,
    pub(crate) changes_on_unique_event: bool,
    pub(crate) changes_on_rebuild_epoch: bool,
    pub(crate) full_set_hash_on_ordinary_read_or_append_allowed: bool,
    pub(crate) truth_identity_or_signature_input: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct WireConsumerContractV1 {
    pub(crate) surface: ProductWireSurface,
    pub(crate) loose_version: WireProjectionVersion,
    pub(crate) loose_event_set_hash_required: bool,
    pub(crate) active_version: WireProjectionVersion,
    pub(crate) active_event_set_hash_required: bool,
    pub(crate) client_transition: ClientVersionTransition,
    pub(crate) schema_version_changes: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ProductParityFixtureContractV1 {
    pub(crate) fixture: ProductParityFixtureId,
    pub(crate) default_off_exact: bool,
    pub(crate) active_domain_parity: bool,
    pub(crate) active_wire_parity: bool,
    pub(crate) request_counter_receipt: bool,
    pub(crate) selected_carrier_validation: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ProductRouteContractV1 {
    pub(crate) route: ProductRouteId,
    pub(crate) active_path: String,
    pub(crate) active_work: Option<ProductWorkClass>,
    pub(crate) fallback: DerivedAccessFallback,
    pub(crate) fallback_work: Option<ProductWorkClass>,
    pub(crate) selected_carrier_validation: bool,
    pub(crate) emits_request_receipt: bool,
    pub(crate) unavailable_is_typed: bool,
    pub(crate) counters: Vec<RequestCounterId>,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct MatchedOperationLimitV1 {
    pub(crate) operation: MatchedOperationId,
    pub(crate) l100_wall_p95_ceiling_ms: u64,
    pub(crate) l100_process_cpu_p95_ceiling_ms: u64,
    pub(crate) comparison: String,
    pub(crate) release_promise: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ProductGateContractV1 {
    pub(crate) gate: ProductGateId,
    pub(crate) requirement: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ProductResourceContractV1 {
    pub(crate) l100_steady_rss_bytes: u64,
    pub(crate) full_history_hydrated_cache_bytes: u64,
    pub(crate) default_off_startup_baseline_required: bool,
    pub(crate) active_startup_paired_to_default_off: bool,
    pub(crate) shell_available_during_rebuild: bool,
    pub(crate) numeric_release_threshold_present: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ProductIntegrationContractV1 {
    pub(crate) schema: String,
    pub(crate) contract_id: String,
    pub(crate) selector_environment: String,
    pub(crate) profiles: Vec<DerivedAccessProfile>,
    pub(crate) default_profile: DerivedAccessProfile,
    pub(crate) authoritative_truth: String,
    pub(crate) availability_states: Vec<AvailabilityStateContractV1>,
    pub(crate) projection_version: ProjectionVersionContractV1,
    pub(crate) wire_consumers: Vec<WireConsumerContractV1>,
    pub(crate) parity_fixtures: Vec<ProductParityFixtureContractV1>,
    pub(crate) routes: Vec<ProductRouteContractV1>,
    pub(crate) request_counters: Vec<RequestCounterId>,
    pub(crate) matched_operation_limits: Vec<MatchedOperationLimitV1>,
    pub(crate) resources: ProductResourceContractV1,
    pub(crate) gates: Vec<ProductGateContractV1>,
    pub(crate) physical_implementation_in_contract: bool,
    pub(crate) product_route_in_contract: bool,
    pub(crate) production_activation_authorized: bool,
    pub(crate) migration_authorized: bool,
    pub(crate) adr_authorized: bool,
}

impl ProductIntegrationContractV1 {
    fn frozen() -> Self {
        Self {
            schema: PRODUCT_INTEGRATION_CONTRACT_SCHEMA_V1.to_owned(),
            contract_id: "derived-access-product-integration-v1".to_owned(),
            selector_environment: DERIVED_ACCESS_PROFILE_ENV.to_owned(),
            profiles: DerivedAccessProfile::ALL.to_vec(),
            default_profile: DerivedAccessProfile::Off,
            authoritative_truth: "loose Journal and ContentStore carriers".to_owned(),
            availability_states: DerivedAccessAvailability::ALL
                .into_iter()
                .map(availability_contract)
                .collect(),
            projection_version: ProjectionVersionContractV1 {
                decision: ProjectionVersionDecision::ProjectionStamp,
                field: "projectionStamp".to_owned(),
                schema: "pointbreak.derived-access-projection-stamp.v1".to_owned(),
                stamp_components: ProjectionStampComponent::ALL.to_vec(),
                event_set_hash_required_on_loose_responses: true,
                event_set_hash_optional_deprecated_on_active_responses: true,
                changes_on_unique_event: true,
                changes_on_rebuild_epoch: true,
                full_set_hash_on_ordinary_read_or_append_allowed: false,
                truth_identity_or_signature_input: false,
            },
            wire_consumers: ProductWireSurface::ALL
                .into_iter()
                .map(wire_consumer_contract)
                .collect(),
            parity_fixtures: ProductParityFixtureId::ALL
                .into_iter()
                .map(parity_fixture_contract)
                .collect(),
            routes: ProductRouteId::ALL
                .into_iter()
                .map(route_contract)
                .collect(),
            request_counters: RequestCounterId::ALL.to_vec(),
            matched_operation_limits: MatchedOperationId::ALL
                .into_iter()
                .map(matched_operation_limit)
                .collect(),
            resources: ProductResourceContractV1 {
                l100_steady_rss_bytes: 128 * MIB,
                full_history_hydrated_cache_bytes: 0,
                default_off_startup_baseline_required: true,
                active_startup_paired_to_default_off: true,
                shell_available_during_rebuild: true,
                numeric_release_threshold_present: false,
            },
            gates: ProductGateId::ALL
                .into_iter()
                .map(product_gate_contract)
                .collect(),
            physical_implementation_in_contract: false,
            product_route_in_contract: false,
            production_activation_authorized: false,
            migration_authorized: false,
            adr_authorized: false,
        }
    }

    pub(crate) fn canonical_sha256(&self) -> Result<String, String> {
        canonical_sha256(self)
    }

    pub(crate) fn validate(&self) -> Result<(), String> {
        if self != &Self::frozen() {
            return Err("unsupported derived-access product-integration contract".to_owned());
        }
        let actual = self.canonical_sha256()?;
        if actual != PRODUCT_INTEGRATION_CONTRACT_SHA256_V1 {
            return Err(format!(
                "derived-access product-integration contract hash is not frozen: expected {}, actual {actual}",
                PRODUCT_INTEGRATION_CONTRACT_SHA256_V1
            ));
        }
        Ok(())
    }

    pub(crate) fn decision_table_markdown(&self) -> String {
        [
            "| Decision | Frozen product-integration requirement |".to_owned(),
            "| --- | --- |".to_owned(),
            format!(
                "| Selector | `{DERIVED_ACCESS_PROFILE_ENV}=off|sqlite-wal-bodyless-v1`; default `off`; unknown/non-Unicode values fail |"
            ),
            "| Authority | loose `Journal` and `ContentStore` carriers remain the only truth; active state is private, bodyless, disposable, and rebuildable |".to_owned(),
            "| Availability | `absent`, `bootstrapping`, `current`, `catching_up`, `rebuild_required`, `quarantined`, `unavailable`; `current` requires exact observed coverage |".to_owned(),
            "| Version wire | preserve `eventSetHash` on loose responses; active responses require cursor-derived `projectionStamp` and may omit the deprecated `eventSetHash` |".to_owned(),
            "| Stamp identity | store identity + profile + schema version + epoch + applied sequence; changes on every unique event and rebuild epoch; never truth/signature identity |".to_owned(),
            "| Ordinary work | fixed-output routes are bounded-selected; complete collections are output-proportional; only body search is history-proportional |".to_owned(),
            "| Fallbacks | cheap freshness detector, typed unavailable responses, intentional exhaustive body search, and instrumented exact-detail fallback; every invocation has a request receipt |".to_owned(),
            format!(
                "| Resource gate | L100 steady RSS at most `{} MiB`; zero full-history hydrated cache bytes; startup paired to measured default-off baseline; shell/status available during rebuild |",
                self.resources.l100_steady_rss_bytes / MIB
            ),
            "| Matched operations | retain the qualified L100 wall/CPU comparison ceilings without creating a release promise; product-only actuals remain paired to loose |".to_owned(),
            "| Stop boundaries | no physical module, sidecar, active product route, production default, migration, production activation, or release |".to_owned(),
        ]
        .join("\n")
    }
}

pub(crate) fn product_integration_contract_v1() -> ProductIntegrationContractV1 {
    ProductIntegrationContractV1::frozen()
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ProductIntegrationContractPublicationV1 {
    pub(crate) schema: String,
    pub(crate) mode: String,
    pub(crate) contract: ProductIntegrationContractV1,
    pub(crate) contract_sha256: String,
    pub(crate) decision_table_markdown: String,
}

pub(crate) fn product_integration_contract_publication_v1()
-> ProductIntegrationContractPublicationV1 {
    let contract = product_integration_contract_v1();
    ProductIntegrationContractPublicationV1 {
        schema: PRODUCT_INTEGRATION_CONTRACT_PUBLICATION_SCHEMA_V1.to_owned(),
        mode: "non_timing_contract_publication".to_owned(),
        contract_sha256: contract
            .canonical_sha256()
            .expect("the frozen product-integration contract is canonical"),
        decision_table_markdown: contract.decision_table_markdown(),
        contract,
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ProductIntegrationContractFixtureV1 {
    pub(crate) schema: String,
    pub(crate) contract_schema: String,
    pub(crate) contract_sha256: String,
    pub(crate) selector_environment: String,
    pub(crate) profiles: Vec<DerivedAccessProfile>,
    pub(crate) availability_states: Vec<AvailabilityStateContractV1>,
    pub(crate) projection_version: ProjectionVersionContractV1,
    pub(crate) wire_consumers: Vec<WireConsumerContractV1>,
    pub(crate) parity_fixtures: Vec<ProductParityFixtureContractV1>,
    pub(crate) routes: Vec<ProductRouteContractV1>,
    pub(crate) request_counters: Vec<RequestCounterId>,
    pub(crate) matched_operation_limits: Vec<MatchedOperationLimitV1>,
    pub(crate) gate_ids: Vec<ProductGateId>,
}

pub(crate) fn product_integration_contract_fixture_v1() -> ProductIntegrationContractFixtureV1 {
    let contract = product_integration_contract_v1();
    ProductIntegrationContractFixtureV1 {
        schema: PRODUCT_INTEGRATION_CONTRACT_FIXTURE_SCHEMA_V1.to_owned(),
        contract_schema: contract.schema,
        contract_sha256: PRODUCT_INTEGRATION_CONTRACT_SHA256_V1.to_owned(),
        selector_environment: contract.selector_environment,
        profiles: contract.profiles,
        availability_states: contract.availability_states,
        projection_version: contract.projection_version,
        wire_consumers: contract.wire_consumers,
        parity_fixtures: contract.parity_fixtures,
        routes: contract.routes,
        request_counters: contract.request_counters,
        matched_operation_limits: contract.matched_operation_limits,
        gate_ids: contract.gates.into_iter().map(|gate| gate.gate).collect(),
    }
}

pub(crate) fn verify_product_integration_contract_fixture_v1() -> Result<(), String> {
    let fixture: ProductIntegrationContractFixtureV1 = serde_json::from_str(include_str!(
        "../../../tests/fixtures/derived-access/product-integration-v1.json"
    ))
    .map_err(|error| format!("product-integration fixture is invalid: {error}"))?;
    let expected = product_integration_contract_fixture_v1();
    if fixture != expected {
        return Err("product-integration fixture differs from the frozen contract".to_owned());
    }
    product_integration_contract_v1().validate()
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ProductIntegrationContractSmokeV1 {
    pub(crate) schema: String,
    pub(crate) contract_sha256: String,
    pub(crate) profiles: Vec<DerivedAccessProfile>,
    pub(crate) availability_state_count: usize,
    pub(crate) route_count: usize,
    pub(crate) gate_count: usize,
    pub(crate) filesystem_actions: u64,
    pub(crate) physical_implementation_opened: bool,
}

pub(crate) fn product_integration_contract_smoke_v1()
-> Result<ProductIntegrationContractSmokeV1, String> {
    verify_product_integration_contract_fixture_v1()?;
    let contract = product_integration_contract_v1();
    for state in DerivedAccessAvailability::ALL {
        let frozen = contract
            .availability_states
            .iter()
            .find(|row| row.state == state)
            .ok_or_else(|| format!("missing availability state {state:?}"))?;
        if frozen.allowed_successors != state.allowed_successors() {
            return Err(format!("availability transition drift for {state:?}"));
        }
    }
    if contract
        .routes
        .iter()
        .filter(|route| route.fallback_work == Some(ProductWorkClass::HistoryProportional))
        .map(|route| route.route)
        .collect::<Vec<_>>()
        != vec![ProductRouteId::HistoryBodySearch]
    {
        return Err("body search is not the sole exhaustive fallback".to_owned());
    }
    Ok(ProductIntegrationContractSmokeV1 {
        schema: PRODUCT_INTEGRATION_CONTRACT_SMOKE_SCHEMA_V1.to_owned(),
        contract_sha256: PRODUCT_INTEGRATION_CONTRACT_SHA256_V1.to_owned(),
        profiles: contract.profiles,
        availability_state_count: contract.availability_states.len(),
        route_count: contract.routes.len(),
        gate_count: contract.gates.len(),
        filesystem_actions: 0,
        physical_implementation_opened: false,
    })
}

fn availability_contract(state: DerivedAccessAvailability) -> AvailabilityStateContractV1 {
    let meaning = match state {
        DerivedAccessAvailability::Absent => {
            "no visible derived generation; loose truth remains available"
        }
        DerivedAccessAvailability::Bootstrapping => {
            "a private generation is rebuilding and is not current"
        }
        DerivedAccessAvailability::Current => {
            "the visible generation proves exact observed truth coverage"
        }
        DerivedAccessAvailability::CatchingUp => {
            "a bounded governed delta is being applied before serving current"
        }
        DerivedAccessAvailability::RebuildRequired => {
            "coverage, mixed-writer, or integrity evidence requires a rebuild"
        }
        DerivedAccessAvailability::Quarantined => {
            "an invalid generation is isolated and cannot serve reads"
        }
        DerivedAccessAvailability::Unavailable => {
            "the active profile cannot safely serve and returns typed unavailability"
        }
    };
    AvailabilityStateContractV1 {
        state,
        allowed_successors: state.allowed_successors(),
        meaning: meaning.to_owned(),
    }
}

fn wire_consumer_contract(surface: ProductWireSurface) -> WireConsumerContractV1 {
    let loose_event_set_hash_required = matches!(
        surface,
        ProductWireSurface::History
            | ProductWireSurface::Revisions
            | ProductWireSurface::RevisionDetail
            | ProductWireSurface::Threads
            | ProductWireSurface::Attention
    );
    WireConsumerContractV1 {
        surface,
        loose_version: if loose_event_set_hash_required {
            WireProjectionVersion::EventSetHash
        } else {
            WireProjectionVersion::None
        },
        loose_event_set_hash_required,
        active_version: WireProjectionVersion::ProjectionStamp,
        active_event_set_hash_required: false,
        client_transition: if loose_event_set_hash_required {
            ClientVersionTransition::ProjectionStampThenEventSetHash
        } else {
            ClientVersionTransition::ProjectionStampOnly
        },
        schema_version_changes: false,
    }
}

fn parity_fixture_contract(fixture: ProductParityFixtureId) -> ProductParityFixtureContractV1 {
    ProductParityFixtureContractV1 {
        fixture,
        default_off_exact: true,
        active_domain_parity: true,
        active_wire_parity: true,
        request_counter_receipt: true,
        selected_carrier_validation: matches!(
            fixture,
            ProductParityFixtureId::History
                | ProductParityFixtureId::Revisions
                | ProductParityFixtureId::RevisionDetail
        ),
    }
}

fn route_contract(route: ProductRouteId) -> ProductRouteContractV1 {
    let common = [
        RequestCounterId::DirectoryEntriesWalked,
        RequestCounterId::CarrierOpens,
        RequestCounterId::CarrierBytesRead,
        RequestCounterId::EventDecodes,
        RequestCounterId::EventValidations,
        RequestCounterId::EventFolds,
        RequestCounterId::ChronologicalSortItems,
        RequestCounterId::BodyArtifactReads,
        RequestCounterId::ObjectArtifactReads,
        RequestCounterId::SelectedOutputRows,
        RequestCounterId::RetainedHydratedBytes,
        RequestCounterId::AuthoritativeFallbacks,
        RequestCounterId::FullHistoryFallbacks,
        RequestCounterId::MixedWriterAudits,
    ]
    .to_vec();
    let (active_path, active_work, fallback, fallback_work, selected_validation) = match route {
        ProductRouteId::Freshness => (
            "derived head and checkpoint with exact mixed-writer detection",
            Some(ProductWorkClass::CheapAuthorityMetadata),
            DerivedAccessFallback::CheapAuthoritativeDetector,
            Some(ProductWorkClass::CheapAuthorityMetadata),
            false,
        ),
        ProductRouteId::HistoryNewCount => (
            "cursor and display-key count",
            Some(ProductWorkClass::BoundedSelected),
            DerivedAccessFallback::TypedUnavailable,
            None,
            false,
        ),
        ProductRouteId::HistoryChronological => (
            "locator window with selected authoritative hydration",
            Some(ProductWorkClass::BoundedSelected),
            DerivedAccessFallback::TypedUnavailable,
            None,
            true,
        ),
        ProductRouteId::HistoryBodylessFilter => (
            "bodyless locator and aggregate state with selected hydration",
            Some(ProductWorkClass::BoundedSelected),
            DerivedAccessFallback::TypedUnavailable,
            None,
            true,
        ),
        ProductRouteId::HistoryBodySearch => (
            "no derived body or search index",
            None,
            DerivedAccessFallback::IntentionalExhaustiveBodySearch,
            Some(ProductWorkClass::HistoryProportional),
            true,
        ),
        ProductRouteId::Revisions => (
            "materialized revision rows with selected overlay joins",
            Some(ProductWorkClass::OutputProportional),
            DerivedAccessFallback::TypedUnavailable,
            None,
            true,
        ),
        ProductRouteId::RevisionDetail => (
            "bounded revision facts with selected authoritative carriers",
            Some(ProductWorkClass::BoundedSelected),
            DerivedAccessFallback::InstrumentedExactDetail,
            Some(ProductWorkClass::BoundedSelected),
            true,
        ),
        ProductRouteId::Threads => (
            "materialized thread and supersession state",
            Some(ProductWorkClass::OutputProportional),
            DerivedAccessFallback::TypedUnavailable,
            None,
            false,
        ),
        ProductRouteId::Attention => (
            "materialized request, assessment, and validation state",
            Some(ProductWorkClass::OutputProportional),
            DerivedAccessFallback::TypedUnavailable,
            None,
            false,
        ),
    };
    ProductRouteContractV1 {
        route,
        active_path: active_path.to_owned(),
        active_work,
        fallback,
        fallback_work,
        selected_carrier_validation: selected_validation,
        emits_request_receipt: true,
        unavailable_is_typed: matches!(fallback, DerivedAccessFallback::TypedUnavailable),
        counters: common,
    }
}

fn matched_operation_limit(operation: MatchedOperationId) -> MatchedOperationLimitV1 {
    let (wall, cpu) = match operation {
        MatchedOperationId::SemanticId => (150, 100),
        MatchedOperationId::FreshNoChange | MatchedOperationId::NewCountZero => (50, 25),
        MatchedOperationId::WindowHead
        | MatchedOperationId::WindowMiddle
        | MatchedOperationId::WindowTail => (150, 100),
        MatchedOperationId::RevisionDetailActive | MatchedOperationId::RevisionDetailRemoved => {
            (250, 175)
        }
        MatchedOperationId::AppendOne => (250, 200),
        MatchedOperationId::PostOne => (500, 400),
        MatchedOperationId::Restart => (3_000, 2_500),
    };
    MatchedOperationLimitV1 {
        operation,
        l100_wall_p95_ceiling_ms: wall,
        l100_process_cpu_p95_ceiling_ms: cpu,
        comparison:
            "retain the qualified matched-operation ceiling and pair product actuals to loose"
                .to_owned(),
        release_promise: false,
    }
}

fn product_gate_contract(gate: ProductGateId) -> ProductGateContractV1 {
    let requirement = match gate {
        ProductGateId::DefaultOffNoSidecarAction => {
            "unset or off performs no sidecar filesystem action and preserves existing behavior"
        }
        ProductGateId::OrdinaryRoutesAvoidFullHistory => {
            "active ordinary fixed-output routes never list, decode, fold, sort, or retain full history"
        }
        ProductGateId::SelectedCarriersAuthoritativelyValidated => {
            "every returned carrier is reread and validated through Journal or ContentStore"
        }
        ProductGateId::BodySearchOnlyExhaustiveFallback => {
            "body search is the only pre-approved history-proportional fallback"
        }
        ProductGateId::EveryFallbackEmitsReceipt => {
            "every fallback, unavailable response, and full-history operation emits request counters"
        }
        ProductGateId::ProjectionStampAvoidsFullSetHash => {
            "ordinary reads and governed appends never whole-list history to preserve a version wire"
        }
        ProductGateId::CurrentRequiresExactCoverage => {
            "current is reported only at exact observed authoritative coverage"
        }
        ProductGateId::TruthSuccessDerivedDegradedIsExplicit => {
            "truth success with derived degradation is represented without ambiguity"
        }
        ProductGateId::L100SteadyRssAtMost128Mib => {
            "active L100 Inspector steady RSS is at most 128 MiB"
        }
        ProductGateId::ZeroFullHistoryHydratedCacheBytes => {
            "ordinary active routes own zero full-history hydrated cache bytes"
        }
        ProductGateId::QualificationMatchedOperationCeilings => {
            "qualified matched-operation ceilings remain independent comparison gates"
        }
        ProductGateId::ShellAvailableDuringRebuild => {
            "Inspector shell and status are available while a large rebuild continues"
        }
        ProductGateId::MixedWriterNeverFalseCurrent => {
            "legacy or out-of-band writes can never leave derived state labeled current"
        }
        ProductGateId::WireTransitionBackwardCompatible => {
            "clients prefer projectionStamp and fall back to eventSetHash on loose responses"
        }
        ProductGateId::NoProductionActivation => {
            "the experiment remains default-off and authorizes no migration, production activation, or release"
        }
    };
    ProductGateContractV1 {
        gate,
        requirement: requirement.to_owned(),
    }
}

pub(crate) const PRODUCTION_READINESS_CONTRACT_SCHEMA_V1: &str =
    "pointbreak.derived-access-production-readiness-contract.v1";
pub(crate) const PRODUCTION_READINESS_CONTRACT_PUBLICATION_SCHEMA_V1: &str =
    "pointbreak.derived-access-production-readiness-contract-publication.v1";
pub(crate) const PRODUCTION_READINESS_CONTRACT_FIXTURE_SCHEMA_V1: &str =
    "pointbreak.derived-access-production-readiness-contract-fixture.v1";
pub(crate) const PRODUCTION_READINESS_CONTRACT_SMOKE_SCHEMA_V1: &str =
    "pointbreak.derived-access-production-readiness-contract-smoke.v1";
pub(crate) const PRODUCTION_READINESS_CONTRACT_SHA256_V1: &str =
    "5445781aadff791537746f1596f577ea1415fd37daf6d0da4fbc0ded94375eb3";
pub(crate) const PRODUCTION_READINESS_FIXTURE_SHA256_V1: &str =
    "b2e2656f6f37a1173bbeb7ce9101e1c703d03b600468fc4b62fb2dc25a05ab6a";

#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ReadinessIdentityField {
    Profile,
    ParentContractSha256,
    ReadinessContractSha256,
    SourceCommit,
    SourceTree,
    CargoLockSha256,
    BinarySha256,
    OperatingSystem,
    Architecture,
    Filesystem,
    RootManifestSha256,
    EvidencePackageSha256,
}

impl ReadinessIdentityField {
    const PROFILE: [Self; 3] = [
        Self::Profile,
        Self::ParentContractSha256,
        Self::ReadinessContractSha256,
    ];
    const SOURCE: [Self; 4] = [
        Self::SourceCommit,
        Self::SourceTree,
        Self::CargoLockSha256,
        Self::BinarySha256,
    ];
    const EVIDENCE: [Self; 5] = [
        Self::OperatingSystem,
        Self::Architecture,
        Self::Filesystem,
        Self::RootManifestSha256,
        Self::EvidencePackageSha256,
    ];
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ReadinessOperationId {
    ActiveOrdinaryNoChange,
    FreshWriterAdmissionNoChange,
    ActiveRevisionsPage,
    BootstrapPopulation,
    FirstBootstrapAvailability,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ReadinessCounterId {
    EventDirectoryEntriesWalked,
    EventCarrierOpens,
    FullDecodedHistoriesRetained,
    RevisionRowsExamined,
    PageEntriesReturned,
    AutomaticAuthoritativeFallbacks,
    FullHistoryFallbacks,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum CounterCeilingBasis {
    Constant,
    RequestedEntries,
    SourceCarriers,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct CounterCeilingContextV1 {
    pub(crate) requested_entries: u64,
    pub(crate) source_carriers: u64,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ReadinessCounterCeilingV1 {
    pub(crate) operation: ReadinessOperationId,
    pub(crate) counter: ReadinessCounterId,
    pub(crate) basis: CounterCeilingBasis,
    pub(crate) multiplier: u64,
    pub(crate) additive: u64,
}

impl ReadinessCounterCeilingV1 {
    pub(crate) fn ceiling(&self, context: CounterCeilingContextV1) -> u64 {
        let basis = match self.basis {
            CounterCeilingBasis::Constant => 0,
            CounterCeilingBasis::RequestedEntries => context.requested_entries,
            CounterCeilingBasis::SourceCarriers => context.source_carriers,
        };
        basis
            .saturating_mul(self.multiplier)
            .saturating_add(self.additive)
    }

    pub(crate) fn allows(&self, observed: u64, context: CounterCeilingContextV1) -> bool {
        observed <= self.ceiling(context)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum AuthorityStampScenarioId {
    AbsentDirectory,
    EmptyDirectory,
    GovernedCreate,
    GovernedBurst,
    EqualDuplicateNoCreate,
    ConflictingDuplicateNoCreate,
    OutOfBandCreate,
    TempCreateThenRename,
    ConcurrentCreateObservation,
    CrashBeforeCarrierPublication,
    CrashAfterCarrierPublication,
    RapidMutations,
    CloseReopen,
    MachineOrVmRestart,
    CanonicalPathAlias,
    ProductionDirectoryLayout,
    SidecarDeletion,
    ExperimentOffRollback,
    UnrelatedFile,
    TemporaryFile,
    ExistingCarrierOverwrite,
}

impl AuthorityStampScenarioId {
    const ALL: [Self; 21] = [
        Self::AbsentDirectory,
        Self::EmptyDirectory,
        Self::GovernedCreate,
        Self::GovernedBurst,
        Self::EqualDuplicateNoCreate,
        Self::ConflictingDuplicateNoCreate,
        Self::OutOfBandCreate,
        Self::TempCreateThenRename,
        Self::ConcurrentCreateObservation,
        Self::CrashBeforeCarrierPublication,
        Self::CrashAfterCarrierPublication,
        Self::RapidMutations,
        Self::CloseReopen,
        Self::MachineOrVmRestart,
        Self::CanonicalPathAlias,
        Self::ProductionDirectoryLayout,
        Self::SidecarDeletion,
        Self::ExperimentOffRollback,
        Self::UnrelatedFile,
        Self::TemporaryFile,
        Self::ExistingCarrierOverwrite,
    ];
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum AuthorityStampExpectation {
    Stable,
    ChangedOrIndeterminate,
    StableOrChangedWithoutTruthClaim,
    ObservationNotApplicable,
    ExplicitNonClaim,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum AuthorityStampMutationLocus {
    None,
    EventDirectoryAuthoritativeCarrier,
    EventDirectoryNonCarrierEntry,
    AuthorityMetadata,
    ExistingAuthoritativeCarrier,
    ProcessLifecycle,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum AuthorityStampObservablePolicy {
    SelectedAndFrozenByNativeFalsifierBeforeIntegration,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct AuthorityStampScenarioContractV1 {
    pub(crate) scenario: AuthorityStampScenarioId,
    pub(crate) mutation_locus: AuthorityStampMutationLocus,
    pub(crate) expectation: AuthorityStampExpectation,
    pub(crate) authoritative_carrier_created: bool,
    pub(crate) event_directory_entries_walked_ceiling: u64,
    pub(crate) event_carrier_opens_ceiling: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum AuthorityFailureId {
    ChangedWithoutReceipt,
    IndeterminateStamp,
    CursorStampMismatch,
    LegacyDescriptor,
    StampPublicationFailedAfterTruthCreate,
}

impl AuthorityFailureId {
    const ALL: [Self; 5] = [
        Self::ChangedWithoutReceipt,
        Self::IndeterminateStamp,
        Self::CursorStampMismatch,
        Self::LegacyDescriptor,
        Self::StampPublicationFailedAfterTruthCreate,
    ];
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct AuthorityFailureContractV1 {
    pub(crate) failure: AuthorityFailureId,
    pub(crate) outcome: DerivedAccessAvailability,
    pub(crate) truth_success_is_preserved: bool,
    pub(crate) may_report_current: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct AuthorityStampContractV1 {
    pub(crate) scope: String,
    pub(crate) observable_policy: AuthorityStampObservablePolicy,
    pub(crate) scenarios: Vec<AuthorityStampScenarioContractV1>,
    pub(crate) failures: Vec<AuthorityFailureContractV1>,
    pub(crate) changed_or_indeterminate_never_proves_truth: bool,
    pub(crate) existing_carrier_overwrite_requires_selected_validation_or_audit: bool,
    pub(crate) malicious_tamper_detection_claimed: bool,
}

impl AuthorityStampContractV1 {
    pub(crate) fn scenario(
        &self,
        scenario: AuthorityStampScenarioId,
    ) -> Option<AuthorityStampExpectation> {
        self.scenarios
            .iter()
            .find(|row| row.scenario == scenario)
            .map(|row| row.expectation)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum BootstrapPhase {
    Preflight,
    Population,
    StrictOracleVerification,
    AuthorityStabilityCheck,
    Publication,
}

impl BootstrapPhase {
    const ALL: [Self; 5] = [
        Self::Preflight,
        Self::Population,
        Self::StrictOracleVerification,
        Self::AuthorityStabilityCheck,
        Self::Publication,
    ];
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum BootstrapProgressField {
    Phase,
    CompletedEvents,
    TotalEvents,
    CompletedBytes,
    TotalBytes,
    ElapsedMilliseconds,
    EtaMilliseconds,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum BootstrapAvailabilityCase {
    FirstBootstrap,
    ReplacementWithValidCurrent,
    ReplacementAfterStampDrift,
    Cancelled,
    Restarting,
}

impl BootstrapAvailabilityCase {
    const ALL: [Self; 5] = [
        Self::FirstBootstrap,
        Self::ReplacementWithValidCurrent,
        Self::ReplacementAfterStampDrift,
        Self::Cancelled,
        Self::Restarting,
    ];
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct BootstrapAvailabilityContractV1 {
    pub(crate) case: BootstrapAvailabilityCase,
    pub(crate) shell_status_and_progress_available: bool,
    pub(crate) offers_explicit_fallback_or_wait: bool,
    pub(crate) may_serve_valid_old_current: bool,
    pub(crate) may_serve_stale_as_current: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct BootstrapReadinessContractV1 {
    pub(crate) phases: Vec<BootstrapPhase>,
    pub(crate) required_progress_fields: Vec<BootstrapProgressField>,
    pub(crate) optional_progress_fields: Vec<BootstrapProgressField>,
    pub(crate) population_carrier_open_ceiling: u64,
    pub(crate) maximum_overlapping_full_decoded_histories: u64,
    pub(crate) strict_oracle_is_serial_or_isolated: bool,
    pub(crate) pre_post_stability_decodes_full_history: bool,
    pub(crate) transaction_batches_are_bounded: bool,
    pub(crate) cancellation_and_restart_are_identity_bound: bool,
    pub(crate) completion_is_published_last: bool,
    pub(crate) eta_requires_observed_stable_rate: bool,
    pub(crate) maximum_automatic_fallbacks: u64,
    pub(crate) long_unexplained_absent_response_allowed: bool,
    pub(crate) availability: Vec<BootstrapAvailabilityContractV1>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum RevisionPageRequestField {
    Limit,
    After,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum RevisionPageResponseField {
    Schema,
    EventSetHash,
    ProjectionStamp,
    EventCount,
    RevisionCount,
    Entries,
    Diagnostics,
    AsOf,
    Next,
}

impl RevisionPageResponseField {
    const ALL: [Self; 9] = [
        Self::Schema,
        Self::EventSetHash,
        Self::ProjectionStamp,
        Self::EventCount,
        Self::RevisionCount,
        Self::Entries,
        Self::Diagnostics,
        Self::AsOf,
        Self::Next,
    ];
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum RevisionPageOrderField {
    CapturedAtDescending,
    RevisionIdDescending,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum RevisionPageTokenFailure {
    InvalidRequest,
    RestartRequired,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum RevisionPageLimitOverflow {
    InvalidRequest,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum RevisionCountSource {
    DerivedIndexedAggregate,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct RevisionPageContractV1 {
    pub(crate) schema: String,
    pub(crate) request_fields: Vec<RevisionPageRequestField>,
    pub(crate) response_fields: Vec<RevisionPageResponseField>,
    pub(crate) default_limit: u64,
    pub(crate) maximum_limit: u64,
    pub(crate) absent_limit_uses_default: bool,
    pub(crate) over_maximum_limit: RevisionPageLimitOverflow,
    pub(crate) requested_entries_are_accepted_limit: bool,
    pub(crate) order: Vec<RevisionPageOrderField>,
    pub(crate) token_is_opaque: bool,
    pub(crate) token_binds_profile_schema_snapshot_and_order: bool,
    pub(crate) invalid_token: RevisionPageTokenFailure,
    pub(crate) stale_or_wrong_profile_token: RevisionPageTokenFailure,
    pub(crate) same_wire_shape_for_active_and_default_off: bool,
    pub(crate) active_work: ProductWorkClass,
    pub(crate) default_off_work: ProductWorkClass,
    pub(crate) default_off_exhaustive_comparator_is_explicit: bool,
    pub(crate) active_complete_collection_materialization_allowed: bool,
    pub(crate) default_off_complete_collection_materialization_allowed: bool,
    pub(crate) revision_count_is_present: bool,
    pub(crate) revision_count_source: RevisionCountSource,
    pub(crate) exact_detail_is_entity_primary: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ReadinessWorkId {
    InitialAuthorityCensus,
    AuthorityStampCheck,
    OrdinaryActiveRead,
    FreshWriterAdmission,
    BootstrapPopulation,
    StrictReplay,
    ExplicitIntegrityAudit,
    FirstBootstrapAuthoritativeFallback,
    DefaultOffRevisionsPage,
    IntentionalBodySearch,
    ActiveRevisionsPage,
    BackupRestoreVerification,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ReadinessWorkClass {
    BoundedMetadata,
    RequestedOutputProportional,
    ExhaustiveBootstrap,
    ExhaustiveAudit,
    ExplicitAuthoritativeFallback,
    ExhaustiveDefaultOffComparator,
    IntentionalExhaustiveSearch,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ReadinessWorkClassificationV1 {
    pub(crate) operation: ReadinessWorkId,
    pub(crate) classification: ReadinessWorkClass,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ReadinessEvidenceId {
    NativeMacosApfsD0L7,
    NativeWindowsNtfsD0L7,
    PackageClosure,
    RollbackAndBackup,
    RetainedL100Apfs,
    RetainedC262Apfs,
    EvidenceAuthority,
}

impl ReadinessEvidenceId {
    const ALL: [Self; 7] = [
        Self::NativeMacosApfsD0L7,
        Self::NativeWindowsNtfsD0L7,
        Self::PackageClosure,
        Self::RollbackAndBackup,
        Self::RetainedL100Apfs,
        Self::RetainedC262Apfs,
        Self::EvidenceAuthority,
    ];
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ReadinessEvidenceGateV1 {
    pub(crate) evidence: ReadinessEvidenceId,
    pub(crate) required: bool,
    pub(crate) native_platform: Option<String>,
    pub(crate) tiers: Vec<String>,
    pub(crate) completion_last: bool,
    pub(crate) read_only_verification: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ProtectedInputId {
    OwnerStore,
    PrivateCorpus,
    PrivateSnapshotBytes,
    QualificationCorpus,
    Credentials,
    InstalledArtifacts,
    InstalledExtensionDogfoodEvidence,
    RemoteMirrorAutomationState,
    ArchivedEvidence,
    ExternalParticipantEvidence,
    ProductionDefault,
    DeferredContentFormatExperiment,
}

impl ProtectedInputId {
    const ALL: [Self; 12] = [
        Self::OwnerStore,
        Self::PrivateCorpus,
        Self::PrivateSnapshotBytes,
        Self::QualificationCorpus,
        Self::Credentials,
        Self::InstalledArtifacts,
        Self::InstalledExtensionDogfoodEvidence,
        Self::RemoteMirrorAutomationState,
        Self::ArchivedEvidence,
        Self::ExternalParticipantEvidence,
        Self::ProductionDefault,
        Self::DeferredContentFormatExperiment,
    ];
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct RetainedRootContractV1 {
    pub(crate) tiers: Vec<String>,
    pub(crate) identity_verified_read_only: bool,
    pub(crate) clone_authoritative_truth_only: bool,
    pub(crate) rematerialization_authorized: bool,
    pub(crate) preserve_sources_and_outputs: bool,
    pub(crate) c524_admitted: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ReadinessOutcome {
    ReadyForDefaultDecision,
    RemainDefaultOff,
    Reject,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ReadinessGateId {
    ExactIdentity,
    ParentContractSatisfied,
    NoFalseCurrentState,
    ActiveOrdinaryWorkBounded,
    FreshWriterAdmissionBounded,
    NativeAuthorityApfs,
    NativeAuthorityNtfs,
    BootstrapSinglePopulation,
    BootstrapAvailability,
    RevisionPaginationCorrectness,
    ActiveRevisionPageBounded,
    NativeLifecycleAndPackaging,
    RollbackAndBackup,
    RetainedL100,
    RetainedC262,
    EvidenceAuthority,
    ProtectedInputsUntouched,
    DefaultRemainsOff,
}

impl ReadinessGateId {
    pub(crate) const ALL: [Self; 18] = [
        Self::ExactIdentity,
        Self::ParentContractSatisfied,
        Self::NoFalseCurrentState,
        Self::ActiveOrdinaryWorkBounded,
        Self::FreshWriterAdmissionBounded,
        Self::NativeAuthorityApfs,
        Self::NativeAuthorityNtfs,
        Self::BootstrapSinglePopulation,
        Self::BootstrapAvailability,
        Self::RevisionPaginationCorrectness,
        Self::ActiveRevisionPageBounded,
        Self::NativeLifecycleAndPackaging,
        Self::RollbackAndBackup,
        Self::RetainedL100,
        Self::RetainedC262,
        Self::EvidenceAuthority,
        Self::ProtectedInputsUntouched,
        Self::DefaultRemainsOff,
    ];
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ReadinessGateContractV1 {
    pub(crate) gate: ReadinessGateId,
    pub(crate) requirement: String,
    pub(crate) failed_outcome: ReadinessOutcome,
    pub(crate) unknown_outcome: ReadinessOutcome,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ReadinessGateStatus {
    Passed,
    Failed,
    Unknown,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ReadinessGateResultV1 {
    pub(crate) gate: ReadinessGateId,
    pub(crate) status: ReadinessGateStatus,
}

impl ReadinessGateResultV1 {
    pub(crate) const fn passed(gate: ReadinessGateId) -> Self {
        Self {
            gate,
            status: ReadinessGateStatus::Passed,
        }
    }

    pub(crate) const fn failed(gate: ReadinessGateId) -> Self {
        Self {
            gate,
            status: ReadinessGateStatus::Failed,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ProductionReadinessContractV1 {
    pub(crate) schema: String,
    pub(crate) contract_id: String,
    pub(crate) profile: DerivedAccessProfile,
    pub(crate) default_profile: DerivedAccessProfile,
    pub(crate) parent_contract_schema: String,
    pub(crate) parent_contract_sha256: String,
    pub(crate) profile_identity_fields: Vec<ReadinessIdentityField>,
    pub(crate) source_identity_fields: Vec<ReadinessIdentityField>,
    pub(crate) evidence_identity_fields: Vec<ReadinessIdentityField>,
    pub(crate) counter_ceilings: Vec<ReadinessCounterCeilingV1>,
    pub(crate) authority_stamp: AuthorityStampContractV1,
    pub(crate) bootstrap: BootstrapReadinessContractV1,
    pub(crate) revisions_page: RevisionPageContractV1,
    pub(crate) work_classifications: Vec<ReadinessWorkClassificationV1>,
    pub(crate) evidence_gates: Vec<ReadinessEvidenceGateV1>,
    pub(crate) retained_roots: RetainedRootContractV1,
    pub(crate) protected_inputs: Vec<ProtectedInputId>,
    pub(crate) gates: Vec<ReadinessGateContractV1>,
    pub(crate) terminal_outcomes: Vec<ReadinessOutcome>,
    pub(crate) numeric_wall_time_thresholds_present: bool,
    pub(crate) physical_implementation_in_contract: bool,
    pub(crate) evidence_collection_in_contract: bool,
    pub(crate) production_default_change_authorized: bool,
    pub(crate) migration_authorized: bool,
    pub(crate) release_authorized: bool,
}

impl ProductionReadinessContractV1 {
    fn frozen() -> Self {
        Self {
            schema: PRODUCTION_READINESS_CONTRACT_SCHEMA_V1.to_owned(),
            contract_id: "derived-access-production-readiness-v1".to_owned(),
            profile: DerivedAccessProfile::SqliteWalBodylessV1,
            default_profile: DerivedAccessProfile::Off,
            parent_contract_schema: PRODUCT_INTEGRATION_CONTRACT_SCHEMA_V1.to_owned(),
            parent_contract_sha256: PRODUCT_INTEGRATION_CONTRACT_SHA256_V1.to_owned(),
            profile_identity_fields: ReadinessIdentityField::PROFILE.to_vec(),
            source_identity_fields: ReadinessIdentityField::SOURCE.to_vec(),
            evidence_identity_fields: ReadinessIdentityField::EVIDENCE.to_vec(),
            counter_ceilings: readiness_counter_ceilings(),
            authority_stamp: authority_stamp_contract(),
            bootstrap: bootstrap_readiness_contract(),
            revisions_page: revision_page_contract(),
            work_classifications: readiness_work_classifications(),
            evidence_gates: ReadinessEvidenceId::ALL
                .into_iter()
                .map(readiness_evidence_gate)
                .collect(),
            retained_roots: RetainedRootContractV1 {
                tiers: vec!["L100".to_owned(), "C262".to_owned()],
                identity_verified_read_only: true,
                clone_authoritative_truth_only: true,
                rematerialization_authorized: false,
                preserve_sources_and_outputs: true,
                c524_admitted: false,
            },
            protected_inputs: ProtectedInputId::ALL.to_vec(),
            gates: ReadinessGateId::ALL
                .into_iter()
                .map(readiness_gate_contract)
                .collect(),
            terminal_outcomes: vec![
                ReadinessOutcome::ReadyForDefaultDecision,
                ReadinessOutcome::RemainDefaultOff,
                ReadinessOutcome::Reject,
            ],
            numeric_wall_time_thresholds_present: false,
            physical_implementation_in_contract: false,
            evidence_collection_in_contract: false,
            production_default_change_authorized: false,
            migration_authorized: false,
            release_authorized: false,
        }
    }

    pub(crate) fn canonical_sha256(&self) -> Result<String, String> {
        canonical_sha256(self)
    }

    pub(crate) fn validate(&self) -> Result<(), String> {
        if self != &Self::frozen() {
            return Err("unsupported derived-access production-readiness contract".to_owned());
        }
        let actual = self.canonical_sha256()?;
        if actual != PRODUCTION_READINESS_CONTRACT_SHA256_V1 {
            return Err(format!(
                "derived-access production-readiness contract hash is not frozen: expected {}, actual {actual}",
                PRODUCTION_READINESS_CONTRACT_SHA256_V1
            ));
        }
        Ok(())
    }

    pub(crate) fn counter_ceiling(
        &self,
        operation: ReadinessOperationId,
        counter: ReadinessCounterId,
    ) -> Option<&ReadinessCounterCeilingV1> {
        self.counter_ceilings
            .iter()
            .find(|row| row.operation == operation && row.counter == counter)
    }

    pub(crate) fn evaluate(
        &self,
        results: &[ReadinessGateResultV1],
    ) -> Result<ReadinessOutcome, String> {
        if results.len() != self.gates.len() {
            return Err("production-readiness result matrix is incomplete".to_owned());
        }
        let mut seen = Vec::with_capacity(results.len());
        let mut outcome = ReadinessOutcome::ReadyForDefaultDecision;
        for result in results {
            if seen.contains(&result.gate) {
                return Err(
                    "production-readiness result matrix contains a duplicate gate".to_owned(),
                );
            }
            seen.push(result.gate);
            let gate = self
                .gates
                .iter()
                .find(|gate| gate.gate == result.gate)
                .ok_or_else(|| {
                    "production-readiness result names an unsupported gate".to_owned()
                })?;
            let candidate = match result.status {
                ReadinessGateStatus::Passed => ReadinessOutcome::ReadyForDefaultDecision,
                ReadinessGateStatus::Failed => gate.failed_outcome,
                ReadinessGateStatus::Unknown => gate.unknown_outcome,
            };
            outcome = combine_readiness_outcomes(outcome, candidate);
        }
        if ReadinessGateId::ALL
            .into_iter()
            .any(|gate| !seen.contains(&gate))
        {
            return Err("production-readiness result matrix omits a gate".to_owned());
        }
        Ok(outcome)
    }

    pub(crate) fn decision_table_markdown(&self) -> String {
        let zero_context = CounterCeilingContextV1 {
            requested_entries: 0,
            source_carriers: 0,
        };
        let ordinary_walk_ceiling = self
            .counter_ceiling(
                ReadinessOperationId::ActiveOrdinaryNoChange,
                ReadinessCounterId::EventDirectoryEntriesWalked,
            )
            .expect("frozen ordinary-work ceiling")
            .ceiling(zero_context);
        let writer_walk_ceiling = self
            .counter_ceiling(
                ReadinessOperationId::FreshWriterAdmissionNoChange,
                ReadinessCounterId::EventDirectoryEntriesWalked,
            )
            .expect("frozen writer-admission ceiling")
            .ceiling(zero_context);
        let authority_walk_ceiling = self
            .authority_stamp
            .scenarios
            .iter()
            .map(|row| row.event_directory_entries_walked_ceiling)
            .max()
            .unwrap_or_default();
        let authority_open_ceiling = self
            .authority_stamp
            .scenarios
            .iter()
            .map(|row| row.event_carrier_opens_ceiling)
            .max()
            .unwrap_or_default();
        let phases = serialized_names(&self.bootstrap.phases).join(" → ");
        let outcomes = serialized_names(&self.terminal_outcomes)
            .into_iter()
            .map(|outcome| format!("`{outcome}`"))
            .collect::<Vec<_>>()
            .join(", ");
        let protected_inputs = serialized_names(&self.protected_inputs)
            .into_iter()
            .map(|input| format!("`{input}`"))
            .collect::<Vec<_>>()
            .join(", ");
        let unknown_outcome = self
            .gates
            .first()
            .map(|gate| serialized_name(&gate.unknown_outcome))
            .unwrap_or_else(|| "missing".to_owned());
        let all_unknown_outcomes_match = self
            .gates
            .iter()
            .all(|gate| serialized_name(&gate.unknown_outcome) == unknown_outcome);
        let wall_time_rule = if self.numeric_wall_time_thresholds_present {
            "adds host-sensitive numeric wall-time thresholds"
        } else {
            "adds no host-sensitive numeric wall-time thresholds"
        };
        [
            "| Decision | Frozen production-readiness requirement |".to_owned(),
            "| --- | --- |".to_owned(),
            format!(
                "| Identity | exact profile, parent/readiness contract, source commit/tree, Cargo.lock, binary, OS/architecture/filesystem, root manifest, and evidence-package identities; parent `{}` |",
                self.parent_contract_sha256
            ),
            format!(
                "| Authority | `{}` selects and freezes the observable before integration; each scenario names its mutation locus; observation opens at most `{authority_open_ceiling}` carriers and walks at most `{authority_walk_ceiling}` event entries; changed/indeterminate never proves truth: `{}`; every mismatch fails closed; existing-carrier overwrite remains a validation/audit non-claim |",
                serialized_name(&self.authority_stamp.observable_policy),
                self.authority_stamp
                    .changed_or_indeterminate_never_proves_truth
            ),
            format!(
                "| Ordinary work | active no-change fixed-output routes walk at most `{ordinary_walk_ceiling}` event-directory entries and fresh writer admission walks at most `{writer_walk_ceiling}`; audits, bootstrap, fallback, default-off comparison, and body search remain explicitly classified |"
            ),
            format!(
                "| Bootstrap | {phases}; population carrier-open ceiling `{}` per input and maximum `{}` overlapping complete decoded histories; strict oracle serial/isolated: `{}`; completion published last: `{}` |",
                self.bootstrap.population_carrier_open_ceiling,
                self.bootstrap.maximum_overlapping_full_decoded_histories,
                self.bootstrap.strict_oracle_is_serial_or_isolated,
                self.bootstrap.completion_is_published_last
            ),
            format!(
                "| Bootstrap availability | status/progress fields `{}` required and `{}` optional; maximum automatic fallbacks `{}`; long unexplained absent allowed: `{}`; no availability case may serve stale as current: `{}` |",
                serialized_names(&self.bootstrap.required_progress_fields).join(", "),
                serialized_names(&self.bootstrap.optional_progress_fields).join(", "),
                self.bootstrap.maximum_automatic_fallbacks,
                self.bootstrap.long_unexplained_absent_response_allowed,
                self.bootstrap
                    .availability
                    .iter()
                    .all(|row| !row.may_serve_stale_as_current)
            ),
            format!(
                "| Revision pages | `{}`; absent limit uses default `{}`: `{}`; maximum `{}` with over-maximum `{}`; requested entries mean the accepted limit: `{}`; normalized `(capturedAt, revisionId)` descending so page one contains current work; opaque snapshot-bound token; revision count comes from `{}`; active work `{}`; default-off same shape through `{}` |",
                self.revisions_page.schema,
                self.revisions_page.default_limit,
                self.revisions_page.absent_limit_uses_default,
                self.revisions_page.maximum_limit,
                serialized_name(&self.revisions_page.over_maximum_limit),
                self.revisions_page.requested_entries_are_accepted_limit,
                serialized_name(&self.revisions_page.revision_count_source),
                serialized_name(&self.revisions_page.active_work),
                serialized_name(&self.revisions_page.default_off_work)
            ),
            format!(
                "| Qualification | required completion-last, read-only verified gates: `{}`; parent contract `{}` and all of its gates and matched-operation ceilings remain load-bearing |",
                serialized_names(
                    &self
                        .evidence_gates
                        .iter()
                        .map(|gate| gate.evidence)
                        .collect::<Vec<_>>()
                )
                .join(", "),
                self.parent_contract_sha256
            ),
            format!(
                "| Retained inputs | tiers `{}`; identity-verified read-only: `{}`; rematerialization authorized: `{}`; preserve sources/outputs: `{}`; C524 admitted: `{}` |",
                self.retained_roots.tiers.join(", "),
                self.retained_roots.identity_verified_read_only,
                self.retained_roots.rematerialization_authorized,
                self.retained_roots.preserve_sources_and_outputs,
                self.retained_roots.c524_admitted
            ),
            format!(
                "| Timing | this readiness contract {wall_time_rule}; the parent contract's matched-operation wall/CPU ceilings still apply; counter, semantic, lifecycle, native, and evidence-authority gates decide readiness |"
            ),
            format!(
                "| Outcomes | {outcomes}; all unknown outcomes match: `{all_unknown_outcomes_match}` (`{unknown_outcome}`); production default change authorized: `{}` |",
                self.production_default_change_authorized
            ),
            format!(
                "| Protected boundary | {protected_inputs}; physical implementation: `{}`; evidence collection: `{}`; migration: `{}`; release: `{}` |",
                self.physical_implementation_in_contract,
                self.evidence_collection_in_contract,
                self.migration_authorized,
                self.release_authorized
            ),
        ]
        .join("\n")
    }
}

fn serialized_name<T: Serialize>(value: &T) -> String {
    serde_json::to_value(value)
        .expect("frozen contract enum serializes")
        .as_str()
        .expect("frozen contract enum serializes as a string")
        .to_owned()
}

fn serialized_names<T: Serialize>(values: &[T]) -> Vec<String> {
    values.iter().map(serialized_name).collect()
}

pub(crate) fn production_readiness_contract_v1() -> ProductionReadinessContractV1 {
    ProductionReadinessContractV1::frozen()
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ProductionReadinessContractPublicationV1 {
    pub(crate) schema: String,
    pub(crate) mode: String,
    pub(crate) contract: ProductionReadinessContractV1,
    pub(crate) contract_sha256: String,
    pub(crate) fixture_sha256: String,
    pub(crate) decision_table_markdown: String,
}

pub(crate) fn production_readiness_contract_publication_v1()
-> ProductionReadinessContractPublicationV1 {
    let contract = production_readiness_contract_v1();
    ProductionReadinessContractPublicationV1 {
        schema: PRODUCTION_READINESS_CONTRACT_PUBLICATION_SCHEMA_V1.to_owned(),
        mode: "non_timing_contract_publication".to_owned(),
        contract_sha256: contract
            .canonical_sha256()
            .expect("the frozen production-readiness contract is canonical"),
        fixture_sha256: PRODUCTION_READINESS_FIXTURE_SHA256_V1.to_owned(),
        decision_table_markdown: contract.decision_table_markdown(),
        contract,
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ProductionReadinessContractFixtureV1 {
    pub(crate) schema: String,
    pub(crate) contract_schema: String,
    pub(crate) contract_sha256: String,
    pub(crate) contract: ProductionReadinessContractV1,
}

pub(crate) fn production_readiness_contract_fixture_v1() -> ProductionReadinessContractFixtureV1 {
    ProductionReadinessContractFixtureV1 {
        schema: PRODUCTION_READINESS_CONTRACT_FIXTURE_SCHEMA_V1.to_owned(),
        contract_schema: PRODUCTION_READINESS_CONTRACT_SCHEMA_V1.to_owned(),
        contract_sha256: PRODUCTION_READINESS_CONTRACT_SHA256_V1.to_owned(),
        contract: production_readiness_contract_v1(),
    }
}

pub(crate) fn verify_production_readiness_contract_fixture_v1() -> Result<(), String> {
    let fixture_json =
        include_str!("../../../tests/fixtures/derived-access/production-readiness-v1.json");
    let fixture: ProductionReadinessContractFixtureV1 = serde_json::from_str(fixture_json)
        .map_err(|error| format!("production-readiness fixture is invalid: {error}"))?;
    let expected = production_readiness_contract_fixture_v1();
    if fixture != expected {
        return Err("production-readiness fixture differs from the frozen contract".to_owned());
    }
    let actual = sha256_bytes_hex(fixture_json.as_bytes());
    if actual != PRODUCTION_READINESS_FIXTURE_SHA256_V1 {
        return Err(format!(
            "production-readiness fixture hash is not frozen: expected {}, actual {actual}",
            PRODUCTION_READINESS_FIXTURE_SHA256_V1
        ));
    }
    production_readiness_contract_v1().validate()
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ProductionReadinessContractSmokeV1 {
    pub(crate) schema: String,
    pub(crate) contract_sha256: String,
    pub(crate) fixture_sha256: String,
    pub(crate) scenario_count: usize,
    pub(crate) gate_count: usize,
    pub(crate) filesystem_actions: u64,
    pub(crate) store_roots_opened: u64,
    pub(crate) expensive_scale_work_run: bool,
    pub(crate) physical_implementation_opened: bool,
    pub(crate) evidence_collected: bool,
}

pub(crate) fn production_readiness_contract_smoke_v1()
-> Result<ProductionReadinessContractSmokeV1, String> {
    verify_production_readiness_contract_fixture_v1()?;
    let contract = production_readiness_contract_v1();
    let passing = ReadinessGateId::ALL
        .into_iter()
        .map(ReadinessGateResultV1::passed)
        .collect::<Vec<_>>();
    if contract.evaluate(&passing)? != ReadinessOutcome::ReadyForDefaultDecision {
        return Err("production-readiness evaluator drifted".to_owned());
    }
    Ok(ProductionReadinessContractSmokeV1 {
        schema: PRODUCTION_READINESS_CONTRACT_SMOKE_SCHEMA_V1.to_owned(),
        contract_sha256: PRODUCTION_READINESS_CONTRACT_SHA256_V1.to_owned(),
        fixture_sha256: PRODUCTION_READINESS_FIXTURE_SHA256_V1.to_owned(),
        scenario_count: contract.authority_stamp.scenarios.len(),
        gate_count: contract.gates.len(),
        filesystem_actions: 0,
        store_roots_opened: 0,
        expensive_scale_work_run: false,
        physical_implementation_opened: false,
        evidence_collected: false,
    })
}

fn readiness_counter_ceilings() -> Vec<ReadinessCounterCeilingV1> {
    [
        (
            ReadinessOperationId::ActiveOrdinaryNoChange,
            ReadinessCounterId::EventDirectoryEntriesWalked,
            CounterCeilingBasis::Constant,
            0,
            0,
        ),
        (
            ReadinessOperationId::ActiveOrdinaryNoChange,
            ReadinessCounterId::FullHistoryFallbacks,
            CounterCeilingBasis::Constant,
            0,
            0,
        ),
        (
            ReadinessOperationId::FreshWriterAdmissionNoChange,
            ReadinessCounterId::EventDirectoryEntriesWalked,
            CounterCeilingBasis::Constant,
            0,
            0,
        ),
        (
            ReadinessOperationId::ActiveRevisionsPage,
            ReadinessCounterId::EventDirectoryEntriesWalked,
            CounterCeilingBasis::Constant,
            0,
            0,
        ),
        (
            ReadinessOperationId::ActiveRevisionsPage,
            ReadinessCounterId::RevisionRowsExamined,
            CounterCeilingBasis::RequestedEntries,
            1,
            1,
        ),
        (
            ReadinessOperationId::ActiveRevisionsPage,
            ReadinessCounterId::PageEntriesReturned,
            CounterCeilingBasis::RequestedEntries,
            1,
            0,
        ),
        (
            ReadinessOperationId::BootstrapPopulation,
            ReadinessCounterId::EventCarrierOpens,
            CounterCeilingBasis::SourceCarriers,
            1,
            0,
        ),
        (
            ReadinessOperationId::BootstrapPopulation,
            ReadinessCounterId::FullDecodedHistoriesRetained,
            CounterCeilingBasis::Constant,
            0,
            1,
        ),
        (
            ReadinessOperationId::FirstBootstrapAvailability,
            ReadinessCounterId::AutomaticAuthoritativeFallbacks,
            CounterCeilingBasis::Constant,
            0,
            1,
        ),
    ]
    .into_iter()
    .map(
        |(operation, counter, basis, multiplier, additive)| ReadinessCounterCeilingV1 {
            operation,
            counter,
            basis,
            multiplier,
            additive,
        },
    )
    .collect()
}

fn authority_stamp_contract() -> AuthorityStampContractV1 {
    let scenarios = AuthorityStampScenarioId::ALL
        .into_iter()
        .map(|scenario| {
            let authoritative_carrier_created = matches!(
                scenario,
                AuthorityStampScenarioId::GovernedCreate
                    | AuthorityStampScenarioId::GovernedBurst
                    | AuthorityStampScenarioId::OutOfBandCreate
                    | AuthorityStampScenarioId::TempCreateThenRename
                    | AuthorityStampScenarioId::ConcurrentCreateObservation
                    | AuthorityStampScenarioId::CrashAfterCarrierPublication
                    | AuthorityStampScenarioId::RapidMutations
                    | AuthorityStampScenarioId::CanonicalPathAlias
                    | AuthorityStampScenarioId::ProductionDirectoryLayout
            );
            let (mutation_locus, expectation) = match scenario {
                AuthorityStampScenarioId::GovernedCreate
                | AuthorityStampScenarioId::GovernedBurst
                | AuthorityStampScenarioId::OutOfBandCreate
                | AuthorityStampScenarioId::TempCreateThenRename
                | AuthorityStampScenarioId::ConcurrentCreateObservation
                | AuthorityStampScenarioId::CrashAfterCarrierPublication
                | AuthorityStampScenarioId::RapidMutations
                | AuthorityStampScenarioId::CanonicalPathAlias
                | AuthorityStampScenarioId::ProductionDirectoryLayout => (
                    AuthorityStampMutationLocus::EventDirectoryAuthoritativeCarrier,
                    AuthorityStampExpectation::ChangedOrIndeterminate,
                ),
                AuthorityStampScenarioId::SidecarDeletion => (
                    AuthorityStampMutationLocus::AuthorityMetadata,
                    AuthorityStampExpectation::ChangedOrIndeterminate,
                ),
                AuthorityStampScenarioId::UnrelatedFile
                | AuthorityStampScenarioId::TemporaryFile => (
                    AuthorityStampMutationLocus::EventDirectoryNonCarrierEntry,
                    AuthorityStampExpectation::StableOrChangedWithoutTruthClaim,
                ),
                AuthorityStampScenarioId::ExistingCarrierOverwrite => (
                    AuthorityStampMutationLocus::ExistingAuthoritativeCarrier,
                    AuthorityStampExpectation::ExplicitNonClaim,
                ),
                AuthorityStampScenarioId::CloseReopen
                | AuthorityStampScenarioId::MachineOrVmRestart => (
                    AuthorityStampMutationLocus::ProcessLifecycle,
                    AuthorityStampExpectation::Stable,
                ),
                AuthorityStampScenarioId::ExperimentOffRollback => (
                    AuthorityStampMutationLocus::AuthorityMetadata,
                    AuthorityStampExpectation::ObservationNotApplicable,
                ),
                AuthorityStampScenarioId::AbsentDirectory
                | AuthorityStampScenarioId::EmptyDirectory
                | AuthorityStampScenarioId::EqualDuplicateNoCreate
                | AuthorityStampScenarioId::ConflictingDuplicateNoCreate
                | AuthorityStampScenarioId::CrashBeforeCarrierPublication => (
                    AuthorityStampMutationLocus::None,
                    AuthorityStampExpectation::Stable,
                ),
            };
            AuthorityStampScenarioContractV1 {
                scenario,
                mutation_locus,
                expectation,
                authoritative_carrier_created,
                event_directory_entries_walked_ceiling: 0,
                event_carrier_opens_ceiling: 0,
            }
        })
        .collect();
    let failures = AuthorityFailureId::ALL
        .into_iter()
        .map(|failure| AuthorityFailureContractV1 {
            failure,
            outcome: if failure == AuthorityFailureId::StampPublicationFailedAfterTruthCreate {
                DerivedAccessAvailability::Unavailable
            } else {
                DerivedAccessAvailability::RebuildRequired
            },
            truth_success_is_preserved: failure
                == AuthorityFailureId::StampPublicationFailedAfterTruthCreate,
            may_report_current: false,
        })
        .collect();
    AuthorityStampContractV1 {
        scope:
            "supported local-filesystem accidental and mixed-version event publication detection"
                .to_owned(),
        observable_policy:
            AuthorityStampObservablePolicy::SelectedAndFrozenByNativeFalsifierBeforeIntegration,
        scenarios,
        failures,
        changed_or_indeterminate_never_proves_truth: true,
        existing_carrier_overwrite_requires_selected_validation_or_audit: true,
        malicious_tamper_detection_claimed: false,
    }
}

fn bootstrap_readiness_contract() -> BootstrapReadinessContractV1 {
    BootstrapReadinessContractV1 {
        phases: BootstrapPhase::ALL.to_vec(),
        required_progress_fields: vec![
            BootstrapProgressField::Phase,
            BootstrapProgressField::CompletedEvents,
            BootstrapProgressField::TotalEvents,
            BootstrapProgressField::ElapsedMilliseconds,
        ],
        optional_progress_fields: vec![
            BootstrapProgressField::CompletedBytes,
            BootstrapProgressField::TotalBytes,
            BootstrapProgressField::EtaMilliseconds,
        ],
        population_carrier_open_ceiling: 1,
        maximum_overlapping_full_decoded_histories: 1,
        strict_oracle_is_serial_or_isolated: true,
        pre_post_stability_decodes_full_history: false,
        transaction_batches_are_bounded: true,
        cancellation_and_restart_are_identity_bound: true,
        completion_is_published_last: true,
        eta_requires_observed_stable_rate: true,
        maximum_automatic_fallbacks: 1,
        long_unexplained_absent_response_allowed: false,
        availability: BootstrapAvailabilityCase::ALL
            .into_iter()
            .map(|case| BootstrapAvailabilityContractV1 {
                case,
                shell_status_and_progress_available: true,
                offers_explicit_fallback_or_wait: !matches!(
                    case,
                    BootstrapAvailabilityCase::ReplacementWithValidCurrent
                ),
                may_serve_valid_old_current: matches!(
                    case,
                    BootstrapAvailabilityCase::ReplacementWithValidCurrent
                ),
                may_serve_stale_as_current: false,
            })
            .collect(),
    }
}

fn revision_page_contract() -> RevisionPageContractV1 {
    RevisionPageContractV1 {
        schema: "pointbreak.inspect-revisions-page.v1".to_owned(),
        request_fields: vec![
            RevisionPageRequestField::Limit,
            RevisionPageRequestField::After,
        ],
        response_fields: RevisionPageResponseField::ALL.to_vec(),
        default_limit: 100,
        maximum_limit: 500,
        absent_limit_uses_default: true,
        over_maximum_limit: RevisionPageLimitOverflow::InvalidRequest,
        requested_entries_are_accepted_limit: true,
        order: vec![
            RevisionPageOrderField::CapturedAtDescending,
            RevisionPageOrderField::RevisionIdDescending,
        ],
        token_is_opaque: true,
        token_binds_profile_schema_snapshot_and_order: true,
        invalid_token: RevisionPageTokenFailure::InvalidRequest,
        stale_or_wrong_profile_token: RevisionPageTokenFailure::RestartRequired,
        same_wire_shape_for_active_and_default_off: true,
        active_work: ProductWorkClass::OutputProportional,
        default_off_work: ProductWorkClass::HistoryProportional,
        default_off_exhaustive_comparator_is_explicit: true,
        active_complete_collection_materialization_allowed: false,
        default_off_complete_collection_materialization_allowed: true,
        revision_count_is_present: true,
        revision_count_source: RevisionCountSource::DerivedIndexedAggregate,
        exact_detail_is_entity_primary: true,
    }
}

fn readiness_work_classifications() -> Vec<ReadinessWorkClassificationV1> {
    [
        (
            ReadinessWorkId::InitialAuthorityCensus,
            ReadinessWorkClass::ExhaustiveAudit,
        ),
        (
            ReadinessWorkId::AuthorityStampCheck,
            ReadinessWorkClass::BoundedMetadata,
        ),
        (
            ReadinessWorkId::OrdinaryActiveRead,
            ReadinessWorkClass::BoundedMetadata,
        ),
        (
            ReadinessWorkId::FreshWriterAdmission,
            ReadinessWorkClass::BoundedMetadata,
        ),
        (
            ReadinessWorkId::BootstrapPopulation,
            ReadinessWorkClass::ExhaustiveBootstrap,
        ),
        (
            ReadinessWorkId::StrictReplay,
            ReadinessWorkClass::ExhaustiveAudit,
        ),
        (
            ReadinessWorkId::ExplicitIntegrityAudit,
            ReadinessWorkClass::ExhaustiveAudit,
        ),
        (
            ReadinessWorkId::FirstBootstrapAuthoritativeFallback,
            ReadinessWorkClass::ExplicitAuthoritativeFallback,
        ),
        (
            ReadinessWorkId::DefaultOffRevisionsPage,
            ReadinessWorkClass::ExhaustiveDefaultOffComparator,
        ),
        (
            ReadinessWorkId::IntentionalBodySearch,
            ReadinessWorkClass::IntentionalExhaustiveSearch,
        ),
        (
            ReadinessWorkId::ActiveRevisionsPage,
            ReadinessWorkClass::RequestedOutputProportional,
        ),
        (
            ReadinessWorkId::BackupRestoreVerification,
            ReadinessWorkClass::ExhaustiveAudit,
        ),
    ]
    .into_iter()
    .map(
        |(operation, classification)| ReadinessWorkClassificationV1 {
            operation,
            classification,
        },
    )
    .collect()
}

fn readiness_evidence_gate(evidence: ReadinessEvidenceId) -> ReadinessEvidenceGateV1 {
    let (native_platform, tiers) = match evidence {
        ReadinessEvidenceId::NativeMacosApfsD0L7 => (
            Some("macos/apfs".to_owned()),
            vec!["D0".to_owned(), "L7".to_owned()],
        ),
        ReadinessEvidenceId::NativeWindowsNtfsD0L7 => (
            Some("windows/ntfs".to_owned()),
            vec!["D0".to_owned(), "L7".to_owned()],
        ),
        ReadinessEvidenceId::RetainedL100Apfs => {
            (Some("macos/apfs".to_owned()), vec!["L100".to_owned()])
        }
        ReadinessEvidenceId::RetainedC262Apfs => {
            (Some("macos/apfs".to_owned()), vec!["C262".to_owned()])
        }
        ReadinessEvidenceId::PackageClosure
        | ReadinessEvidenceId::RollbackAndBackup
        | ReadinessEvidenceId::EvidenceAuthority => (None, Vec::new()),
    };
    ReadinessEvidenceGateV1 {
        evidence,
        required: true,
        native_platform,
        tiers,
        completion_last: true,
        read_only_verification: true,
    }
}

fn readiness_gate_contract(gate: ReadinessGateId) -> ReadinessGateContractV1 {
    let (requirement, failed_outcome) = match gate {
        ReadinessGateId::ExactIdentity => (
            "profile, source, root, platform, binary, and package identities agree exactly",
            ReadinessOutcome::RemainDefaultOff,
        ),
        ReadinessGateId::ParentContractSatisfied => (
            "every parent product-integration gate and matched-operation ceiling passes",
            ReadinessOutcome::RemainDefaultOff,
        ),
        ReadinessGateId::NoFalseCurrentState => (
            "no changed, indeterminate, mismatched, or unsupported authority may report current",
            ReadinessOutcome::Reject,
        ),
        ReadinessGateId::ActiveOrdinaryWorkBounded => (
            "active no-change fixed-output routes walk zero event entries",
            ReadinessOutcome::RemainDefaultOff,
        ),
        ReadinessGateId::FreshWriterAdmissionBounded => (
            "fresh writer admission at a no-change head walks zero event entries",
            ReadinessOutcome::RemainDefaultOff,
        ),
        ReadinessGateId::NativeAuthorityApfs => (
            "every supported authority scenario passes natively on macOS/APFS",
            ReadinessOutcome::Reject,
        ),
        ReadinessGateId::NativeAuthorityNtfs => (
            "every supported authority scenario passes natively on Windows/NTFS",
            ReadinessOutcome::Reject,
        ),
        ReadinessGateId::BootstrapSinglePopulation => (
            "bootstrap opens each carrier at most once for population and overlaps no complete histories",
            ReadinessOutcome::RemainDefaultOff,
        ),
        ReadinessGateId::BootstrapAvailability => (
            "bootstrap exposes progress and explicit bounded fallback-or-wait without stale current state",
            ReadinessOutcome::RemainDefaultOff,
        ),
        ReadinessGateId::RevisionPaginationCorrectness => (
            "snapshot-bound pages have no gaps or duplicates and exact detail remains entity-primary",
            ReadinessOutcome::Reject,
        ),
        ReadinessGateId::ActiveRevisionPageBounded => (
            "active page work is requested-page/output-proportional and never materializes the complete collection",
            ReadinessOutcome::RemainDefaultOff,
        ),
        ReadinessGateId::NativeLifecycleAndPackaging => (
            "native lifecycle, shutdown, package closure, and read-only verification pass",
            ReadinessOutcome::Reject,
        ),
        ReadinessGateId::RollbackAndBackup => (
            "rollback and backup reconstruction preserve authoritative truth and recoverability",
            ReadinessOutcome::Reject,
        ),
        ReadinessGateId::RetainedL100 => (
            "retained L100 product evidence passes without rematerialization",
            ReadinessOutcome::RemainDefaultOff,
        ),
        ReadinessGateId::RetainedC262 => (
            "retained C262 product evidence passes without rematerialization",
            ReadinessOutcome::RemainDefaultOff,
        ),
        ReadinessGateId::EvidenceAuthority => (
            "raw evidence, manifests, completion markers, and recursive verification bind one exact source",
            ReadinessOutcome::RemainDefaultOff,
        ),
        ReadinessGateId::ProtectedInputsUntouched => (
            "protected owner, private, installed, release, and held-work boundaries remain untouched",
            ReadinessOutcome::Reject,
        ),
        ReadinessGateId::DefaultRemainsOff => (
            "qualification and recommendation do not change the production default",
            ReadinessOutcome::Reject,
        ),
    };
    ReadinessGateContractV1 {
        gate,
        requirement: requirement.to_owned(),
        failed_outcome,
        unknown_outcome: ReadinessOutcome::RemainDefaultOff,
    }
}

fn combine_readiness_outcomes(left: ReadinessOutcome, right: ReadinessOutcome) -> ReadinessOutcome {
    match (left, right) {
        (ReadinessOutcome::Reject, _) | (_, ReadinessOutcome::Reject) => ReadinessOutcome::Reject,
        (ReadinessOutcome::RemainDefaultOff, _) | (_, ReadinessOutcome::RemainDefaultOff) => {
            ReadinessOutcome::RemainDefaultOff
        }
        _ => ReadinessOutcome::ReadyForDefaultDecision,
    }
}

fn canonical_sha256<T: Serialize>(value: &T) -> Result<String, String> {
    let value = serde_json::to_value(value)
        .map_err(|error| format!("product-integration JSON failed: {error}"))?;
    let bytes = canonical_json_bytes(&value)
        .map_err(|error| format!("product-integration canonical JSON failed: {error}"))?;
    Ok(sha256_bytes_hex(&bytes))
}
