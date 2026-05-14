use crate::error::{Result, ShoreError};
use crate::model::TrackId;
use crate::session::body_artifact::{BodyArtifactOutcome, stage_body_artifact};

pub(crate) fn required_title(title: Option<&str>) -> Result<String> {
    let title = title.unwrap_or_default().trim();
    if title.is_empty() {
        return Err(ShoreError::Message("title is required".to_owned()));
    }
    Ok(title.to_owned())
}

pub(crate) type StagedBody = (Option<String>, Option<String>, Option<Vec<u8>>, Option<u64>);

pub(crate) fn staged_body(body: Option<&str>) -> Result<StagedBody> {
    match body {
        Some(body) => match stage_body_artifact(body.as_bytes())? {
            BodyArtifactOutcome::Inline { byte_size } => {
                Ok((Some(body.to_owned()), None, None, Some(byte_size)))
            }
            BodyArtifactOutcome::Artifact {
                relative_path,
                byte_size,
                body_envelope,
            } => Ok((
                None,
                Some(relative_path),
                Some(body_envelope.to_json_bytes()?),
                Some(byte_size),
            )),
        },
        None => Ok((None, None, None, None)),
    }
}

pub(crate) fn validated_track_id(value: &str) -> Result<TrackId> {
    let value = value.trim();
    if value.is_empty() {
        return Err(invalid_track_id("track id cannot be empty"));
    }
    if value.len() > 128 {
        return Err(invalid_track_id("track id must be 128 bytes or fewer"));
    }
    if matches!(value, "all" | "none" | "null" | "default" | "*") {
        return Err(invalid_track_id("track id is reserved"));
    }
    if value.starts_with("system:") || value.starts_with("import:") {
        return Err(invalid_track_id("track namespace is reserved"));
    }
    if !value.bytes().all(|byte| {
        byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'-' | b':')
    }) {
        return Err(invalid_track_id(
            "track id may only contain lowercase ASCII letters, digits, '-' and ':'",
        ));
    }

    Ok(TrackId::new(value.to_owned()))
}

fn invalid_track_id(message: &str) -> ShoreError {
    ShoreError::Message(message.to_owned())
}
