//! Re-check a saved counted-input receipt against the store as it is now: for
//! each listed fact, is the same event still there with the same payload and
//! the same record content.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde_json::Value;

use super::receipt::{COUNTED_INPUT_DIGEST_ALGORITHM, CountedInputEntry, counted_input_digest};
use crate::crypto::EventVerificationStatus;
use crate::error::{Result, ShoreError};
use crate::session::EventStore;
use crate::session::event::ShoreEvent;
use crate::session::projection::skipped_to_diagnostics;
use crate::session::state::ProjectionDiagnostic;
use crate::session::store::resolution::resolve_read_store;

const REVIEW_SUMMARY_DOCUMENT_SCHEMA: &str = "pointbreak.review-summary";
const REVIEW_SUMMARY_DOCUMENT_VERSION: u64 = 1;

/// Which saved document to check, and which repository's store to check it
/// against.
#[derive(Clone, Debug)]
pub struct ReceiptCheckOptions {
    repo: PathBuf,
    receipt_document: PathBuf,
}

impl ReceiptCheckOptions {
    /// Check the summary document at `receipt_document` against `repo`'s store.
    pub fn new(repo: impl AsRef<Path>, receipt_document: impl AsRef<Path>) -> Self {
        Self {
            repo: repo.as_ref().to_path_buf(),
            receipt_document: receipt_document.as_ref().to_path_buf(),
        }
    }
}

/// How one listed fact compares with the store now.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReceiptEntryOutcome {
    /// The event is present with the recorded payload hash and record hash.
    Matched,
    /// The event is present but its payload hash or record hash differs.
    Changed,
    /// The store holds no event with that id.
    Missing,
}

/// One listed fact that no longer matches: what the receipt recorded and what
/// the store holds now.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReceiptEntryDifference {
    pub event_id: String,
    pub outcome: ReceiptEntryOutcome,
    pub recorded_payload_hash: String,
    pub recorded_event_record_hash: String,
    /// Absent when the event is missing.
    pub current_payload_hash: Option<String>,
    /// Absent when the event is missing.
    pub current_event_record_hash: Option<String>,
}

/// The outcome of re-checking a receipt. It speaks only for the facts the
/// receipt lists: events added to or removed from the store elsewhere are out
/// of its reach.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReceiptCheckResult {
    pub algorithm: String,
    pub recorded_digest: String,
    /// The number of entries the saved document lists.
    pub recorded_count: usize,
    /// Whether the saved document's entries reproduce its own digest.
    pub digest_matches_entries: bool,
    pub matched: usize,
    pub changed: usize,
    pub missing: usize,
    /// The changed and missing entries, in the receipt's entry order.
    pub differing: Vec<ReceiptEntryDifference>,
    pub diagnostics: Vec<ProjectionDiagnostic>,
}

/// Re-check the receipt in a saved `pointbreak.review-summary` document (one
/// written with `--receipt entries`) against the repo's store. Differences are
/// results, not errors; only an unreadable or unusable document, or an
/// unreadable store, fails the check.
pub fn check_counted_input_receipt(options: ReceiptCheckOptions) -> Result<ReceiptCheckResult> {
    let bytes = std::fs::read(&options.receipt_document).map_err(|error| {
        usage_error(format!(
            "cannot read {}: {error}",
            options.receipt_document.display()
        ))
    })?;
    let recorded = parse_recorded_receipt(&bytes)?;

    let read_store = resolve_read_store(&options.repo)?;
    let store = EventStore::from_backend(read_store.backend());
    let (events, skipped) = store.list_events_lenient()?;

    let mut result = compare_receipt(&recorded, &events)?;
    result.diagnostics = skipped_to_diagnostics(skipped);
    Ok(result)
}

/// The receipt as a saved document recorded it.
struct RecordedReceipt {
    algorithm: String,
    digest: String,
    entries: Vec<CountedInputEntry>,
}

