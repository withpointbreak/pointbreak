//! Versioned documents shared by the bundled inspect server and its machine
//! clients.
//!
//! The stored object artifact remains the content-addressed source of truth.
//! The snapshot document below retags only the served envelope and adds
//! best-effort presentation spans to copied rows.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::highlight::{EmphSpan, RowKey, TokenSpan, emphasis_file, highlight_file};
use crate::model::{
    DiffFile, DiffRow, DiffRowKind, DiffSnapshot, FileId, FileMetadataRow, FileStatus, HunkId,
    ObjectId, ReviewHunk, ReviewId,
};
use crate::session::{ObjectArtifact, build_object_artifact_v2};

pub const REVIEW_SNAPSHOT_SCHEMA: &str = "pointbreak.review-snapshot";
pub const INSPECT_FRESHNESS_SCHEMA: &str = "pointbreak.inspect-freshness";
pub const INSPECT_STARTUP_SCHEMA: &str = "pointbreak.inspect-startup";
pub const INSPECT_READER_PROFILE_SCHEMA: &str = "pointbreak.inspect-reader-profile";
pub const INSPECT_CHANGES_PAGE_SCHEMA: &str = "pointbreak.inspect-changes-page";
pub const INSPECT_ATTENTION_SCHEMA_V2: &str = "pointbreak.inspect-attention";

const PROMOTED_INSPECT_DOCUMENTS: [(&str, u32); 3] = [
    (REVIEW_SNAPSHOT_SCHEMA, 1),
    (INSPECT_FRESHNESS_SCHEMA, 1),
    (INSPECT_STARTUP_SCHEMA, 1),
];

/// The complete set of inspect documents that bundled machine clients may
/// compatibility-check through `pointbreak.version`.
pub fn promoted_inspect_document_registry() -> &'static [(&'static str, u32)] {
    &PROMOTED_INSPECT_DOCUMENTS
}

/// A served, enriched view of one validated stored object artifact.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReviewSnapshotDocument {
    schema: String,
    version: u32,
    content_hash: String,
    snapshot: ReviewSnapshot,
}

impl ReviewSnapshotDocument {
    fn from_snapshot(content_hash: String, snapshot: &DiffSnapshot) -> Self {
        Self {
            schema: REVIEW_SNAPSHOT_SCHEMA.to_owned(),
            version: 1,
            content_hash,
            snapshot: ReviewSnapshot::from_snapshot(snapshot),
        }
    }

    /// Re-establish the immutable captured-resource binding after transport or
    /// deserialization. Presentation spans are deliberately excluded from the
    /// canonical artifact hash; the reconstructed captured rows must still
    /// reproduce the Revision-bound object artifact exactly.
    pub(crate) fn validate_binding(
        &self,
        expected_content_hash: &str,
        expected_object_id: &ObjectId,
    ) -> crate::error::Result<()> {
        if self.schema != REVIEW_SNAPSHOT_SCHEMA
            || self.version != 1
            || self.content_hash != expected_content_hash
            || &self.snapshot.object_id != expected_object_id
        {
            return Err(crate::error::ShoreError::Message(
                "captured review snapshot identity does not match the exact resource".to_owned(),
            ));
        }
        let snapshot = self.snapshot.to_diff_snapshot();
        if self.snapshot != ReviewSnapshot::from_snapshot(&snapshot) {
            return Err(crate::error::ShoreError::Message(
                "captured review snapshot presentation spans are not canonical".to_owned(),
            ));
        }
        let artifact = build_object_artifact_v2(snapshot)?;
        if artifact.content_hash != self.content_hash {
            return Err(crate::error::ShoreError::Message(
                "captured review snapshot rows do not reproduce the bound artifact hash".to_owned(),
            ));
        }
        Ok(())
    }
}

/// Build the v1 served snapshot without changing the at-rest `shore.object`
/// artifact or its content hash.
pub fn review_snapshot_document(artifact: &ObjectArtifact) -> ReviewSnapshotDocument {
    ReviewSnapshotDocument::from_snapshot(artifact.content_hash.clone(), &artifact.snapshot)
}

