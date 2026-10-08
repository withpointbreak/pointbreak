//! Response composition within the final-admitted enrolled request.
use pointbreak::session::{OwnedInspectorPage, OwnedInspectorReader};

use super::*;
use crate::cli::inspect::{change_page, event_history_page, event_history_query, page_token};

pub(in crate::cli::inspect) fn compose(
    reader: &mut OwnedInspectorReader<'_>,
    repo: &Path,
    path: &str,
    query: Option<&str>,
    signer: &page_token::PageTokenSigner,
) -> Result<ChangeV2Json, String> {
    match path {
        "/api/v2/profile" => {
            let mut profile = reader.profile();
            profile.commit_graph_stamp =
                Some(freshness_commit_graph_stamp(repo).ok_or("commit graph unavailable")?);
            serde_json::to_string(&profile)
                .map(ChangeV2Json::Ok)
                .map_err(|e| e.to_string())
        }
        "/api/v2/changes" | "/api/v2/attention" => {
            let attention = path.ends_with("attention");
            let request = match change_page::parse_signed(
                if attention {
                    change_page::Lens::Attention
                } else {
                    change_page::Lens::Changes
                },
                query,
                signer,
            ) {
                Ok(v) => v,
                Err(e) => return Ok(ChangeV2Json::Invalid(page_error_json(e))),
            };
            let derived = match request.derived_request() {
                Ok(v) => v,
                Err(e) => return Ok(ChangeV2Json::Invalid(page_error_json(e))),
            };
            if let pointbreak::session::DerivedChangePageRequestV1::Bounded(selection) = &derived
                && let Some(after) = selection.after()
                && after.expected_projection_stamp()
                    != reader
                        .generation()
                        .map_err(|e| e.to_string())?
                        .facade
                        .projection_stamp()
            {
                return Ok(ChangeV2Json::Stale(page_error_json(
                    change_page::PageError::Stale,
                )));
            }
            match reader
                .page(&derived, attention)
                .map_err(|e| e.to_string())?
            {
                OwnedInspectorPage::Changes(page) => derived_page_json(
                    serde_json::to_value(page.document).map_err(|e| e.to_string())?,
                    &request,
                    page.window.as_ref(),
                    signer,
                ),
                OwnedInspectorPage::Attention(page) => {
                    let mut value =
                        serde_json::to_value(page.document).map_err(|e| e.to_string())?;
                    merge_derived_attention_presentations(
                        &mut value,
                        page.attention_presentations,
                    )?;
                    derived_page_json(value, &request, page.window.as_ref(), signer)
                }
            }
        }
        "/api/v2/history" => {
            let invalid = |e: event_history_page::PageError| match e {
                event_history_page::PageError::Invalid(message) => ChangeV2Json::Invalid(
                    event_history_error_json("invalid_query", &message, false),
                ),
            };
            let request = match event_history_page::parse_signed(query, signer) {
                Ok(v) => v,
                Err(e) => return Ok(invalid(e)),
            };
            let derived = match request.derived_request() {
                Ok(v) => v,
                Err(e) => return Ok(invalid(e)),
            };
            let trust = reader.trust_set().map_err(|e| e.to_string())?;
            let outcome = match reader.timeline(&derived, &trust) {
                Ok(outcome) => outcome,
                Err(pointbreak::error::ShoreError::WorkflowInputInvalid { reason }) => {
                    return Ok(ChangeV2Json::Invalid(event_history_error_json(
                        "invalid_query",
                        &reason,
                        false,
                    )));
                }
                Err(error) => return Err(error.to_string()),
            };
            if let DerivedChangeOutcomeV1::ProjectionUnavailable(document) = &outcome
                && document.code() == DerivedProjectionFailureCodeV1::ProjectionStale
            {
                return Ok(ChangeV2Json::Stale(event_history_error_json(
                    "stale_projection",
                    document.message(),
                    false,
                )));
            }
            derived_change_outcome_json(
                outcome,
                |page| match event_history_query::bind_derived_page(page, &request, signer) {
                    Ok(doc) => serde_json::to_string(&doc)
                        .map(ChangeV2Json::Ok)
                        .map_err(|e| e.to_string()),
                    Err(event_history_query::ApplyError::Invalid(message)) => {
                        Ok(ChangeV2Json::Invalid(event_history_error_json(
                            "invalid_query",
                            &message,
                            false,
                        )))
                    }
                    Err(event_history_query::ApplyError::Stale) => {
                        Ok(ChangeV2Json::Stale(event_history_error_json(
                            "stale_projection",
                            "continuation belongs to a stale Timeline projection",
                            false,
                        )))
                    }
                },
            )
        }
        _ => member(reader, path, query),
    }
}
fn member(
    reader: &mut OwnedInspectorReader<'_>,
    path: &str,
    query: Option<&str>,
) -> Result<ChangeV2Json, String> {
    let Some(path) = path.strip_prefix("/api/v2/changes/") else {
        return Err("unsupported enrolled Inspector route".into());
    };
    let segments = match path
        .split('/')
        .map(|raw| {
            if raw.is_empty() {
                Err("empty route identity".into())
            } else {
                crate::cli::inspect::server::strict_percent_decode(raw)
            }
        })
        .collect::<Result<Vec<_>, String>>()
    {
        Ok(segments) => segments,
        Err(error) => return Ok(ChangeV2Json::Invalid(exact_selection_error_json(&error))),
    };
    let generation = reader.generation().map_err(|e| e.to_string())?;
    let facade = &generation.facade;
    let body = match segments.as_slice() {
        [change] => {
            let document = facade
                .detail_document(&ChangeId::new(change))
                .map_err(|e| e.to_string())?;
            let graph = change_revision_graph(&document.detail)?;
            let mut value = serde_json::to_value(document).map_err(|e| e.to_string())?;
            if let Some(graph) = graph {
                splice_inspector_presentation(&mut value, "revisionGraph", graph)?;
            }
            serde_json::to_string(&value).map_err(|e| e.to_string())?
        }
        [change, revisions, revision] | [change, revisions, revision, _]
            if revisions == "revisions" =>
        {
            let resource = segments.len() == 4;
            if resource && segments[3] != "resource" {
                return Err("unsupported Revision route".into());
            }
            let hash = match crate::cli::inspect::server::exact_selector_values(
                query,
                &["artifactHash"],
            ) {
                Ok(v) => v[0].clone(),
                Err(e) => return Ok(ChangeV2Json::Invalid(exact_selection_error_json(&e))),
            };
            let change = ChangeId::new(change);
            let exact = match crate::cli::change::exact_ref_from_projections(
                &generation.projection,
                &generation.document_projection,
                &change,
                &RevisionId::new(revision),
                &hash,
            ) {
                Ok(v) => v,
                Err(e) => {
                    return Ok(ChangeV2Json::Invalid(exact_selection_error_json(
                        &e.to_string(),
                    )));
                }
            };
            let read = reader
                .exact(
                    &change,
                    &ExactRevisionReadPlanV1 {
                        revisions: vec![exact.clone()],
                        include_body: true,
                        read_for_display: true,
                        fact_port_context: (!resource).then(|| exact.clone()),
                    },
                )
                .map_err(|e| e.to_string())?;
            let mut exact_read = if resource {
                crate::cli::change::exact_read_from_shown(
                    read.result(&exact).ok_or("selected Revision absent")?,
                    &exact,
                    true,
                )
            } else {
                crate::cli::change::contextual_exact_read_from_derived(&read, &change, &exact, true)
            }
            .map_err(|e| e.to_string())?;
            exact_read.resource = exact_read
                .resource
                .with_projection_stamp(facade.projection_stamp().into());
            if resource {
                serde_json::to_string(&exact_read.resource).map_err(|e| e.to_string())?
            } else {
                let relationships = exact_read.fact_relationships.clone();
                let document = facade
                    .contextual_revision_document_with_fact_content(
                        &change,
                        &exact,
                        exact_read.resource,
                        exact_read.facts,
                        exact_read.associations,
                        exact_read.fact_content,
                    )
                    .map_err(|e| e.to_string())?;
                let graph = fact_relationship_graph(&document.document.detail, &relationships)?;
                let mut value = serde_json::to_value(document).map_err(|e| e.to_string())?;
                if let Some(graph) = graph {
                    splice_inspector_presentation(&mut value, "factGraph", graph)?;
                }
                serde_json::to_string(&value).map_err(|e| e.to_string())?
            }
        }
        [change, kind, from, to] if kind == "interdiff" => {
            let hashes = match crate::cli::inspect::server::exact_selector_values(
                query,
                &["fromArtifactHash", "toArtifactHash"],
            ) {
                Ok(v) => v,
                Err(e) => return Ok(ChangeV2Json::Invalid(exact_selection_error_json(&e))),
            };
            let change = ChangeId::new(change);
            for (revision, hash) in [(from, &hashes[0]), (to, &hashes[1])] {
                if let Err(e) = crate::cli::change::exact_ref_from_projections(
                    &generation.projection,
                    &generation.document_projection,
                    &change,
                    &RevisionId::new(revision),
                    hash,
                ) {
                    return Ok(ChangeV2Json::Invalid(exact_selection_error_json(
                        &e.to_string(),
                    )));
                }
            }
            let doc = crate::cli::change::build_interdiff_from_projections(
                &generation.projection,
                &generation.document_projection,
                &change,
                &RevisionId::new(from),
                &hashes[0],
                &RevisionId::new(to),
                &hashes[1],
            )
            .map_err(|e| e.to_string())?
            .with_projection_stamp(facade.projection_stamp().into());
            serde_json::to_string(&doc).map_err(|e| e.to_string())?
        }
        _ => return Err("unsupported enrolled Change route".into()),
    };
    Ok(ChangeV2Json::Ok(body))
}
