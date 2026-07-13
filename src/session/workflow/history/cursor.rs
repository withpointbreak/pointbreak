use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;

use crate::error::{Result, ShoreError};
use crate::model::EventId;
use crate::session::identity::instant::parse_event_instant;

/// An opaque, deterministic continuation token for the review-history read path.
///
/// The token encodes the sort key `(occurred_at, event_id)` of the last entry a
/// caller has already seen. Clients treat it as opaque base64 and never parse
/// it; the server owns its meaning and may change the encoded shape later (for
/// example, to a future index key) without a client-visible change. The codec is
/// a pure function of its fields — no clock or randomness — so a given key always
/// yields the same token.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HistoryCursor {
    pub occurred_at: String,
    pub event_id: EventId,
}

impl HistoryCursor {
    /// Encode the cursor as a stable, opaque base64url (no-pad) token.
    pub fn encode(&self) -> String {
        // `occurred_at` and the event id never contain a newline, so '\n' is a
        // safe delimiter between the two fields (decode validates the split).
        let raw = format!("{}\n{}", self.occurred_at, self.event_id.as_str());
        URL_SAFE_NO_PAD.encode(raw.as_bytes())
    }

    /// Decode a token produced by [`HistoryCursor::encode`]. Returns a typed
    /// error on any malformed input (callers map it to an HTTP 400 / CLI usage
    /// error).
    pub fn decode(token: &str) -> Result<HistoryCursor> {
        let bytes = URL_SAFE_NO_PAD
            .decode(token.as_bytes())
            .map_err(|_| invalid_cursor())?;
        let raw = String::from_utf8(bytes).map_err(|_| invalid_cursor())?;
        let (occurred_at, event_id) = raw.split_once('\n').ok_or_else(invalid_cursor)?;
        Ok(HistoryCursor {
            occurred_at: occurred_at.to_owned(),
            event_id: EventId::new(event_id),
        })
    }
}

fn invalid_cursor() -> ShoreError {
    ShoreError::Message("invalid history cursor".to_owned())
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(super) enum EventInstantKey<'a> {
    Unparseable(&'a str),
    Parsed(i64),
}

/// The total order over the review-history stream: ascending normalized
/// `occurred_at`, then ascending `event_id`. The projection sort and the window
/// slicer both key on this, so a continuation cursor lands between exactly the
/// neighbours the sort produced. Unparseable legacy values retain a stable raw
/// ordering before legal timestamps.
pub(super) fn cmp_key<'a>(
    occurred_at: &'a str,
    event_id: &'a str,
) -> (EventInstantKey<'a>, &'a str) {
    let instant = parse_event_instant(occurred_at)
        .map(EventInstantKey::Parsed)
        .unwrap_or(EventInstantKey::Unparseable(occurred_at));
    (instant, event_id)
}

/// The forward continuation token for a windowed `range` over display-ordered
/// `keys`: emit only when at least one entry was taken AND at least one remains
/// after it (a full/last page, an empty window, and a cursor past the end all
/// yield `None`). `keys[range.end - 1]` is the display-ordered last entry, so the
/// token continues in ascending display order. Descending pages do not expose
/// continuation cursors.
pub(super) fn next_cursor_for(
    keys: &[HistoryCursor],
    range: &std::ops::Range<usize>,
) -> Option<HistoryCursor> {
    (range.end > range.start && range.end < keys.len()).then(|| keys[range.end - 1].clone())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::EventId;

    #[test]
    fn cursor_round_trips() {
        let cursor = HistoryCursor {
            occurred_at: "unix-ms:1782800000000".into(),
            event_id: EventId::new("evt:sha256:abc"),
        };
        let token = cursor.encode();
        assert_eq!(HistoryCursor::decode(&token).unwrap(), cursor);
    }

    #[test]
    fn cursor_encode_is_deterministic() {
        let cursor = HistoryCursor {
            occurred_at: "unix-ms:1".into(),
            event_id: EventId::new("evt:sha256:x"),
        };
        assert_eq!(cursor.encode(), cursor.encode());
    }

    #[test]
    fn cursor_decode_rejects_malformed() {
        assert!(HistoryCursor::decode("not-base64!!").is_err());
        assert!(HistoryCursor::decode("").is_err());
    }
}
