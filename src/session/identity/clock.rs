use std::time::{SystemTime, UNIX_EPOCH};

use super::instant::format_rfc3339_utc_millis;

pub(crate) trait IngestClock {
    fn received_at(&self) -> String;
}

pub(crate) struct SystemIngestClock;

impl IngestClock for SystemIngestClock {
    fn received_at(&self) -> String {
        now_rfc3339_utc()
    }
}

pub(crate) fn current_timestamp() -> String {
    SystemIngestClock.received_at()
}

/// "Now" as an RFC 3339 UTC instant with millisecond precision.
pub fn now_rfc3339_utc() -> String {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64;
    format_rfc3339_utc_millis(millis)
}

#[cfg(test)]
mod tests {
    struct FrozenTestClock;

    impl super::IngestClock for FrozenTestClock {
        fn received_at(&self) -> String {
            "2026-01-02T03:04:05.006Z".to_owned()
        }
    }

    #[test]
    fn ingest_clock_can_be_injected_without_changing_the_public_now_function() {
        let injected = super::IngestClock::received_at(&FrozenTestClock);
        let system = super::IngestClock::received_at(&super::SystemIngestClock);

        assert_eq!(injected, "2026-01-02T03:04:05.006Z");
        assert_ne!(system, injected);
    }

    #[test]
    fn current_timestamp_uses_rfc3339_utc_with_millisecond_precision() {
        use crate::session::identity::instant::parse_rfc3339_utc_millis;

        let value = super::current_timestamp();
        let fraction = value
            .strip_suffix('Z')
            .and_then(|without_z| without_z.rsplit_once('.'))
            .map(|(_, fraction)| fraction)
            .expect("timestamp has fractional seconds");

        assert_eq!(fraction.len(), 3, "timestamp has millisecond precision");
        assert!(fraction.bytes().all(|byte| byte.is_ascii_digit()));
        assert!(parse_rfc3339_utc_millis(&value).is_some());
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
