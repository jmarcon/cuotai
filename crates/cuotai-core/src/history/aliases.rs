//! PRD 0028 RF11.2 — the metric identity alias table.
//!
//! A series is named by [`super::metric_key`], the digest of a metric's kind,
//! window, label and unit. When an adapter changes one of those on purpose,
//! the label snapshot test (`providers::label_snapshot`) fails and the change
//! adds an entry here: `(old metric_key, new metric_key)`. On the next open
//! the stored series and samples of the old identity move to the new one, so
//! PRD 0006 series and PRD 0026 statistics continue across the rename.
//!
//! Only add an alias when the quantity is the same and its unit did not
//! change meaning: without an alias the series breaks visibly, which is
//! better than silently merging values of different units. Rows already
//! stored under the new identity win; the old ones are dropped.

use rusqlite::Connection;

/// `(old metric_key, new metric_key)`; append, never edit a shipped entry.
pub const METRIC_ALIASES: &[(&str, &str)] = &[];

/// Move the rows of every `old` identity to its `new` one, once.
pub(crate) fn apply(db: &Connection, aliases: &[(&str, &str)]) -> rusqlite::Result<()> {
    if aliases.is_empty() {
        return Ok(());
    }
    let tx = db.unchecked_transaction()?;
    for (old, new) in aliases {
        // The new identity's series is created where missing; the old
        // series' samples join it (rows already there win) and the old goes.
        tx.execute(
            "INSERT OR IGNORE INTO series (config_key, account_digest, metric_key, label,
                position, seen_at, removed_at, billing, scope, unit, kind, \"window\")
             SELECT config_key, account_digest, ?2, label, position, seen_at, removed_at, billing,
                scope, unit, kind, \"window\"
             FROM series WHERE metric_key = ?1",
            rusqlite::params![old, new],
        )?;
        tx.execute(
            "INSERT OR IGNORE INTO samples (series_id, taken_at, used, \"limit\", remaining,
                resets_at)
             SELECT n.id, x.taken_at, x.used, x.\"limit\", x.remaining, x.resets_at
             FROM samples x JOIN series o ON o.id = x.series_id
               JOIN series n ON n.config_key = o.config_key
                AND n.account_digest = o.account_digest AND n.metric_key = ?2
             WHERE o.metric_key = ?1",
            rusqlite::params![old, new],
        )?;
        super::delete_series(&tx, "metric_key = ?1", rusqlite::params![old])?;
    }
    tx.commit()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn db() -> Connection {
        let mut db = Connection::open_in_memory().unwrap();
        super::super::migrate(&mut db, &super::super::MIGRATIONS).unwrap();
        db
    }

    fn put(db: &Connection, key: &str, at: i64, used: f64) {
        db.execute(
            "INSERT OR IGNORE INTO series (config_key, account_digest, metric_key, label, position,
                seen_at, removed_at, unit, kind, \"window\")
             VALUES ('c', 'a', ?1, 'label', 0, 1, NULL, 'u', 'quota', NULL)",
            [key],
        )
        .unwrap();
        db.execute(
            "INSERT INTO samples (series_id, taken_at, used)
             SELECT id, ?2, ?3 FROM series WHERE metric_key = ?1",
            rusqlite::params![key, at, used],
        )
        .unwrap();
    }

    fn keys(db: &Connection, table: &str) -> Vec<(String, i64)> {
        let from = match table {
            "samples" => "samples x JOIN series s ON s.id = x.series_id",
            _ => "series s",
        };
        let mut stmt = db
            .prepare(&format!(
                "SELECT s.metric_key, COUNT(*) FROM {from} GROUP BY s.metric_key ORDER BY 1"
            ))
            .unwrap();
        stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
            .unwrap()
            .map(Result::unwrap)
            .collect()
    }

    #[test]
    fn an_alias_moves_the_old_series_and_keeps_rows_already_under_the_new_identity() {
        let db = db();
        put(&db, "old", 1, 1.0);
        put(&db, "old", 2, 2.0);
        put(&db, "new", 2, 9.0);
        put(&db, "other", 3, 3.0);
        apply(&db, &[("old", "new")]).unwrap();
        assert_eq!(
            keys(&db, "samples"),
            [("new".to_string(), 2), ("other".to_string(), 1)]
        );
        assert_eq!(
            keys(&db, "series"),
            [("new".to_string(), 1), ("other".to_string(), 1)]
        );
        let kept: f64 = db
            .query_row(
                "SELECT x.used FROM samples x JOIN series s ON s.id = x.series_id
                 WHERE s.metric_key = 'new' AND x.taken_at = 2",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(kept, 9.0, "the row already under the new identity wins");
        apply(&db, &[("old", "new")]).unwrap();
        assert_eq!(keys(&db, "samples").len(), 2, "a second run is a no-op");
    }

    #[test]
    fn the_shipped_table_is_well_formed() {
        for (old, new) in METRIC_ALIASES {
            assert_ne!(old, new);
            assert_eq!(old.len(), 64, "old digests are SHA-256 hex");
            assert_eq!(new.len(), 64);
        }
    }
}
