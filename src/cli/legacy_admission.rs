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
pub(crate) enum LegacyAdmissionSurfaceV1 {
    PublicCliCommand,
    InspectorLegacyRoute,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum LegacyAdmissionVerdictV1 {
    Serve,
    RefuseMigrationRequired,
    RefuseMigrationInProgress,
    RefuseReaderUpgrade,
}

/// Admission reads only the capability status, so a surface may answer it from
/// the complete record inspection or from the bounded capability-pair probe.
pub(crate) fn legacy_admission_v1(
    surface: LegacyAdmissionSurfaceV1,
    capability: Option<&pointbreak::session::StoreCapabilityStatus>,
) -> LegacyAdmissionVerdictV1 {
    use LegacyAdmissionSurfaceV1::{InspectorLegacyRoute, PublicCliCommand};
    use LegacyAdmissionVerdictV1::{
        RefuseMigrationInProgress, RefuseMigrationRequired, RefuseReaderUpgrade, Serve,
    };
    use pointbreak::session::StoreCapabilityStatus as Status;

    match (surface, capability) {
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
    use pointbreak::session::StoreCapabilityStatus;

    use super::*;

    #[test]
    fn table_covers_every_store_state_on_both_surfaces() {
        let m1 = StoreCapabilityStatus::MigrationInProgress {
            activation_id: "activation:sha256:test".to_owned(),
            manifest_hash: format!("sha256:{}", "1".repeat(64)),
        };
        let l0_rooted = StoreCapabilityStatus::MigrationRequired;
        let l2 = StoreCapabilityStatus::Ready {
            activation_id: "activation:sha256:test".to_owned(),
            manifest_hash: format!("sha256:{}", "1".repeat(64)),
            completion_id: "completion:sha256:test".to_owned(),
        };
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
        for capability in [None, Some(&StoreCapabilityStatus::MigrationRequired)] {
            assert_ne!(
                legacy_admission_v1(PublicCliCommand, capability),
                RefuseReaderUpgrade
            );
        }
    }
}
