//! RF4 terminal motion, reduced by PRD 0022 RF16.3 (docs/design-system.md
//! section 7.2 item 24): the refresh spinner stays; the class blink and the
//! screen wipe are gone; the bar tween runs only when `CUOTAI_GAUGE_TWEEN` is
//! set. Everything is driven by the existing event loop (no threads): the
//! loop asks [`Anim::active`] whether to draw at ~30 fps. With animations
//! off, `State.anim` is `None` and the dashboard renders exactly as before.

use crate::palette;
use cuotai_core::metrics::{classify_metric, MetricState};
use cuotai_core::{Account, Report};
use ratatui::style::Color;
use std::{
    collections::HashMap,
    time::{Duration, Instant},
};

const TWEEN: Duration = Duration::from_millis(300);
const SPINNER: [char; 10] = ['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];
const PARTIAL: [char; 7] = ['▏', '▎', '▍', '▌', '▋', '▊', '▉'];
const RED: (u8, u8, u8) = palette::BAD_RGB;
const YELLOW: (u8, u8, u8) = palette::WARN_RGB;
const GREEN: (u8, u8, u8) = palette::GOOD_RGB;

/// `--no-animations` and `CUOTAI_NO_ANIMATIONS` always win; `TERM=dumb` is
/// always off; SSH is off unless `CUOTAI_ANIMATIONS` forces it on.
pub(crate) fn enabled(no_flag: bool, var: impl Fn(&str) -> Option<String>) -> bool {
    let set = |key: &str| var(key).is_some_and(|v| !v.is_empty() && v != "0");
    if no_flag || set("CUOTAI_NO_ANIMATIONS") || var("TERM").as_deref() == Some("dumb") {
        return false;
    }
    set("CUOTAI_ANIMATIONS") || (var("SSH_CONNECTION").is_none() && var("SSH_TTY").is_none())
}

pub(crate) fn truecolor(var: impl Fn(&str) -> Option<String>) -> bool {
    matches!(var("COLORTERM").as_deref(), Some("truecolor" | "24bit"))
}

/// Ease-out cubic over `elapsed / duration`, clamped to `[0, 1]`.
fn eased(elapsed: Duration, duration: Duration) -> f64 {
    let t = (elapsed.as_secs_f64() / duration.as_secs_f64()).clamp(0.0, 1.0);
    1.0 - (1.0 - t).powi(3)
}

/// `width` cells at 1/8-cell resolution; the remainder stays `░`.
pub(crate) fn eighth_bar(ratio: f64, width: usize) -> String {
    let eighths = (ratio.clamp(0.0, 1.0) * (width * 8) as f64).round() as usize;
    let (full, part) = (eighths / 8, eighths % 8);
    let mut bar = "█".repeat(full);
    if part > 0 {
        bar.push(PARTIAL[part - 1]);
    }
    bar + &"░".repeat(width - full - usize::from(part > 0))
}

fn spinner(elapsed: Duration) -> char {
    SPINNER[(elapsed.as_millis() / 80) as usize % SPINNER.len()]
}

/// Level colour: red up to 20% left, yellow at 35%, green from 50% (the
/// dashboard's thresholds), blended in between.
fn gradient(ratio: f64) -> Color {
    let mix = |a: (u8, u8, u8), b: (u8, u8, u8), t: f64| {
        let ch = |x: u8, y: u8| (f64::from(x) + (f64::from(y) - f64::from(x)) * t).round() as u8;
        palette::rgb((ch(a.0, b.0), ch(a.1, b.1), ch(a.2, b.2)))
    };
    let r = ratio.clamp(0.0, 1.0);
    if r < 0.35 {
        mix(RED, YELLOW, ((r - 0.2) / 0.15).clamp(0.0, 1.0))
    } else {
        mix(YELLOW, GREEN, ((r - 0.35) / 0.15).clamp(0.0, 1.0))
    }
}

/// Bar fill a metric draws, if it draws one.
fn target(state: MetricState) -> Option<f64> {
    match state {
        MetricState::Available {
            ratio,
            uncapped: false,
        } => Some(ratio),
        MetricState::Exhausted { no_cap: false } => Some(0.0),
        _ => None,
    }
}

/// Per-card tween (from, to, start).
struct Card {
    from: Vec<Option<f64>>,
    to: Vec<Option<f64>>,
    start: Instant,
}

impl Card {
    fn value(&self, metric: usize, now: Instant) -> Option<f64> {
        let to = (*self.to.get(metric)?)?;
        let Some(Some(from)) = self.from.get(metric) else {
            return Some(to);
        };
        let t = eased(now.saturating_duration_since(self.start), TWEEN);
        Some(if t >= 1.0 { to } else { from + (to - from) * t })
    }
}

pub(crate) struct Anim {
    truecolor: bool,
    epoch: Instant,
    pub(crate) now: Instant,
    /// Keyed by account name (unique per config) so reorders never tween.
    cards: HashMap<String, Card>,
    refreshing: Vec<bool>,
    /// The bar tween (off by default, `CUOTAI_GAUGE_TWEEN`).
    tween: bool,
}

impl Anim {
    pub(crate) fn new(truecolor: bool) -> Self {
        let now = Instant::now();
        Self {
            truecolor,
            epoch: now,
            now,
            cards: HashMap::new(),
            refreshing: Vec::new(),
            tween: std::env::var_os("CUOTAI_GAUGE_TWEEN").is_some_and(|v| !v.is_empty()),
        }
    }

    #[cfg(test)]
    pub(crate) fn with_tween(mut self) -> Self {
        self.tween = true;
        self
    }

    /// Every loop iteration: frame clock and per-account refresh flags.
    pub(crate) fn track(&mut self, refreshing: impl Iterator<Item = bool>, now: Instant) {
        self.now = now;
        self.refreshing.clear();
        self.refreshing.extend(refreshing);
    }

    /// Every dashboard draw: retarget a card's tween when its bars change.
    // ponytail: entries of renamed/removed accounts stay until exit; prune
    // here if configs ever grow to thousands of accounts.
    pub(crate) fn observe(&mut self, accounts: &[Account], reports: &[Option<Report>]) {
        let now = self.now;
        for (account, report) in accounts.iter().zip(reports) {
            let targets = || {
                report
                    .iter()
                    .flat_map(|report| &report.metrics)
                    .map(|metric| target(classify_metric(metric)))
            };
            let Some(card) = self.cards.get_mut(&account.state_key()) else {
                self.cards.insert(
                    account.state_key(),
                    Card {
                        from: Vec::new(),
                        to: targets().collect(),
                        start: now,
                    },
                );
                continue;
            };
            if !self.tween {
                card.to = targets().collect();
                continue;
            }
            if !card.to.iter().copied().eq(targets()) {
                let to: Vec<_> = targets().collect();
                card.from = (0..to.len())
                    .map(|metric| Some(card.value(metric, now).unwrap_or(0.0)))
                    .collect();
                card.to = to;
                card.start = now;
            }
        }
    }

    /// True while any effect still has frames to draw.
    pub(crate) fn active(&self) -> bool {
        let since = |start: Instant| self.now.saturating_duration_since(start);
        self.refreshing.contains(&true)
            || self
                .cards
                .values()
                .any(|card| !card.from.is_empty() && since(card.start) < TWEEN)
    }

    /// Displayed fill of `metric` on `account`'s card; `fallback` without a tween.
    pub(crate) fn bar(&self, account: &str, metric: usize, fallback: f64) -> f64 {
        self.cards
            .get(account)
            .and_then(|card| card.value(metric, self.now))
            .unwrap_or(fallback)
    }

    pub(crate) fn color(&self, ratio: f64, fallback: Color) -> Color {
        if self.truecolor {
            gradient(ratio)
        } else {
            fallback
        }
    }

    pub(crate) fn spinner(&self, index: usize) -> Option<char> {
        self.refreshing
            .get(index)
            .is_some_and(|busy| *busy)
            .then(|| spinner(self.now.saturating_duration_since(self.epoch)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cuotai_core::{Metric, MetricKind, Provider, Status};

    const MS: fn(u64) -> Duration = Duration::from_millis;

    #[test]
    fn easing_clamps_at_both_ends() {
        assert_eq!(eased(MS(0), TWEEN), 0.0);
        assert_eq!(eased(TWEEN, TWEEN), 1.0);
        assert_eq!(eased(MS(5000), TWEEN), 1.0);
        let mid = eased(MS(150), TWEEN);
        assert!(
            mid > 0.5 && mid < 1.0,
            "ease-out is past linear halfway: {mid}"
        );
    }

    #[test]
    fn eighth_bar_resolves_partial_cells_and_bounds() {
        assert_eq!(eighth_bar(0.0, 4), "░░░░");
        assert_eq!(eighth_bar(1.0, 4), "████");
        assert_eq!(eighth_bar(1.7, 2), "██", "overshoot clamps");
        assert_eq!(eighth_bar(-1.0, 2), "░░");
        assert_eq!(eighth_bar(0.5, 0), "");
        assert_eq!(eighth_bar(0.5, 1), "▌");
        assert_eq!(eighth_bar(0.99, 1), "█");
        assert_eq!(eighth_bar(1.0 / 32.0, 4), "▏░░░");
        for width in 0..12 {
            assert_eq!(eighth_bar(0.37, width).chars().count(), width);
        }
    }

    #[test]
    fn gradient_follows_dashboard_thresholds() {
        assert_eq!(gradient(0.0), palette::BAD);
        assert_eq!(gradient(0.2), palette::BAD);
        assert_eq!(gradient(0.35), palette::WARN);
        assert_eq!(gradient(1.0), palette::GOOD);
        assert_ne!(gradient(0.42), gradient(0.35));
    }

    #[test]
    fn opt_outs_beat_ssh_override_and_ssh_defaults_off() {
        let env = |pairs: &'static [(&'static str, &'static str)]| {
            move |key: &str| {
                pairs
                    .iter()
                    .find(|(k, _)| *k == key)
                    .map(|(_, v)| v.to_string())
            }
        };
        assert!(enabled(false, env(&[("TERM", "xterm-256color")])));
        assert!(!enabled(true, env(&[])));
        assert!(!enabled(false, env(&[("CUOTAI_NO_ANIMATIONS", "1")])));
        assert!(enabled(false, env(&[("CUOTAI_NO_ANIMATIONS", "0")])));
        assert!(!enabled(
            false,
            env(&[("TERM", "dumb"), ("CUOTAI_ANIMATIONS", "1")])
        ));
        assert!(!enabled(false, env(&[("SSH_CONNECTION", "a b c d")])));
        assert!(!enabled(false, env(&[("SSH_TTY", "/dev/ttys001")])));
        assert!(enabled(
            false,
            env(&[("SSH_TTY", "x"), ("CUOTAI_ANIMATIONS", "1")])
        ));
        assert!(!enabled(
            true,
            env(&[("SSH_TTY", "x"), ("CUOTAI_ANIMATIONS", "1")])
        ));
        assert!(!enabled(
            false,
            env(&[("CUOTAI_ANIMATIONS", "1"), ("CUOTAI_NO_ANIMATIONS", "yes")])
        ));
        assert!(truecolor(env(&[("COLORTERM", "24bit")])));
        assert!(!truecolor(env(&[("COLORTERM", "yes")])));
    }

    fn accounts() -> Vec<Account> {
        vec![Account {
            id: None,
            name: "a".into(),
            provider: Provider::Groq,
            credential_env: None,
            session: None,
            oauth: None,
            params: Default::default(),
            refresh_seconds: None,
            group: None,
        }]
    }

    pub(crate) fn ready(remaining: f64) -> Option<Report> {
        Some(Report {
            account_id: None,
            diagnostic: None,
            account: "a".into(),
            provider: Provider::Groq,
            checked_at: chrono::Utc::now(),
            status: Status::Ready,
            metrics: vec![Metric {
                billing: Default::default(),
                scope: None,
                label: "m".into(),
                used: Some(10.0 - remaining),
                limit: Some(10.0),
                remaining: Some(remaining),
                unit: "req".into(),
                kind: MetricKind::Quota,
                window: None,
                period: None,
                resets_at: None,
            }],
            notes: Vec::new(),
            notes_l10n: Vec::new(),
            source: String::new(),
            session_expires_at: None,
            sub_reports: Vec::new(),
            as_of: None,
            overage_hint: None,
        })
    }

    /// Cards are keyed by the account's state key (PRD 0029).
    fn key() -> String {
        accounts()[0].state_key()
    }

    fn at(anim: &mut Anim, start: Instant, ms: u64) {
        anim.track([false].into_iter(), start + MS(ms));
    }

    #[test]
    fn refresh_tweens_old_to_new_then_goes_idle() {
        let mut anim = Anim::new(false).with_tween();
        let start = anim.now;
        anim.observe(&accounts(), &[ready(8.0)]);
        assert!(!anim.active(), "first sight never animates");
        assert_eq!(anim.bar(&key(), 0, -1.0), 0.8);

        anim.observe(&accounts(), &[ready(2.0)]);
        assert!(anim.active());
        assert_eq!(anim.bar(&key(), 0, -1.0), 0.8, "t=0 shows the old value");
        at(&mut anim, start, 100);
        let mid = anim.bar(&key(), 0, -1.0);
        assert!(mid > 0.2 && mid < 0.8, "intermediate: {mid}");
        assert!(anim.active());
        at(&mut anim, start, 300);
        assert_eq!(anim.bar(&key(), 0, -1.0), 0.2);
        assert!(!anim.active(), "idle after the tween: no redraws");
        assert_eq!(anim.bar("missing", 0, 0.5), 0.5);
    }

    #[test]
    fn the_spinner_keeps_the_loop_active_and_nothing_else_does() {
        let mut anim = Anim::new(false);
        let start = anim.now;
        anim.track([false, true].into_iter(), start);
        assert_eq!((anim.spinner(0), anim.spinner(1)), (None, Some('⠋')));
        assert_eq!(anim.spinner(9), None);
        assert!(anim.active());
        anim.track([false].into_iter(), start + MS(80));
        anim.observe(&accounts(), &[ready(8.0)]);
        anim.observe(&accounts(), &[ready(2.0)]);
        assert!(
            !anim.active(),
            "no tween by default: the new value shows at once"
        );
        assert_eq!(anim.bar(&key(), 0, -1.0), 0.2);
    }
}
