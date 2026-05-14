use std::path::Path;

use super::identity::ReviewUnitProjectionIdentity;
use crate::error::{Result, ShoreError};
use crate::model::DiffSnapshot;
use crate::session::snapshot_artifact::read_snapshot_artifact;

pub(super) fn load_bound_snapshot_artifact(
    repo: &Path,
    review_unit: &ReviewUnitProjectionIdentity,
) -> Result<DiffSnapshot> {
    let artifact = read_snapshot_artifact(repo, &review_unit.snapshot_id)?;
    if artifact.review_unit_id != review_unit.id
        || artifact.source != review_unit.source
        || artifact.base != review_unit.base
        || artifact.target != review_unit.target
        || artifact.snapshot.snapshot_id != review_unit.snapshot_id
    {
        return Err(ShoreError::Message(format!(
            "snapshot artifact metadata mismatch for {}",
            review_unit.id.as_str()
        )));
    }
    if artifact.content_hash != review_unit.snapshot_artifact_content_hash {
        return Err(ShoreError::Message(format!(
            "snapshot artifact content hash mismatch for {}",
            review_unit.id.as_str()
        )));
    }

    Ok(artifact.snapshot)
}
