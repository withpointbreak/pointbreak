use std::io::Write;

use clap::Args;
use pointbreak::documents::{VersionBody, version_document};

use crate::cli::output::{self, FormatArgs};

/// Report CLI and document versions for compatibility checks.
#[derive(Debug, Args)]
pub(super) struct VersionArgs {
    #[command(flatten)]
    format: FormatArgs,
}

impl VersionArgs {
    #[cfg(feature = "longitudinal-counting")]
    pub(super) fn explicit_format_v1(&self) -> Option<output::OutputFormat> {
        self.format.explicit()
    }
}

pub(super) fn run(
    args: VersionArgs,
    stdout: &mut dyn Write,
) -> Result<(), Box<dyn std::error::Error>> {
    let format = output::resolve_format(args.format.explicit(), output::OutputFormat::Json)?;
    #[cfg(feature = "longitudinal-counting")]
    if let Some(scope) =
        pointbreak::bench_support::longitudinal::LongitudinalCountingScopeV1::current()
    {
        scope.record_observed_route_state_once(
            pointbreak::bench_support::longitudinal::InteractionObservedRouteStateV1::NotApplicable,
        );
    }
    let document = version_document();
    let text_source =
        matches!(format.format, output::OutputFormat::Text).then(|| document.body().clone());
    output::write_document(stdout, format, &document, || {
        render_version_text(
            text_source
                .as_ref()
                .expect("text lane resolves the version source"),
        )
    })
}

fn render_version_text(body: &VersionBody) -> String {
    format!(
        "pointbreak {}\ndocuments: {}",
        body.display_version(),
        body.documents.len()
    )
}

#[cfg(all(test, feature = "longitudinal-counting"))]
mod tests {
    use std::collections::BTreeMap;

    use clap::Parser;
    use pointbreak::bench_support::longitudinal::{
        InteractionActorV1, InteractionExecutionIdentityV1, InteractionObservedRouteStateV1,
        InteractionPerformanceExpectedContextV1, InteractionRouteV1, InteractionSetupExpectationV1,
        LongitudinalCountingScopeV1, LongitudinalDerivedAccessPhaseV1,
    };
    use sha2::{Digest, Sha256};

    use super::*;
    use crate::cli::{Cli, Command};

    #[test]
    fn version_records_non_store_state_and_only_output_work() {
        let cli = Cli::try_parse_from(["pointbreak", "version", "--format", "json"]).unwrap();
        let Command::Version(args) = cli.command else {
            panic!("version fixture parsed another command");
        };
        let counting = LongitudinalCountingScopeV1::new("e".repeat(64)).unwrap();
        counting.record_observed_route_once(InteractionRouteV1::VersionJson);
        counting.record_execution_actor_once(InteractionActorV1::RequestReader);
        let _guard = counting.enter();
        let mut stdout = Vec::new();

        let result = run(args, &mut stdout);

        counting.record_outcome_once(result.is_ok(), i32::from(result.is_err()));
        counting.record_semantic_result_sha256_once(format!("{:x}", Sha256::digest(&stdout)));
        let receipt = counting
            .interaction_receipt(InteractionPerformanceExpectedContextV1 {
                execution: InteractionExecutionIdentityV1 {
                    source_commit: "a".repeat(40),
                    source_tree: "b".repeat(40),
                    cargo_lock_sha256: "c".repeat(64),
                    binary_path: "/tmp/pointbreak-version-test".to_owned(),
                    binary_sha256: "d".repeat(64),
                    build_profile: "debug".to_owned(),
                    rustc_version: "rustc test".to_owned(),
                    features: vec!["longitudinal-counting".to_owned()],
                },
                route: InteractionRouteV1::VersionJson,
                arguments: vec![
                    "version".to_owned(),
                    "--format".to_owned(),
                    "json".to_owned(),
                ],
                setup_expectation: InteractionSetupExpectationV1::NotApplicable,
                fixture_identity_sha256: None,
                revision: None,
                track: None,
                domain_actor: None,
                expected_child_actors: BTreeMap::new(),
            })
            .expect("complete version receipt");

        assert_eq!(
            receipt.observed.route_state,
            InteractionObservedRouteStateV1::NotApplicable
        );
        assert_eq!(
            receipt
                .phases
                .iter()
                .map(|sample| sample.phase)
                .collect::<Vec<_>>(),
            vec![LongitudinalDerivedAccessPhaseV1::SerializationAndOutput]
        );
    }
}
