use std::path::Path;
use std::process::Command;

use pointbreak::documents::{
    ChangeAttentionPresentationDocumentV2, ChangeListPresentationDocumentV1,
    ReaderProfileDocumentV1,
};
use pointbreak::error::Result;
use pointbreak::session::{
    DerivedAttentionPageV1, DerivedChangeAccess, DerivedChangeAttentionFilterV1,
    DerivedChangeAvailabilityFilterV1, DerivedChangeOutcomeV1, DerivedChangePageBoundaryV1,
    DerivedChangePageContinuationV1, DerivedChangePageRequestV1, DerivedChangePageSelectionV1,
    DerivedChangePageV1, DerivedChangePageWindowV1,
};

#[test]
fn derived_change_contract_is_visible_without_storage_types() {
    fn resolve(repo: &Path) -> Result<DerivedChangeAccess> {
        DerivedChangeAccess::resolve_for_inspector(repo)
    }
    let _: fn(&Path) -> Result<DerivedChangeAccess> = resolve;
    let _: fn(&DerivedChangeAccess) -> Result<DerivedChangeOutcomeV1<ReaderProfileDocumentV1>> =
        DerivedChangeAccess::profile;
    let _: fn(
        &DerivedChangeAccess,
        &DerivedChangePageRequestV1,
    ) -> Result<DerivedChangeOutcomeV1<DerivedChangePageV1>> = DerivedChangeAccess::changes;
    let _: fn(
        &DerivedChangeAccess,
        &DerivedChangePageRequestV1,
    ) -> Result<DerivedChangeOutcomeV1<DerivedAttentionPageV1>> = DerivedChangeAccess::attention;

    let selection = DerivedChangePageSelectionV1::new(
        50,
        Some(
            DerivedChangePageContinuationV1::new(
                "sha256:checkpoint",
                DerivedChangePageBoundaryV1::page_one(),
            )
            .expect("normalized continuation"),
        ),
        Some("needle".to_owned()),
        None,
        None,
        Some(DerivedChangeAttentionFilterV1::InProgress),
        Some(DerivedChangeAvailabilityFilterV1::Available),
    )
    .expect("normalized selection");
    let request = DerivedChangePageRequestV1::Bounded(selection);
    let DerivedChangePageRequestV1::Bounded(selection) = &request else {
        panic!("bounded selection changed request variants");
    };
    assert_eq!(selection.limit(), 50);
    assert_eq!(selection.summary_query(), Some("needle"));
    assert_eq!(
        selection.attention_filter(),
        Some(DerivedChangeAttentionFilterV1::InProgress)
    );
    assert_eq!(
        selection.availability_filter(),
        Some(DerivedChangeAvailabilityFilterV1::Available)
    );

    fn page_contract(
        _changes: DerivedChangePageV1,
        _attention: DerivedAttentionPageV1,
        _window: Option<DerivedChangePageWindowV1>,
        _change_document: ChangeListPresentationDocumentV1,
        _attention_document: ChangeAttentionPresentationDocumentV2,
    ) {
    }
    let _ = page_contract;
}

#[test]
fn derived_change_recipe_binds_pointbreak_home_from_its_request() {
    let justfile = std::fs::read_to_string("Justfile").expect("read Justfile");
    let recipe = justfile
        .split("derived-change-read request:")
        .nth(1)
        .and_then(|suffix| {
            suffix
                .split("derived-change-read-diagnostic request:")
                .next()
        })
        .expect("derived Change read recipe");

    assert!(recipe.contains(
        r#"POINTBREAK_HOME="$(jq -ejr '.base.pointbreakHome // .pointbreakHome' "{{ request }}")" \"#,
    ));
    assert!(!recipe.contains("POINTBREAK_HOME=\"$$("));
    assert!(recipe.contains("POINTBREAK_DERIVED_ACCESS=sqlite-wal-bodyless-v1"));
}

#[test]
fn derived_change_recipe_executes_with_the_request_bound_home() {
    let missing_prerequisites = ["just", "jq"]
        .into_iter()
        .filter(|command| Command::new(command).arg("--version").output().is_err())
        .collect::<Vec<_>>();
    if !missing_prerequisites.is_empty() {
        eprintln!(
            "skipping derived Change recipe execution; missing prerequisite(s): {}",
            missing_prerequisites.join(", ")
        );
        return;
    }

    let temporary = tempfile::tempdir().expect("create recipe test root");
    let request = temporary.path().join("request.json");
    let pointbreak_home = temporary.path().join("request-bound-home");
    std::fs::write(
        &request,
        serde_json::to_vec(&serde_json::json!({ "pointbreakHome": pointbreak_home }))
            .expect("serialize request"),
    )
    .expect("write request");

    let bin = temporary.path().join("bin");
    std::fs::create_dir(&bin).expect("create fake binary directory");
    let fake_cargo = bin.join("cargo");
    std::fs::write(
        &fake_cargo,
        r#"#!/bin/sh
set -eu
test "$POINTBREAK_HOME" = "$POINTBREAK_EXPECTED_HOME"
test "$POINTBREAK_DERIVED_ACCESS" = "sqlite-wal-bodyless-v1"
printf '%s\n' 'pointbreak-change-read-environment-ok'
"#,
    )
    .expect("write fake cargo");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&fake_cargo, std::fs::Permissions::from_mode(0o755))
            .expect("make fake cargo executable");
    }

    let inherited_path = std::env::var_os("PATH").unwrap_or_default();
    let path =
        std::env::join_paths(std::iter::once(bin).chain(std::env::split_paths(&inherited_path)))
            .expect("construct test PATH");
    let output = Command::new("just")
        .args([
            "derived-change-read",
            request.to_str().expect("UTF-8 request path"),
        ])
        .env("PATH", path)
        .env("POINTBREAK_EXPECTED_HOME", &pointbreak_home)
        .output()
        .expect("run derived Change-read recipe");

    assert!(
        output.status.success(),
        "recipe failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        String::from_utf8_lossy(&output.stdout).contains("pointbreak-change-read-environment-ok"),
        "fake cargo did not observe the bound environment"
    );
}
