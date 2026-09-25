use super::*;
use std::fs;

use rusqlite::{Connection, OpenFlags, params};
use serde_json::{Value, json};

use super::canonical::{Key, Record, content_fingerprint, fingerprint, records};
use crate::ir::{Fingerprint, WORKSPACE_SCHEMA_VERSION};
use crate::provenance::fingerprint_bytes;

#[cfg(test)]
mod tests;

pub(super) fn publish(snapshot: &Snapshot, path: &Path) -> Result<(), SnapshotError> {
    ensure_no_sidecars(path)?;
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    fs::create_dir_all(parent)?;
    let temporary = tempfile::Builder::new()
        .prefix(".diplodocus-snapshot-")
        .suffix(".sqlite")
        .tempfile_in(parent)?;
    let mut connection = Connection::open_with_flags(
        temporary.path(),
        OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    connection.execute_batch(
        "PRAGMA journal_mode = DELETE; PRAGMA synchronous = FULL; PRAGMA trusted_schema = OFF;",
    )?;
    let transaction = connection.transaction()?;
    transaction.execute_batch(include_str!("schema.sql"))?;
    let records = records(snapshot)?;
    transaction.execute(
        "INSERT INTO manifest VALUES (1, ?1, ?2, ?3, ?4, ?5)",
        params![
            STORAGE_SCHEMA_VERSION,
            WORKSPACE_SCHEMA_VERSION,
            RECORD_ENCODING_VERSION,
            snapshot.producer,
            content_fingerprint(snapshot, &records)?
        ],
    )?;
    {
        let mut insert = transaction.prepare("INSERT INTO records VALUES (?1, ?2, ?3, ?4, ?5)")?;
        for record in records {
            insert.execute(params![
                record.key.kind,
                record.key.owner,
                record.key.id,
                serde_json::to_string(&record.content)?,
                record.fingerprint
            ])?;
        }
        let mut insert = transaction.prepare("INSERT INTO assets VALUES (?1, ?2, ?3)")?;
        for (digest, asset) in &snapshot.assets {
            insert.execute(params![digest, asset.media_type, asset.bytes])?;
        }
    }
    transaction.commit()?;
    connection.close().map_err(|(_, error)| error)?;
    temporary.as_file().sync_all()?;
    // Close and validate the standalone artifact before the publication point.
    ensure_no_sidecars(temporary.path())?;
    load(temporary.path())?;
    ensure_no_sidecars(path)?;
    temporary.persist(path).map_err(|error| error.error)?;
    Ok(())
}

fn ensure_no_sidecars(path: &Path) -> Result<(), SnapshotError> {
    // SQLite names recovery files after the destination. Leaving one beside a
    // replacement could apply the previous database's state to the new file.
    for suffix in ["-journal", "-wal", "-shm"] {
        let mut companion = path.as_os_str().to_owned();
        companion.push(suffix);
        match fs::symlink_metadata(&companion) {
            Ok(_) => {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::AlreadyExists,
                    format!(
                        "SQLite sidecar exists at {}; close SQLite writers and resolve their recovery files before publishing",
                        Path::new(&companion).display()
                    ),
                )
                .into());
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
    }
    Ok(())
}

pub(super) fn load(path: &Path) -> Result<Snapshot, SnapshotError> {
    use std::io::Read;
    let mut header = [0_u8; 20];
    fs::File::open(path)?.read_exact(&mut header)?;
    if &header[..16] != b"SQLite format 3\0" || header[18..] != [1, 1] {
        return Err(SnapshotError::Invalid(
            "standalone rollback-journal database required",
        ));
    }
    let connection = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    connection.execute_batch("PRAGMA query_only = ON; PRAGMA trusted_schema = OFF;")?;
    let count: u32 = connection.query_row("SELECT count(*) FROM manifest", [], |r| r.get(0))?;
    if count != 1 {
        return Err(SnapshotError::Invalid("manifest count"));
    }
    let (storage, ir, encoding, producer, expected): (u32, u32, u32, String, String) = connection.query_row(
        "SELECT storage_version, ir_version, encoding_version, producer, content_fingerprint FROM manifest WHERE id = 1", [],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?))
    )?;
    if (storage, ir, encoding)
        != (
            STORAGE_SCHEMA_VERSION,
            WORKSPACE_SCHEMA_VERSION,
            RECORD_ENCODING_VERSION,
        )
    {
        return Err(SnapshotError::Version);
    }
    let mut statement = connection.prepare(
        "SELECT kind, owner, id, content, fingerprint FROM records ORDER BY kind, owner, id",
    )?;
    let raw = statement.query_map([], |r| {
        Ok((
            Key {
                kind: r.get(0)?,
                owner: r.get(1)?,
                id: r.get(2)?,
            },
            r.get::<_, String>(3)?,
            r.get::<_, String>(4)?,
        ))
    })?;
    let mut records = Vec::new();
    for row in raw {
        let (key, text, digest) = row?;
        let content = serde_json::from_str(&text)?;
        if fingerprint(&key, &content)? != digest {
            return Err(SnapshotError::Invalid("record fingerprint"));
        }
        records.push(Record {
            key,
            content,
            fingerprint: digest,
        });
    }
    let mut assets = BTreeMap::new();
    let mut statement =
        connection.prepare("SELECT digest, media_type, bytes FROM assets ORDER BY digest")?;
    for row in statement.query_map([], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, Vec<u8>>(2)?,
        ))
    })? {
        let (digest, media_type, bytes) = row?;
        if fingerprint_bytes(&bytes).value != digest {
            return Err(SnapshotError::Invalid("asset fingerprint"));
        }
        if assets
            .insert(
                digest.clone(),
                ContentAsset {
                    fingerprint: Fingerprint {
                        algorithm: "sha256".into(),
                        value: digest,
                    },
                    media_type,
                    bytes,
                },
            )
            .is_some()
        {
            return Err(SnapshotError::Invalid("duplicate asset"));
        }
    }
    let mut snapshot = decode(&records, assets, producer)?;
    if content_fingerprint(&snapshot, &records)? != expected {
        return Err(SnapshotError::Invalid("snapshot fingerprint"));
    }
    snapshot.validated_outputs = snapshot.validate()?;
    if self::records(&snapshot)? != records {
        return Err(SnapshotError::Invalid("noncanonical record set"));
    }
    Ok(snapshot)
}

