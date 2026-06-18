use std::time::{SystemTime, UNIX_EPOCH};

use super::instant::format_rfc3339_utc_millis;

pub(crate) fn current_timestamp() -> String {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    format!("unix-ms:{millis}")
}

/// "Now" as an RFC 3339 UTC instant — the only new wall-clock boundary this plan
/// adds. Unlike `current_timestamp` (which mints `unix-ms:` for event `occurredAt`),
/// delegation `validFrom`/`validUntil` must be RFC 3339 UTC (INV-C), the form the
/// delegates reader parses.
pub fn now_rfc3339_utc() -> String {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64;
    format_rfc3339_utc_millis(millis)
}

#[cfg(test)]
mod tests {
    #[test]
    fn current_timestamp_uses_unix_ms_prefix() {
        let value = super::current_timestamp();
        let millis = value
            .strip_prefix("unix-ms:")
            .expect("timestamp has prefix")
            .parse::<u128>()
            .expect("timestamp suffix is millis");
        assert!(millis > 0);
    }

    #[test]
    fn now_rfc3339_utc_is_a_parseable_rfc3339_instant() {
        use crate::session::identity::instant::parse_rfc3339_utc_millis;
        let now = super::now_rfc3339_utc();
        assert!(
            now.ends_with('Z') && now.contains('T'),
            "RFC 3339 UTC shape: {now}"
        );
        assert!(
            parse_rfc3339_utc_millis(&now).is_some(),
            "now must re-parse: {now}"
        );
    }
}
