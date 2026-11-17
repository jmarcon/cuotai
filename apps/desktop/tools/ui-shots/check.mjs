#!/usr/bin/env node
// PRD 0022 RF14.2 (DS-MSG-14..16): the overflow check of the mockups, run against the REAL UI in the browser demo at
// the minimum sizes (tray 380×600, main 520 and 1200 wide, settings 640×460) in en-US, pt-BR, es, it and the en-XA
// pseudo-locale. It reports (a) text wider than its box with ellipsis or hidden overflow, (b) text or controls
// outside the window, (c) text clipped by an ancestor, (d) text spilling out of a chip, button, tab or card, (e)
// overlapping text, (f) a status line taller than one line, (g) a placeholder wider than its field, and (h) a figure
// wrapped away from its unit. Elements that may truncate by design (account labels
// and names, the status label after its fitting, one-line notices) are reported as allowed.
// Usage: node apps/desktop/tools/ui-shots/check.mjs [--json] [--selftest] [--zoom] [--shard i/n] [locale ...]
//        node apps/desktop/tools/ui-shots/check.mjs --shard-selftest   Exit: 0 clean, 1 with problems, 2 bad --shard.
// `--shard i/n` runs every n-th item of the run from the i-th (CI splits the full run in parallel jobs this way).
// `--selftest` forces a long string into a figure and a tab of the tray and passes only when the check catches it.
// Every full run also measures a danger button (Revoke token, Erase log, Remove) in every built-in theme and mode:
// its computed text color on its computed fill must reach 4.5:1 (DS-COL-6), which no token-level gate can see when a
// theme's selector repaints buttons. It also renders the dashboard, Stats and the tray in the reference themes
// (PRD 0022 re-validation V-09) and measures what the token gate cannot: a stale figure's effective opacity (50 %,
// DS-CMP-HON-3) and the rendered contrast of chip text on its tint and of Stats text on the page (DS-CMP-BADGE-2,
// DS-COL-5/7).
import { launch } from "./driver.mjs";
import { THEMES } from "../../ui/js/themes.js";