/// Build the served snapshot from an already validated bound snapshot. Exact
/// Revision readers have performed the content-addressed load before their
/// contextual document is assembled; this variant preserves that one read and
/// emits the identical captured rows without manufacturing an `ObjectArtifact`.
pub(crate) fn review_snapshot_document_from_snapshot(
    content_hash: impl Into<String>,
    snapshot: &DiffSnapshot,
) -> ReviewSnapshotDocument {
    ReviewSnapshotDocument::from_snapshot(content_hash.into(), snapshot)
}

/// Cheap change marker for a running inspect server.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InspectFreshnessDocument {
    schema: &'static str,
    version: u32,
    event_count: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    projection_stamp: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    commit_graph_stamp: Option<String>,
}

impl InspectFreshnessDocument {
    pub fn new(event_count: u64, commit_graph_stamp: Option<String>) -> Self {
        Self {
            schema: INSPECT_FRESHNESS_SCHEMA,
            version: 1,
            event_count,
            projection_stamp: None,
            commit_graph_stamp,
        }
    }

    #[doc(hidden)]
    pub fn with_projection_stamp(mut self, projection_stamp: String) -> Self {
        self.projection_stamp = Some(projection_stamp);
        self
    }
}

/// The one-line stdout handshake emitted when inspect startup output is JSON.
///
/// This type intentionally does not implement `Debug` or `Display`: its token
/// is serialized only for the single startup handoff.
#[derive(Serialize)]
pub struct InspectStartupDocument {
    schema: &'static str,
    version: u32,
    host: String,
    port: u16,
    token: String,
}

impl InspectStartupDocument {
    pub fn new(host: impl Into<String>, port: u16, token: impl Into<String>) -> Self {
        Self {
            schema: INSPECT_STARTUP_SCHEMA,
            version: 1,
            host: host.into(),
            port,
            token: token.into(),
        }
    }
}

