mod support;

use support::inspect::{Inspector, representative_store};

fn served() -> String {
    let store = representative_store();
    Inspector::spawn(store.repo.path()).get_text("/")
}

#[test]
fn index_html_carries_the_command_palette_overlay() {
    let html = served();
    let palette = html
        .split("id=\"cmd-palette\"")
        .nth(1)
        .and_then(|tail| tail.split("id=\"key-help\"").next())
        .expect("command palette markup exists");
    assert!(
        palette.contains("role=\"dialog\"") && palette.contains("aria-label=\"Command palette\""),
        "the palette is a labelled dialog"
    );
    // Results are ordinary focusable buttons, not a partial ARIA combobox.
    // The Change interaction tests own filtering and focus-trap behavior.
    assert!(
        palette.contains("id=\"cmd-input\"") && palette.contains("aria-label=\"Filter commands\""),
        "the palette has a labelled command filter"
    );
    assert!(
        palette.contains("placeholder=\"Filter commands…\"")
            && palette.contains("id=\"cmd-results\"")
            && palette.contains("aria-label=\"Commands\""),
        "the palette input carries a visible placeholder"
    );
}