const args = process.argv.slice(2);
const json = args.includes("--json");
const selftest = args.includes("--selftest");
// `--zoom` (PRD 0020 RNF): the 200 % zoom pass. At 200 % a 1280×820 screen offers a 640×410 CSS viewport, so the main
// window and Settings are checked there (the tray is a fixed 380 px popover whose content does not reflow).
const zoom = args.includes("--zoom");
// `--pages a,b` (PRD 0025 re-validation) checks only the pages whose names start with one of these prefixes, and
// skips the themed and danger passes: a quick local look at the surfaces a change touched. CI runs everything.
// A name ending in `$` matches that page alone (`--pages 'main$'` is the main window, not every `main-*`).
const pagesArg = args[args.indexOf("--pages") + 1];
const only = args.includes("--pages") ? (pagesArg ?? "").split(",").filter(Boolean) : null;
// `--shard i/n` and `--pages` take a value, which is not a locale.
const locales = args.filter((a, k) => !a.startsWith("--") && args[k - 1] !== "--shard" && args[k - 1] !== "--pages");
const LOCALES = locales.length ? locales : selftest ? ["en-US"] : ["en-US", "pt-BR", "es", "it", "de", "ru", "ko", "hi-IN", "id-ID", "tr", "vi", "zh-CN", "fr", "ja", "en-XA"];
// PRD 0030 re-validation (L-16): the demo's plan, second-currency, price-age and settings-note fixtures.
const COST_PLANS = "fixture=six&cost&cost-plans&cost-age";
const COST_FOLDS = ['.card[data-card="claude::work@example.com"] .cost-fold summary', '.card[data-card="claude::side@example.com"] .cost-fold summary'];
const ZOOM_PAGES = [
  ["zoom200-main", "index.html", 640, 410, {}],
  ["zoom200-main-stats", "index.html", 640, 410, { "cuotai.openView": "stats" }],
  // PRD 0023 re-validation (V-01, V-09): the extra-usage states (provider units, capped, unknown, armed) at 200 %.
  ["zoom200-main-overage", "index.html?overage", 640, 410, {}],
  // PRD 0033 re-validation (V-15): the renewed slot and the labelled pace line at 200 %.
  ["zoom200-main-renewed", "index.html?renewed", 640, 410, {}],
  ["zoom200-settings-general", "settings.html?page=general", 640, 360, {}],
  ["zoom200-settings-dashboard", "settings.html?page=dashboard", 640, 360, {}],
  ["zoom200-settings-alerts", "settings.html?page=alerts", 640, 360, {}],
  // PRD 0032 re-validation R-03: the quiet-hours sub-page with the demo's schedule on (`?quiet`).
  ["zoom200-settings-quiet", "settings.html?page=alerts&sub=quiet&quiet", 640, 360, {}],
  ["zoom200-settings-menubar", "settings.html?page=menubar", 640, 360, {}],
  // PRD 0020 re-validation V-15: every Settings page at the 200 % viewport.
  ["zoom200-settings-data", "settings.html?page=data", 640, 360, {}],
  ["zoom200-settings-integrations", "settings.html?page=integrations", 640, 360, {}],
  ["zoom200-settings-cost", "settings.html?page=cost&cost", 640, 360, {}],
  ["zoom200-settings-cost-plans", `settings.html?page=cost&sub=cost-plans&${COST_PLANS}`, 640, 360, {}],
  ["zoom200-settings-extensions", "settings.html?page=extensions", 640, 360, {}],
  ["zoom200-settings-files", "settings.html?page=files", 640, 360, {}],
  ["zoom200-settings-about", "settings.html?page=about", 640, 360, {}],
];
const PAGES = zoom ? ZOOM_PAGES : [
  ["tray", "tray.html", 380, 600, {}],
  ["tray-rings", "tray.html", 380, 600, { "cuotai.trayView": "rings" }],
  ["tray-compact", "tray.html", 380, 600, { "cuotai.trayView": "compact" }],
  // The 24-hour strip under the rows (with its hidden table), with many accounts and with few: the popover's list is
  // its only scroll container, the document never scrolls.
  ["tray-strip", "tray.html?strip", 380, 600, {}],
  ["tray-strip-few", "tray.html?fixture=six&strip", 380, 600, {}],
  ["main-narrow", "index.html", 520, 820, {}],
  ["main-narrow-cost", "index.html?cost", 520, 820, {}],
  ["main-two-columns-cost", "index.html?cost", 820, 820, {}],
  ["main-one-row-topbar", "index.html", 905, 600, {}],
  ["main-one-row-topbar-wide", "index.html", 1105, 600, {}],
  ["main", "index.html", 1200, 820, {}],
  // PRD 0023 re-validation (V-01, V-03, V-05, V-09): every extra-usage state on the card and the tray.
  ["main-overage", "index.html?overage", 1200, 820, {}],
  ["main-overage-narrow", "index.html?overage", 520, 820, {}],
  ["tray-overage", "tray.html?overage", 380, 600, {}],
  // PRD 0033 re-validation (V-15): a renewed window beside a current one, a restored reading whose only quota renewed,
  // a labelled pace line on another window than the binding one, readings whose data lags the query, and the AWS
  // Bedrock account as the core reports it.
  ["main-renewed", "index.html?renewed", 1200, 820, {}],
  ["main-renewed-narrow", "index.html?renewed", 520, 820, {}],
  ["main-lagging", "index.html?lagging&metered", 1200, 820, {}],
  ["tray-renewed", "tray.html?renewed&lagging", 380, 600, {}],
  ["main-win98-light", "index.html?fixture=six", 1200, 820, { "cuotai.theme": "win98", "cuotai.themeMode": "light" }],
  ["main-win98-dark", "index.html?fixture=six", 1200, 820, { "cuotai.theme": "win98", "cuotai.themeMode": "dark" }],
  ["main-winxp-light", "index.html?fixture=six", 1200, 820, { "cuotai.theme": "winxp", "cuotai.themeMode": "light" }],
  ["main-winxp-dark", "index.html?fixture=six", 1200, 820, { "cuotai.theme": "winxp", "cuotai.themeMode": "dark" }],
  ["settings-general", "settings.html?page=general", 640, 460, {}],
  // PRD 0035 re-validation V35-15 (DS-CMP-SEL-2): the open language menu stays inside the window, at the minimum and at
  // the default Settings size (`settings_window.rs`).
  ["settings-general-lang-open", "settings.html?page=general", 640, 460, {}, "[data-lang-toggle]"],
  ["settings-general-lang-open-default", "settings.html?page=general", 860, 640, {}, "[data-lang-toggle]"],
  // PRD 0035 re-validation S-03: a user language pack active (the demo's `?pack`, Dutch): the "user pack · file" marker
  // in the field and the open list, and the Settings › General line naming the file; the main window shows its notice.
  ["settings-general-pack", "settings.html?page=general&pack", 640, 460, { "cuotai.locale": "nl" }],
  ["settings-general-pack-open", "settings.html?page=general&pack", 640, 460, { "cuotai.locale": "nl" }, "[data-lang-toggle]"],
  ["main-pack", "index.html?pack", 1200, 820, { "cuotai.locale": "nl" }],
  ["settings-dashboard", "settings.html?page=dashboard", 640, 460, {}],
  ["settings-alerts", "settings.html?page=alerts", 640, 460, {}],
  // PRD 0032 re-validation R-03: the quiet-hours sub-page with the demo's schedule on (weekday rows, two intervals on
  // Monday, the clock read-outs), and the "crossed while closed" chips on the cards (`?missed`).
  ["settings-quiet", "settings.html?page=alerts&sub=quiet&quiet", 640, 460, {}],
  ["main-missed", "index.html?missed", 1200, 820, {}],
  ["settings-menubar", "settings.html?page=menubar", 640, 460, {}],
  // PRD 0025 re-validation V-15/V-19: the Row display sub-page, a custom menu-bar selection (Settings and the popover's
  // "Not in menu bar" group) and the non-Standard row layouts.
  ["settings-rows", "settings.html?page=dashboard&sub=rows", 640, 460, {}],
  ["settings-menubar-custom", "settings.html?page=menubar&traysel=3", 640, 460, {}],
  ["tray-custom-selection", "tray.html?traysel=3", 380, 600, {}],
  ["tray-rows-minimal", "tray.html?rows=minimal", 380, 600, {}],
  ["tray-rows-detailed", "tray.html?rows=detailed", 380, 600, {}],
  ["main-rows-minimal", "index.html?rows=minimal", 1200, 820, {}],
  ["settings-about", "settings.html?page=about", 640, 460, {}],
  // PRD 0020 re-validation V-15: the other pages, Integrations also with its disclosures open.
  ["settings-data", "settings.html?page=data", 640, 460, {}],
  ["settings-integrations", "settings.html?page=integrations", 640, 460, {}],
  ["settings-integrations-open", "settings.html?page=integrations", 640, 460, { "cuotai.settings.disc.serve-connection": "open", "cuotai.settings.disc.widgets-path": "open" }],
  ["settings-extensions", "settings.html?page=extensions", 640, 460, {}],
  ["settings-files", "settings.html?page=files", 640, 460, {}],
  // PRD 0015 V-02: the Local cost page with its sources on, and the worker paused (the status line inside its group).
  ["settings-cost", "settings.html?page=cost&cost", 640, 460, {}],
  ["settings-cost-paused", "settings.html?page=cost&cost&cost-paused", 640, 460, {}],
  ["main-cost", "index.html?cost", 1200, 820, {}],
  ["main-stats", "index.html", 1200, 820, { "cuotai.openView": "stats" }],
  ["tray-stats", "tray.html", 380, 600, {}, "[data-open-stats]"],
  // PRD 0026 re-validation (V-07, V-08): the 30-day range in the tray and in the narrow window, where day labels are
  // densest. Every `*stats*` page also runs STATS_JS (words split mid-word, chart labels outside their card).
  ["tray-stats-30", "tray.html", 380, 600, {}, ["[data-open-stats]", '[data-stats-range="days30"]']],
  ["main-stats-30", "index.html", 520, 820, { "cuotai.openView": "stats" }, '[data-stats-range="days30"]'],
  // PRD 0002 V-01: an account expanded with Models open (the demo's failed-sync fixture), so the sync bar is checked.
  ["tray-models", "tray.html?models=failed", 380, 600, {}, "[data-open]", true],
  ["details-short", "index.html", 1200, 500, {}, "[data-details]"],
  // PRD 0033 re-validation (V-15): the details of the preserved reading whose 5 h window renewed.
  ["details-renewed", "index.html?renewed", 1200, 820, {}, '.card[data-card="claude::mixed@example.com"] [data-details]'],
  ["details-narrow", "index.html", 520, 600, {}, "[data-details]"],
  // PRD 0028 RF1.3: the demo's Zed account, whose response changed, with its "What changed" block.
  ["details-drift", "index.html", 1200, 820, {}, '.card[data-card*="zed"] [data-details]'],
  ["catalog-short", "index.html", 1200, 500, {}, "[data-add]"],
  ["catalog-minimum", "index.html", 520, 480, {}, "[data-add]"],
  // PRD 0022 re-validation V-09: every other dialog, each over its own window (an array of clicks runs in order).
  ["dialog-extensions", "index.html", 1200, 820, {}, "[data-extensions]"],
  ["dialog-report", "index.html", 1200, 820, {}, [".card:has(.sum-note.cause) [data-details]", "[data-report]"]],
  ["dialog-first-run", "index.html?first-run", 1200, 820, {}],
  ["dialog-libraries", "settings.html?page=about", 640, 460, {}, "[data-libraries]"],
  ["dialog-whats-new", "settings.html?page=about", 640, 460, {}, "[data-whats-new]"],
  ["dialog-backup", "settings.html?page=files", 640, 460, {}, "[data-backup]"],
  // PRD 0029 re-validation (V-01, V-04): the Add account form, beside a named Groq account (the name is required, no
  // peer field) and, with `?unnamed`, beside an unnamed one (the peer field shows); every `hidden` part stays hidden.
  ["dialog-add", "index.html", 1200, 820, {}, ["[data-add]", '.pick[data-provider="groq"]']],
  ["dialog-add-peer", "index.html?unnamed", 1200, 820, {}, ["[data-add]", '.pick[data-provider="groq"]']],
  // PRD 0033 re-validation (V-15): the AWS Bedrock form (profile, CLI path, SSO login, interval and cost advisory).
  ["dialog-add-bedrock", "index.html", 1200, 820, {}, ["[data-add]", '.pick[data-provider="aws_bedrock"]']],
  // PRD 0033 re-validation (V-06): the confirmation before a billed refresh of the AWS Bedrock account, in the main
  // window and in the popover.
  ["dialog-billed-refresh", "index.html?metered", 1200, 820, {}, '.card[data-card="aws_bedrock::billing@example.com"] [data-refresh]'],
  ["tray-billed-refresh", "tray.html?metered", 380, 600, {}, ['[data-open="aws_bedrock::billing@example.com"]', '[data-refresh-one]']],
  // V-03, V-07: unnamed accounts (a plain one and the 9router instance) on the cards, the tray and Stats.
  ["main-unnamed", "index.html?unnamed", 1200, 820, {}],
  ["tray-unnamed", "tray.html?unnamed", 380, 600, {}],
  // PRD 0030 re-validation (C-05..C-08, L-16): two Claude accounts with only the first planned, a second currency, a
  // 319-day user price table and an unpriced model. The cards with their local usage open, the details of the planned
  // account (plan line, period in the second currency, the table's age on its line, "No price for", the settings
  // note) and of the unplanned one ("Add plan price", no plan line), and Settings › Plans.
  ["main-cost-plans", `index.html?${COST_PLANS}`, 1200, 820, {}, COST_FOLDS],
  ["main-narrow-cost-plans", `index.html?${COST_PLANS}`, 520, 820, {}, COST_FOLDS],
  ["details-cost-plans", `index.html?${COST_PLANS}`, 1200, 820, {}, '.card[data-card="claude::work@example.com"] [data-details]'],
  ["details-cost-unplanned", `index.html?${COST_PLANS}`, 520, 600, {}, '.card[data-card="claude::side@example.com"] [data-details]'],
  ["settings-cost-plans", `settings.html?page=cost&sub=cost-plans&${COST_PLANS}`, 640, 460, {}],
  // PRD 0036 re-validation (V-12): the 9router and LiteLLM add forms (address, filters, TLS, vault password or pasted
  // session, Test connection) and a failed Test connection with a long result (`?aggtest=locked`).
  ["dialog-add-9router", "index.html", 1200, 820, {}, ["[data-add]", '.pick[data-provider="nine_router"]']],
  ["dialog-add-litellm", "index.html", 1200, 820, {}, ["[data-add]", '.pick[data-provider="litellm"]']],
  ["dialog-add-9router-warn", "index.html?aggtest=locked", 1200, 820, {}, ["[data-add]", '.pick[data-provider="nine_router"]', "#aggregator-test"]],
  // Aggregator instances of every size (`?agg=`): none, 12 (a duplicate and one off), 40 (long labels), an exhausted
  // pool and a LiteLLM proxy; the 12-connection instance expanded in the popover, its details and a connection's.
  ["main-agg-0", "index.html?agg=0,1", 1200, 820, {}],
  ["main-agg-12", "index.html?agg=12", 1200, 820, {}],
  ["main-agg-40", "index.html?agg=40", 1200, 820, {}],
  ["main-agg-exhausted", "index.html?agg=exhausted", 1200, 820, {}],
  ["main-agg-litellm", "index.html?agg=litellm", 1200, 820, {}],
  ["tray-agg-expanded", "tray.html?agg=12", 380, 600, {}, '[data-agg-toggle="nine_router::lab"]'],
  ["details-agg-instance", "index.html?agg=12", 1200, 820, {}, '.agg-section[data-card="nine_router::lab"] .agg-head [data-details]'],
  ["details-agg-connection", "index.html?agg=12", 1200, 820, {}, '.agg-section[data-card="nine_router::lab"] .card [data-details]'],
  // Re-validation S-03: Test connection meets a certificate this computer does not trust (first sighting) and a pinned
  // certificate that changed (its own dialog: old and new fingerprints, Cancel focused, Replace behind a checkbox).
  ["dialog-cert-first", "index.html?aggtest=cert-first", 1200, 820, {}, ["[data-add]", '.pick[data-provider="nine_router"]', "#aggregator-test"]],
  ["dialog-cert-changed", "index.html?aggtest=cert-changed", 1200, 820, {}, ["[data-add]", '.pick[data-provider="nine_router"]', "#aggregator-test"]],
  // PRD 0037 RF4: a reset already past when the provider answered: "Renewal due since …" on the card, the tray and the
  // details, never a past "blocked until".
  ["main-renewal-due", "index.html?renewal-due", 1200, 820, {}],
  ["tray-renewal-due", "tray.html?renewal-due", 380, 600, {}],
  ["details-renewal-due", "index.html?renewal-due", 1200, 820, {}, '.card[data-card="claude::renewal@example.com"] [data-details]'],  // PRD 0037 re-validation (V-09): the ElevenLabs set (extra usage armed, active, capped and with no cap, a missing
  // limit, the zero-limit free plan, a renewal due, a key without permission, a changed response, the EU incident beside
  // the operational US page), its details and tray rows, and the Add account form with the core's region choice.
  ["main-eleven", "index.html?eleven", 1200, 820, {}],
  ["main-eleven-narrow", "index.html?eleven", 520, 820, {}],
  ["tray-eleven", "tray.html?eleven", 380, 600, {}],
  ["details-eleven", "index.html?eleven", 1200, 820, {}, '.card[data-card="eleven_labs::unlimited@example.com"] [data-details]'],
  ["details-eleven-incident", "index.html?eleven", 1200, 820, {}, '.card[data-card="eleven_labs::eu@example.com"] [data-details]'],
  ["dialog-add-eleven", "index.html", 1200, 820, {}, ["[data-add]", '.pick[data-provider="eleven_labs"]']],
];
// V-09: the reference themes (dark, light, a retro theme with a picture background, and a monochrome one), in en-US.
const THEME_PAGES = [["dracula", "dark"], ["github", "light"], ["win98", "light"], ["crt", "dark"]].flatMap(([theme, mode]) => [
  [`themed-main-${theme}-${mode}`, "index.html", 1200, 820, { "cuotai.theme": theme, "cuotai.themeMode": mode }],
  [`themed-stats-${theme}-${mode}`, "index.html", 1200, 820, { "cuotai.theme": theme, "cuotai.themeMode": mode, "cuotai.openView": "stats" }],
  [`themed-tray-${theme}-${mode}`, "tray.html", 380, 600, { "cuotai.theme": theme, "cuotai.themeMode": mode }],
]);
// DS-MSG-15: what may truncate (with its full text in a tooltip or the accessible name).
export const ALLOWED = [".r1 .nm", ".r1 .ac", ".crow .nm", ".tile-name b", ".tile-st span", ".who h2", ".who p", ".stl.truncated .lbl",
  ".xline span", ".pop-notice span", ".sum-top .lt", ".set-path", ".lc-title", ".models-meta", ".access-line", ".win .reset", ".pop-head h1",
  ".set-nav-item span", ".sum-note", ".tl-name", ".st-name", "option", "select", ".fig.dual .wl.model"].join(", ");

