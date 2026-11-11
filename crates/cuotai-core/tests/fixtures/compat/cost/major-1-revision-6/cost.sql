PRAGMA foreign_keys=OFF;
BEGIN TRANSACTION;
CREATE TABLE usage (
    source TEXT NOT NULL,
    day TEXT NOT NULL,
    model TEXT NOT NULL,
    file TEXT NOT NULL,
    input INTEGER NOT NULL,
    output INTEGER NOT NULL,
    cache_read INTEGER NOT NULL,
    cache_write INTEGER NOT NULL,
    requests INTEGER NOT NULL, cache_write_1h INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (source, day, model, file)
) WITHOUT ROWID;
CREATE TABLE sessions (
    source TEXT NOT NULL,
    day TEXT NOT NULL,
    model TEXT NOT NULL,
    session TEXT NOT NULL,
    file TEXT NOT NULL,
    PRIMARY KEY (source, day, model, session)
) WITHOUT ROWID;
CREATE TABLE seen (
    source TEXT NOT NULL,
    key TEXT NOT NULL,
    file TEXT NOT NULL,
    day TEXT NOT NULL, model TEXT NOT NULL DEFAULT '', input INTEGER NOT NULL DEFAULT 0, output INTEGER NOT NULL DEFAULT 0, cache_read INTEGER NOT NULL DEFAULT 0, cache_write INTEGER NOT NULL DEFAULT 0, cache_write_1h INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (source, key)
) WITHOUT ROWID;
CREATE TABLE files (
    source TEXT NOT NULL,
    file TEXT NOT NULL,
    identity TEXT NOT NULL,
    examined INTEGER NOT NULL,
    position INTEGER NOT NULL,
    state TEXT NOT NULL,
    invalid INTEGER NOT NULL,
    read_at INTEGER NOT NULL, backfill INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (source, file)
) WITHOUT ROWID;
INSERT INTO files VALUES('codex_cli','f','i',0,0,'{}',0,0,0);
CREATE TABLE requests (
    source TEXT NOT NULL,
    file TEXT NOT NULL,
    key TEXT,
    at INTEGER NOT NULL,
    day TEXT NOT NULL,
    model TEXT NOT NULL,
    session TEXT NOT NULL,
    project TEXT,
    input INTEGER NOT NULL,
    output INTEGER NOT NULL,
    cache_read INTEGER NOT NULL,
    cache_write INTEGER NOT NULL,
    cache_write_1h INTEGER NOT NULL
);
CREATE TABLE projects (
    project TEXT NOT NULL PRIMARY KEY,
    label TEXT,
    folder_hint TEXT,
    hint_source TEXT,
    first_seen INTEGER NOT NULL,
    last_seen INTEGER NOT NULL,
    hidden INTEGER NOT NULL DEFAULT 0
) WITHOUT ROWID;
CREATE TABLE session_labels (
    session TEXT NOT NULL PRIMARY KEY,
    label TEXT NOT NULL
) WITHOUT ROWID;
CREATE TABLE state_meta (
    key TEXT PRIMARY KEY,
    value INTEGER NOT NULL
) WITHOUT ROWID;
INSERT INTO state_meta VALUES('major',1);
CREATE INDEX usage_by_day ON usage (day);
CREATE INDEX usage_by_file ON usage (source, file);
CREATE INDEX sessions_by_file ON sessions (source, file);
CREATE INDEX seen_by_file ON seen (source, file);
CREATE INDEX seen_by_day ON seen (day);
CREATE INDEX requests_by_time ON requests (source, at);
CREATE INDEX requests_by_file ON requests (source, file);
CREATE INDEX requests_by_key ON requests (source, key);
CREATE INDEX requests_by_day ON requests (day);
COMMIT;
PRAGMA user_version=3;
ALTER TABLE files ADD COLUMN later_note TEXT;
PRAGMA user_version=6;
