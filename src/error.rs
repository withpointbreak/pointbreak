use crate::crypto::EventVerificationStatus;
use crate::model::{EventId, RevisionId};

pub type Result<T> = std::result::Result<T, ShoreError>;

#[derive(Debug, thiserror::Error)]
pub enum ShoreError {
    #[error("{0}")]
    Message(String),

    #[error("json parse failed: {0}")]
    Json(#[from] serde_json::Error),

    #[error(
        "git command failed: {command}\nstatus: {status}\nstdout:\n{stdout}\nstderr:\n{stderr}"
    )]
    GitCommand {
        command: String,
        status: String,
        stdout: String,
        stderr: String,
    },

    #[error("invalid event: {message}")]
    InvalidEvent { message: String },

    #[error("unsupported event schema/version: {schema} v{version}")]
    UnsupportedEventSchemaVersion { schema: String, version: u32 },

    #[error("unsupported event type: {0}")]
    UnsupportedEventType(SchemaBreakRecord),

    #[error("unsupported event envelope: {0}")]
    UnsupportedEventEnvelope(SchemaBreakRecord),

    #[error("unsupported state schema/version: {schema} v{version}")]
    UnsupportedStateSchemaVersion { schema: String, version: u32 },

    #[error("{reason}")]
    WorkflowInputInvalid { reason: String },

    #[error(
        "capture proposal for revision {} conflicts with the proposal already visible to this writer; create a genuinely new content state before retrying with different capture metadata",
        .revision_id.as_str()
    )]
    CaptureProposalConflict { revision_id: RevisionId },

    #[error("event signature verification rejected event {} with status {}", .event_id.as_str(), .status.as_str())]
    EventVerificationRejected {
        event_id: EventId,
        status: EventVerificationStatus,
    },

    #[error("unknown claude code session line type `{kind}` at line {line}")]
    UnknownClaudeSessionLineType { line: usize, kind: String },

    /// The volume holding the store has no active NTFS change journal, so
    /// derived-access authority cannot be proven there. Only an administrator
    /// can clear it; it is never transient.
    #[error("{0}")]
    JournalUnavailable(JournalUnavailable),
}

/// Typed, non-transient derived-access unavailability: the NTFS volume that
/// holds the store has no active USN change journal.
///
/// Pointbreak only reads a journal. It never creates, deletes, or resizes one,
/// so the remedy is an administrator's `fsutil usn createjournal` or the
/// `POINTBREAK_DERIVED_ACCESS=off` switch, which trades acceleration for
/// authoritative reads.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct JournalUnavailable {
    /// The volume as `fsutil` accepts it: a drive letter such as `D:`, or the
    /// mount-point path of a volume mounted in a folder.
    pub volume: String,
    /// The native error the journal query returned
    /// (`ERROR_JOURNAL_NOT_ACTIVE`, 1179).
    pub os_error: i32,
}

impl std::fmt::Display for JournalUnavailable {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "journal_unavailable: the NTFS change journal is not active on volume {volume} \
             (os error {code}), so derived access cannot prove authority on it; an administrator \
             can create one with `fsutil usn createjournal m=<size> a=<delta> {volume}`, or set \
             POINTBREAK_DERIVED_ACCESS=off to use authoritative reads (slower but correct); \
             Pointbreak never creates a journal",
            volume = self.volume,
            code = self.os_error,
        )
    }
}

/// A structured record describing a retired event type or envelope shape: the
/// identifier that was retired, an advisory marker for the release it broke at,
/// and the doc anchor where migration guidance lives. Minted from a single
/// source-of-truth table and rendered into both error text and read diagnostics.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SchemaBreakRecord {
    /// The retired event-type wire tag or envelope field name
    /// (e.g. "review_disposition_recorded", "writer.role").
    pub retired: String,
    /// Hand-authored advisory marker for the release the shape broke at
    /// (e.g. "0.1"). Advisory only — NOT a derived or enforced version.
    pub broken_at: String,
    /// Doc anchor where migration guidance lives
    /// (e.g. "docs/assessment-model.md#legacy-disposition-events").
    pub anchor: String,
}

impl std::fmt::Display for SchemaBreakRecord {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{} is no longer supported (broken at {}); see {}",
            self.retired, self.broken_at, self.anchor
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn workflow_input_invalid_preserves_display_text() {
        let err = ShoreError::WorkflowInputInvalid {
            reason: "track is required".to_owned(),
        };
        assert_eq!(err.to_string(), "track is required");
    }

    #[test]
    fn event_verification_rejected_preserves_display_text() {
        let err = ShoreError::EventVerificationRejected {
            event_id: crate::model::EventId::new("evt:sha256:abc"),
            status: crate::crypto::EventVerificationStatus::Unsigned,
        };
        assert_eq!(
            err.to_string(),
            "event signature verification rejected event evt:sha256:abc with status unsigned"
        );
    }

    #[test]
    fn schema_break_record_renders_canonical_sentence() {
        let record = SchemaBreakRecord {
            retired: "review_disposition_recorded".to_owned(),
            broken_at: "0.1".to_owned(),
            anchor: "docs/assessment-model.md#legacy-disposition-events".to_owned(),
        };
        assert_eq!(
            record.to_string(),
            "review_disposition_recorded is no longer supported (broken at 0.1); \
             see docs/assessment-model.md#legacy-disposition-events",
        );
    }
}