// PRD 0034 re-validation V-10: the units a figure must stay on one line with (see the figure-split rule below).
const FIGURE_UNITS = ["s", "ms", "min", "h", "d", "j", "g", "T", "Sek.", "Min.", "Std.", "T.", "sa", "dk", "sn", "dtk", "mnt", "hr",
  "KiB", "MiB", "GiB", "KB", "MB", "GB", "pt", "pts", "Pkt.", "tok/min", "Tok./Min.", "%", "с", "мс", "мин", "ч", "д", "дн.", "giây",
  "phút", "giờ", "ngày", "सेकंड", "मिनट", "घंटे", "घं", "मि", "दिन", "AM", "PM"];

const PAGE_JS = `(() => {
  const allowed = ${JSON.stringify(ALLOWED)};
  const out = [];
  const vw = innerWidth;
  const sel = (e) => { const c = typeof e.className === "string" ? e.className.trim().split(/\\s+/).filter(Boolean).slice(0, 3).join(".") : ""; return e.tagName.toLowerCase() + (c ? "." + c : ""); };
  const ok = (e) => !!e.closest(allowed);
  const leaves = [];
  // A modal dialog covers the page: only its own text can collide (the topmost one when a dialog opens another).
  const modal = [...document.querySelectorAll("dialog[open]")].pop() ?? null;
  for (const e of document.querySelectorAll("body *")) {
    if (e.closest("svg, .sr-only, #prov-label, .wm-name, [hidden], dialog:not([open]), .set-pop, .menu, .lang-menu")) continue;
    if (modal && !modal.contains(e)) continue;
    // the hidden part of a closed disclosure (its own text and every child but the summary)
    if (e.matches("details:not([open])") || (e.closest("details:not([open])") && !e.closest("summary"))) continue;
    const cs = getComputedStyle(e);
    if (cs.display === "none" || cs.visibility === "hidden") continue;
    if (cs.position === "absolute" && cs.clip.replace(/[^0-9]/g, "") === "0000") continue; // visually hidden
    const text = [...e.childNodes].filter((n) => n.nodeType === 3).map((n) => n.textContent.replace(/\\s+/g, " ").trim()).join(" ").trim();
    if (!text) continue;
    const r = e.getBoundingClientRect();
    if (!r.width) continue;
    if (e.scrollWidth > e.clientWidth + 1 && (cs.textOverflow === "ellipsis" || cs.overflowX !== "visible")) out.push({ kind: "truncated", ok: ok(e), sel: sel(e), text: text.slice(0, 60) });
    // The visible part of a box: intersected with every ancestor (and itself) that clips its overflow.
    let clipper = null;
    const clip = (x) => {
      let L = x.left, T = x.top, R = x.right, B = x.bottom;
      for (let p = e; p && p !== document.documentElement; p = p.parentElement) {
        const c = getComputedStyle(p);
        if (c.overflowX !== "visible" || c.overflowY !== "visible") {
          const q = p.getBoundingClientRect();
          if (R > q.right + 1 && !clipper) clipper = p;
          L = Math.max(L, q.left); R = Math.min(R, q.right);
          if (!p.matches(".pop-body, .set-content, body")) { T = Math.max(T, q.top); B = Math.min(B, q.bottom); } // scroll areas: all their content counts
        }
      }
      return R - L > 1 && B - T > 1 ? { left: L, top: T, right: R, bottom: B } : null;
    };
    const own = [];
    for (const n of e.childNodes) if (n.nodeType === 3 && n.textContent.trim()) { const rg = document.createRange(); rg.selectNodeContents(n); for (const x of rg.getClientRects()) { if (x.width <= 1 || x.height <= 1) continue; const c = clip(x); if (c) own.push(c); } }
    const seen = clip(r);
    if (seen && (seen.right > vw + 1 || seen.left < -1)) out.push({ kind: "outside-viewport", ok: false, sel: sel(e), text: text.slice(0, 60) });
    if (clipper && clipper !== e) out.push({ kind: "clipped-by-parent", ok: ok(e) || ok(clipper), sel: sel(e), text: text.slice(0, 60), parent: sel(clipper) });
    if (own.length) leaves.push({ e, own, text, ok: ok(e) });
    const box = e.closest(".schip, .mini, .btn, .tab, .opt-card, .seg [role=radio], .fig, .kbd, kbd");
    if (box && box !== e && !ok(e)) { const br = box.getBoundingClientRect(); if (own.some((x) => x.right > br.right + 1 || x.left < br.left - 1)) out.push({ kind: "spills-container", ok: false, sel: sel(e) + " in " + sel(box), text: text.slice(0, 40) }); }
  }
  // PRD 0029 re-validation (V-01): an element carrying \`hidden\` is never drawn, whatever its class's \`display\` says.
  // An open menu (language, sort, theme) lies entirely inside the window: it opens upward or shrinks when needed.
  for (const m of document.querySelectorAll(".lang-menu:not([hidden])")) { const r = m.getBoundingClientRect(); if (r.width && (r.top < 0 || r.left < 0 || r.bottom > innerHeight || r.right > innerWidth)) out.push({ kind: "menu-outside-window", ok: false, sel: sel(m), text: [r.top, r.bottom, r.left, r.right].map(Math.round).join(" ") }); }
  for (const e of document.querySelectorAll("[hidden]")) if (e.checkVisibility()) out.push({ kind: "hidden-shown", ok: false, sel: sel(e) + (e.id ? "#" + e.id : ""), text: (e.textContent || "").trim().slice(0, 40) });
  for (const s of document.querySelectorAll(".rc > .stl")) if (s.getBoundingClientRect().height > 22) out.push({ kind: "status-line-taller-than-one-line", ok: false, sel: sel(s), text: s.textContent.trim().slice(0, 50) });
  // The local-cost row's title and its "estimate" mark stay on one line each; only the values may wrap.
  for (const s of document.querySelectorAll(".cost-fold .cf-t, .cost-fold .cf-e")) { const rg = document.createRange(); rg.selectNodeContents(s); if (new Set([...rg.getClientRects()].map((x) => Math.round(x.top))).size > 1) out.push({ kind: "title-wraps", ok: false, sel: sel(s), text: s.textContent.trim().slice(0, 50) }); }
  // A card's limit columns: a name never breaks inside a word, and meters on one row of columns line up.
  for (const n of document.querySelectorAll(".wins .win-name")) { const rg = document.createRange(); rg.selectNodeContents(n); const lines = new Set([...rg.getClientRects()].map((x) => Math.round(x.top))).size; if (lines > n.textContent.trim().split(/\\s+/).length) out.push({ kind: "word-broken", ok: false, sel: sel(n), text: n.textContent.trim().slice(0, 50) }); }
  for (const w of document.querySelectorAll(".wins")) {
    const rows = new Map();
    for (const c of w.children) { const m = c.querySelector(":scope > .mtr"); if (!m) continue; const top = Math.round(c.getBoundingClientRect().top); rows.set(top, [...(rows.get(top) ?? []), Math.round(m.getBoundingClientRect().top)]); }
    for (const tops of rows.values()) if (new Set(tops).size > 1) out.push({ kind: "limits-misaligned", ok: false, sel: "div.wins", text: tops.join(" / ") });
  }
  // DS-CMP-TAB-4: the topbar's filter tabs never clip: what does not fit is reachable through "More".
  const strip = document.querySelector(".tb-tabs .tabs");
  if (strip && strip.scrollWidth > strip.parentElement.clientWidth + 1) out.push({ kind: "tabs-clipped", ok: false, sel: "div.tabs", text: strip.scrollWidth + " > " + strip.parentElement.clientWidth });
  // DS-CMP-HEAD-2: the tray overview header starts with the Cuotai mark button (opens the main window): named, a
  // 24 px target at least, before the tabs without overlapping them, and the tabs stay inside the header.
  const popHead = document.querySelector(".pop-head:not(.drill)"), popTabs = popHead?.querySelector(":scope > .tabs");
  if (popTabs) {
    const home = popHead.querySelector(":scope > button.pop-home[data-open-main]"), hd = popHead.getBoundingClientRect(), tb = popTabs.getBoundingClientRect();
    if (!home || !home.getAttribute("aria-label") || !home.title) out.push({ kind: "open-window-control", ok: false, sel: "header.pop-head", text: home ? "no name or tooltip" : "missing" });
    else {
      const hr = home.getBoundingClientRect();
      if (hr.width < 24 || hr.height < 24) out.push({ kind: "small-target", ok: false, sel: "button.pop-home", text: hr.width + "x" + hr.height });
      if (hr.left < hd.left - 1 || hr.right > tb.left + 1) out.push({ kind: "open-window-overlaps-tabs", ok: false, sel: "button.pop-home", text: hr.right + " > " + tb.left });
    }
    if (tb.right > hd.right + 1) out.push({ kind: "tabs-clipped", ok: false, sel: "header.pop-head div.tabs", text: tb.right + " > " + hd.right });
  }
  // A retro theme's control or title-bar rule must not take the text color away from a primary button, a plan badge or a button on the title bar.
  const rgb = (c) => [...c.matchAll(/rgba?\\(([^)]+)\\)/g)].map((m) => m[1].split(/[ ,\\/]+/).map(Number)).filter((p) => p.length < 4 || p[3] > 0).map((p) => p.slice(0, 3));
  const lum = (p) => { const f = p.map((v) => { v /= 255; return v <= 0.03928 ? v / 12.92 : ((v + 0.055) / 1.055) ** 2.4; }); return 0.2126 * f[0] + 0.7152 * f[1] + 0.0722 * f[2]; };
  for (const e of document.querySelectorAll(".btn.primary:not(:disabled), .card-head .plan-badge, .topbar .btn.outline")) {
    if (!e.getClientRects().length) continue;
    let bg = [];
    for (let p = e; p && !bg.length; p = p.parentElement) { const c = getComputedStyle(p); bg = rgb(c.backgroundImage === "none" ? c.backgroundColor : c.backgroundImage); }
    const fg = rgb(getComputedStyle(e).color)[0];
    const worst = Math.min(...bg.map((b) => { const [x, y] = [lum(fg), lum(b)]; return (Math.max(x, y) + 0.05) / (Math.min(x, y) + 0.05); }));
    // On a title bar the measure is its lightest gradient stop, the theme's own limit for white text there.
    if (worst < (e.matches(".btn.primary") ? 4.5 : 3)) out.push({ kind: "low-contrast", ok: false, sel: sel(e), text: worst.toFixed(1) + ":1 " + e.textContent.trim().slice(0, 30) });
  }
  // Everything in a Settings group keeps the rows' inset from the card's edge; the group's label sits above the card.
  for (const s of document.querySelectorAll(".set-content .set-sec")) {
    const box = s.getBoundingClientRect();
    // An open sub-page turns its page's group into a transparent frame (settings.css); the sub-page's own cards are
    // checked as sections of their own (PRD 0025 re-validation V-15).
    if (!box.width || s.querySelector(":scope .set-subpage.on")) continue;
    for (const e of s.querySelectorAll("h2, h3, p, button, pre, code")) { if (e.closest("[hidden], dialog, .set-pop, .menu, summary") || !e.textContent.trim()) continue; const r = e.getBoundingClientRect(), inset = e.matches("button") ? 0 : parseFloat(getComputedStyle(e).paddingLeft); if (r.width && (e.matches("h2") ? r.bottom > box.top + 1 : r.left + inset < box.left + 8)) out.push({ kind: "flush-to-group-edge", ok: false, sel: sel(e), text: e.textContent.trim().slice(0, 50) }); }
  }
  // A Settings row's label never runs under its control (the control drops below when both do not fit).
  for (const row of document.querySelectorAll(".set-row")) {
    const l = row.querySelector(":scope > .set-row-text > .set-row-label"), c = row.querySelector(":scope > .set-row-ctl");
    if (!l || !c || !l.textContent.trim() || row.closest("[hidden]")) continue;
    const cr = c.getBoundingClientRect(), rg = document.createRange();
    rg.selectNodeContents(l);
    if (cr.width && [...rg.getClientRects()].some((x) => Math.min(x.right, cr.right) - Math.max(x.left, cr.left) > 1 && Math.min(x.bottom, cr.bottom) - Math.max(x.top, cr.top) > 1)) out.push({ kind: "label-under-control", ok: false, sel: sel(l), text: l.textContent.trim().slice(0, 40) });
  }
  for (const b of document.querySelectorAll("button, input, select")) {
    if (b.closest("[hidden], dialog:not([open]), .menu, .lang-menu, .set-pop")) continue;
    const r = b.getBoundingClientRect();
    if (r.width && (r.right > vw + 1 || r.left < -1)) out.push({ kind: "control-outside-viewport", ok: false, sel: sel(b), text: (b.getAttribute("aria-label") || b.textContent || "").trim().slice(0, 40) });
  }
  const scroller = (e) => e.closest(".pop-body, .set-content, .grid, main");
  for (let i = 0; i < leaves.length; i++) for (let j = i + 1; j < leaves.length; j++) {
    const a = leaves[i], b = leaves[j];
    if (a.e.contains(b.e) || b.e.contains(a.e) || scroller(a.e) !== scroller(b.e)) continue;
    const hit = a.own.some((x) => b.own.some((y) => Math.min(x.right, y.right) - Math.max(x.left, y.left) > 2 && Math.min(x.bottom, y.bottom) - Math.max(x.top, y.top) > 3));
    if (hit) out.push({ kind: "overlap", ok: false, sel: sel(a.e) + " x " + sel(b.e), text: a.text.slice(0, 30) + " | " + b.text.slice(0, 30) });
  }
  // The details dialog and the Add-account catalog scroll inside only, never around it and never sideways.
  if (modal?.matches(".details, .dlg-catalog") && (modal.scrollHeight > modal.clientHeight + 1 || modal.scrollWidth > modal.clientWidth + 1)) out.push({ kind: "nested-scroll", ok: false, sel: "dialog." + modal.className.replaceAll(" ", "."), text: modal.scrollWidth + "x" + modal.scrollHeight + " > " + modal.clientWidth + "x" + modal.clientHeight });
  if (document.documentElement.scrollWidth > vw + 1) out.push({ kind: "horizontal-scroll", ok: false, sel: "html", text: document.documentElement.scrollWidth + " > " + vw });
  // The tray popover scrolls inside its list only: a scrolling document draws a second scroll bar beside the panel.
  // The live region (components.js) joins the body, after the full-height root, once a refresh announces a change.
  if (document.querySelector(".pop") && !document.getElementById("cuotai-live")) {
    const live = document.createElement("div");
    live.id = "cuotai-live"; live.className = "sr-only"; live.textContent = "Claude: 98% left, was 99%";
    document.body.append(live);
  }
  const doc = document.scrollingElement;
  if (document.querySelector(".pop") && doc.scrollHeight > innerHeight + 1) out.push({ kind: "document-scroll", ok: false, sel: "html", text: doc.scrollHeight + " > " + innerHeight });
  // PRD 0034 re-validation V-11: a shown placeholder fits its field's content box (the checks above read text nodes,
  // and a placeholder is none). A transparent placeholder (the filter collapsed to its icon) is not drawn.
  const pen = document.createElement("canvas").getContext("2d");
  for (const e of document.querySelectorAll("input[placeholder], textarea[placeholder]")) {
    if (!e.placeholder || !e.matches(":placeholder-shown") || !e.checkVisibility() || e.closest("[hidden], dialog:not([open]), .set-pop, .menu")) continue;
    if (modal && !modal.contains(e)) continue;
    const ph = getComputedStyle(e, "::placeholder"), cs = getComputedStyle(e);
    if (ph.visibility === "hidden" || /^rgba\\(.*,\\s*0\\)$|^transparent$/.test(ph.color)) continue;
    pen.font = ph.font || cs.font;
    const need = pen.measureText(e.placeholder).width, room = e.clientWidth - parseFloat(cs.paddingLeft) - parseFloat(cs.paddingRight);
    // A textarea wraps its placeholder: it fits when its lines fit the content height (PRD 0036 re-validation V-12).
    const lines = Math.ceil((need * 1.1) / Math.max(room, 1)), line = parseFloat(ph.lineHeight) || parseFloat(cs.lineHeight) || parseFloat(cs.fontSize) * 1.2;
    if (e.tagName === "TEXTAREA" && room > 0 && lines * line <= e.clientHeight - parseFloat(cs.paddingTop) - parseFloat(cs.paddingBottom) + 1) continue;
    if (room > 0 && need > room + 1) out.push({ kind: "placeholder-clipped", ok: false, sel: sel(e) + (e.id ? "#" + e.id : ""), text: Math.round(need) + " > " + Math.round(room) + " " + e.placeholder.slice(0, 40) });
  }
  // PRD 0034 re-validation V-10 (DS-A11Y-7): a figure never wraps away from its unit. A number and the unit after it
  // (the time, size and percent units of the catalogs' figure rule in \`text/parity.rs\`, the CJK time units, a clock's
  // AM/PM), joined by nothing or by a space, sit on one line box. A count before an ordinary word may wrap.
  const UNITS = new Set(${JSON.stringify(FIGURE_UNITS)});
  const walk = document.createTreeWalker(document.body, NodeFilter.SHOW_TEXT);
  for (let n = walk.nextNode(); n; n = walk.nextNode()) {
    const p = n.parentElement;
    if (!p || p.closest("svg, .sr-only, option, select, [hidden], dialog:not([open]), .set-pop, .menu, .lang-menu") || (modal && !modal.contains(p))) continue;
    if ((p.closest("details:not([open])") && !p.closest("summary")) || !p.getClientRects().length) continue;
    for (const m of n.textContent.matchAll(/(\\d)[\\s\\u00a0\\u202f]?(?=([%\\p{L}\\p{M}][\\p{L}\\p{M}.\\/]{0,8}))/gu)) {
      const word = m[2].replace(/[.,;:)]+$/, "");
      if (!UNITS.has(word) && !UNITS.has(m[2].replace(/[,;:)]+$/, "")) && !/^(秒|分|時間|日|小时|天)/u.test(m[2])) continue;
      const top = (from, to) => { const r = document.createRange(); r.setStart(n, from); r.setEnd(n, to); const x = [...r.getClientRects()].find((q) => q.width > 0); return x ? Math.round(x.top) : null; };
      const a = top(m.index, m.index + 1), b = top(m.index + m[0].length, m.index + m[0].length + 1);
      if (a != null && b != null && a !== b) out.push({ kind: "figure-split", ok: false, sel: sel(p), text: n.textContent.slice(Math.max(0, m.index - 12), m.index + m[0].length + m[2].length).replace(/\\s+/g, " ") });
    }
  }
  return out;
})()`;

