use std::path::Path;

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

    assert!(
        recipe.contains(r#"POINTBREAK_HOME="$$(jq -er '.pointbreakHome' "{{ request }}")" \"#,)
    );
    assert!(recipe.contains("POINTBREAK_DERIVED_ACCESS=sqlite-wal-bodyless-v1"));
}
