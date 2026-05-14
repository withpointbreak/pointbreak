use std::time::{SystemTime, UNIX_EPOCH};

pub(crate) fn current_timestamp() -> String {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    format!("unix-ms:{millis}")
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
}