// Opens the Models disclosure, gives its first read a moment and scrolls it into view (the `tray-models` page).
const OPEN_MODELS = `(async () => { const d = document.querySelector("details.models"); if (!d) return; d.open = true;
  await new Promise((r) => setTimeout(r, 400)); d.scrollIntoView({ block: "start" }); })()`;

// A danger button placed in a Settings group and on the page body, per built-in theme and mode.
const DANGER_JS = `(() => {
  const rgb = (c) => (c.match(/rgba?\\(([^)]+)\\)/) || [, ""])[1].split(/[ ,\\/]+/).map(Number).slice(0, 3);
  const lum = (p) => { const f = p.map((v) => { v /= 255; return v <= 0.03928 ? v / 12.92 : ((v + 0.055) / 1.055) ** 2.4; }); return 0.2126 * f[0] + 0.7152 * f[1] + 0.0722 * f[2]; };
  const out = [];
  for (const host of [document.querySelector(".set-content .set-sec"), document.body]) {
    if (!host) continue;
    const b = document.createElement("button");
    b.className = "btn danger-btn";
    b.textContent = "Revoke token";
    host.append(b);
    const c = getComputedStyle(b), [x, y] = [lum(rgb(c.color)), lum(rgb(c.backgroundColor))];
    out.push((Math.max(x, y) + 0.05) / (Math.min(x, y) + 0.05));
    b.remove();
  }
  return out;
})()`;

