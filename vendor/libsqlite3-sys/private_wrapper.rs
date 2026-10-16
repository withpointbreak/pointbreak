use std::ffi::OsStr;

// Cargo forwards an empty canonical wrapper even when it executes no wrapper.
// Only that spelling and that empty value have the documented disabled meaning.
pub(crate) fn absent_or_disabled(name: &str, value: Option<&OsStr>) -> bool {
    value.is_none_or(|value| name == "RUSTC_WRAPPER" && value.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn absence_does_not_supply_an_override() {
        for name in ["RUSTC_WRAPPER", "HOST_RUSTC_WRAPPER", "CFLAGS"] {
            assert!(absent_or_disabled(name, None));
        }
    }

    #[test]
    fn canonical_empty_wrapper_is_disabled() {
        assert!(absent_or_disabled("RUSTC_WRAPPER", Some(OsStr::new(""))));
    }

    #[test]
    fn nonempty_canonical_values_remain_rejected() {
        for value in ["sccache", "/usr/bin/sccache", " ", "\t", "\n", "0", "false", "\"\""] {
            assert!(!absent_or_disabled("RUSTC_WRAPPER", Some(OsStr::new(value))));
        }
    }

    #[test]
    fn other_names_never_receive_the_empty_exception() {
        for name in [
            "rustc_wrapper", "Rustc_Wrapper", "HOST_RUSTC_WRAPPER",
            "TARGET_RUSTC_WRAPPER", "RUSTC_WRAPPER_aarch64-apple-darwin",
            "RUSTC_WRAPPER_aarch64_apple_darwin", "RUSTC_WRAPPER_x86_64-pc-windows-msvc",
            "RUSTC_WRAPPER_aarch64-pc-windows-msvc", "RUSTC_WRAPPER_EXTRA",
            "CARGO_BUILD_RUSTC_WRAPPER", "RUSTC_WORKSPACE_WRAPPER",
            "CFLAGS", "LIBSQLITE3_FLAGS", "CC_KNOWN_WRAPPER_CUSTOM",
        ] {
            assert!(!absent_or_disabled(name, Some(OsStr::new(""))), "{name}");
        }
    }

    #[cfg(unix)]
    #[test]
    fn non_utf8_wrapper_is_not_empty() {
        use std::os::unix::ffi::OsStrExt;
        assert!(!absent_or_disabled("RUSTC_WRAPPER", Some(OsStr::from_bytes(&[0xff]))));
    }

    #[cfg(windows)]
    #[test]
    fn non_unicode_wrapper_is_not_empty() {
        use std::os::windows::ffi::OsStringExt;
        let value = std::ffi::OsString::from_wide(&[0xd800]);
        assert!(!absent_or_disabled("RUSTC_WRAPPER", Some(value.as_os_str())));
    }
}
