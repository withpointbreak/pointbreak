use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use super::codec::logical_key_digest;
use super::{
    NeverCancelled, PhysicalCodecError, PhysicalRecordKindV1, PhysicalRecordV1,
    QualificationCreateOutcome, QualificationEntry, QualificationInventoryV1,
    QualificationRecordKindV1,
};
use crate::canonical_hash::sha256_bytes_hex;

const CONTENT_KINDS: [QualificationRecordKindV1; 5] = [
    QualificationRecordKindV1::ObjectArtifact,
    QualificationRecordKindV1::NoteBody,
    QualificationRecordKindV1::RelationProof,
    QualificationRecordKindV1::DocumentManifest,
    QualificationRecordKindV1::DocumentBlob,
];

#[derive(Clone, Debug)]
pub struct IndependentContentStoreV1 {
    root: PathBuf,
}

impl IndependentContentStoreV1 {
    pub fn open(root: &Path) -> Result<Self, ContentCarrierError> {
        fs::create_dir_all(root).map_err(|error| ContentCarrierError::Io {
            path: root.to_path_buf(),
            message: error.to_string(),
        })?;
        Ok(Self {
            root: root.to_path_buf(),
        })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn put_once(
        &self,
        content_key: &str,
        record_kind: QualificationRecordKindV1,
        decoded_bytes: &[u8],
    ) -> Result<QualificationCreateOutcome, ContentCarrierError> {
        ensure_content_kind(record_kind)?;
        if content_key.is_empty() {
            return Err(ContentCarrierError::InvalidKey {
                message: "content key must not be empty".to_owned(),
            });
        }
        if let Some((existing_kind, path)) = self.find_existing(content_key)? {
            let existing = read_carrier(&path, content_key, existing_kind)?;
            return if existing_kind == record_kind && existing.decoded_bytes == decoded_bytes {
                Ok(QualificationCreateOutcome::AlreadyExists)
            } else {
                Err(ContentCarrierError::Conflict {
                    content_key: content_key.to_owned(),
                })
            };
        }

        let directory = self.kind_directory(record_kind);
        fs::create_dir_all(&directory).map_err(|error| ContentCarrierError::Io {
            path: directory.clone(),
            message: error.to_string(),
        })?;
        let path = directory.join(carrier_file_name(content_key));
        let encoded =
            PhysicalRecordV1::encode(content_key, record_kind, decoded_bytes, &NeverCancelled)?;
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .map_err(|error| ContentCarrierError::Io {
                path: path.clone(),
                message: error.to_string(),
            })?;
        file.write_all(&encoded)
            .and_then(|()| file.sync_all())
            .map_err(|error| ContentCarrierError::Io {
                path: path.clone(),
                message: error.to_string(),
            })?;
        Ok(QualificationCreateOutcome::Created)
    }

    pub fn read(
        &self,
        content_key: &str,
    ) -> Result<Option<QualificationEntry>, ContentCarrierError> {
        let Some((kind, path)) = self.find_existing(content_key)? else {
            return Ok(None);
        };
        read_carrier(&path, content_key, kind).map(Some)
    }

    pub fn list(&self) -> Result<Vec<QualificationEntry>, ContentCarrierError> {
        let mut entries = Vec::new();
        for kind in CONTENT_KINDS {
            let directory = self.kind_directory(kind);
            let directory_entries = match fs::read_dir(&directory) {
                Ok(entries) => entries,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
                Err(error) => {
                    return Err(ContentCarrierError::Io {
                        path: directory,
                        message: error.to_string(),
                    });
                }
            };
            for entry in directory_entries {
                let entry = entry.map_err(|error| ContentCarrierError::Io {
                    path: directory.clone(),
                    message: error.to_string(),
                })?;
                let file_type = entry.file_type().map_err(|error| ContentCarrierError::Io {
                    path: entry.path(),
                    message: error.to_string(),
                })?;
                if !file_type.is_file() {
                    return Err(ContentCarrierError::UnexpectedCarrier { path: entry.path() });
                }
                let content_key = content_key_from_file_name(&entry.file_name())?;
                entries.push(read_carrier(&entry.path(), &content_key, kind)?);
            }
        }
        entries.sort_by(|left, right| {
            left.logical_key
                .as_bytes()
                .cmp(right.logical_key.as_bytes())
        });
        for pair in entries.windows(2) {
            if pair[0].logical_key == pair[1].logical_key {
                return Err(ContentCarrierError::Conflict {
                    content_key: pair[0].logical_key.clone(),
                });
            }
        }
        Ok(entries)
    }

    pub fn remove(&self, content_key: &str) -> Result<bool, ContentCarrierError> {
        let Some((_kind, path)) = self.find_existing(content_key)? else {
            return Ok(false);
        };
        fs::remove_file(&path).map_err(|error| ContentCarrierError::Io {
            path,
            message: error.to_string(),
        })?;
        Ok(true)
    }

    pub fn inventory(&self) -> Result<QualificationInventoryV1, ContentCarrierError> {
        let mut carriers = Vec::new();
        let mut logical_bytes = 0_u64;
        let mut encoded_bytes = 0_u64;
        let mut allocated_bytes = 0_u64;
        for kind in CONTENT_KINDS {
            let directory = self.kind_directory(kind);
            let directory_entries = match fs::read_dir(&directory) {
                Ok(entries) => entries,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
                Err(error) => {
                    return Err(ContentCarrierError::Io {
                        path: directory,
                        message: error.to_string(),
                    });
                }
            };
            for entry in directory_entries {
                let entry = entry.map_err(|error| ContentCarrierError::Io {
                    path: directory.clone(),
                    message: error.to_string(),
                })?;
                let path = entry.path();
                let content_key = content_key_from_file_name(&entry.file_name())?;
                let decoded = read_carrier(&path, &content_key, kind)?;
                let metadata = fs::metadata(&path).map_err(|error| ContentCarrierError::Io {
                    path: path.clone(),
                    message: error.to_string(),
                })?;
                logical_bytes = logical_bytes
                    .checked_add(decoded.decoded_bytes.len() as u64)
                    .ok_or(ContentCarrierError::InventoryOverflow)?;
                encoded_bytes = encoded_bytes
                    .checked_add(metadata.len())
                    .ok_or(ContentCarrierError::InventoryOverflow)?;
                allocated_bytes = allocated_bytes
                    .checked_add(allocated_file_bytes(&metadata))
                    .ok_or(ContentCarrierError::InventoryOverflow)?;
                carriers.push(relative_path_string(&self.root, &path)?);
            }
        }
        carriers.sort_by(|left, right| left.as_bytes().cmp(right.as_bytes()));
        Ok(QualificationInventoryV1 {
            carriers,
            logical_bytes,
            encoded_bytes,
            allocated_bytes,
            high_water_bytes: allocated_bytes.max(encoded_bytes),
        })
    }

    fn kind_directory(&self, kind: QualificationRecordKindV1) -> PathBuf {
        self.root
            .join(PhysicalRecordKindV1::from_qualification(kind).directory_name())
    }

    fn find_existing(
        &self,
        content_key: &str,
    ) -> Result<Option<(QualificationRecordKindV1, PathBuf)>, ContentCarrierError> {
        let mut found = None;
        for kind in CONTENT_KINDS {
            let path = self
                .kind_directory(kind)
                .join(carrier_file_name(content_key));
            if path.try_exists().map_err(|error| ContentCarrierError::Io {
                path: path.clone(),
                message: error.to_string(),
            })? {
                if found.is_some() {
                    return Err(ContentCarrierError::Conflict {
                        content_key: content_key.to_owned(),
                    });
                }
                found = Some((kind, path));
            }
        }
        Ok(found)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ContentCarrierError {
    #[error("unsupported content record kind {record_kind:?}")]
    UnsupportedRecordKind {
        record_kind: QualificationRecordKindV1,
    },
    #[error("invalid content key: {message}")]
    InvalidKey { message: String },
    #[error("content key {content_key} already exists with different bytes or kind")]
    Conflict { content_key: String },
    #[error("unexpected content carrier {path}", path = .path.display())]
    UnexpectedCarrier { path: PathBuf },
    #[error("content carrier inventory overflow")]
    InventoryOverflow,
    #[error("content carrier I/O failed at {path}: {message}", path = .path.display())]
    Io { path: PathBuf, message: String },
    #[error(transparent)]
    Codec(#[from] PhysicalCodecError),
}

fn ensure_content_kind(kind: QualificationRecordKindV1) -> Result<(), ContentCarrierError> {
    if CONTENT_KINDS.contains(&kind) {
        Ok(())
    } else {
        Err(ContentCarrierError::UnsupportedRecordKind { record_kind: kind })
    }
}

fn read_carrier(
    path: &Path,
    content_key: &str,
    expected_kind: QualificationRecordKindV1,
) -> Result<QualificationEntry, ContentCarrierError> {
    let bytes = fs::read(path).map_err(|error| ContentCarrierError::Io {
        path: path.to_path_buf(),
        message: error.to_string(),
    })?;
    let decoded = PhysicalRecordV1::decode(&bytes, &NeverCancelled)?;
    if decoded.record_kind != PhysicalRecordKindV1::from_qualification(expected_kind) {
        return Err(ContentCarrierError::UnexpectedCarrier {
            path: path.to_path_buf(),
        });
    }
    if logical_key_digest(content_key) != decoded.logical_key_digest {
        return Err(ContentCarrierError::UnexpectedCarrier {
            path: path.to_path_buf(),
        });
    }
    Ok(QualificationEntry {
        logical_key: content_key.to_owned(),
        decoded_sha256: sha256_bytes_hex(&decoded.decoded_bytes),
        decoded_bytes: decoded.decoded_bytes,
    })
}

fn carrier_file_name(content_key: &str) -> String {
    let mut output = String::with_capacity(content_key.len() * 2 + 5);
    for byte in content_key.as_bytes() {
        use std::fmt::Write as _;
        write!(output, "{byte:02x}").expect("write to String");
    }
    output.push_str(".pbrf");
    output
}

fn content_key_from_file_name(name: &std::ffi::OsStr) -> Result<String, ContentCarrierError> {
    let name = name
        .to_str()
        .ok_or_else(|| ContentCarrierError::InvalidKey {
            message: "carrier filename is not UTF-8".to_owned(),
        })?;
    let hex = name
        .strip_suffix(".pbrf")
        .ok_or_else(|| ContentCarrierError::InvalidKey {
            message: format!("carrier filename {name} lacks .pbrf suffix"),
        })?;
    if hex.len() % 2 != 0 {
        return Err(ContentCarrierError::InvalidKey {
            message: format!("carrier filename {name} has odd hex length"),
        });
    }
    let bytes = hex
        .as_bytes()
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| {
            std::str::from_utf8(pair)
                .ok()
                .and_then(|value| u8::from_str_radix(value, 16).ok())
                .ok_or_else(|| ContentCarrierError::InvalidKey {
                    message: format!("carrier filename {name} contains invalid hex"),
                })
        })
        .collect::<Result<Vec<_>, _>>()?;
    String::from_utf8(bytes).map_err(|error| ContentCarrierError::InvalidKey {
        message: error.to_string(),
    })
}

fn relative_path_string(root: &Path, path: &Path) -> Result<String, ContentCarrierError> {
    let relative = path
        .strip_prefix(root)
        .map_err(|error| ContentCarrierError::InvalidKey {
            message: error.to_string(),
        })?;
    Ok(relative
        .components()
        .map(|component| component.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/"))
}

#[cfg(unix)]
fn allocated_file_bytes(metadata: &fs::Metadata) -> u64 {
    use std::os::unix::fs::MetadataExt;
    metadata.blocks().saturating_mul(512)
}

#[cfg(not(unix))]
fn allocated_file_bytes(metadata: &fs::Metadata) -> u64 {
    metadata.len()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn content_kinds() -> [QualificationRecordKindV1; 5] {
        [
            QualificationRecordKindV1::ObjectArtifact,
            QualificationRecordKindV1::NoteBody,
            QualificationRecordKindV1::RelationProof,
            QualificationRecordKindV1::DocumentManifest,
            QualificationRecordKindV1::DocumentBlob,
        ]
    }

    #[test]
    fn independent_content_carriers_create_read_list_remove_and_rescan() {
        let temporary = tempfile::tempdir().expect("tempdir");
        let store = IndependentContentStoreV1::open(temporary.path()).expect("content store");

        for (index, kind) in content_kinds().into_iter().enumerate() {
            let key = format!("sha256:{index:064x}");
            let bytes = format!("content-{index}").into_bytes();
            assert_eq!(
                store.put_once(&key, kind, &bytes).expect("create"),
                QualificationCreateOutcome::Created
            );
            assert_eq!(
                store.put_once(&key, kind, &bytes).expect("idempotent"),
                QualificationCreateOutcome::AlreadyExists
            );
            assert_eq!(
                store
                    .read(&key)
                    .expect("read")
                    .expect("entry")
                    .decoded_bytes,
                bytes
            );
        }

        let listed = store.list().expect("list");
        assert_eq!(listed.len(), 5);
        assert!(
            listed
                .windows(2)
                .all(|entries| entries[0].logical_key < entries[1].logical_key)
        );

        let proof_key = format!("sha256:{:064x}", 2);
        let modeled_attestation_still_exists = true;
        assert!(store.remove(&proof_key).expect("remove"));
        assert!(store.read(&proof_key).expect("read removed").is_none());
        assert!(modeled_attestation_still_exists);

        let reopened = IndependentContentStoreV1::open(temporary.path()).expect("rescan");
        assert_eq!(reopened.list().expect("rescan list").len(), 4);
        let inventory = reopened.inventory().expect("inventory");
        assert_eq!(inventory.carriers.len(), 4);
        assert!(inventory.encoded_bytes >= inventory.logical_bytes);
        assert!(inventory.high_water_bytes >= inventory.encoded_bytes);
    }

    #[test]
    fn independent_content_rejects_event_kinds_and_conflicts() {
        let temporary = tempfile::tempdir().expect("tempdir");
        let store = IndependentContentStoreV1::open(temporary.path()).expect("content store");

        assert!(matches!(
            store.put_once(
                "events/one",
                QualificationRecordKindV1::LegacyEvent,
                b"event"
            ),
            Err(ContentCarrierError::UnsupportedRecordKind { .. })
        ));

        let key = format!("sha256:{:064x}", 9);
        store
            .put_once(&key, QualificationRecordKindV1::NoteBody, b"first")
            .expect("first create");
        assert!(matches!(
            store.put_once(&key, QualificationRecordKindV1::NoteBody, b"different"),
            Err(ContentCarrierError::Conflict { .. })
        ));
    }
}
