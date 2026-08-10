//! One SQLite-WAL bodyless implementation shared by product and qualification callers.

use std::path::Path;

mod cursor;
mod locator;
mod semantic;
mod writer_lock;

pub(crate) use cursor::*;
pub(crate) use locator::*;
pub(crate) use semantic::*;
pub(crate) use writer_lock::*;

pub(crate) fn sqlite_companion_exists(path: &Path) -> bool {
    ["-wal", "-shm"].into_iter().any(|suffix| {
        let mut companion = path.as_os_str().to_owned();
        companion.push(suffix);
        Path::new(&companion).exists()
    })
}

#[cfg(unix)]
pub(super) fn immutable_read_only_open_is_safe(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;

    if sqlite_companion_exists(path) {
        return false;
    }
    let has_no_write_bits = |candidate: &Path| {
        std::fs::metadata(candidate)
            .map(|metadata| metadata.permissions().mode() & 0o222 == 0)
            .unwrap_or(false)
    };
    has_no_write_bits(path) && path.parent().is_some_and(has_no_write_bits)
}

#[cfg(windows)]
pub(super) fn immutable_read_only_open_is_safe(path: &Path) -> bool {
    // A Windows directory's read-only attribute does not express its ACL and
    // therefore cannot prove that companions cannot be created. The main
    // database's read-only attribute is the relevant cold-generation proof: a
    // new SQLite writer cannot open it read-write, while absent companions prove
    // there is no live WAL state for immutable mode to hide.
    !sqlite_companion_exists(path)
        && std::fs::metadata(path)
            .map(|metadata| metadata.permissions().readonly())
            .unwrap_or(false)
}

#[cfg(not(any(unix, windows)))]
pub(super) fn immutable_read_only_open_is_safe(_path: &Path) -> bool {
    false
}

pub(crate) fn sqlite_immutable_read_only_uri(path: &Path) -> String {
    let mut uri = String::from("file:");
    for byte in path.as_os_str().as_encoded_bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'/' | b':' | b'-' | b'_' | b'.' | b'~') {
            uri.push(char::from(*byte));
        } else {
            use std::fmt::Write as _;
            write!(&mut uri, "%{byte:02X}").expect("writing to a String cannot fail");
        }
    }
    uri.push_str("?mode=ro&immutable=1");
    uri
}

#[cfg(all(test, windows))]
mod windows_tests {
    use std::fs;

    use rusqlite::{Connection, OpenFlags};
    use tempfile::TempDir;

    use super::*;

    #[test]
    fn cold_read_only_database_selects_immutable_without_creating_wal_companions() {
        let temp = TempDir::new().unwrap();
        let database = temp.path().join("cursor.sqlite3");
        let connection = Connection::open(&database).unwrap();
        connection
            .execute_batch(
                "PRAGMA journal_mode=WAL;
                 CREATE TABLE value (id INTEGER PRIMARY KEY, body TEXT NOT NULL) STRICT;
                 INSERT INTO value (id, body) VALUES (1, 'published');
                 PRAGMA wal_checkpoint(TRUNCATE);",
            )
            .unwrap();
        drop(connection);
        for suffix in ["-wal", "-shm"] {
            let mut companion = database.as_os_str().to_owned();
            companion.push(suffix);
            let companion = Path::new(&companion);
            if companion.exists() {
                fs::remove_file(companion).unwrap();
            }
        }

        let original = fs::metadata(&database).unwrap().permissions();
        assert!(!immutable_read_only_open_is_safe(&database));
        let mut read_only = original.clone();
        read_only.set_readonly(true);
        fs::set_permissions(&database, read_only).unwrap();

        let wal = database.with_extension("sqlite3-wal");
        fs::write(&wal, []).unwrap();
        assert!(!immutable_read_only_open_is_safe(&database));
        fs::remove_file(&wal).unwrap();
        assert!(immutable_read_only_open_is_safe(&database));
        let opened = Connection::open_with_flags(
            sqlite_immutable_read_only_uri(&database),
            OpenFlags::SQLITE_OPEN_READ_ONLY
                | OpenFlags::SQLITE_OPEN_NO_MUTEX
                | OpenFlags::SQLITE_OPEN_URI,
        )
        .unwrap();
        assert_eq!(
            opened
                .query_row("SELECT body FROM value WHERE id = 1", [], |row| {
                    row.get::<_, String>(0)
                })
                .unwrap(),
            "published"
        );
        drop(opened);
        assert!(!sqlite_companion_exists(&database));

        fs::set_permissions(&database, original).unwrap();
        assert!(!immutable_read_only_open_is_safe(&database));
    }
}
