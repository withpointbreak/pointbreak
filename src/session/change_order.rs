//! Presentation order for Change list surfaces.
//!
//! One order vocabulary and one keyed comparator serve the Inspector Change
//! pages, the CLI, the authoritative facade, and the derived reader, so no
//! surface can order Changes differently. Instant ordering goes through
//! [`compare_event_instants`], the crate's single instant comparator: under
//! `activity_desc` an unparseable instant therefore lands last, and equal
//! instants with different spellings tie and fall through to `change_id`.
//!
//! Ordering is presentation only. Nothing here may be read by membership,
//! topology, lifecycle, currency, or acceptance logic.

use std::cmp::Ordering;

use serde::{Deserialize, Serialize};

use super::identity::compare_event_instants;

/// Presentation order for Change list surfaces. Shared by the Inspector, the
/// CLI, and both lanes so no surface can order Changes differently.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChangeListOrderV1 {
    ChangeIdAsc,
    #[default]
    ActivityDesc,
    /// Attention lens only; rejected on the Changes lens.
    AttentionWait,
}

impl ChangeListOrderV1 {
    /// The wire spelling, identical on every surface.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ChangeIdAsc => "change_id_asc",
            Self::ActivityDesc => "activity_desc",
            Self::AttentionWait => "attention_wait",
        }
    }

    /// Parse the wire spelling. Rejects every other string, including the
    /// kebab-case form clap would otherwise invent.
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "change_id_asc" => Some(Self::ChangeIdAsc),
            "activity_desc" => Some(Self::ActivityDesc),
            "attention_wait" => Some(Self::AttentionWait),
            _ => None,
        }
    }

    /// Orders admitted on a Change list surface; `attention_wait` only means
    /// something where attention items exist.
    pub fn admitted_on_attention_lens(self) -> bool {
        true
    }

    pub fn admitted_on_changes_lens(self) -> bool {
        !matches!(self, Self::AttentionWait)
    }

    /// The per-lens default: Changes asks "what moved lately?", Attention asks
    /// "what has waited on me longest?".
    pub const fn default_for_changes() -> Self {
        Self::ActivityDesc
    }

    pub const fn default_for_attention() -> Self {
        Self::AttentionWait
    }
}

/// Wait-time ordering key for the Attention lens: tier first (primary before
/// secondary, matching the attention item tier rank), then the OLDEST
/// unresolved `observed_at` for that Change. `oldest_observed_at` is raw and
/// unparsed; [`compare_event_instants`] orders it ASCENDING here, because
/// longest-waiting-first means oldest-first.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AttentionWaitKeyV1 {
    pub tier_rank: u8,
    pub oldest_observed_at: String,
}

/// The comparator's input. Keyed rather than typed: the Inspector page sorts
/// `serde_json::Value` and cannot produce a typed summary, while the derived
/// lane sorts typed summaries. Every component any order needs is carried so
/// `attention_wait` can be compared at all.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ChangeOrderKey<'a> {
    pub change_id: &'a str,
    pub activity_at: Option<&'a str>,
    pub tier_rank: Option<u8>,
    pub oldest_observed_at: Option<&'a str>,
}

/// The one Change-order comparator. Total under every order: ties fall
/// through to `change_id` ascending, and an absent key sorts after every
/// present key so a Change with nothing to report never outranks one with a
/// value.
pub fn compare_change_order(
    order: ChangeListOrderV1,
    left: ChangeOrderKey<'_>,
    right: ChangeOrderKey<'_>,
) -> Ordering {
    let by_id = || left.change_id.cmp(right.change_id);
    match order {
        ChangeListOrderV1::ChangeIdAsc => by_id(),
        ChangeListOrderV1::ActivityDesc => match (left.activity_at, right.activity_at) {
            (Some(left_at), Some(right_at)) => compare_event_instants(right_at, left_at),
            (Some(_), None) => Ordering::Less,
            (None, Some(_)) => Ordering::Greater,
            (None, None) => Ordering::Equal,
        }
        .then_with(by_id),
        ChangeListOrderV1::AttentionWait => match (
            left.tier_rank.zip(left.oldest_observed_at),
            right.tier_rank.zip(right.oldest_observed_at),
        ) {
            (Some((left_tier, left_at)), Some((right_tier, right_at))) => left_tier
                .cmp(&right_tier)
                .then_with(|| compare_event_instants(left_at, right_at)),
            (Some(_), None) => Ordering::Less,
            (None, Some(_)) => Ordering::Greater,
            (None, None) => Ordering::Equal,
        }
        .then_with(by_id),
    }
}