// V-09 probes on the rendered page: a stale figure's effective opacity, chip text on its tint, Stats text on the page.
const PROBE_JS = `(() => {
  const out = [];
  const parse = (c) => {
    let m = c.match(/^rgba?\\(([^)]+)\\)/);
    if (m) { const p = m[1].split(/[ ,\\/]+/).filter(Boolean).map(Number); return [p[0], p[1], p[2], p.length > 3 ? p[3] : 1]; }
    m = c.match(/^color\\(srgb ([^)]+)\\)/);
    if (m) { const p = m[1].split(/[ \\/]+/).filter(Boolean).map(Number); return [p[0] * 255, p[1] * 255, p[2] * 255, p.length > 3 ? p[3] : 1]; }
    return null;
  };
  const over = (top, base) => [0, 1, 2].map((i) => top[i] * top[3] + base[i] * (1 - top[3]));
  // The color behind an element: its own and its ancestors' fills, composited from the first opaque one (a picture
  // background counts as its first color stop).
  const behind = (e) => {
    const layers = [];
    for (let p = e; p; p = p.parentElement) {
      const cs = getComputedStyle(p);
      const fill = parse(cs.backgroundColor);
      if (fill && fill[3] > 0) layers.push(fill);
      if (fill && fill[3] >= 1) break;
      if (cs.backgroundImage !== "none") { const stop = cs.backgroundImage.match(/(rgba?\\([^)]+\\)|color\\(srgb [^)]+\\))/); const s = stop && parse(stop[1]); if (s && s[3] >= 1) { layers.push(s); break; } }
    }
    let base = [255, 255, 255];
    for (const l of layers.reverse()) base = l[3] >= 1 ? l.slice(0, 3) : over(l, base);
    return base;
  };
  const lum = (p) => { const f = p.map((v) => { v /= 255; return v <= 0.03928 ? v / 12.92 : ((v + 0.055) / 1.055) ** 2.4; }); return 0.2126 * f[0] + 0.7152 * f[1] + 0.0722 * f[2]; };
  const ratio = (a, b) => { const [x, y] = [lum(a), lum(b)]; return (Math.max(x, y) + 0.05) / (Math.min(x, y) + 0.05); };
  const seen = (e) => e.getClientRects().length && !e.closest("[hidden], dialog:not([open]), .menu, .lang-menu, .sr-only");
  const name = (e) => e.tagName.toLowerCase() + "." + [...e.classList].slice(0, 3).join(".");
  for (const f of document.querySelectorAll(".fig.stale")) {
    if (!seen(f)) continue;
    let o = 1;
    for (let p = f.querySelector("b") ?? f; p; p = p.parentElement) o *= Number(getComputedStyle(p).opacity);
    if (Math.abs(o - 0.5) > 0.01) out.push({ kind: "stale-opacity", ok: false, sel: name(f), text: o.toFixed(2) + " " + f.textContent.trim().slice(0, 20) });
  }
  const text = ".mini, .schip, .tchip, .btag, .badge, .age.stale, .stats-page h2, .stats-page h3, .stats-page .sub, .tl-name, .tl-time, .stats .link, .text-link";
  for (const e of document.querySelectorAll(text)) {
    if (!seen(e) || !e.textContent.trim()) continue;
    const fg = parse(getComputedStyle(e).color);
    if (!fg) continue;
    const r = ratio(fg.slice(0, 3), behind(e));
    if (r < 4.5) out.push({ kind: "low-contrast-text", ok: false, sel: name(e), text: r.toFixed(2) + ":1 " + e.textContent.trim().slice(0, 30) });
  }
  return out;
})()`;

