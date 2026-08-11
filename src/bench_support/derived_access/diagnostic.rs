use std::path::{Component, Path};

pub const DERIVED_ACCESS_LIFECYCLE_DIAGNOSTIC_MODE_V1: &str =
    "--derived-access-lifecycle-diagnostic";
pub const DERIVED_CHANGE_DIAGNOSTIC_NATIVE_MODE_V1: &str = "--derived-change-diagnostic-native";
pub const DERIVED_CHANGE_DIAGNOSTIC_REPORT_SCHEMA_V1: &str =
    "pointbreak.derived-change-diagnostic-report.v1";
pub const DERIVED_CHANGE_DIAGNOSTIC_FRAGMENT_SCHEMA_V1: &str =
    "pointbreak.derived-change-diagnostic-fragment.v1";
pub const DERIVED_CHANGE_DIAGNOSTIC_COLLECTION_SCHEMA_V1: &str =
    "pointbreak.derived-change-diagnostic-collection.v1";
pub const DERIVED_CHANGE_DIAGNOSTIC_REPORT_BASENAME_V1: &str =
    "derived-change-diagnostic-report.json";
pub const DERIVED_CHANGE_DIAGNOSTIC_ROOT_COMPONENT_V1: &str = "derived-change-diagnostic";
pub const DERIVED_CHANGE_DIAGNOSTIC_REPORT_INADMISSIBLE_ERROR_V1: &str =
    "derived-Change diagnostic report is never qualification evidence";

pub fn reject_derived_change_diagnostic_evidence_path_v1(path: &Path) -> Result<(), String> {
    let reserved_basename = path.file_name().is_some_and(|name| {
        name.to_string_lossy()
            .eq_ignore_ascii_case(DERIVED_CHANGE_DIAGNOSTIC_REPORT_BASENAME_V1)
    });
    let reserved_root = path.components().any(|component| {
        matches!(component, Component::Normal(name)
            if name
                .to_string_lossy()
                .eq_ignore_ascii_case(DERIVED_CHANGE_DIAGNOSTIC_ROOT_COMPONENT_V1))
    });
    if reserved_basename || reserved_root {
        Err(DERIVED_CHANGE_DIAGNOSTIC_REPORT_INADMISSIBLE_ERROR_V1.to_owned())
    } else {
        Ok(())
    }
}

pub fn reject_derived_change_diagnostic_evidence_document_v1(
    document: &serde_json::Value,
) -> Result<(), String> {
    if matches!(
        document.get("schema").and_then(serde_json::Value::as_str),
        Some(DERIVED_CHANGE_DIAGNOSTIC_REPORT_SCHEMA_V1)
            | Some(DERIVED_CHANGE_DIAGNOSTIC_FRAGMENT_SCHEMA_V1)
            | Some(DERIVED_CHANGE_DIAGNOSTIC_COLLECTION_SCHEMA_V1)
    ) {
        Err(DERIVED_CHANGE_DIAGNOSTIC_REPORT_INADMISSIBLE_ERROR_V1.to_owned())
    } else {
        Ok(())
    }
}

pub fn reject_derived_change_diagnostic_evidence_input_v1(
    path: &Path,
    document: &serde_json::Value,
) -> Result<(), String> {
    reject_derived_change_diagnostic_evidence_path_v1(path)?;
    reject_derived_change_diagnostic_evidence_document_v1(document)
}