/// Per-file enrichment cap. Larger diffs remain fully readable but omit both
/// optional presentation channels.
const HIGHLIGHT_FILE_ROW_CAP: usize = 500;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ReviewSnapshot {
    review_id: ReviewId,
    object_id: ObjectId,
    files: Vec<ReviewSnapshotFile>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ReviewSnapshotFile {
    id: FileId,
    status: FileStatus,
    old_path: Option<String>,
    new_path: Option<String>,
    old_mode: Option<String>,
    new_mode: Option<String>,
    old_oid: Option<String>,
    new_oid: Option<String>,
    similarity: Option<u16>,
    is_binary: bool,
    is_submodule: bool,
    is_mode_only: bool,
    synthetic: bool,
    metadata_rows: Vec<FileMetadataRow>,
    hunks: Vec<ReviewSnapshotHunk>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ReviewSnapshotHunk {
    id: HunkId,
    header: String,
    old_start: u32,
    old_lines: u32,
    new_start: u32,
    new_lines: u32,
    rows: Vec<ReviewSnapshotRow>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ReviewSnapshotRow {
    kind: DiffRowKind,
    old_line: Option<u32>,
    new_line: Option<u32>,
    text: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    tokens: Vec<ReviewTokenSpan>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    emphasis: Vec<ReviewEmphasisSpan>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ReviewTokenSpan {
    start: usize,
    end: usize,
    kind: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ReviewEmphasisSpan {
    start: usize,
    end: usize,
}

impl ReviewSnapshot {
    fn from_snapshot(snapshot: &DiffSnapshot) -> Self {
        Self {
            review_id: snapshot.review_id.clone(),
            object_id: snapshot.object_id.clone(),
            files: snapshot
                .files
                .iter()
                .map(ReviewSnapshotFile::from_file)
                .collect(),
        }
    }

    fn to_diff_snapshot(&self) -> DiffSnapshot {
        DiffSnapshot {
            review_id: self.review_id.clone(),
            object_id: self.object_id.clone(),
            files: self
                .files
                .iter()
                .map(ReviewSnapshotFile::to_diff_file)
                .collect(),
        }
    }
}

impl ReviewSnapshotFile {
    fn from_file(file: &DiffFile) -> Self {
        let total_rows = file.hunks.iter().map(|hunk| hunk.rows.len()).sum::<usize>();
        let (tokens, emphasis) = if total_rows > HIGHLIGHT_FILE_ROW_CAP {
            (HashMap::new(), HashMap::new())
        } else {
            (highlight_file(file), emphasis_file(file))
        };

        Self {
            id: file.id.clone(),
            status: file.status.clone(),
            old_path: file.old_path.clone(),
            new_path: file.new_path.clone(),
            old_mode: file.old_mode.clone(),
            new_mode: file.new_mode.clone(),
            old_oid: file.old_oid.clone(),
            new_oid: file.new_oid.clone(),
            similarity: file.similarity,
            is_binary: file.is_binary,
            is_submodule: file.is_submodule,
            is_mode_only: file.is_mode_only,
            synthetic: file.synthetic,
            metadata_rows: file.metadata_rows.clone(),
            hunks: file
                .hunks
                .iter()
                .enumerate()
                .map(|(hunk_index, hunk)| {
                    ReviewSnapshotHunk::from_hunk(hunk_index, hunk, &tokens, &emphasis)
                })
                .collect(),
        }
    }

    fn to_diff_file(&self) -> DiffFile {
        DiffFile {
            id: self.id.clone(),
            status: self.status.clone(),
            old_path: self.old_path.clone(),
            new_path: self.new_path.clone(),
            old_mode: self.old_mode.clone(),
            new_mode: self.new_mode.clone(),
            old_oid: self.old_oid.clone(),
            new_oid: self.new_oid.clone(),
            similarity: self.similarity,
            is_binary: self.is_binary,
            is_submodule: self.is_submodule,
            is_mode_only: self.is_mode_only,
            synthetic: self.synthetic,
            metadata_rows: self.metadata_rows.clone(),
            hunks: self
                .hunks
                .iter()
                .map(ReviewSnapshotHunk::to_review_hunk)
                .collect(),
        }
    }
}

impl ReviewSnapshotHunk {
    fn from_hunk(
        hunk_index: usize,
        hunk: &ReviewHunk,
        tokens: &HashMap<RowKey, Vec<TokenSpan>>,
        emphasis: &HashMap<RowKey, Vec<EmphSpan>>,
    ) -> Self {
        Self {
            id: hunk.id.clone(),
            header: hunk.header.clone(),
            old_start: hunk.old_start,
            old_lines: hunk.old_lines,
            new_start: hunk.new_start,
            new_lines: hunk.new_lines,
            rows: hunk
                .rows
                .iter()
                .enumerate()
                .map(|(row_index, row)| {
                    let key = (hunk_index, row_index);
                    ReviewSnapshotRow::from_row(
                        row,
                        tokens.get(&key).map(Vec::as_slice).unwrap_or_default(),
                        emphasis.get(&key).map(Vec::as_slice).unwrap_or_default(),
                    )
                })
                .collect(),
        }
    }

    fn to_review_hunk(&self) -> ReviewHunk {
        ReviewHunk {
            id: self.id.clone(),
            header: self.header.clone(),
            old_start: self.old_start,
            old_lines: self.old_lines,
            new_start: self.new_start,
            new_lines: self.new_lines,
            rows: self
                .rows
                .iter()
                .map(ReviewSnapshotRow::to_diff_row)
                .collect(),
        }
    }
}

impl ReviewSnapshotRow {
    fn from_row(row: &DiffRow, tokens: &[TokenSpan], emphasis: &[EmphSpan]) -> Self {
        Self {
            kind: row.kind.clone(),
            old_line: row.old_line,
            new_line: row.new_line,
            text: row.text.clone(),
            tokens: translate_tokens(&row.text, tokens).unwrap_or_default(),
            emphasis: translate_emphasis(&row.text, emphasis).unwrap_or_default(),
        }
    }

    fn to_diff_row(&self) -> DiffRow {
        DiffRow {
            kind: self.kind.clone(),
            old_line: self.old_line,
            new_line: self.new_line,
            text: self.text.clone(),
        }
    }
}

fn to_utf16_span(text: &str, start: usize, end: usize) -> Option<(usize, usize)> {
    if start > end
        || end > text.len()
        || !text.is_char_boundary(start)
        || !text.is_char_boundary(end)
    {
        return None;
    }
    Some((utf16_len(&text[..start]), utf16_len(&text[..end])))
}

fn translate_tokens(text: &str, spans: &[TokenSpan]) -> Option<Vec<ReviewTokenSpan>> {
    spans
        .iter()
        .map(|span| {
            let (start, end) = to_utf16_span(text, span.start, span.end)?;
            Some(ReviewTokenSpan {
                start,
                end,
                kind: span.kind.as_str().to_owned(),
            })
        })
        .collect()
}

fn translate_emphasis(text: &str, spans: &[EmphSpan]) -> Option<Vec<ReviewEmphasisSpan>> {
    spans
        .iter()
        .map(|span| {
            let (start, end) = to_utf16_span(text, span.start, span.end)?;
            Some(ReviewEmphasisSpan { start, end })
        })
        .collect()
}

fn utf16_len(text: &str) -> usize {
    text.chars().map(char::len_utf16).sum()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::highlight::{EmphSpan, TokenKind, TokenSpan};
    use crate::model::{DiffRow, DiffRowKind};

    fn row(text: &str) -> DiffRow {
        DiffRow {
            kind: DiffRowKind::Context,
            old_line: Some(1),
            new_line: Some(1),
            text: text.to_owned(),
        }
    }

    #[test]
    fn freshness_projection_stamp_is_additive_and_default_off_stays_exact() {
        assert_eq!(
            serde_json::to_string(&InspectFreshnessDocument::new(7, None)).unwrap(),
            r#"{"schema":"pointbreak.inspect-freshness","version":1,"eventCount":7}"#
        );
        assert_eq!(
            serde_json::to_value(
                InspectFreshnessDocument::new(7, None)
                    .with_projection_stamp("sha256:projection".to_owned())
            )
            .unwrap()["projectionStamp"],
            "sha256:projection"
        );
    }

    #[test]
    fn row_spans_are_utf16_and_channels_fail_independently() {
        let tokens = [TokenSpan {
            start: 3,
            end: 6,
            kind: TokenKind::Keyword,
        }];
        let invalid_emphasis = [EmphSpan { start: 1, end: 99 }];
        let value = serde_json::to_value(ReviewSnapshotRow::from_row(
            &row("é let"),
            &tokens,
            &invalid_emphasis,
        ))
        .unwrap();

        assert_eq!(value["tokens"][0]["start"], 2);
        assert_eq!(value["tokens"][0]["end"], 5);
        assert!(value.get("emphasis").is_none());
    }

    #[test]
    fn reversed_or_out_of_range_spans_are_omitted() {
        let reversed = [TokenSpan {
            start: 3,
            end: 0,
            kind: TokenKind::Keyword,
        }];
        let value =
            serde_json::to_value(ReviewSnapshotRow::from_row(&row("let x"), &reversed, &[]))
                .unwrap();

        assert!(value.get("tokens").is_none());
    }

    #[test]
    fn validated_artifact_and_exact_resource_parts_emit_identical_documents() {
        let artifact = build_object_artifact_v2(DiffSnapshot::new(
            ReviewId::new("review:exact-resource"),
            ObjectId::new("obj:sha256:exact-resource"),
            Vec::new(),
        ))
        .unwrap();

        assert_eq!(
            review_snapshot_document(&artifact),
            review_snapshot_document_from_snapshot(&artifact.content_hash, &artifact.snapshot)
        );
    }
}