/// The page boundary a continuation carries, tagged with the order it was
/// issued under. One shape cannot slice three orders, so the key is
/// order-specific; a continuation presented under a different order is
/// rejected by the order check, never re-interpreted.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "order",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum ChangePageKeyV1 {
    ChangeIdAsc {
        change_id: String,
    },
    ActivityDesc {
        activity_at: Option<String>,
        change_id: String,
    },
    AttentionWait {
        tier_rank: Option<u8>,
        oldest_observed_at: Option<String>,
        change_id: String,
    },
}

impl ChangePageKeyV1 {
    /// Build the boundary key for one row under the order in effect.
    pub fn from_order_key(order: ChangeListOrderV1, key: ChangeOrderKey<'_>) -> Self {
        match order {
            ChangeListOrderV1::ChangeIdAsc => Self::ChangeIdAsc {
                change_id: key.change_id.to_owned(),
            },
            ChangeListOrderV1::ActivityDesc => Self::ActivityDesc {
                activity_at: key.activity_at.map(str::to_owned),
                change_id: key.change_id.to_owned(),
            },
            ChangeListOrderV1::AttentionWait => Self::AttentionWait {
                tier_rank: key.tier_rank,
                oldest_observed_at: key.oldest_observed_at.map(str::to_owned),
                change_id: key.change_id.to_owned(),
            },
        }
    }

    /// The order this key was issued under.
    pub const fn order(&self) -> ChangeListOrderV1 {
        match self {
            Self::ChangeIdAsc { .. } => ChangeListOrderV1::ChangeIdAsc,
            Self::ActivityDesc { .. } => ChangeListOrderV1::ActivityDesc,
            Self::AttentionWait { .. } => ChangeListOrderV1::AttentionWait,
        }
    }

    pub fn change_id(&self) -> &str {
        match self {
            Self::ChangeIdAsc { change_id }
            | Self::ActivityDesc { change_id, .. }
            | Self::AttentionWait { change_id, .. } => change_id,
        }
    }

    /// Borrow the boundary as a comparator input.
    pub fn order_key(&self) -> ChangeOrderKey<'_> {
        match self {
            Self::ChangeIdAsc { change_id } => ChangeOrderKey {
                change_id,
                activity_at: None,
                tier_rank: None,
                oldest_observed_at: None,
            },
            Self::ActivityDesc {
                activity_at,
                change_id,
            } => ChangeOrderKey {
                change_id,
                activity_at: activity_at.as_deref(),
                tier_rank: None,
                oldest_observed_at: None,
            },
            Self::AttentionWait {
                tier_rank,
                oldest_observed_at,
                change_id,
            } => ChangeOrderKey {
                change_id,
                activity_at: None,
                tier_rank: *tier_rank,
                oldest_observed_at: oldest_observed_at.as_deref(),
            },
        }
    }
}

