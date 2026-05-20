use std::collections::BTreeMap;
use std::io::Write;

use shoreline::session::ProjectionDiagnostic;

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct DiagnosticDocument<T> {
    schema: &'static str,
    version: u32,
    #[serde(flatten)]
    body: T,
    diagnostics: Vec<ProjectionDiagnostic>,
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct EventWriteDocument<T> {
    schema: &'static str,
    version: u32,
    #[serde(flatten)]
    body: T,
    events_created: usize,
    events_existing: usize,
    events_created_by_type: BTreeMap<String, usize>,
    diagnostics: Vec<ProjectionDiagnostic>,
}

pub(super) fn write_json<T: serde::Serialize>(
    stdout: &mut dyn Write,
    document: &T,
    pretty: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    if pretty {
        serde_json::to_writer_pretty(&mut *stdout, document)?;
    } else {
        serde_json::to_writer(&mut *stdout, document)?;
    }
    writeln!(stdout)?;
    Ok(())
}

impl<T> DiagnosticDocument<T> {
    pub(super) fn new(
        schema: &'static str,
        body: T,
        diagnostics: Vec<ProjectionDiagnostic>,
    ) -> Self {
        Self {
            schema,
            version: 1,
            body,
            diagnostics,
        }
    }
}

impl<T> EventWriteDocument<T> {
    pub(super) fn new(
        schema: &'static str,
        body: T,
        events_created: usize,
        events_existing: usize,
        events_created_by_type: BTreeMap<String, usize>,
        diagnostics: Vec<ProjectionDiagnostic>,
    ) -> Self {
        Self {
            schema,
            version: 1,
            body,
            events_created,
            events_existing,
            events_created_by_type,
            diagnostics,
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn event_write_document_preserves_field_order() {
        use std::collections::BTreeMap;

        #[derive(serde::Serialize)]
        #[serde(rename_all = "camelCase")]
        struct Body {
            review_unit_id: &'static str,
            event_id: &'static str,
        }

        let doc = super::EventWriteDocument::new(
            "shore.test-write",
            Body {
                review_unit_id: "unit:1",
                event_id: "evt:1",
            },
            1,
            2,
            BTreeMap::new(),
            Vec::new(),
        );

        let mut stdout = Vec::new();
        super::write_json(&mut stdout, &doc, false).unwrap();

        assert_eq!(
            String::from_utf8(stdout).unwrap(),
            "{\"schema\":\"shore.test-write\",\"version\":1,\"reviewUnitId\":\"unit:1\",\"eventId\":\"evt:1\",\"eventsCreated\":1,\"eventsExisting\":2,\"eventsCreatedByType\":{},\"diagnostics\":[]}\n"
        );
    }

    #[test]
    fn diagnostic_document_preserves_trailing_diagnostics() {
        #[derive(serde::Serialize)]
        #[serde(rename_all = "camelCase")]
        struct Body {
            review_unit_id: &'static str,
            count: usize,
        }

        let doc = super::DiagnosticDocument::new(
            "shore.test-read",
            Body {
                review_unit_id: "unit:1",
                count: 3,
            },
            Vec::new(),
        );

        let mut stdout = Vec::new();
        super::write_json(&mut stdout, &doc, false).unwrap();

        assert_eq!(
            String::from_utf8(stdout).unwrap(),
            "{\"schema\":\"shore.test-read\",\"version\":1,\"reviewUnitId\":\"unit:1\",\"count\":3,\"diagnostics\":[]}\n"
        );
    }

    #[test]
    fn write_json_respects_pretty_flag() {
        #[derive(serde::Serialize)]
        struct Doc {
            schema: &'static str,
            version: u32,
        }

        let mut compact = Vec::new();
        super::write_json(
            &mut compact,
            &Doc {
                schema: "test",
                version: 1,
            },
            false,
        )
        .unwrap();
        assert_eq!(
            String::from_utf8(compact).unwrap(),
            "{\"schema\":\"test\",\"version\":1}\n"
        );

        let mut pretty = Vec::new();
        super::write_json(
            &mut pretty,
            &Doc {
                schema: "test",
                version: 1,
            },
            true,
        )
        .unwrap();
        let pretty = String::from_utf8(pretty).unwrap();
        assert!(pretty.contains("\n  \"schema\""));
    }
}
