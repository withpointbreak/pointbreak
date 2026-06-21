/// Sort and dedup a vector of ordered ids into a canonical, set-equal form so
/// equivalent inputs hash identically. Shared by every forward-pointer field
/// (supersedes, replaces, related) that must converge regardless of input order.
pub(in crate::session) fn sorted_unique<T: Ord>(mut values: Vec<T>) -> Vec<T> {
    values.sort();
    values.dedup();
    values
}
