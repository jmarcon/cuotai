//! PRD 0043 re-validation G3 (S-01): a channel limited to some accounts
//! receives the weekly digest of those accounts only.

use super::payload::{prepare, Source};
use super::*;
use crate::digest::{DayShare, Digest, DigestAccount, DigestWeek};
use crate::{Account, Provider};
use chrono::{NaiveDate, TimeZone};

fn account(name: &str) -> Account {
    Account {
        name: name.into(),
        provider: Provider::Claude,
        ..Account::default()
    }
}

fn day(d: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(2026, 10, d).unwrap()
}

fn week_of(account: &Account, hits: u32, peak: f64, busy_hour: usize) -> DigestAccount {
    let mut hours = [0.0; 24];
    hours[busy_hour] = 50.0;
    let days = vec![
        DayShare {
            day: day(19),
            share: peak / 2.0,
        },
        DayShare {
            day: day(20),
            share: peak,
        },
    ];
    DigestAccount {
        name: account.name.clone(),
        provider: account.provider,
        hits,
        average_peak: Some(peak * 0.75),
        days_at_limit: 0,
        heaviest: Some(days[1]),
        lightest: Some(days[0]),
        busiest_hours: Some((busy_hour as u8, (busy_hour + 3) as u8)),
        limit_changes: Vec::new(),
        budgets: Vec::new(),
        allowances: Vec::new(),
        account_id: account.state_key(),
        days,
        hours,
        extra_spent: None,
    }
}

fn digest(work: &Account, personal: &Account) -> Digest {
    let full = Digest {
        week: DigestWeek::Last,
        first_day: day(19),
        last_day: day(25),
        hits: 0,
        average_peak: None,
        extra_spent: None,
        heaviest: None,
        lightest: None,
        busiest_hours: None,
        accounts: vec![week_of(work, 1, 0.4, 9), week_of(personal, 3, 0.8, 20)],
        recorded: true,
    };
    // Recompute the overall figures as `weekly` does.
    full.retain_accounts(|_| true).unwrap()
}

fn prepared(digest: &Digest, accounts: &[&Account], names: bool) -> Option<payload::Outgoing> {
    let filters = Filters {
        kinds: NoticeKind::ALL.into_iter().collect(),
        accounts: accounts.iter().map(|a| a.state_key()).collect(),
        include_names: Some(names),
        ..Filters::default()
    };
    let t0 = chrono::Utc.with_ymd_and_hms(2026, 10, 26, 9, 0, 0).unwrap();
    prepare(
        &Source::WeeklyDigest(Box::new(digest.clone())),
        &filters,
        true,
        "en-US",
        t0,
        &chrono::Utc,
    )
}

#[test]
fn a_channel_limited_to_one_account_gets_that_accounts_digest_only() {
    let (work, personal) = (account("Work"), account("Personal"));
    let digest = digest(&work, &personal);
    assert_eq!(digest.hits, 4, "the full digest counts both accounts");

    let named = prepared(&digest, &[&work], true).unwrap();
    let markdown = named.markdown.unwrap();
    assert!(markdown.contains("## Work · Claude"), "{markdown}");
    assert!(!markdown.contains("Personal"), "{markdown}");
    assert!(named.body.contains("1 limit hit"), "{}", named.body);
    assert!(!named.body.contains("4 limit hits"), "{}", named.body);
    assert!(named.body.contains("Average peak: 30%"), "{}", named.body);
    assert!(
        named.body.contains("Busiest hours: 09:00"),
        "{}",
        named.body
    );

    let hidden = prepared(&digest, &[&work], false).unwrap();
    assert!(hidden.body.contains("1 limit hit"), "{}", hidden.body);
    assert!(!hidden.body.contains("Personal") && !hidden.body.contains("Work"));
    assert!(!hidden.markdown.unwrap().contains("4 limit hits"));

    // No filter: every account.
    let every = prepared(&digest, &[], true).unwrap();
    assert!(every.body.contains("4 limit hits"), "{}", every.body);
    assert!(every.markdown.unwrap().contains("## Personal · Claude"));
}

#[test]
fn a_channel_whose_accounts_are_not_in_the_digest_gets_nothing() {
    let (work, personal) = (account("Work"), account("Personal"));
    let digest = digest(&work, &personal);
    let other = account("Other");
    assert!(prepared(&digest, &[&other], true).is_none());
    assert!(digest.retain_accounts(|_| false).is_none());
}
