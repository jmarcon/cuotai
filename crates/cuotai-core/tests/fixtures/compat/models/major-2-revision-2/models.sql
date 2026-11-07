PRAGMA foreign_keys=OFF;
BEGIN TRANSACTION;
CREATE TABLE model_catalogs (
    config_key TEXT NOT NULL,
    account_key TEXT NOT NULL,
    status TEXT NOT NULL,
    catalog TEXT,
    error TEXT,
    attempted_at TEXT NOT NULL,
    owner TEXT,
    lease_until INTEGER,
    PRIMARY KEY (config_key, account_key)
) WITHOUT ROWID;
INSERT INTO model_catalogs VALUES('acc31b1357944d8898652c806a20ffbc4d9d386b12e87f4df75ecc106b59f883','b29d45da32376f889ad929300c095cdb5af165de3f1b54e5d61d64bd870b78f0','ready','{"models":[{"id":"compat-model","name":null,"description":null}],"source":{"id":"models.source.compat","fallback":"compat"},"fetched_at":"2026-11-01T12:00:00Z"}',NULL,'2026-11-01T12:00:00+00:00',NULL,NULL);
CREATE TABLE state_meta (
    key TEXT PRIMARY KEY,
    value INTEGER NOT NULL
) WITHOUT ROWID;
INSERT INTO state_meta VALUES('major',1);
COMMIT;
PRAGMA user_version=1;
ALTER TABLE model_catalogs ADD COLUMN etag TEXT;
PRAGMA user_version=2;
UPDATE state_meta SET value = 2 WHERE key = 'major';
