//! One executable admission table for the legacy (pre-Change) read surfaces.
//!
//! The ordinary public CLI commands and the Inspector's legacy aggregate HTTP
//! routes admit or refuse a store by its capability status. The two surfaces
//! intentionally differ on an untouched legacy root: the Inspector keeps
//! migration-era readers (the signed v0.9 binary) working there, while the
//! ordinary CLI fence refuses pre-migration stores. On a ready Change-aware
//! root the roles invert: the CLI serves its legacy documents, the Inspector
//! answers typed reader-upgrade guidance. This table is the single statement
//! of that policy; each surface only renders the verdict it receives.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[allow(
    dead_code,
    reason = "consumed by the CLI preflight and the Inspector legacy gate in the following changes"
)]
pub(crate) enum LegacyAdmissionSurfaceV1 {
    PublicCliCommand,
    InspectorLegacyRoute,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[allow(
    dead_code,
    reason = "consumed by the CLI preflight and the Inspector legacy gate in the following changes"
)]
pub(crate) enum LegacyAdmissionVerdictV1 {
    Serve,
    RefuseMigrationRequired,
    RefuseMigrationInProgress,
    RefuseReaderUpgrade,
}

#[allow(
    dead_code,
    reason = "consumed by the CLI preflight and the Inspector legacy gate in the following changes"
)]
pub(crate) fn legacy_admission_v1(
    surface: LegacyAdmissionSurfaceV1,
    capability: Option<&pointbreak::session::StoreCapabilityInspection>,
) -> LegacyAdmissionVerdictV1 {
    use LegacyAdmissionSurfaceV1::{InspectorLegacyRoute, PublicCliCommand};
    use LegacyAdmissionVerdictV1::{
        RefuseMigrationInProgress, RefuseMigrationRequired, RefuseReaderUpgrade, Serve,
    };
    use pointbreak::session::StoreCapabilityStatus as Status;

    match (surface, capability.map(|inspection| &inspection.status)) {
        (PublicCliCommand, None | Some(Status::MigrationRequired)) => RefuseMigrationRequired,
        (InspectorLegacyRoute, None) => Serve,
        (InspectorLegacyRoute, Some(Status::MigrationRequired)) => RefuseMigrationRequired,
        (_, Some(Status::MigrationInProgress { .. })) => RefuseMigrationInProgress,
        (PublicCliCommand, Some(Status::Ready { .. })) => Serve,
        (InspectorLegacyRoute, Some(Status::Ready { .. })) => RefuseReaderUpgrade,
    }
}

#[cfg(test)]
mod tests {
    use LegacyAdmissionSurfaceV1::{InspectorLegacyRoute, PublicCliCommand};
    use LegacyAdmissionVerdictV1::{
        RefuseMigrationInProgress, RefuseMigrationRequired, RefuseReaderUpgrade, Serve,
    };
    use pointbreak::session::{
        AuthorityCursorV2, StoreCapabilityInspection, StoreCapabilityStatus,
    };

    use super::*;

    fn inspection(status: StoreCapabilityStatus) -> StoreCapabilityInspection {
        StoreCapabilityInspection {
            status,
            cursor: AuthorityCursorV2 {
                schema: "pointbreak.authority-cursor.v2".to_owned(),
                journal_record_count: 1,
                event_count: 1,
                journal_record_set_hash: format!("sha256:{}", "2".repeat(64)),
                event_set_hash: format!("sha256:{}", "3".repeat(64)),
                capability_set_hash: format!("sha256:{}", "4".repeat(64)),
            },
            minimum_reader_profile: None,
        }
    }

    #[test]
    fn table_covers_every_store_state_on_both_surfaces() {
        let m1 = inspection(StoreCapabilityStatus::MigrationInProgress {
            activation_id: "activation:sha256:test".to_owned(),
            manifest_hash: format!("sha256:{}", "1".repeat(64)),
        });
        let l0_rooted = inspection(StoreCapabilityStatus::MigrationRequired);
        let l2 = inspection(StoreCapabilityStatus::Ready {
            activation_id: "activation:sha256:test".to_owned(),
            manifest_hash: format!("sha256:{}", "1".repeat(64)),
            completion_id: "completion:sha256:test".to_owned(),
        });
        let cells = [
            (PublicCliCommand, None, RefuseMigrationRequired),
            (PublicCliCommand, Some(&l0_rooted), RefuseMigrationRequired),
            (PublicCliCommand, Some(&m1), RefuseMigrationInProgress),
            (PublicCliCommand, Some(&l2), Serve),
            (InspectorLegacyRoute, None, Serve),
            (
                InspectorLegacyRoute,
                Some(&l0_rooted),
                RefuseMigrationRequired,
            ),
            (InspectorLegacyRoute, Some(&m1), RefuseMigrationInProgress),
            (InspectorLegacyRoute, Some(&l2), RefuseReaderUpgrade),
        ];
        for (surface, capability, expected) in cells {
            assert_eq!(
                legacy_admission_v1(surface, capability),
                expected,
                "{surface:?} / {capability:?}"
            );
        }
    }

    #[test]
    fn the_cli_surface_never_receives_reader_upgrade() {
        // The total match in the CLI renders the arm; the table must never select it.
        for capability in [
            None,
            Some(&inspection(StoreCapabilityStatus::MigrationRequired)),
        ] {
            assert_ne!(
                legacy_admission_v1(PublicCliCommand, capability),
                RefuseReaderUpgrade
            );
        }
    }
}
