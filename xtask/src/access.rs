//! Evidence staleness of the access matrix (PRD 0028 RF6.2). Reads the
//! generated matrix in `docs/providers.md` (the core's drift test keeps it
//! equal to `access::describe`), so the check needs no network and no
//! dependency on the core.

use crate::{err, Res};
use std::path::Path;

const MATRIX_START: &str = "<!-- access-matrix:start";
const MATRIX_END: &str = "<!-- access-matrix:end -->";

/// One interface row of the matrix.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    pub provider: String,
    pub mode: String,
    pub level: String,
    pub credential: String,
    pub verified_on: String,
}

/// Interface rows of the matrix in `doc`.
pub fn parse(doc: &str) -> Res<Vec<Row>> {
    let doc = doc.replace("\r\n", "\n");
    let Some((_, rest)) = doc.split_once(MATRIX_START) else {
        return err("docs/providers.md has no access matrix start marker");
    };
    let Some((matrix, _)) = rest.split_once(MATRIX_END) else {
        return err("docs/providers.md has no access matrix end marker");
    };
    let mut rows = Vec::new();
    for line in matrix.lines().filter(|l| l.starts_with("| `")) {
        let cells: Vec<&str> = line
            .trim_matches('|')
            .split(" | ")
            .map(str::trim)
            .collect();
        if cells.len() != 8 {
            return err(format!("matrix row has {} cells, expected 8", cells.len()));
        }
        rows.push(Row {
            provider: cells[0].trim_matches('`').to_string(),
            mode: cells[1].to_string(),
            level: cells[2].to_string(),
            credential: cells[3].to_string(),
            verified_on: cells[7].to_string(),
        });
    }
    if rows.is_empty() {
        return err("the access matrix has no rows");
    }
    Ok(rows)
}

/// Days since 1970-01-01 of an ISO `YYYY-MM-DD` date.
pub fn day_number(date: &str) -> Res<i64> {
    let parts: Vec<&str> = date.split('-').collect();
    let [y, m, d] = parts[..] else {
        return err(format!("'{date}' is not a YYYY-MM-DD date"));
    };
    let (Ok(y), Ok(m), Ok(d)) = (y.parse::<i64>(), m.parse::<i64>(), d.parse::<i64>()) else {
        return err(format!("'{date}' is not a YYYY-MM-DD date"));
    };
    if !(1..=12).contains(&m) || !(1..=31).contains(&d) {
        return err(format!("'{date}' is not a YYYY-MM-DD date"));
    }
    // Days-from-civil (Howard Hinnant's algorithm).
    let y = y - i64::from(m <= 2);
    let era = y.div_euclid(400);
    let yoe = y.rem_euclid(400);
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    Ok(era * 146_097 + doe - 719_468)
}

/// A row whose evidence is older than the limit.
#[derive(Debug, PartialEq, Eq)]
pub struct Stale {
    pub row: Row,
    pub age_days: i64,
}

/// Interfaces PRD 0028 RF6.2 watches (`Undocumented` level or `User
/// session` credential) with evidence older than `days` on `today`.
pub fn stale(rows: &[Row], today: &str, days: i64) -> Res<Vec<Stale>> {
    let today = day_number(today)?;
    let mut found = Vec::new();
    for row in rows {
        if row.level != "Undocumented" && row.credential != "User session" {
            continue;
        }
        let age_days = today - day_number(&row.verified_on)?;
        if age_days > days {
            found.push(Stale {
                row: row.clone(),
                age_days,
            });
        }
    }
    Ok(found)
}

/// Run the check for the repository at `repo`: GitHub annotations for every
/// interface past `days` (warnings), and an error when any is past
/// `fail_after` (the separate, non-required job).
pub fn run(repo: &Path, today: &str, days: i64, fail_after: Option<i64>) -> Res<String> {
    let doc = std::fs::read_to_string(repo.join("docs/providers.md"))?;
    let rows = parse(&doc)?;
    let mut out = String::new();
    for item in stale(&rows, today, days)? {
        out.push_str(&format!(
            "::warning file=docs/providers.md::{} ({}) not verified since {} ({} days)\n",
            item.row.provider, item.row.mode, item.row.verified_on, item.age_days
        ));
    }
    if out.is_empty() {
        out.push_str(&format!(
            "access evidence is at most {days} days old for every undocumented or user-session interface\n"
        ));
    }
    if let Some(limit) = fail_after {
        let failing = stale(&rows, today, limit)?;
        if !failing.is_empty() {
            let names: Vec<String> = failing
                .iter()
                .map(|s| format!("{} ({})", s.row.provider, s.row.mode))
                .collect();
            return err(format!(
                "{out}evidence older than {limit} days: {}",
                names.join(", ")
            ));
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    const DOC: &str = "x\n<!-- access-matrix:start (generated) -->\n\
| Identifier | Mode | Level | Credential | Plan | Scope | Evidence | Verified on |\n\
|---|---|---|---|---|---|---|---|\n\
| `claude` | default | Undocumented | Subscription session | — | Account | e | 2026-01-01 |\n\
| `groq` | default | Derived | Inference key | — | Organization | e | 2025-01-01 |\n\
| `alibaba_token_plan` | `session_env` set | Official | User session | — | Account | e | 2026-06-01 |\n\
| `open_ai_api` | default | Official | Admin key | — | Organization | e | 2026-10-20 |\n\
<!-- access-matrix:end -->\ny\n";

    #[test]
    fn day_numbers_follow_the_calendar() {
        assert_eq!(day_number("1970-01-01").unwrap(), 0);
        assert_eq!(day_number("2026-10-20").unwrap(), 20_746);
        assert_eq!(
            day_number("2024-03-01").unwrap() - day_number("2024-02-28").unwrap(),
            2
        );
        assert!(day_number("2026-13-01").is_err());
        assert!(day_number("soon").is_err());
    }

    #[test]
    fn only_undocumented_and_user_session_rows_age() {
        let rows = parse(DOC).unwrap();
        assert_eq!(rows.len(), 4);
        let old = stale(&rows, "2026-10-25", 90).unwrap();
        let names: Vec<_> = old.iter().map(|s| s.row.provider.as_str()).collect();
        // `groq` is old but Derived over an inference key: not watched.
        assert_eq!(names, ["claude", "alibaba_token_plan"]);
        assert_eq!(old[0].age_days, 297);
        assert!(stale(&rows, "2026-02-01", 90).unwrap().is_empty());
    }

    #[test]
    fn warnings_do_not_fail_but_the_limit_does() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("docs")).unwrap();
        std::fs::write(dir.path().join("docs/providers.md"), DOC).unwrap();
        let out = run(dir.path(), "2026-10-25", 90, None).unwrap();
        assert!(out.starts_with("::warning file=docs/providers.md::claude (default)"));
        assert!(out.contains("alibaba_token_plan"));
        let error = run(dir.path(), "2026-10-25", 90, Some(180)).unwrap_err();
        assert!(error.to_string().contains("evidence older than 180 days: claude"));
        let fresh = run(dir.path(), "2026-02-01", 90, Some(180)).unwrap();
        assert!(fresh.starts_with("access evidence is at most 90 days old"));
    }
}
