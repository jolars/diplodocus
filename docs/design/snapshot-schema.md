# SQLite snapshot schema 1

A snapshot contains portable workspace IR version 1 and uses canonical record
encoding version 1. These version numbers are independent. Readers reject an
unsupported version rather than migrating it. `extract` replaces the complete
snapshot, so deleted entities and unused assets disappear on the next refresh.

The standalone database uses SQLite's rollback journal mode. Publication writes
a temporary sibling database in one transaction, closes it, validates the
completed artifact, and replaces the destination with a rename. Failed
extraction, validation, or writing preserves the previous database. Generation
opens the database read-only and requires no source files, execution cache, or
language runtime.

## Tables

| Table | Key | Fields |
| --- | --- | --- |
| `manifest` | `id = 1` | `storage_version`, `ir_version`, `encoding_version`, `producer`, `content_fingerprint` |
| `records` | `(kind, owner, id)` | `content` (UTF-8 JSON), `fingerprint` (lowercase SHA-256) |
| `assets` | `digest` (lowercase SHA-256) | `media_type`, `bytes` (BLOB) |

The `records` kinds are:

| Kind | Owner | ID | Content |
| --- | --- | --- | --- |
| `workspace` | Empty | Empty | Workspace fields other than the entity maps. Includes name, IR version, relationships, diagnostics, and provenance. |
| `repository` | Empty | Repository ID | Portable repository metadata, including revision and source-link information. |
| `package` | Empty | Package ID | Package record without the item and extraction-target maps. Includes slug, version, visibility, and repository-relative paths. |
| `target` | Package ID | Target ID | Extraction-target record. |
| `item` | Package ID | Semantic item ID | Item, signatures, documentation, language data, children, and provenance. |
| `collection` | Empty | Collection ID | Authored profile, owner, mount, and declared execution policy. |
| `page` | Empty | Semantic page ID | Page title, structured document, retained source, output records, and provenance. |
| `concept` | Empty | Concept ID | Kind, member identities, and optional documentation. |
| `document` | Empty | Serialized `DocumentIdentity` | Collection-relative path, authored anchors, and resolved references in traversal order. |
| `execution` | Empty | Page ID | Complete page execution record, typed diagnostic ledger and offset, and final output slots with their producer attribution and all accepted representations. |

Nested values use the versioned Rust IR's Serde field shapes. Execution slot
representations use a `kind` discriminator: `text`, `markdown`, `html`, or
`asset`. Markdown stores the inert fragment tree and exact producer/fragment
origin. HTML stores the canonical markup candidate. Assets store portable image
metadata. These values carry no trust when decoded. The loader binds source
preparation, document projection, diagnostic associations, and representations
again using the shared execution validators. It verifies every image from the
database's bytes and revalidates every Markdown and HTML alternative, including
hidden output and unselected alternatives, without writing staging files.

For example, query all package names and versions:

```sql
SELECT id,
       json_extract(content, '$.name') AS name,
       json_extract(content, '$.version') AS version
FROM records WHERE kind = 'package' ORDER BY id;
```

Query an API item by its package and semantic identity:

```sql
SELECT content, fingerprint
FROM records WHERE kind = 'item' AND owner = ?1 AND id = ?2;
```

Documents retain semantic reference targets rather than site URLs:

```sql
SELECT id, json_extract(content, '$.references') AS references
FROM records WHERE kind = 'document' ORDER BY id;

SELECT digest, media_type, length(bytes) AS byte_size
FROM assets ORDER BY digest;
```

## Canonical encodings

A record fingerprint hashes the compact UTF-8 JSON encoding of
`["diplodocus/snapshot-record-v1", key, content]`. The key object contains `kind`,
`owner`, and `id`. Object keys sort lexicographically, while arrays preserve
semantic order. The snapshot fingerprint covers the version numbers, producer,
ordered records and their fingerprints, and asset digests, media types, and byte
sizes. Asset digests separately cover the exact stored bytes. Neither digest
includes SQLite page layout, local paths, timestamps, or journal state.

`Snapshot::canonical_export` returns readable JSON with the same records and
asset metadata, plus base64 asset bytes. Comparing this export establishes
logical equivalence independently of physical database layout. Fingerprints
provide integrity checks, not authentication: loading also checks record sets,
semantic references, anchors, paths, asset media, and active output policies.

## Generation boundary

The loader returns an immutable `Snapshot`. `Site::new` assigns collision-checked
routes and prepares cell visibility from the retained declarations. Rendering
consumes that model and its verified output wrappers, escapes ordinary text,
chooses the first accepted MIME representation, and maps asset digests to local
URLs. It never opens a database or source checkout. The output contains local
styles, search data and behavior, and all referenced asset bytes.

Site publication stages a sibling directory. On Linux, an existing site is
replaced with an atomic directory exchange. Other platforms use a backup rename
with rollback. Publication accepts an empty destination or a tree marked by an
earlier Diplodocus publication. Commands also reject destinations that would
replace selected inputs or the input snapshot.
