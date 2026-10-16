//! Private C7 binding; it supplements, never replaces, the full authority cursor.
use serde::{Deserialize, Serialize};

use crate::canonical_hash::canonical_json_bytes;
#[cfg(target_os = "macos")]
use crate::canonical_hash::sha256_json_hex;
use crate::error::{Result, ShoreError};
use crate::session::store::backend::JournalChangeStamp;
#[cfg(target_os = "macos")]
use crate::session::store::capabilities::AuthorityCursorV2;

/// Shared derived-access loops also run on platforms without an owned process.
/// Preserve the native cancellation checkpoint where that owner exists.
pub(crate) fn data_work_checkpoint() -> Result<()> {
    #[cfg(any(target_os = "macos", windows))]
    {
        crate::session::append_progress::data_work_checkpoint()
    }
    #[cfg(not(any(target_os = "macos", windows)))]
    {
        Ok(())
    }
}

const SCHEMA: &str = "pointbreak.append-progress-projection-binding";

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ProgressProjectionBindingV1 {
    schema: String,
    version: u32,
    selection_sha256: String,
    baseline_sha256: String,
    capability_pair_sha256: String,
    progress_epoch: String,
    applied_head: u64,
    applied_prefix_sha256: String,
    native_stamp: JournalChangeStamp,
    generation_id: String,
    generation_epoch: u64,
    authority_cursor_sha256: String,
}

fn refused() -> ShoreError {
    ShoreError::Message(
        "enrolled projection binding is invalid or stale; explicit rebuild required".into(),
    )
}

impl ProgressProjectionBindingV1 {
    #[cfg(target_os = "macos")]
    pub(crate) fn from_observation(
        observed: &crate::session::append_progress::ProgressReadFence,
        generation_id: &str,
        generation_epoch: u64,
        authority_cursor: &AuthorityCursorV2,
    ) -> Result<Self> {
        let binding = Self {
            schema: SCHEMA.into(),
            version: 1,
            selection_sha256: observed.selection_sha256().into(),
            baseline_sha256: observed.baseline_sha256().into(),
            capability_pair_sha256: observed.capability_pair_sha256().into(),
            progress_epoch: observed.progress_epoch().into(),
            applied_head: observed.head(),
            applied_prefix_sha256: observed.prefix_sha256().into(),
            native_stamp: observed.native_stamp().clone(),
            generation_id: generation_id.into(),
            generation_epoch,
            authority_cursor_sha256: sha256_json_hex(&serde_json::to_value(authority_cursor)?)?,
        };
        binding.validate_shape()?;
        Ok(binding)
    }

    pub(crate) fn decode(bytes: &str) -> Result<Self> {
        let binding: Self = serde_json::from_str(bytes).map_err(|_| refused())?;
        binding.validate_shape()?;
        if binding.canonical_json()?.as_bytes() != bytes.as_bytes() {
            return Err(refused());
        }
        Ok(binding)
    }

    pub(crate) fn canonical_json(&self) -> Result<String> {
        self.validate_shape()?;
        String::from_utf8(canonical_json_bytes(&serde_json::to_value(self)?)?)
            .map_err(|_| refused())
    }

    fn validate_shape(&self) -> Result<()> {
        let hash = |s: &str| {
            s.len() == 64
                && s.bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        };
        if self.schema != SCHEMA
            || self.version != 1
            || self.generation_epoch == 0
            || self.generation_id.is_empty()
            || !self
                .generation_id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-')
            || ![
                &self.selection_sha256,
                &self.baseline_sha256,
                &self.capability_pair_sha256,
                &self.progress_epoch,
                &self.applied_prefix_sha256,
                &self.authority_cursor_sha256,
            ]
            .into_iter()
            .all(|s| hash(s))
            || !matches!(self.native_stamp, JournalChangeStamp::Observed { .. })
        {
            return Err(refused());
        }
        Ok(())
    }

    #[cfg(target_os = "macos")]
    pub(crate) fn validate(
        &self,
        observed: &crate::session::append_progress::ProgressReadFence,
        generation_id: &str,
        generation_epoch: u64,
        authority_cursor: &AuthorityCursorV2,
    ) -> Result<()> {
        if self
            != &Self::from_observation(observed, generation_id, generation_epoch, authority_cursor)?
        {
            return Err(refused());
        }
        Ok(())
    }
}
