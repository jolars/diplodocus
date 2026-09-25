CREATE TABLE manifest (
    id INTEGER PRIMARY KEY CHECK (id = 1),
    storage_version INTEGER NOT NULL,
    ir_version INTEGER NOT NULL,
    encoding_version INTEGER NOT NULL,
    producer TEXT NOT NULL,
    content_fingerprint TEXT NOT NULL
);

CREATE TABLE records (
    kind TEXT NOT NULL,
    owner TEXT NOT NULL,
    id TEXT NOT NULL,
    content TEXT NOT NULL,
    fingerprint TEXT NOT NULL,
    PRIMARY KEY (kind, owner, id)
) WITHOUT ROWID;

CREATE TABLE assets (
    digest TEXT PRIMARY KEY NOT NULL,
    media_type TEXT NOT NULL,
    bytes BLOB NOT NULL
) WITHOUT ROWID;