// PRD 0026 re-validation (V-07, V-08) on every Stats page: (a) a word of the visible Stats or timeline text that the
// layout split across two lines (a word without a hyphen of its own: the global `overflow-wrap` breaks it mid-word when
// it does not fit, which no overflow check sees); (b) a chart day label, hidden from assistive technology but on
// screen, that ends outside its card.
const STATS_JS = `(() => {
  const out = [];
  const name = (e) => e.tagName.toLowerCase() + "." + [...e.classList].slice(0, 3).join(".");
  for (const root of document.querySelectorAll(".stats, .timeline")) {
    if (!root.getClientRects().length) continue;
    const walk = document.createTreeWalker(root, NodeFilter.SHOW_TEXT);
    for (let n = walk.nextNode(); n; n = walk.nextNode()) {
      const p = n.parentElement;
      // Text that opted into hyphenation breaks with a hyphen the browser draws; CJK wraps between any two characters.
      if (!p || p.closest(".sr-only, option, select, [hidden]") || !p.getClientRects().length || getComputedStyle(p).hyphens === "auto") continue;
      for (const m of n.textContent.matchAll(/[^\\s\\-‐–—\\/·]{3,}/g)) {
        const r = document.createRange();
        r.setStart(n, m.index); r.setEnd(n, m.index + m[0].length);
        const tops = new Set([...r.getClientRects()].filter((x) => x.width > 0).map((x) => Math.round(x.top)));
        if (tops.size > 1 && !/[\\u3000-\\u9fff\\uac00-\\ud7af\\uff00-\\uffef]/.test(m[0])) out.push({ kind: "word-split", ok: false, sel: name(p), text: m[0].slice(0, 30) });
      }
    }
  }
  for (const label of document.querySelectorAll(".bars .x-label")) {
    const card = label.closest(".st-card, .rhythm, .pop-strip");
    if (!card || !label.textContent.trim() || !label.getClientRects().length) continue;
    const a = label.getBoundingClientRect(), b = card.getBoundingClientRect();
    if (a.left < b.left - 0.5 || a.right > b.right + 0.5) out.push({ kind: "label-outside-card", ok: false, sel: name(label), text: label.textContent.trim() + " " + Math.round(Math.max(b.left - a.left, a.right - b.right)) + "px" });
  }
  return out;
})()`;

