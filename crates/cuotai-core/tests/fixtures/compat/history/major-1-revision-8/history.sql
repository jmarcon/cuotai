PRAGMA foreign_keys=OFF;
BEGIN TRANSACTION;
CREATE TABLE meta (key TEXT PRIMARY KEY, value INTEGER NOT NULL) WITHOUT ROWID;
CREATE TABLE journal (
    config_key TEXT NOT NULL,
    account_digest TEXT NOT NULL,
    occurred_at INTEGER NOT NULL,
    minute INTEGER NOT NULL,
    kind TEXT NOT NULL,
    metric_digest TEXT NOT NULL,
    metric_label TEXT,
    threshold INTEGER,
    used_ratio REAL,
    resets_at INTEGER,
    category TEXT,
    incident TEXT,
    projected_at INTEGER,
    spent REAL,
    currency TEXT
, parent_digest TEXT);
CREATE TABLE IF NOT EXISTS "series" (
    id INTEGER PRIMARY KEY,
    config_key TEXT NOT NULL,
    account_digest TEXT NOT NULL,
    metric_key TEXT NOT NULL,
    label TEXT NOT NULL,
    position INTEGER NOT NULL,
    seen_at INTEGER NOT NULL,
    removed_at INTEGER,
    billing TEXT NOT NULL DEFAULT 'included',
    scope TEXT,
    unit TEXT NOT NULL,
    kind TEXT NOT NULL,
    "window" TEXT,
    UNIQUE (config_key, account_digest, metric_key)
);
INSERT INTO series VALUES(1,'acc31b1357944d8898652c806a20ffbc4d9d386b12e87f4df75ecc106b59f883','89c0f1bd24a723e51757d87144b294dd0425ae48e7ff336cd33968d23824782c','de28f0ed2de0734906fa1d3dc05638611e43f45d84399502c47d064c48fe8c15','5h',0,1793534400000,NULL,'included',NULL,'%','quota','window_5h');
CREATE TABLE IF NOT EXISTS "samples" (
    series_id INTEGER NOT NULL,
    taken_at INTEGER NOT NULL,
    used REAL,
    "limit" REAL,
    remaining REAL,
    resets_at INTEGER,
    PRIMARY KEY (series_id, taken_at)
) WITHOUT ROWID;
INSERT INTO samples VALUES(1,1793534400000,42.0,100.0,NULL,1793552400000);
CREATE TABLE state_meta (
    key TEXT PRIMARY KEY,
    value INTEGER NOT NULL
) WITHOUT ROWID;
INSERT INTO state_meta VALUES('major',1);
CREATE TABLE aliases (
    config_key TEXT NOT NULL,
    legacy_digest TEXT NOT NULL,
    account_id TEXT NOT NULL,
    PRIMARY KEY (config_key, legacy_digest)
) WITHOUT ROWID;
CREATE UNIQUE INDEX journal_unique
    ON journal (config_key, account_digest, kind, metric_digest, minute);
CREATE INDEX journal_by_time ON journal (config_key, occurred_at);
CREATE INDEX journal_by_age ON journal (occurred_at);
CREATE INDEX samples_by_age ON samples (taken_at);
COMMIT;
PRAGMA user_version=7;
ALTER TABLE samples ADD COLUMN source_note TEXT;
CREATE TABLE later_feature (k TEXT PRIMARY KEY, v INTEGER) WITHOUT ROWID;
PRAGMA user_version=8;
