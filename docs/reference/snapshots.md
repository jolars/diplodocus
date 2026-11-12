# Snapshot format

A Diplodocus snapshot is a standalone SQLite database. It contains structured
documentation and the local assets needed to generate a site. The [snapshot
guide](../guide/snapshots.md) explains extraction and use in other frontends;
the [design](../development/design.md) explains why extraction and generation
are separate.

## Compatibility

Read the single `manifest` row before interpreting records:

  | Field              | Current value | Meaning                                            |
  | ------------------ | ------------- | -------------------------------------------------- |
  | `storage_version`  | `2`           | Tables, keys, record kinds, and storage envelopes. |
  | `ir_version`       | `14`          | Documentation records and their semantic meaning.  |
  | `encoding_version` | `1`           | Canonical encoding, fingerprints, and text export. |

The reader accepts only these versions and does not migrate older snapshots. The
workspace record's `schema_version` agrees with `ir_version`. Nested JSON
inherits the manifest versions. `producer` records the Diplodocus package
version; it is not the compatibility discriminator.

## Tables

The executable [SQL
schema](https://github.com/jolars/diplodocus/blob/main/src/snapshots/schema.sql)
defines three tables:

  | Table      | Key                 | Other columns                                                                          |
  | ---------- | ------------------- | -------------------------------------------------------------------------------------- |
  | `manifest` | `id`, always `1`    | `storage_version`, `ir_version`, `encoding_version`, `producer`, `content_fingerprint` |
  | `records`  | `(kind, owner, id)` | `content`, `fingerprint`                                                               |
  | `assets`   | `digest`            | `media_type`, `bytes`                                                                  |

Record keys and content are text. `content` is a UTF-8 JSON object, and each
fingerprint or digest is a lowercase SHA-256 hex string. Assets retain their
exact bytes as a BLOB. All columns are non-null; an empty key component is an
empty string. Keys are case-sensitive. The tables do not declare SQL foreign
keys, so successful insertion does not establish a valid snapshot.

## Records and identities

Semantic IDs identify records independently of display names and generated URLs.
Items and extraction targets are scoped to their package; other IDs are
workspace-scoped.

  | Kind           | Owner      | ID                           | Contents                                                                        |
  | -------------- | ---------- | ---------------------------- | ------------------------------------------------------------------------------- |
  | `workspace`    | Empty      | Empty                        | Name, IR version, package relationships, diagnostics, and provenance.           |
  | `presentation` | Empty      | Empty                        | Site defaults, navigation, branding, and retained component bundles.            |
  | `repository`   | Empty      | Repository ID                | Canonical URL, revision, and source fingerprint.                                |
  | `package`      | Empty      | Package ID                   | Package metadata, slug, ecosystem, repository, kind, and visibility.            |
  | `target`       | Package ID | Target ID                    | Extractor, source path, role, and extractor options.                            |
  | `item`         | Package ID | Item ID                      | API kind, names, signatures, documentation, language data, and source evidence. |
  | `collection`   | Empty      | Collection ID                | Content owner, source path, mount, format, navigation, and execution settings.  |
  | `page`         | Empty      | Page ID                      | Owner, page kind, title, and document.                                          |
  | `concept`      | Empty      | Concept ID                   | Relationship kind, member items, and documentation.                             |
  | `document`     | Empty      | Serialized document identity | Anchors, resolved references, collection path, and include source maps.         |
  | `execution`    | Empty      | Page ID                      | Authored execution results, diagnostics, and output slots.                      |

There is exactly one workspace and one presentation record. The writer splits
repositories, packages, targets, items, collections, pages, and concepts into
their own rows rather than duplicating those maps in their parent records.
Signatures, relationships, diagnostics, and provenance remain nested values. The
serialized field shapes follow the versioned [Rust
IR](https://github.com/jolars/diplodocus/blob/main/src/ir.rs) and [snapshot
records](https://github.com/jolars/diplodocus/blob/main/src/snapshots.rs).

Each page has a document resolution record. Items and concepts have one when
they contain documentation. The document's syntax stays in its owning record;
the resolution record adds anchors and semantic targets. Its SQL ID is an opaque
compact JSON string identifying a page, item, or concept. Query the structured
`document` field to find an owner rather than constructing that string.

References join semantic keys. An item reference contains `package` and `item`,
which select an item row's `owner` and `id`. Page and concept references select
their respective rows. Repository-relative source locations provide evidence;
they do not require generation to reopen a file. Execution evidence can also
live in document provenance, as with API doctests and native articles, rather
than in a separate execution row.

## Assets and validation

Asset references contain a portable `path` and a `fingerprint` with
`algorithm: "sha256"` and a `value` matching `assets.digest`. Several references
can share one asset row. Images retain their validated media type; downloads use
`application/octet-stream`. Generated figures, checked-in images, downloads, and
branding all travel with the database.

The loader validates versions, record shapes, semantic relationships, source
maps, paths, fingerprints, and retained asset bytes. It also reapplies active
HTML and image policies. Deserializing an output does not make it trusted.
Component bundles remain inert unless generation explicitly enables them.

Snapshots are replaced as complete artifacts. Manual edits are discarded by the
next extraction. Readers open the database read-only; extraction refuses to
replace a destination with SQLite journal or WAL sidecars. Close external
writers before publishing a replacement.

## Query examples

These queries use SQLite's JSON functions. They are exercised against a
published snapshot by the [schema
tests](https://github.com/jolars/diplodocus/blob/main/tests/snapshot_schema.rs).

### Package metadata

Read package names and versions:

```sql
SELECT id,
       json_extract(content, '$.name') AS name,
       json_extract(content, '$.version') AS version
FROM records WHERE kind = 'package' ORDER BY id;
```

### Item lookup

Bind `?1` to the package ID and `?2` to the semantic item ID. A reexport name is
an alias and need not have a separate row.

```sql
SELECT content, fingerprint
FROM records WHERE kind = 'item' AND owner = ?1 AND id = ?2;
```

### Item documentation

Join an item to its resolved references using the same parameters:

```sql
SELECT i.owner, i.id, json_extract(d.content, '$.references') AS references_json
FROM records AS i
JOIN records AS d
  ON d.kind = 'document' AND d.owner = ''
 AND json_extract(d.content, '$.document.kind') = 'item'
 AND json_extract(d.content, '$.document.item.package') = i.owner
 AND json_extract(d.content, '$.document.item.item') = i.id
WHERE i.kind = 'item' AND i.owner = ?1 AND i.id = ?2;
```

### Resolved documents

Read references in document order. Targets retain semantic identities rather
than generated site URLs.

```sql
SELECT id, json_extract(content, '$.references') AS references_json
FROM records WHERE kind = 'document' ORDER BY id;
```

### Asset lookup

Read retained asset metadata without a source checkout:

```sql
SELECT digest, media_type, length(bytes) AS byte_size
FROM assets ORDER BY digest;
```

## Fingerprints and text export

Fingerprints describe logical records and asset contents. SQLite page layout,
row insertion order, and JSON object order do not affect them. Canonical
encoding sorts object keys recursively, preserves meaningful array order, and
uses compact UTF-8 JSON. A record fingerprint includes its semantic key and
content. It does not include the contents of referenced entities. The manifest
fingerprint covers the versioned record and asset inventory, including producer
and portable provenance.

The [canonical
encoder](https://github.com/jolars/diplodocus/blob/main/src/snapshots/canonical.rs)
and its tests define the exact encoding. Fingerprints check integrity; they do
not authenticate the producer or replace semantic validation.

`Snapshot::canonical_export` produces readable JSON with embedded asset bytes:

```rust
let snapshot = diplodocus::snapshots::Snapshot::load("documentation.sqlite")?;
std::fs::write("documentation.json", snapshot.canonical_export()?)?;
```

Compare these exports when reviewing logical snapshot changes. The export is not
an import format, and hashing its pretty-printed bytes does not reproduce the
manifest fingerprint.
