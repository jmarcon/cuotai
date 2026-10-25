//! RF4 terminal motion: bar tweens, class blink, refresh spinner and screen
//! wipe. Everything is driven by the existing event loop (no threads): the
//! loop asks [`Anim::active`] whether to draw at ~30 fps. With animations
//! off, `State.anim` is `None` and the dashboard renders exactly as before.

use cuotai_core::metrics::{classify_metric, classify_report, AccountClass, MetricState};
use cuotai_core::{Account, Report};
use ratatui::{buffer::Buffer, style::Color};
use std::{
    collections::HashMap,
    time::{Duration, Instant},
};

const TWEEN: Duration = Duration::from_millis(300);
/// Two highlighted phases of 250 ms (2 flashes/s, under the WCAG 3/s cap).
const BLINK: Duration = Duration::from_millis(750);
const WIPE: Duration = Duration::from_millis(120);
const SPINNER: [char; 10] = ['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];
const PARTIAL: [char; 7] = ['▏', '▎', '▍', '▌', '▋', '▊', '▉'];
const BG: Color = Color::Rgb(40, 42, 54);
const RED: (u8, u8, u8) = (255, 85, 85);
const YELLOW: (u8, u8, u8) = (241, 250, 140);
const GREEN: (u8, u8, u8) = (80, 250, 123);

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

fn blink_on(elapsed: Duration) -> bool {
    elapsed < BLINK && (elapsed.as_millis() / 250).is_multiple_of(2)
}

fn spinner(elapsed: Duration) -> char {
    SPINNER[(elapsed.as_millis() / 80) as usize % SPINNER.len()]
}

/// Rows already revealed by the top-down wipe; `None` once it is over.
fn wipe_rows(height: u16, elapsed: Duration) -> Option<u16> {
    (elapsed < WIPE).then(|| (f64::from(height) * eased(elapsed, WIPE)).round() as u16)
}

/// Level colour: red up to 20% left, yellow at 35%, green from 50% (the
/// dashboard's thresholds), blended in between.
fn gradient(ratio: f64) -> Color {
    let mix = |a: (u8, u8, u8), b: (u8, u8, u8), t: f64| {
        let ch = |x: u8, y: u8| (f64::from(x) + (f64::from(y) - f64::from(x)) * t).round() as u8;
        Color::Rgb(ch(a.0, b.0), ch(a.1, b.1), ch(a.2, b.2))
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

/// Per-card tween (from, to, start) plus the last class for the blink.
struct Card {
    from: Vec<Option<f64>>,
    to: Vec<Option<f64>>,
    start: Instant,
    class: Option<AccountClass>,
    blink: Option<Instant>,
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
    screen: Option<(bool, bool, bool)>,
    wipe: Option<Instant>,
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
            screen: None,
            wipe: None,
        }
    }

    /// Every loop iteration: frame clock, per-account refresh flags and the
    /// open screen (details, Sobre, Modelos); a screen change starts a wipe.
    pub(crate) fn track(
        &mut self,
        refreshing: impl Iterator<Item = bool>,
        screen: (bool, bool, bool),
        now: Instant,
    ) {
        self.now = now;
        self.refreshing.clear();
        self.refreshing.extend(refreshing);
        if self.screen.is_some_and(|old| old != screen) {
            self.wipe = Some(now);
        }
        self.screen = Some(screen);
    }

    /// Every dashboard draw: retarget a card's tween when its bars change and
    /// blink its class label when the class differs between two reports.
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
            let class = report.as_ref().map(|report| classify_report(Some(report)));
            let Some(card) = self.cards.get_mut(&account.state_key()) else {
                self.cards.insert(
                    account.state_key(),
                    Card {
                        from: Vec::new(),
                        to: targets().collect(),
                        start: now,
                        class,
                        blink: None,
                    },
                );
                continue;
            };
            if !card.to.iter().copied().eq(targets()) {
                let to: Vec<_> = targets().collect();
                card.from = (0..to.len())
                    .map(|metric| Some(card.value(metric, now).unwrap_or(0.0)))
                    .collect();
                card.to = to;
                card.start = now;
            }
            if class.is_some() {
                if card.class.is_some() && card.class != class {
                    card.blink = Some(now);
                }
                card.class = class;
            }
        }
    }

    /// True while any effect still has frames to draw.
    pub(crate) fn active(&self) -> bool {
        let since = |start: Instant| self.now.saturating_duration_since(start);
        self.refreshing.contains(&true)
            || self.wipe.is_some_and(|start| since(start) < WIPE)
            || self.cards.values().any(|card| {
                (!card.from.is_empty() && since(card.start) < TWEEN)
                    || card.blink.is_some_and(|start| since(start) < BLINK)
            })
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

    pub(crate) fn blinking(&self, account: &str) -> bool {
        self.cards
            .get(account)
            .and_then(|card| card.blink)
            .is_some_and(|start| blink_on(self.now.saturating_duration_since(start)))
    }

    /// Blank the rows the top-down wipe has not revealed yet.
    pub(crate) fn wipe(&self, buffer: &mut Buffer) {
        let area = buffer.area;
        let Some(rows) = self
            .wipe
            .and_then(|start| wipe_rows(area.height, self.now.saturating_duration_since(start)))
        else {
            return;
        };
        for y in area.top() + rows..area.bottom() {
            for x in area.left()..area.right() {
                buffer[(x, y)].reset();
                buffer[(x, y)].set_bg(BG);
            }
        }
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
    fn blink_flashes_twice_under_three_per_second() {
        let on: Vec<bool> = (0..8).map(|i| blink_on(MS(i * 125))).collect();
        assert_eq!(on, [true, true, false, false, true, true, false, false]);
        assert!(!blink_on(BLINK));
    }

    #[test]
    fn wipe_reveals_top_down_then_ends() {
        assert_eq!(wipe_rows(40, MS(0)), Some(0));
        assert!(wipe_rows(40, MS(60)).is_some_and(|rows| rows > 20 && rows < 40));
        assert_eq!(wipe_rows(40, WIPE), None);
    }

    #[test]
    fn gradient_follows_dashboard_thresholds() {
        assert_eq!(gradient(0.0), Color::Rgb(255, 85, 85));
        assert_eq!(gradient(0.2), Color::Rgb(255, 85, 85));
        assert_eq!(gradient(0.35), Color::Rgb(241, 250, 140));
        assert_eq!(gradient(1.0), Color::Rgb(80, 250, 123));
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
        anim.track([false].into_iter(), (false, false, false), start + MS(ms));
    }

    #[test]
    fn refresh_tweens_old_to_new_then_goes_idle() {
        let mut anim = Anim::new(false);
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
    fn class_change_blinks_but_first_report_does_not() {
        let mut anim = Anim::new(false);
        let start = anim.now;
        anim.observe(&accounts(), &[None]);
        anim.observe(&accounts(), &[ready(5.0)]);
        assert!(
            !anim.blinking(&key()),
            "pending -> data is not a class change"
        );
        at(&mut anim, start, 400);
        anim.observe(&accounts(), &[ready(0.0)]);
        assert!(anim.blinking(&key()), "Disponível -> Esgotado blinks");
        at(&mut anim, start, 400 + 300);
        assert!(!anim.blinking(&key()));
        at(&mut anim, start, 400 + 600);
        assert!(anim.blinking(&key()), "second flash");
        at(&mut anim, start, 400 + 750);
        assert!(!anim.blinking(&key()) && !anim.active());
    }

    #[test]
    fn spinner_and_wipe_keep_the_loop_active() {
        let mut anim = Anim::new(false);
        let start = anim.now;
        anim.track([false, true].into_iter(), (false, false, false), start);
        assert_eq!((anim.spinner(0), anim.spinner(1)), (None, Some('⠋')));
        assert_eq!(anim.spinner(9), None);
        assert!(anim.active());
        anim.track([false].into_iter(), (true, false, false), start + MS(80));
        assert!(anim.active(), "opening details wipes");
        let mut buffer = Buffer::with_lines(["xx", "xx", "xx", "xx"]);
        anim.track(
            [false].into_iter(),
            (true, false, false),
            start + MS(80 + 30),
        );
        anim.wipe(&mut buffer);
        let rows: Vec<&str> = (0..4).map(|y| buffer[(0, y)].symbol()).collect();
        assert_eq!(rows, ["x", "x", " ", " "]);
        anim.track(
            [false].into_iter(),
            (true, false, false),
            start + MS(80 + 120),
        );
        assert!(!anim.active(), "wipe over on close of the 120 ms window");
    }
}