// Every unit of work of this run, in a fixed order: each page in each locale, then (full run) the themed pages and
// the danger button of every built-in theme and mode. `--shard i/n` keeps every n-th item from the i-th, so the n
// shards of CI cover the whole list exactly once whatever is added to it; `--shard-selftest` proves that.
function tasks() {
  const list = [];
  for (const locale of LOCALES) {
    for (const [name, page, w, h, storage, click, openModels] of PAGES) {
      // The selftest forces its string into the plain tray views only.
      if (selftest && (!page.startsWith("tray") || storage["cuotai.trayView"])) continue;
      if (only && !only.some((p) => (p.endsWith("$") ? name === p.slice(0, -1) : name.startsWith(p)))) continue;
      list.push({ id: `${name}.${locale}`, run: () => checkPage(name, page, w, h, storage, click, openModels, locale) });
    }
  }
  if (zoom || selftest || only) return list;
  for (const [name, page, w, h, storage] of THEME_PAGES) list.push({ id: `${name}.en-US`, run: () => themedPage(name, page, w, h, storage) });
  for (const theme of THEMES.filter((t) => !t.user)) {
    for (const mode of theme.modes) list.push({ id: `danger.${theme.id}.${mode}`, run: () => dangerButton(theme, mode) });
  }
  return list;
}

