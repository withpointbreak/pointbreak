mod support;

use support::inspect::{Inspector, representative_store};

fn served() -> String {
    let store = representative_store();
    Inspector::spawn(store.repo.path()).get_text("/")
}

#[test]
fn index_html_carries_the_command_palette_overlay() {
    let html = served();
    assert!(
        html.contains("id=\"cmd-palette\""),
        "the palette overlay slot exists"
    );
    // A combobox + listbox with a visible, user-facing placeholder.
    assert!(
        html.contains("role=\"combobox\"") || html.contains("role=\"listbox\""),
        "the palette is an aria combobox/listbox"
    );
    assert!(
        html.contains("Jump to") || html.contains("Type a command"),
        "the palette input carries a visible placeholder"
    );
}
