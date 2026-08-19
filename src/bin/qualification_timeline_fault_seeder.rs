//! Byte-clone a validated canonical qualification matrix into a disjoint
//! Timeline fault root and print the fault-seed receipt as JSON on stdout.
//!
//! The clone protocol itself lives in the library
//! (`seed_qualification_derived_timeline_fault_root_v1`); this binary is the
//! exact-source command-line entry so qualification operators do not need a
//! separately built scratch tool whose provenance is not bound to the
//! candidate commit.

use std::path::PathBuf;

use pointbreak::bench_support::derived_access::{
    QualificationDerivedTimelineFaultSeedPlanV1, seed_qualification_derived_timeline_fault_root_v1,
};

fn main() {
    let arguments = std::env::args().skip(1).collect::<Vec<_>>();
    let [
        reference_repository,
        reference_fixture_witness,
        fault_repository,
        fault_fixture_witness,
    ] = arguments.as_slice()
    else {
        eprintln!(
            "usage: qualification-timeline-fault-seeder <reference-repository> \
             <reference-witness> <fault-repository> <fault-witness>"
        );
        std::process::exit(2);
    };
    let plan = QualificationDerivedTimelineFaultSeedPlanV1 {
        reference_repository: PathBuf::from(reference_repository),
        reference_fixture_witness: PathBuf::from(reference_fixture_witness),
        fault_repository: PathBuf::from(fault_repository),
        fault_fixture_witness: PathBuf::from(fault_fixture_witness),
    };
    match seed_qualification_derived_timeline_fault_root_v1(&plan) {
        Ok(receipt) => {
            println!(
                "{}",
                serde_json::to_string_pretty(&receipt).expect("fault-seed receipt serializes")
            );
        }
        Err(error) => {
            eprintln!("fault-seed failed: {error}");
            std::process::exit(1);
        }
    }
}
