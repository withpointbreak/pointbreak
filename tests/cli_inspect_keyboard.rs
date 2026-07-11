mod support;

use support::inspect::{Inspector, representative_store};

#[test]
fn served_assets_carry_a_keyboard_cheat_sheet() {
    let store = representative_store();
    let html = Inspector::spawn(store.repo.path()).get_text("/");
    // A stable overlay slot with a visible, user-facing title (not a private fn name).
    assert!(
        html.contains("id=\"key-help\""),
        "a keyboard cheat-sheet overlay slot exists"
    );
    assert!(
        html.contains("Keyboard shortcuts"),
        "the cheat sheet carries a visible title"
    );
}

#[test]
fn served_keyboard_help_lists_shipped_shortcuts() {
    let store = representative_store();
    let html = Inspector::spawn(store.repo.path()).get_text("/");
    let help = html
        .split("id=\"key-help\"")
        .nth(1)
        .and_then(|tail| tail.split("<script").next())
        .expect("keyboard help overlay markup exists");

    for shortcut in [
        "<kbd>Cmd</kbd>",
        "<kbd>Ctrl</kbd>",
        "<kbd>Shift</kbd>",
        "<kbd>K</kbd>",
        "<kbd>P</kbd>",
        "<kbd>n</kbd>",
        "<kbd>p</kbd>",
        "<kbd>]</kbd>",
        "<kbd>[</kbd>",
        "<kbd>j</kbd>",
        "<kbd>k</kbd>",
        "<kbd>/</kbd>",
        "<kbd>g</kbd>",
        "<kbd>3</kbd>",
        "<kbd>Esc</kbd>",
        "<kbd>?</kbd>",
    ] {
        assert!(
            help.contains(shortcut),
            "keyboard help should list {shortcut}"
        );
    }
}

#[test]
fn served_lens_buttons_do_not_use_selected_state_without_tab_semantics() {
    let store = representative_store();
    let html = Inspector::spawn(store.repo.path()).get_text("/");
    let lens = html
        .split("id=\"lens-switcher\"")
        .nth(1)
        .and_then(|tail| tail.split("</nav>").next())
        .expect("lens switcher markup exists");
    let tab_model = lens.contains("role=\"tablist\"") && lens.contains("role=\"tab\"");
    let pressed_button_model = lens.contains("aria-pressed") && !lens.contains("aria-selected");

    assert!(
        tab_model || pressed_button_model,
        "lens switcher should either be a real tablist or use pressed button state"
    );
}
