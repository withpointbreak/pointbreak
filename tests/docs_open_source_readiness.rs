#[test]
fn cli_reference_exists_and_covers_current_commands() {
    let cli = std::fs::read_to_string("docs/cli-reference.md").expect("read CLI reference");

    for command in [
        "shore show",
        "shore dump",
        "shore review capture",
        "shore review observation add",
        "shore review input-request open",
        "shore review assessment add",
        "shore review history",
        "shore review unit show",
        "shore notes apply",
    ] {
        assert!(
            cli.contains(command),
            "missing command reference for {command}"
        );
    }

    assert!(cli.contains("shore.review-capture"));
    assert!(cli.contains("shore.review-unit"));
    assert!(cli.contains("eventSetHash"));
    assert!(!cli.contains("Gumbo"));
}

#[test]
fn getting_started_walks_through_first_review() {
    let guide = std::fs::read_to_string("docs/getting-started.md").expect("read getting started");

    for required in [
        "cargo install shoreline",
        "shore review capture",
        "shore review unit show",
        "shore review observation add",
        "shore review input-request open",
        "shore review assessment add",
        ".shore/",
    ] {
        assert!(
            guide.contains(required),
            "missing getting-started step: {required}"
        );
    }

    assert!(!guide.contains("Gumbo"));
}
