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

fn canonical_sha256<T: Serialize>(value: &T) -> Result<String, String> {
    let value = serde_json::to_value(value)
        .map_err(|error| format!("product-integration JSON failed: {error}"))?;
    let bytes = canonical_json_bytes(&value)
        .map_err(|error| format!("product-integration canonical JSON failed: {error}"))?;
    Ok(sha256_bytes_hex(&bytes))
}