fn decode(
    records: &[Record],
    assets: BTreeMap<String, ContentAsset>,
    producer: String,
) -> Result<Snapshot, SnapshotError> {
    let mut workspace = None;
    let mut presentation = None;
    let mut entities: BTreeMap<&str, serde_json::Map<String, Value>> = BTreeMap::new();
    let mut nested = Vec::new();
    let mut documents = Vec::new();
    let mut executions = BTreeMap::new();
    for record in records {
        let Key { kind, owner, id } = &record.key;
        let field = match kind.as_str() {
            "presentation" if owner.is_empty() && id.is_empty() && presentation.is_none() => {
                presentation = Some(serde_json::from_value(record.content.clone())?);
                continue;
            }
            "workspace" if owner.is_empty() && id.is_empty() && workspace.is_none() => {
                workspace = Some(record.content.clone());
                continue;
            }
            "repository" => "repositories",
            "package" => "packages",
            "collection" => "content_collections",
            "page" => "pages",
            "concept" => "concepts",
            "item" | "target" if !owner.is_empty() && !id.is_empty() => {
                nested.push(record);
                continue;
            }
            "execution" if owner.is_empty() && !id.is_empty() => {
                if executions
                    .insert(id.clone(), serde_json::from_value(record.content.clone())?)
                    .is_some()
                {
                    return Err(SnapshotError::Invalid("duplicate executed page"));
                }
                continue;
            }
            "document" if owner.is_empty() => {
                let document: ResolvedDocument = serde_json::from_value(record.content.clone())?;
                if serde_json::to_string(&document.document)? != *id {
                    return Err(SnapshotError::Invalid("document identity"));
                }
                documents.push(document);
                continue;
            }
            _ => return Err(SnapshotError::Invalid("record kind or key")),
        };
        if !owner.is_empty()
            || id.is_empty()
            || entities
                .entry(field)
                .or_default()
                .insert(id.clone(), record.content.clone())
                .is_some()
        {
            return Err(SnapshotError::Invalid("entity identity"));
        }
    }
    let mut workspace = workspace.ok_or(SnapshotError::Invalid("missing workspace"))?;
    let object = workspace
        .as_object_mut()
        .ok_or(SnapshotError::Invalid("workspace shape"))?;
    for field in [
        "repositories",
        "packages",
        "content_collections",
        "pages",
        "concepts",
    ] {
        if object
            .insert(
                field.into(),
                Value::Object(entities.remove(field).unwrap_or_default()),
            )
            .is_some()
        {
            return Err(SnapshotError::Invalid("duplicate entity map"));
        }
    }
    let packages = object.get_mut("packages").unwrap().as_object_mut().unwrap();
    for value in packages.values_mut() {
        let package = value
            .as_object_mut()
            .ok_or(SnapshotError::Invalid("package shape"))?;
        for field in ["items", "extraction_targets"] {
            if package.insert(field.into(), json!({})).is_some() {
                return Err(SnapshotError::Invalid("duplicate package map"));
            }
        }
    }
    for record in nested {
        let field = if record.key.kind == "item" {
            "items"
        } else {
            "extraction_targets"
        };
        let entries = packages
            .get_mut(&record.key.owner)
            .and_then(|p| p.get_mut(field))
            .and_then(Value::as_object_mut)
            .ok_or(SnapshotError::Invalid("missing owning package"))?;
        if entries
            .insert(record.key.id.clone(), record.content.clone())
            .is_some()
        {
            return Err(SnapshotError::Invalid("duplicate item or target"));
        }
    }
    documents.sort_by(|a, b| a.document.cmp(&b.document));
    Ok(Snapshot {
        workspace: serde_json::from_value(workspace)?,
        presentation: presentation.ok_or(SnapshotError::Invalid("missing presentation"))?,
        documents,
        assets,
        producer,
        executions,
        validated_outputs: BTreeMap::new(),
    })
}
