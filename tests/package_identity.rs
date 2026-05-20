use std::process::Command;

#[test]
fn package_and_library_identity_are_shoreline() {
    assert_eq!(env!("CARGO_PKG_NAME"), "shoreline");

    let _ = std::any::type_name::<shoreline::model::ReviewStream>();
}

#[test]
fn installed_command_remains_shore() {
    let output = Command::new(env!("CARGO_BIN_EXE_shore"))
        .arg("--help")
        .output()
        .expect("run shore help");

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).expect("help is utf8");
    assert!(stdout.contains("Usage: shore "));
}
