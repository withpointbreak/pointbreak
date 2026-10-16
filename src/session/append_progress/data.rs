//! Closed private data requests; the binary owns the whole executor thread.
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use serde::{Deserialize, Serialize};

use super::{AuditLimits, OwnedDataCommand, ReadContext};
use crate::error::{Result, ShoreError};
use crate::session::derived_access::enrolled;
use crate::session::derived_access::locator::LocatorRead;
use crate::session::derived_access::sqlite::SqliteSemantic;

#[doc(hidden)]
#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum OwnedDataOperation {
    Identity,
    Rebuild,
    ReadChanges,
}

#[doc(hidden)]
#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct OwnedDataRequest {
    pub version: u32,
    pub repository: PathBuf,
    pub request_id: String,
    pub enrollment_actor: String,
    pub operation: OwnedDataOperation,
    #[serde(default)]
    pub limits: AuditLimits,
}

/// The non-Send token is minted only by the unsafe binary bootstrap. This
/// function neither enters process ownership nor accepts arbitrary SQL.
#[doc(hidden)]
pub fn run_owned_data_request(
    _owner: &OwnedDataCommand,
    request: OwnedDataRequest,
    cancelled: Arc<AtomicBool>,
) -> Result<serde_json::Value> {
    super::owner::require_quiescent()?;
    if !super::owner::data_selected() || request.version != 1 {
        return Err(ShoreError::Message(
            "owned_process_required; invalid data request".into(),
        ));
    }
    let _work = super::enter_data_work(request.limits, cancelled.clone())?;
    let context = ReadContext::begin(
        &request.repository,
        &request.request_id,
        &request.enrollment_actor,
    )?;
    match request.operation {
        OwnedDataOperation::Identity => context.identity_document(),
        OwnedDataOperation::Rebuild => {
            let publication = enrolled::rebuild(&context, cancelled)?;
            Ok(serde_json::json!({"version":1,"operation":"rebuild","publication":publication}))
        }
        OwnedDataOperation::ReadChanges => {
            enrolled::read(&context, None, |db, checkpoint, receipt| {
                let LocatorRead::Ready(changes) =
                    SqliteSemantic::materialized_change_projection_on(db, checkpoint.truth_cursor)
                        .map_err(|error| ShoreError::Message(error.to_string()))?
                else {
                    return Err(ShoreError::Message(
                        "enrolled generation is stale; explicit rebuild required".into(),
                    ));
                };
                let facade = crate::documents::ChangeDocumentFacadeV1::new(
                    changes.projection,
                    changes.document_projection,
                )?;
                Ok(serde_json::json!({"version":1,"operation":"read_changes",
                    "minimumReaderProfile":receipt.minimum_reader_profile,
                    "generationId":receipt.publication_generation_id,
                    "authorityCursor":checkpoint.authority_cursor,
                    "generationEpoch":checkpoint.truth_cursor.epoch,
                    "head":checkpoint.truth_cursor.sequence,
                    "changes":facade.list_document()}))
            })
        }
    }
}

/// Private binary-only callback. It can compose detached documents but cannot
/// acquire or retain a raw SQL connection. Final admission follows the callback.
#[doc(hidden)]
pub fn with_owned_inspector_request<T>(
    _owner: &OwnedDataCommand,
    repository: &std::path::Path,
    request_id: &str,
    enrollment_actor: &str,
    limits: AuditLimits,
    cancelled: Arc<AtomicBool>,
    compose: impl FnOnce(
        &mut crate::session::derived_access::enrolled_inspector::OwnedInspectorReader<'_>,
    ) -> Result<T>,
) -> Result<T> {
    super::require_quiescent_data()?;
    let _work = super::enter_data_work(limits, cancelled)?;
    let context = ReadContext::begin(repository, request_id, enrollment_actor)?;
    crate::session::derived_access::enrolled::with_request_pin(&context, |pin| {
        let mut reader =
            crate::session::derived_access::enrolled_inspector::OwnedInspectorReader::from_pin(
                pin, repository,
            )?;
        let value = compose(&mut reader)?;
        reader.finish()?;
        Ok(value)
    })
}
