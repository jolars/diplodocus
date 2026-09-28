//! Identify storage artifacts before selecting source files or watch inputs.

use std::fs::File;
use std::io::Read;
use std::path::Path;

use rusqlite::{Connection, OpenFlags};

#[cfg(test)]
mod tests;

pub(crate) const SNAPSHOT_TEMP_PREFIX: &str = ".diplodocus-snapshot-";
pub(crate) const SNAPSHOT_APPLICATION_ID: i32 = i32::from_be_bytes(*b"DIPL");

pub(crate) fn is_reserved(path: &Path) -> bool {
    path.components().any(|part| {
        part.as_os_str()
            .to_str()
            .is_some_and(|name| name == ".diplodocus" || name.starts_with(SNAPSHOT_TEMP_PREFIX))
    })
}

pub(crate) fn is_generated_file(path: &Path) -> bool {
    if is_reserved(path) || is_snapshot(path) {
        return true;
    }
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| {
            ["-journal", "-wal", "-shm"].iter().any(|suffix| {
                name.strip_suffix(suffix)
                    .is_some_and(|base| is_snapshot(&path.with_file_name(base)))
            })
        })
}

/// Inspect only regular files after the caller has checked its source boundary.
/// Unrecognized or unreadable files remain inputs so normal diagnostics survive.
fn is_snapshot(path: &Path) -> bool {
    if !std::fs::symlink_metadata(path).is_ok_and(|metadata| metadata.is_file()) {
        return false;
    }
    let mut header = [0; 72];
    if File::open(path)
        .and_then(|mut file| file.read_exact(&mut header))
        .is_err()
        || &header[..16] != b"SQLite format 3\0"
    {
        return false;
    }
    if header[68..72] == SNAPSHOT_APPLICATION_ID.to_be_bytes() {
        return true;
    }
    if header[68..72] != [0; 4] {
        return false;
    }

    // Older snapshots have no application ID. Inspect their schema without
    // recovering journals, opening WAL files, or creating storage during check.
    let Ok(mut uri) = url::Url::from_file_path(path) else {
        return false;
    };
    uri.query_pairs_mut().append_pair("immutable", "1");
    let Ok(connection) = Connection::open_with_flags(
        uri.as_str(),
        OpenFlags::SQLITE_OPEN_READ_ONLY
            | OpenFlags::SQLITE_OPEN_URI
            | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    ) else {
        return false;
    };
    if connection
        .execute_batch("PRAGMA query_only = ON; PRAGMA trusted_schema = OFF;")
        .is_err()
    {
        return false;
    }
    connection
        .prepare(
            "SELECT m.storage_version, m.ir_version, m.encoding_version, m.producer,
                    m.content_fingerprint, r.kind, r.owner, r.id, r.content,
                    r.fingerprint, a.digest, a.media_type, a.bytes
             FROM manifest AS m, records AS r, assets AS a WHERE m.id = 1 LIMIT 0",
        )
        .is_ok()
}
