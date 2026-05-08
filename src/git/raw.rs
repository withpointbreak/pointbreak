use crate::error::{Result, ShoreError};
use crate::model::FileStatus;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct RawFile {
    pub status: FileStatus,
    pub old_path: Option<String>,
    pub new_path: Option<String>,
}

impl RawFile {
    pub fn key(&self) -> String {
        self.new_path
            .as_ref()
            .or(self.old_path.as_ref())
            .expect("raw file has at least one path")
            .clone()
    }
}

pub(crate) fn parse_raw(raw: &[u8]) -> Result<Vec<RawFile>> {
    let fields = raw
        .split(|byte| *byte == 0)
        .filter(|field| !field.is_empty())
        .map(|field| {
            std::str::from_utf8(field)
                .map(str::to_owned)
                .map_err(|error| {
                    ShoreError::Message(format!("git raw output is not utf-8: {error}"))
                })
        })
        .collect::<Result<Vec<_>>>()?;

    let mut files = Vec::new();
    let mut index = 0;
    while index < fields.len() {
        let header = &fields[index];
        index += 1;
        let path = fields
            .get(index)
            .ok_or_else(|| ShoreError::Message(format!("missing path for raw header {header}")))?
            .clone();
        index += 1;

        let status = header
            .split_whitespace()
            .last()
            .ok_or_else(|| ShoreError::Message(format!("missing status in raw header {header}")))?;
        let status = match status.chars().next() {
            Some('M') => FileStatus::Modified,
            Some('A') => FileStatus::Added,
            Some('D') => FileStatus::Deleted,
            _ => {
                return Err(ShoreError::Message(format!(
                    "unsupported raw status {status} for {path}"
                )));
            }
        };

        let (old_path, new_path) = match status {
            FileStatus::Modified => (Some(path.clone()), Some(path)),
            FileStatus::Added => (None, Some(path)),
            FileStatus::Deleted => (Some(path), None),
        };
        files.push(RawFile {
            status,
            old_path,
            new_path,
        });
    }

    Ok(files)
}
