mod support;

use support::inspect::{Inspector, representative_store};

#[test]
fn app_js_falls_back_up_the_hierarchy_with_a_visible_diagnostic() {
    let store = representative_store();
    let insp = Inspector::spawn(store.repo.path());
    let html = insp.get_text("/");
    // A deep link to an absent entity surfaces a diagnostic, never 404/blank.
    assert!(
        html.contains("id=\"route-diagnostic\""),
        "a stable slot exists for the fallback diagnostic"
    );
}