/** `i/n` (1 ≤ i ≤ n) → the items of shard i; anything else is a usage error. */
function parseShard(value) {
  const m = /^(\d+)\/(\d+)$/.exec(value ?? "");
  const [i, n] = m ? [Number(m[1]), Number(m[2])] : [0, 0];
  if (!m || n < 1 || i < 1 || i > n) {
    console.error(`ui-shots check: --shard needs i/n with 1 <= i <= n, got ${JSON.stringify(value)}`);
    process.exit(2);
  }
  return [i, n];
}
const pick = (list, i, n) => list.filter((_, k) => k % n === i - 1);

async function checkPage(name, page, w, h, storage, click, openModels, locale) {
  const target = locale === "en-XA" ? `${page}${page.includes("?") ? "&" : "?"}pseudo` : page;
  await ui.open(target, { w, h, scale: 1, locale: locale === "en-XA" ? "en-US" : locale, storage });
  if (click) await ui.click(...[].concat(click));
  if (openModels) await ui.evaluate(OPEN_MODELS);
  if (selftest) await ui.evaluate(`(() => { const f = document.querySelector(".fig b"); if (f) f.textContent = "W".repeat(60); })()`);
  const found = (await ui.evaluate(PAGE_JS)) ?? [{ kind: "error", ok: false, sel: "page", text: "evaluation failed" }];
  if (name.includes("stats")) found.push(...((await ui.evaluate(STATS_JS)) ?? [{ kind: "error", ok: false, sel: "page", text: "stats probe failed" }]));
  // PRD 0034 re-validation V-11: the pseudo-locale pads every text by design, so its placeholder ends in the field's
  // ellipsis (allowed); a real language's placeholder must fit.
  if (locale === "en-XA") for (const f of found) if (f.kind === "placeholder-clipped") f.ok = true;
  const bad = found.filter((f) => !f.ok);
  report[`${name}.${locale}`] = found;
  if (!json) console.log(`${bad.length ? "FAIL" : "ok  "} ${name} ${locale} (${w}×${h}): ${bad.length} problem(s), ${found.length - bad.length} allowed`);
  if (!json) for (const f of bad) console.log(`       ${f.kind}: ${f.sel} "${f.text}"`);
  return bad.length;
}

async function themedPage(name, page, w, h, storage) {
  await ui.open(page, { w, h, scale: 1, locale: "en-US", storage });
  const found = [...((await ui.evaluate(PAGE_JS)) ?? []), ...((await ui.evaluate(PROBE_JS)) ?? [{ kind: "error", ok: false, sel: "page", text: "probe failed" }])];
  const problems = found.filter((f) => !f.ok);
  report[`${name}.en-US`] = found;
  if (!json) console.log(`${problems.length ? "FAIL" : "ok  "} ${name} en-US (${w}×${h}): ${problems.length} problem(s), ${found.length - problems.length} allowed`);
  if (!json) for (const f of problems) console.log(`       ${f.kind}: ${f.sel} "${f.text}"`);
  return problems.length;
}

async function dangerButton(theme, mode) {
  await ui.open("settings.html?page=general", { w: 640, h: 460, scale: 1, locale: "en-US", storage: { "cuotai.theme": theme.id, "cuotai.themeMode": mode } });
  const ratios = (await ui.evaluate(DANGER_JS)) ?? [0];
  const worst = Math.min(...ratios);
  if (!json || worst < 4.5) console.log(`${worst < 4.5 ? "FAIL" : "ok  "} danger button ${theme.id} ${mode}: ${worst.toFixed(2)}:1`);
  return worst < 4.5 ? 1 : 0;
}

const all = tasks();
if (args.includes("--shard-selftest")) {
  // The shards of every count up to 8 must partition the full list: each item in exactly one shard.
  const ids = all.map((t) => t.id);
  let failed = new Set(ids).size !== ids.length ? "duplicate item ids" : "";
  for (let n = 1; n <= 8 && !failed; n++) {
    const seen = [];
    for (let i = 1; i <= n; i++) seen.push(...pick(ids, i, n));
    if (seen.length !== ids.length || [...ids].sort().join("\n") !== [...seen].sort().join("\n")) failed = `the ${n} shards do not cover the list exactly once`;
  }
  console.error(failed ? `shard selftest FAILED: ${failed}` : `shard selftest: ${ids.length} items, shards of 1 to 8 each cover them exactly once`);
  process.exit(failed ? 1 : 0);
}
const shardArg = args.indexOf("--shard");
const [shardIndex, shardCount] = shardArg < 0 ? [1, 1] : parseShard(args[shardArg + 1]);
const mine = pick(all, shardIndex, shardCount);
if (shardCount > 1) console.error(`ui-shots check: shard ${shardIndex}/${shardCount}, ${mine.length} of ${all.length} items`);

const ui = await launch();
const report = {};
let problems = 0;
try {
  for (const task of mine) problems += await task.run();
} finally {
  ui.close();
}
if (json) console.log(JSON.stringify(report, null, 1));
console.error(`ui-shots check: ${problems} problem(s)`);
if (selftest) {
  console.error(problems ? "selftest: the forced long string was caught" : "selftest FAILED: the forced long string went unnoticed");
  process.exit(problems ? 0 : 1);
}
process.exit(problems ? 1 : 0);
