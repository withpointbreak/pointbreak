use crate::canonical_hash::sha256_json_hex;
use crate::error::Result;
use crate::model::ReviewTargetRef;

pub(super) fn sorted_unique<T: Ord>(mut values: Vec<T>) -> Vec<T> {
    values.sort();
    values.dedup();
    values
}

pub(super) fn sorted_unique_targets(targets: Vec<ReviewTargetRef>) -> Result<Vec<ReviewTargetRef>> {
    let mut keyed_targets = targets
        .into_iter()
        .map(|target| Ok((sha256_json_hex(&target)?, target)))
        .collect::<Result<Vec<_>>>()?;
    keyed_targets.sort_by(|(left, _), (right, _)| left.cmp(right));
    keyed_targets.dedup_by(|(left, _), (right, _)| left == right);
    Ok(keyed_targets
        .into_iter()
        .map(|(_, target)| target)
        .collect())
}