/// Every row of a page must be strictly ordered under the order it declares.
/// Strict (never equal) so duplicates and mis-slicing are still caught.
pub fn is_strictly_ordered<'a>(
    order: ChangeListOrderV1,
    keys: impl IntoIterator<Item = ChangeOrderKey<'a>>,
) -> bool {
    let mut previous: Option<ChangeOrderKey<'a>> = None;
    for key in keys {
        if let Some(last) = previous
            && compare_change_order(order, last, key) != Ordering::Less
        {
            return false;
        }
        previous = Some(key);
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    const PARITY: &str = include_str!("../cli/inspect/web/test/fixtures/change-order-parity.json");

    fn key<'a>(change_id: &'a str, activity_at: Option<&'a str>) -> ChangeOrderKey<'a> {
        ChangeOrderKey {
            change_id,
            activity_at,
            tier_rank: None,
            oldest_observed_at: None,
        }
    }

    fn wait<'a>(
        change_id: &'a str,
        tier_rank: Option<u8>,
        oldest_observed_at: Option<&'a str>,
    ) -> ChangeOrderKey<'a> {
        ChangeOrderKey {
            change_id,
            activity_at: None,
            tier_rank,
            oldest_observed_at,
        }
    }

    #[test]
    fn wire_spellings_are_snake_case_and_round_trip() {
        for order in [
            ChangeListOrderV1::ChangeIdAsc,
            ChangeListOrderV1::ActivityDesc,
            ChangeListOrderV1::AttentionWait,
        ] {
            assert_eq!(ChangeListOrderV1::parse(order.as_str()), Some(order));
            assert_eq!(
                serde_json::to_value(order).unwrap(),
                serde_json::Value::String(order.as_str().to_owned())
            );
        }
        assert_eq!(ChangeListOrderV1::parse("activity-desc"), None);
        assert_eq!(ChangeListOrderV1::parse("activity_asc"), None);
        assert_eq!(
            ChangeListOrderV1::default(),
            ChangeListOrderV1::ActivityDesc
        );
        assert!(!ChangeListOrderV1::AttentionWait.admitted_on_changes_lens());
        assert!(ChangeListOrderV1::AttentionWait.admitted_on_attention_lens());
    }

    #[test]
    fn activity_desc_orders_newest_first_then_change_id_ascending() {
        let newest = key("change:sha256:3b77", Some("2026-09-12T19:20:00.000Z"));
        let older = key("change:sha256:0a1f", Some("2026-09-12T18:04:00.000Z"));
        assert_eq!(
            compare_change_order(ChangeListOrderV1::ActivityDesc, newest, older),
            Ordering::Less
        );
        let tied = key("change:sha256:91cd", Some("2026-09-12T18:04:00.000Z"));
        assert_eq!(
            compare_change_order(ChangeListOrderV1::ActivityDesc, older, tied),
            Ordering::Less
        );
        assert_eq!(
            compare_change_order(ChangeListOrderV1::ChangeIdAsc, newest, older),
            Ordering::Greater
        );
    }

    #[test]
    fn activity_desc_places_unreadable_and_absent_instants_last() {
        let readable = key("change:sha256:zzzz", Some("1970-01-01T00:00:00.000Z"));
        let unreadable = key("change:sha256:0000", Some("unix-ms:not-a-number"));
        let absent = key("change:sha256:0000", None);
        assert_eq!(
            compare_change_order(ChangeListOrderV1::ActivityDesc, readable, unreadable),
            Ordering::Less
        );
        assert_eq!(
            compare_change_order(ChangeListOrderV1::ActivityDesc, unreadable, absent),
            Ordering::Less
        );
    }

    #[test]
    fn equal_instants_with_different_spellings_tie_on_the_instant() {
        let left = key("change:sha256:b", Some("unix-ms:0"));
        let right = key("change:sha256:a", Some("1970-01-01T00:00:00.000Z"));
        assert_eq!(
            compare_change_order(ChangeListOrderV1::ActivityDesc, left, right),
            Ordering::Greater
        );
    }

    #[test]
    fn attention_wait_orders_tier_then_oldest_then_change_id_with_absent_last() {
        let primary_old = wait(
            "change:sha256:91cd",
            Some(0),
            Some("2026-09-01T00:00:00.000Z"),
        );
        let primary_new = wait(
            "change:sha256:0a1f",
            Some(0),
            Some("2026-09-02T00:00:00.000Z"),
        );
        let primary_new_tie = wait(
            "change:sha256:3b77",
            Some(0),
            Some("2026-09-02T00:00:00.000Z"),
        );
        let secondary_older = wait(
            "change:sha256:7e02",
            Some(1),
            Some("2026-08-01T00:00:00.000Z"),
        );
        let absent = wait("change:sha256:0000", None, None);
        let order = ChangeListOrderV1::AttentionWait;
        assert_eq!(
            compare_change_order(order, primary_old, primary_new),
            Ordering::Less
        );
        assert_eq!(
            compare_change_order(order, primary_new, primary_new_tie),
            Ordering::Less
        );
        assert_eq!(
            compare_change_order(order, primary_new_tie, secondary_older),
            Ordering::Less
        );
        assert_eq!(
            compare_change_order(order, secondary_older, absent),
            Ordering::Less
        );
    }

    #[test]
    fn page_keys_round_trip_their_order_and_components() {
        let source = wait(
            "change:sha256:91cd",
            Some(0),
            Some("2026-09-01T00:00:00.000Z"),
        );
        let boundary = ChangePageKeyV1::from_order_key(ChangeListOrderV1::AttentionWait, source);
        assert_eq!(boundary.order(), ChangeListOrderV1::AttentionWait);
        assert_eq!(boundary.order_key(), source);
        let json = serde_json::to_value(&boundary).unwrap();
        assert_eq!(json["order"], "attention_wait");
        assert_eq!(json["tierRank"], 0);
        let decoded: ChangePageKeyV1 = serde_json::from_value(json).unwrap();
        assert_eq!(decoded, boundary);
        let activity = ChangePageKeyV1::from_order_key(
            ChangeListOrderV1::ActivityDesc,
            key("change:sha256:0a1f", None),
        );
        assert_eq!(activity.order_key().activity_at, None);
        assert_eq!(activity.change_id(), "change:sha256:0a1f");
    }

    #[test]
    fn strict_ordering_rejects_ties_and_reversals() {
        let a = key("change:sha256:a", Some("2026-09-12T19:20:00.000Z"));
        let b = key("change:sha256:b", Some("2026-09-12T18:04:00.000Z"));
        assert!(is_strictly_ordered(ChangeListOrderV1::ActivityDesc, [a, b]));
        assert!(!is_strictly_ordered(
            ChangeListOrderV1::ActivityDesc,
            [b, a]
        ));
        assert!(!is_strictly_ordered(
            ChangeListOrderV1::ActivityDesc,
            [a, a]
        ));
        assert!(is_strictly_ordered(ChangeListOrderV1::ActivityDesc, []));
    }

    #[test]
    fn the_shared_parity_vectors_hold_for_the_rust_comparator() {
        let vectors: serde_json::Value = serde_json::from_str(PARITY).unwrap();
        for case in vectors["instantComparisons"].as_array().unwrap() {
            let left = case["left"].as_str().unwrap();
            let right = case["right"].as_str().unwrap();
            let expected = match case["expected"].as_str().unwrap() {
                "lt" => Ordering::Less,
                "eq" => Ordering::Equal,
                "gt" => Ordering::Greater,
                other => panic!("unknown expectation {other}"),
            };
            assert_eq!(
                compare_event_instants(left, right),
                expected,
                "{left} vs {right}"
            );
        }
        for page in vectors["activityDescPages"].as_array().unwrap() {
            let order = ChangeListOrderV1::parse(page["order"].as_str().unwrap()).unwrap();
            let changes = page["changes"].as_array().unwrap();
            let keys = changes.iter().map(|change| ChangeOrderKey {
                change_id: change["changeId"].as_str().unwrap(),
                activity_at: change["activityAt"].as_str(),
                tier_rank: change["attentionWaitAt"]["tierRank"]
                    .as_u64()
                    .map(|rank| rank as u8),
                oldest_observed_at: change["attentionWaitAt"]["oldestObservedAt"].as_str(),
            });
            assert_eq!(
                is_strictly_ordered(order, keys),
                page["monotonic"].as_bool().unwrap(),
                "{}",
                page["note"]
            );
        }
    }
}