fn parse_recorded_receipt(bytes: &[u8]) -> Result<RecordedReceipt> {
    let document: Value = serde_json::from_slice(bytes)
        .map_err(|error| usage_error(format!("the file is not JSON: {error}")))?;
    if document["schema"] != REVIEW_SUMMARY_DOCUMENT_SCHEMA
        || document["version"] != REVIEW_SUMMARY_DOCUMENT_VERSION
    {
        return Err(usage_error(format!(
            "the file is not a {REVIEW_SUMMARY_DOCUMENT_SCHEMA} version \
             {REVIEW_SUMMARY_DOCUMENT_VERSION} document"
        )));
    }
    let counted = &document["provenance"]["countedInputs"];
    let algorithm = string_field(counted, "algorithm", "provenance.countedInputs")?;
    if algorithm != COUNTED_INPUT_DIGEST_ALGORITHM {
        return Err(usage_error(format!(
            "the receipt digest algorithm {algorithm} is not one this build can re-check \
             (expected {COUNTED_INPUT_DIGEST_ALGORITHM})"
        )));
    }
    let digest = string_field(counted, "digest", "provenance.countedInputs")?;
    let Some(entries) = counted.get("entries").and_then(Value::as_array) else {
        return Err(usage_error(
            "the document carries a receipt digest but no entries to re-check; \
             re-run `pointbreak summary show --receipt entries` and save that output"
                .to_owned(),
        ));
    };
    let entries = entries
        .iter()
        .enumerate()
        .map(|(index, entry)| {
            let context = format!("provenance.countedInputs.entries[{index}]");
            Ok(CountedInputEntry {
                event_id: string_field(entry, "eventId", &context)?,
                payload_hash: string_field(entry, "payloadHash", &context)?,
                event_record_hash: string_field(entry, "eventRecordHash", &context)?,
                // Verification depends on the reader's trust set and is outside
                // the digest; it is neither read back nor compared.
                verification_status: EventVerificationStatus::Unsigned,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    Ok(RecordedReceipt {
        algorithm,
        digest,
        entries,
    })
}

fn string_field(value: &Value, field: &str, context: &str) -> Result<String> {
    value
        .get(field)
        .and_then(Value::as_str)
        .map(str::to_owned)
        .ok_or_else(|| usage_error(format!("{context}.{field} is missing or not a string")))
}

/// Compare every recorded entry with the event of the same id in `events`.
fn compare_receipt(
    recorded: &RecordedReceipt,
    events: &[ShoreEvent],
) -> Result<ReceiptCheckResult> {
    let current: BTreeMap<&str, &ShoreEvent> = events
        .iter()
        .map(|event| (event.event_id.as_str(), event))
        .collect();

    let mut matched = 0;
    let mut differing = Vec::new();
    for entry in &recorded.entries {
        let Some(event) = current.get(entry.event_id.as_str()) else {
            differing.push(ReceiptEntryDifference {
                event_id: entry.event_id.clone(),
                outcome: ReceiptEntryOutcome::Missing,
                recorded_payload_hash: entry.payload_hash.clone(),
                recorded_event_record_hash: entry.event_record_hash.clone(),
                current_payload_hash: None,
                current_event_record_hash: None,
            });
            continue;
        };
        let current_record_hash = event.event_record_hash()?;
        if event.payload_hash == entry.payload_hash
            && current_record_hash == entry.event_record_hash
        {
            matched += 1;
        } else {
            differing.push(ReceiptEntryDifference {
                event_id: entry.event_id.clone(),
                outcome: ReceiptEntryOutcome::Changed,
                recorded_payload_hash: entry.payload_hash.clone(),
                recorded_event_record_hash: entry.event_record_hash.clone(),
                current_payload_hash: Some(event.payload_hash.clone()),
                current_event_record_hash: Some(current_record_hash),
            });
        }
    }

    let changed = differing
        .iter()
        .filter(|difference| difference.outcome == ReceiptEntryOutcome::Changed)
        .count();
    Ok(ReceiptCheckResult {
        algorithm: recorded.algorithm.clone(),
        recorded_digest: recorded.digest.clone(),
        recorded_count: recorded.entries.len(),
        digest_matches_entries: counted_input_digest(&recorded.entries) == recorded.digest,
        matched,
        changed,
        missing: differing.len() - changed,
        differing,
        diagnostics: Vec::new(),
    })
}

fn usage_error(reason: String) -> ShoreError {
    ShoreError::WorkflowInputInvalid { reason }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{ActorId, JournalId};
    use crate::session::event::{
        EventTarget, EventType, ReviewInitializedPayload, Writer, WriterProducer,
    };

    fn event(key: &str, producer_version: &str) -> ShoreEvent {
        ShoreEvent::new(
            EventType::ReviewInitialized,
            key,
            EventTarget::for_journal(JournalId::new("journal:default")),
            Writer {
                actor_id: ActorId::new("actor:human:someone".to_owned()),
                producer: WriterProducer {
                    name: "pointbreak".to_owned(),
                    version: producer_version.to_owned(),
                },
            },
            ReviewInitializedPayload {},
            "2026-06-01T00:00:00Z",
        )
        .expect("event")
    }

    fn entry_for(event: &ShoreEvent) -> CountedInputEntry {
        CountedInputEntry {
            event_id: event.event_id.as_str().to_owned(),
            payload_hash: event.payload_hash.clone(),
            event_record_hash: event.event_record_hash().expect("record hash"),
            verification_status: EventVerificationStatus::Unsigned,
        }
    }

    fn receipt_of(events: &[&ShoreEvent]) -> RecordedReceipt {
        let entries: Vec<_> = events.iter().map(|event| entry_for(event)).collect();
        RecordedReceipt {
            algorithm: COUNTED_INPUT_DIGEST_ALGORITHM.to_owned(),
            digest: counted_input_digest(&entries),
            entries,
        }
    }

    #[test]
    fn an_untouched_store_matches_every_entry() {
        let first = event("first", "1");
        let second = event("second", "1");
        let recorded = receipt_of(&[&first, &second]);

        let result = compare_receipt(&recorded, &[first, second]).expect("compare");

        assert_eq!((result.matched, result.changed, result.missing), (2, 0, 0));
        assert!(result.differing.is_empty());
        assert!(result.digest_matches_entries);
        assert_eq!(result.recorded_count, 2);
    }

    #[test]
    fn extra_events_in_the_store_are_outside_the_check() {
        let listed = event("listed", "1");
        let elsewhere = event("elsewhere", "1");
        let recorded = receipt_of(&[&listed]);

        let result = compare_receipt(&recorded, &[listed, elsewhere]).expect("compare");

        assert_eq!((result.matched, result.changed, result.missing), (1, 0, 0));
    }

    #[test]
    fn a_rewritten_payload_is_changed_with_both_hash_pairs() {
        let original = event("rewritten", "1");
        let recorded = receipt_of(&[&original]);
        // Same event id (same idempotency key), different payload content.
        let mut rewritten = original.clone();
        rewritten.payload = serde_json::json!({"rewritten": true});
        rewritten.payload_hash =
            crate::canonical_hash::sha256_json_prefixed(&rewritten.payload).expect("payload hash");

        let result = compare_receipt(&recorded, std::slice::from_ref(&rewritten)).expect("compare");

        assert_eq!((result.matched, result.changed, result.missing), (0, 1, 0));
        let difference = &result.differing[0];
        assert_eq!(difference.outcome, ReceiptEntryOutcome::Changed);
        assert_eq!(difference.event_id, original.event_id.as_str());
        assert_eq!(difference.recorded_payload_hash, original.payload_hash);
        assert_eq!(
            difference.current_payload_hash.as_deref(),
            Some(rewritten.payload_hash.as_str())
        );
        assert_eq!(
            difference.current_event_record_hash,
            Some(rewritten.event_record_hash().expect("record hash"))
        );
        assert_ne!(
            difference.current_event_record_hash.as_deref(),
            Some(difference.recorded_event_record_hash.as_str())
        );
    }

    #[test]
    fn a_record_edit_with_the_payload_untouched_is_changed() {
        let original = event("record-edit", "1");
        let recorded = receipt_of(&[&original]);
        let mut edited = original.clone();
        edited.writer.producer.version = "2".to_owned();
        assert_eq!(edited.payload_hash, original.payload_hash);

        let result = compare_receipt(&recorded, &[edited]).expect("compare");

        assert_eq!((result.matched, result.changed, result.missing), (0, 1, 0));
        let difference = &result.differing[0];
        assert_eq!(
            difference.current_payload_hash.as_deref(),
            Some(difference.recorded_payload_hash.as_str()),
            "only the record hash moved"
        );
    }

    #[test]
    fn an_absent_event_is_missing_with_no_current_hashes() {
        let present = event("present", "1");
        let gone = event("gone", "1");
        let recorded = receipt_of(&[&present, &gone]);

        let result = compare_receipt(&recorded, &[present]).expect("compare");

        assert_eq!((result.matched, result.changed, result.missing), (1, 0, 1));
        let difference = &result.differing[0];
        assert_eq!(difference.outcome, ReceiptEntryOutcome::Missing);
        assert_eq!(difference.event_id, gone.event_id.as_str());
        assert_eq!(difference.current_payload_hash, None);
        assert_eq!(difference.current_event_record_hash, None);
    }

    #[test]
    fn an_entry_edited_in_the_saved_file_breaks_the_digest_not_the_check() {
        let listed = event("listed", "1");
        let mut recorded = receipt_of(&[&listed]);
        recorded.entries[0].payload_hash = "sha256:tampered".to_owned();

        let result = compare_receipt(&recorded, &[listed]).expect("compare");

        assert!(!result.digest_matches_entries);
        assert_eq!(result.changed, 1);
    }

    fn saved_document(counted_inputs: Value) -> Vec<u8> {
        serde_json::to_vec(&serde_json::json!({
            "schema": "pointbreak.review-summary",
            "version": 1,
            "provenance": {"countedInputs": counted_inputs},
        }))
        .expect("serialize")
    }

    fn usage_reason(bytes: &[u8]) -> String {
        match parse_recorded_receipt(bytes) {
            Err(ShoreError::WorkflowInputInvalid { reason }) => reason,
            other => panic!("expected a usage error, got {:?}", other.map(|_| ())),
        }
    }

    #[test]
    fn parsing_reads_the_entries_of_a_saved_document() {
        let listed = event("listed", "1");
        let entry = entry_for(&listed);
        let bytes = saved_document(serde_json::json!({
            "algorithm": COUNTED_INPUT_DIGEST_ALGORITHM,
            "digest": counted_input_digest(std::slice::from_ref(&entry)),
            "entries": [{
                "eventId": entry.event_id,
                "payloadHash": entry.payload_hash,
                "eventRecordHash": entry.event_record_hash,
                "verificationStatus": "valid",
            }],
        }));

        let recorded = parse_recorded_receipt(&bytes).expect("parse");

        assert_eq!(recorded.entries.len(), 1);
        assert_eq!(recorded.entries[0].event_id, entry.event_id);
        assert_eq!(
            recorded.entries[0].event_record_hash,
            entry.event_record_hash
        );
    }

    #[test]
    fn a_digest_only_document_says_to_rerun_with_entries() {
        let reason = usage_reason(&saved_document(serde_json::json!({
            "algorithm": COUNTED_INPUT_DIGEST_ALGORITHM,
            "digest": "sha256:x",
        })));

        assert!(reason.contains("--receipt entries"), "{reason}");
    }

    #[test]
    fn a_document_of_another_schema_or_algorithm_is_refused() {
        let other_schema = serde_json::to_vec(&serde_json::json!({
            "schema": "pointbreak.review-history", "version": 1,
        }))
        .expect("serialize");
        assert!(usage_reason(&other_schema).contains("pointbreak.review-summary"));

        let other_algorithm = saved_document(serde_json::json!({
            "algorithm": "some.other.algorithm", "digest": "sha256:x", "entries": [],
        }));
        assert!(usage_reason(&other_algorithm).contains("some.other.algorithm"));

        assert!(usage_reason(b"not json").contains("not JSON"));
    }

    #[test]
    fn a_malformed_entry_names_its_position_and_field() {
        let reason = usage_reason(&saved_document(serde_json::json!({
            "algorithm": COUNTED_INPUT_DIGEST_ALGORITHM,
            "digest": "sha256:x",
            "entries": [{"eventId": "evt:sha256:a", "payloadHash": "sha256:b"}],
        })));

        assert!(reason.contains("entries[0].eventRecordHash"), "{reason}");
    }
}
