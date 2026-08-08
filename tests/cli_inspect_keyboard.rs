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
        "<kbd>1</kbd>",
        "<kbd>2</kbd>",
        "<kbd>j</kbd>",
        "<kbd>k</kbd>",
        "<kbd>g</kbd>",
        "<kbd>G</kbd>",
        "<kbd>Enter</kbd>",
        "<kbd>h</kbd>",
        "<kbd>l</kbd>",
        "<kbd>/</kbd>",
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
fn served_lens_switcher_uses_one_valid_selection_model() {
    let store = representative_store();
    let html = Inspector::spawn(store.repo.path()).get_text("/");
    let lens = html
        .split("id=\"lens-switcher\"")
        .nth(1)
        .and_then(|tail| tail.split("</nav>").next())
        .expect("lens switcher markup exists");
    let tab_model = lens.contains("role=\"tablist\"") && lens.contains("role=\"tab\"");
    let pressed_button_model = lens.contains("aria-pressed") && !lens.contains("aria-selected");
    let routed_navigation_model = lens.matches("<a ").count() == 3
        && ["timeline", "changes", "attention"]
            .into_iter()
            .all(|route| lens.contains(&format!("href=\"#/{route}\"")))
        && lens.matches("aria-current=\"page\"").count() == 1
        && !lens.contains("aria-selected");
    let model_count = [tab_model, pressed_button_model, routed_navigation_model]
        .into_iter()
        .filter(|model| *model)
        .count();

    assert_eq!(
        model_count, 1,
        "lens switcher should be a real tablist, pressed buttons, or routed navigation links"
    );
}
