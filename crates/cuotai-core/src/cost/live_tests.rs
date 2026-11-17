//! PRD 0042 re-validation H-01, M-04 and M-08: the live block keeps the
//! usage without a known price in its line and projection, formats money
//! in the reader's locale, and says why it has no burn rate yet.

use super::*;
use crate::cost::Tokens;
use chrono::{FixedOffset, TimeZone};

const PRICED: &str = "claude-sonnet-4-5-20250929";
const UNPRICED: &str = "model-without-a-price";

fn at(h: u32, m: u32) -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 10, 25, h, m, 0).unwrap()
}

fn request(when: DateTime<Utc>, model: &str, tokens: u64) -> Request {
    Request {
        source: SourceId::ClaudeCode,
        at: when,
        day: when.date_naive(),
        model: model.into(),
        session: "s".into(),
        project: Some("p".into()),
        tokens: Tokens {
            input: tokens,
            ..Tokens::default()
        },
    }
}

fn utc() -> DayZone {
    DayZone::Fixed(FixedOffset::east_opt(0).unwrap())
}

/// A block from 11:00 seen at 12:00 (20% elapsed, 4 hours left), whose four recent
/// requests are `priced` and `unpriced` input tokens, two of each.
fn mixed(priced: u64, unpriced: u64) -> LiveBlock {
    let prices = Prices::embedded();
    let rows = [
        request(at(11, 52), PRICED, priced),
        request(at(11, 54), UNPRICED, unpriced),
        request(at(11, 56), PRICED, priced),
        request(at(11, 58), UNPRICED, unpriced),
    ];
    let block = &blocks(&rows, &|_| 5, at(12, 0), &prices)[0];
    assert_eq!(block.start, at(11, 0));
    live(block, &rows, at(12, 0), &prices, utc())
}

#[test]
fn a_half_unpriced_block_names_its_unknown_cost_in_the_line_and_projects_tokens() {
    // 600K priced tokens at $3/M: $1.80; 600K more without a price.
    let summary = mixed(300_000, 300_000);
    assert_eq!(summary.unknown_tokens, 600_000);
    let line = summary.line.clone().unwrap();
    assert_eq!(
        line.resolve("en-US"),
        "This block ≈ $1.80 + unknown cost of 600K tokens · 120K tok/min"
    );
    for locale in ["de", "pt-BR", "ru"] {
        let money = Usd::from_micros(1_800_000).money().resolve(locale);
        assert!(money.contains("1,80") && money != "$1.80", "{money}");
        let text = line.resolve(locale);
        assert!(text.contains(&money) && !text.contains("$1.80"), "{text}");
        assert!(!text.contains("unknown cost"), "{locale}: {text}");
    }
    // Half of the projected tokens have no price: tokens, no money.
    assert_eq!(summary.projection_kind, Some(ProjectionKind::Tokens));
    assert_eq!(summary.projected_block_cost, None);
    assert_eq!(summary.projected_tokens, Some(25 * 1_200_000));
    assert_eq!(summary.projected_unknown_tokens, Some(25 * 600_000));
    assert_eq!(
        summary.projection.unwrap().resolve("en-US"),
        "At this rate ≈ 30M tokens by 16:00; most of them have no known price, so no cost is projected"
    );
}

#[test]
fn a_block_under_half_unpriced_projects_money_with_its_unknown_part() {
    let summary = mixed(350_000, 150_000);
    assert_eq!(summary.projection_kind, Some(ProjectionKind::MoneyPartial));
    // $2.10 so far plus 24 more ten-minute windows of $2.10.
    assert_eq!(
        summary.projected_block_cost,
        Some(Usd::from_micros(2_100_000 * 25))
    );
    assert_eq!(summary.projected_unknown_tokens, Some(300_000 * 25));
    assert_eq!(
        summary.projection.unwrap().resolve("en-US"),
        "At this rate ≈ $52.50 + unknown cost of about 7.5M tokens by 16:00"
    );
    assert!(summary
        .line
        .unwrap()
        .resolve("en-US")
        .contains("+ unknown cost of 300K tokens"));
}

#[test]
fn a_mostly_unpriced_block_projects_only_tokens() {
    let summary = mixed(150_000, 350_000);
    assert_eq!(summary.projection_kind, Some(ProjectionKind::Tokens));
    assert_eq!(summary.projected_block_cost, None);
    let projection = summary.projection.unwrap();
    assert!(projection.resolve("en-US").contains("no cost is projected"));
    assert!(!projection.resolve("en-US").contains('$'));
}

#[test]
fn a_fully_priced_block_projects_money_in_the_reader_s_locale() {
    let summary = mixed(300_000, 0);
    assert_eq!(summary.unknown_tokens, 0);
    assert_eq!(summary.projection_kind, Some(ProjectionKind::Money));
    assert_eq!(summary.projected_unknown_tokens, Some(0));
    let projection = summary.projection.unwrap();
    assert_eq!(
        projection.resolve("en-US"),
        "At this rate ≈ $45.00 by 16:00"
    );
    let money = Usd::from_micros(45_000_000).money().resolve("de");
    assert!(projection.resolve("de").contains(&money), "{money}");
    assert_eq!(
        summary.line.unwrap().resolve("en-US"),
        "This block ≈ $1.80 · 60K tok/min"
    );
}

#[test]
fn the_missing_burn_rate_says_how_many_requests_were_seen() {
    let prices = Prices::embedded();
    let few = [
        request(at(10, 0), PRICED, 10),
        request(at(11, 55), PRICED, 600),
        request(at(11, 58), PRICED, 600),
    ];
    let block = &blocks(&few, &|_| 5, at(12, 0), &prices)[0];
    let summary = live(block, &few, at(12, 0), &prices, utc());
    assert!(summary.line.is_none() && summary.projection_kind.is_none());
    assert_eq!(
        summary.rate_missing.unwrap().resolve("en-US"),
        "Burn rate: needs at least 3 requests in the last 10 minutes; 2 so far."
    );

    // Enough requests, but the block is two minutes old.
    let young = [
        request(at(12, 0), PRICED, 1),
        request(at(12, 1), PRICED, 1),
        request(at(12, 1), PRICED, 1),
    ];
    let block = &blocks(&young, &|_| 5, at(12, 2), &prices)[0];
    let summary = live(block, &young, at(12, 2), &prices, utc());
    let text = summary.rate_missing.unwrap();
    assert_eq!(
        text.resolve("en-US"),
        "Burn rate: the block started 2m ago; it needs 3 minutes."
    );
    assert_ne!(text.resolve("de"), text.resolve("en-US"));

    // With a rate there is no reason.
    assert_eq!(mixed(1, 1).rate_missing, None);
}

#[test]
fn the_live_block_serializes_its_new_fields_only_when_set() {
    let json = serde_json::to_value(mixed(350_000, 150_000)).unwrap();
    assert_eq!(json["projection_kind"], "money_partial");
    assert_eq!(json["unknown_tokens"], 300_000);
    assert!(json.get("rate_missing").is_none());
    let json = serde_json::to_value(mixed(150_000, 350_000)).unwrap();
    assert_eq!(json["projection_kind"], "tokens");
    assert!(json["projected_block_cost"].is_null());
}
