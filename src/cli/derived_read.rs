//! Shared CLI presentation for optional derived read acceleration.

use pointbreak::session::DerivedHistoryAccess;

/// Explain one command-local fallback without exposing the disposable
/// implementation. Reads and writes share the exact-store cadence owned by
/// the session layer.
pub(super) fn emit_authoritative_fallback_hint(access: &DerivedHistoryAccess) {
    if let Some(hint) = access.claim_authoritative_fallback_hint() {
        eprintln!("{hint}");
    }
}
