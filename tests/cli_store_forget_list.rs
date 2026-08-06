mod support;

use serde_json::Value;
use support::pointbreak_env;

fn parse_json(bytes: &[u8]) -> Value {
    serde_json::from_slice(bytes).expect("valid json on stdout")
}

fn family_fixture(home: &std::path::Path) {
    let family = home.join("stores/acme");
    std::fs::create_dir_all(family.join("events")).unwrap();
    std::fs::create_dir_all(family.join("artifacts")).unwrap();
    std::fs::write(
        family.join("family.json"),
        br#"{"schema":"shore.family-manifest","version":1,"familyId":"acme","createdAt":"2026-07-15T00:00:00.000Z","rootCommitOids":[]}"#,
    )
    .unwrap();
    std::fs::write(
        family.join("registry.json"),
        br#"{"schema":"shore.family-registry","version":1,"entries":[]}"#,
    )
    .unwrap();
}

#[test]
fn store_forget_without_yes_previews_and_refuses_to_delete() {
    let home = tempfile::tempdir().unwrap();
    family_fixture(home.path());
    let home_str = home.path().to_str().unwrap();

    let forget = pointbreak_env(
        ["store", "forget", "acme"],
        &[("POINTBREAK_HOME", home_str)],
    );
    assert!(forget.status.success());
    let json = parse_json(&forget.stdout);
    assert_eq!(json["schema"], "pointbreak.store-forget");
    assert_eq!(json["dryRun"], true);
    assert_eq!(json["deleted"], false);
    assert!(home.path().join("stores/acme/family.json").is_file());
}

#[test]
fn store_forget_yes_on_an_orphaned_family_deletes_it() {
    let home = tempfile::tempdir().unwrap();
    family_fixture(home.path());
    let forget = pointbreak_env(
        ["store", "forget", "acme", "--yes"],
        &[("POINTBREAK_HOME", home.path().to_str().unwrap())],
    );

    assert!(forget.status.success());
    assert_eq!(parse_json(&forget.stdout)["deleted"], true);
    assert!(!home.path().join("stores/acme").exists());
}

#[test]
fn store_list_shows_a_scaffolded_family_without_a_repo() {
    let home = tempfile::tempdir().unwrap();
    family_fixture(home.path());
    let home_str = home.path().to_str().unwrap();

    let list = pointbreak_env(["store", "list"], &[("POINTBREAK_HOME", home_str)]);
    assert!(list.status.success());
    let json = parse_json(&list.stdout);
    assert_eq!(json["schema"], "pointbreak.store-list");
    assert_eq!(json["families"][0]["familyRef"], "acme");

    let text = pointbreak_env(
        ["store", "list", "--format", "text"],
        &[("POINTBREAK_HOME", home_str)],
    );
    assert!(text.status.success());
    let stdout = String::from_utf8(text.stdout).unwrap();
    assert!(stdout.contains("1 family store"), "stdout:\n{stdout}");
    assert!(stdout.contains("acme"), "stdout:\n{stdout}");
}

#[test]
fn store_list_with_an_empty_home_prints_an_empty_result() {
    let home = tempfile::tempdir().unwrap();
    let home_str = home.path().to_str().unwrap();
    let list = pointbreak_env(["store", "list"], &[("POINTBREAK_HOME", home_str)]);
    assert!(list.status.success());
    assert!(
        parse_json(&list.stdout)["families"]
            .as_array()
            .unwrap()
            .is_empty()
    );

    let text = pointbreak_env(
        ["store", "list", "--format", "text"],
        &[("POINTBREAK_HOME", home_str)],
    );
    assert!(
        String::from_utf8(text.stdout)
            .unwrap()
            .contains("no family stores")
    );
}
