//! PRD 0042 re-validation S-04: a log line dated in the future is not
//! stored, and the prune removes rows an older version stored.

use super::*;

#[test]
fn a_line_dated_in_the_future_is_counted_invalid_and_stores_nothing() {
    let f = fixture();
    f.enable(SourceId::ClaudeCode);
    // `now()` is 2025-10-03 12:00 UTC: up to an hour ahead is a clock skew.
    let lines = [
        claude_at(1, "s", PERSONAL, "2025-10-03T11:00:00Z", 100),
        claude_at(2, "s", PERSONAL, "2025-10-03T12:59:00Z", 10),
        claude_at(3, "s", PERSONAL, "2025-10-03T13:01:00Z", 1000),
        claude_at(4, "s", PERSONAL, "2099-01-01T00:00:00.000Z", 1000),
    ];
    write(&f.claude_log("future"), &lines.concat());
    let report = f.scan();
    let claude = source_scan(&report, SourceId::ClaudeCode);
    assert_eq!((claude.requests, claude.invalid), (2, 2));
    assert_eq!(f.input(), 110);
    assert_eq!(f.rows("requests"), 2);
    let db = rusqlite::Connection::open(f.scanner.index().path()).unwrap();
    let latest: String = db
        .query_row("SELECT MAX(day) FROM requests", [], |r| r.get(0))
        .unwrap();
    assert_eq!(latest, "2025-10-03");
    // A rescan reads nothing new and counts nothing again.
    assert_eq!(source_scan(&f.scan(), SourceId::ClaudeCode).invalid, 0);
}

#[test]
fn the_prune_removes_rows_dated_after_tomorrow() {
    let f = fixture();
    f.enable(SourceId::ClaudeCode);
    let lines = [
        claude_at(1, "s", PERSONAL, "2025-10-03T08:00:00Z", 100),
        claude_at(2, "t", PERSONAL, "2025-10-03T09:00:00Z", 10),
    ];
    write(&f.claude_log("planted"), &lines.concat());
    f.scan();
    assert_eq!(f.rows("requests"), 2);
    // As an older version stored them: one row tomorrow (a time zone ahead
    // of the scan's day), the rest in 2099.
    let db = rusqlite::Connection::open(f.scanner.index().path()).unwrap();
    db.execute(
        "UPDATE requests SET day = '2025-10-04' WHERE input = 100",
        [],
    )
    .unwrap();
    db.execute(
        "UPDATE requests SET day = '2099-01-01' WHERE input = 10",
        [],
    )
    .unwrap();
    for table in ["usage", "sessions", "seen"] {
        db.execute(&format!("UPDATE {table} SET day = '2099-01-01'"), [])
            .unwrap();
    }
    drop(db);
    f.scan();
    assert_eq!(f.rows("requests"), 1, "tomorrow is kept");
    for table in ["usage", "sessions", "seen"] {
        assert_eq!(f.rows(table), 0, "{table}");
    }
}
