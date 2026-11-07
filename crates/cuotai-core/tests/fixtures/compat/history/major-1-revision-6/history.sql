PRAGMA foreign_keys=OFF;
BEGIN TRANSACTION;
CREATE TABLE series (
    config_key TEXT NOT NULL,
    account_digest TEXT NOT NULL,
    metric_key TEXT NOT NULL,
    label TEXT NOT NULL,
    position INTEGER NOT NULL,
    seen_at INTEGER NOT NULL,
    removed_at INTEGER, billing TEXT NOT NULL DEFAULT 'included', scope TEXT,
    PRIMARY KEY (config_key, account_digest, metric_key)
) WITHOUT ROWID;
INSERT INTO series VALUES('acc31b1357944d8898652c806a20ffbc4d9d386b12e87f4df75ecc106b59f883','89c0f1bd24a723e51757d87144b294dd0425ae48e7ff336cd33968d23824782c','de28f0ed2de0734906fa1d3dc05638611e43f45d84399502c47d064c48fe8c15','5h',0,1793534400000,NULL,'included',NULL);
CREATE TABLE samples (
    config_key TEXT NOT NULL,
    account_digest TEXT NOT NULL,
    metric_key TEXT NOT NULL,
    taken_at INTEGER NOT NULL,
    used REAL,
    "limit" REAL,
    remaining REAL,
    unit TEXT NOT NULL,
    kind TEXT NOT NULL,
    "window" TEXT,
    resets_at INTEGER,
    PRIMARY KEY (config_key, account_digest, metric_key, taken_at)
) WITHOUT ROWID;
INSERT INTO samples VALUES('acc31b1357944d8898652c806a20ffbc4d9d386b12e87f4df75ecc106b59f883','89c0f1bd24a723e51757d87144b294dd0425ae48e7ff336cd33968d23824782c','de28f0ed2de0734906fa1d3dc05638611e43f45d84399502c47d064c48fe8c15',1793534400000,42.0,100.0,NULL,'%','quota','window_5h',1793552400000);
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
CREATE TABLE state_meta (
    key TEXT PRIMARY KEY,
    value INTEGER NOT NULL
) WITHOUT ROWID;
INSERT INTO state_meta VALUES('major',1);
CREATE INDEX samples_by_time ON samples (config_key, taken_at);
CREATE INDEX samples_by_age ON samples (taken_at);
CREATE UNIQUE INDEX journal_unique
    ON journal (config_key, account_digest, kind, metric_digest, minute);
CREATE INDEX journal_by_time ON journal (config_key, occurred_at);
CREATE INDEX journal_by_age ON journal (occurred_at);
COMMIT;
PRAGMA user_version=6;
