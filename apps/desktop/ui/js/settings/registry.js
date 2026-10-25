// PRD 0020 RF2.4/RF2.5: the one registry of the Settings window's pages (DS-CMP-SEG-5). Adding a page is one entry
// here plus its module under `js/settings/`; the window shell has no page of its own.
//
// Order: General, Dashboard, Menu bar (PRD 0025 RF4), Alerts, Data & privacy, the collapsible Advanced group
// (Integrations, Local cost, (Extensions), Files), then About separated at the bottom. The Extensions page (PRDs 0017
// and 0028 RF7) gets its entry when its content moves here; until then the extension manager stays in the main
// window.
import { te } from "../i18n.js";
import { esc, icon } from "../lib.js";

export const ADVANCED = "advanced";

/** `sections`: the groups the page shows, in order; each one is a former section of the old dialog (or part of
 *  one, see LEGACY_SECTIONS) and also an anchor (`#set-<section>`) that links can land on. */
export const PAGES = [
  { id: "general", icon: "sliders", label: "settings.page.general", load: () => import("./general.js"), sections: ["appearance", "language", "startup"] },
  { id: "dashboard", icon: "grid", label: "settings.page.dashboard", load: () => import("./dashboard.js"), sections: ["accounts", "order", "display", "refresh"] },
  { id: "menubar", icon: "window", label: "settings.page.menubar", load: () => import("./menubar.js"), sections: ["menubar-style", "menubar-accounts"] },
  { id: "alerts", icon: "bell", label: "settings.page.alerts", load: () => import("./alerts.js"), sections: ["notify"] },
  { id: "data", icon: "shield", label: "settings.page.data", load: () => import("./data.js"), sections: ["history", "privacy"] },
  { id: "integrations", group: ADVANCED, icon: "plug", label: "settings.page.integrations", load: () => import("./integrations.js"), sections: ["serve", "hooks", "widgets"] },
  { id: "cost", group: ADVANCED, icon: "coins", label: "settings.page.cost", load: () => import("./cost.js"), sections: ["cost"] },
  { id: "files", group: ADVANCED, icon: "folder", label: "settings.page.files", load: () => import("./files.js"), sections: ["config", "folder", "credentials"] },
  { id: "about", bottom: true, icon: "info", label: "settings.page.about", load: () => import("./about.js"), sections: ["about", "updates"] },
];

/** RF2 acceptance / RF10.2 seed: each section of the old 12-section dialog (plus Updates, PRD 0018) and the
 *  pages its controls moved to. Appearance splits: theme, mode and animations stay in General; the "Recharging"
 *  group and the pace switch go to Dashboard, next to the order (RF2.5). */
export const LEGACY_SECTIONS = {
  appearance: ["general", "dashboard"],
  language: ["general"],
  order: ["dashboard"],
  refresh: ["dashboard"],
  notifications: ["alerts"],
  history: ["data"],
  privacy: ["data"],
  cost: ["cost"],
  integration: ["integrations"],
  config: ["files"],
  folder: ["files"],
  about: ["about"],
  updates: ["about"],
};

/** RF10.2 seed: every control of the old dialog, the page that now holds it and the selector it renders with. A
 *  smoke renders each page and fails when a control is missing. `async`: drawn into its placeholder after the
 *  page loads (the state comes from a command), so the smoke renders that part from the same function. */
export const CONTROLS = [
  { section: "appearance", page: "general", selector: "data-theme-mode" },
  { section: "appearance", page: "general", selector: "data-theme-toggle" },
  { section: "appearance", page: "general", selector: 'id="motion"' },
  { section: "appearance", page: "dashboard", selector: 'id="group-recharging"' },
  { section: "appearance", page: "dashboard", selector: 'id="show-pace"' },
  { section: "appearance", page: "dashboard", selector: 'id="pace-unit"' },
  { section: "appearance", page: "dashboard", selector: 'id="advice-on"' },
  { section: "language", page: "general", selector: "data-locale" },
  { section: "order", page: "dashboard", selector: 'id="order-binding"' },
  { section: "refresh", page: "dashboard", selector: 'id="refresh-seconds"' },
  { section: "refresh", page: "dashboard", selector: "data-preset" },
  { section: "refresh", page: "dashboard", selector: "data-apply-refresh" },
  { section: "notifications", page: "alerts", selector: 'id="notify-on"' },
  { section: "notifications", page: "alerts", selector: 'id="notify-lead"' },
  { section: "notifications", page: "alerts", selector: 'id="notify-when"' },
  { section: "notifications", page: "alerts", selector: 'id="notify-other"' },
  { section: "notifications", page: "alerts", selector: 'id="alerts-on"' },
  { section: "notifications", page: "alerts", selector: 'id="projected-on"' },
  { section: "notifications", page: "alerts", selector: 'id="projected-lead"' },
  { section: "notifications", page: "alerts", selector: 'id="alert-new"' },
  { section: "notifications", page: "alerts", selector: "data-alert-add" },
  { section: "notifications", page: "alerts", selector: 'id="alerts-exhaustion"' },
  { section: "notifications", page: "alerts", selector: 'id="alerts-recovery"' },
  { section: "notifications", page: "alerts", selector: "data-notify-test" },
  { section: "history", page: "data", selector: 'id="history-recording"' },
  { section: "history", page: "data", selector: 'id="history-days"' },
  { section: "history", page: "data", selector: "data-apply-history" },
  { section: "history", page: "data", selector: 'id="history-account"' },
  { section: "history", page: "data", selector: 'id="history-period"' },
  { section: "history", page: "data", selector: 'data-export="csv"' },
  { section: "history", page: "data", selector: 'data-export="json"' },
  { section: "history", page: "data", selector: "data-erase" },
  { section: "history", page: "data", selector: "data-erase-confirm" },
  { section: "privacy", page: "data", selector: 'id="status-queries"' },
  { section: "cost", page: "cost", selector: "data-cost-body" },
  { section: "cost", page: "cost", selector: "data-cost-toggle", async: "cost" },
  { section: "cost", page: "cost", selector: "data-cost-paths-save", async: "cost" },
  { section: "cost", page: "cost", selector: "data-cost-paths-reset", async: "cost" },
  { section: "cost", page: "cost", selector: 'id="cost-period"', async: "cost" },
  { section: "cost", page: "cost", selector: 'id="cost-retention"', async: "cost" },
  { section: "cost", page: "cost", selector: "data-cost-save", async: "cost" },
  { section: "cost", page: "cost", selector: "data-cost-plan-save", async: "cost" },
  { section: "cost", page: "cost", selector: "data-cost-display-save", async: "cost" },
  { section: "cost", page: "cost", selector: "data-cost-erase", async: "cost" },
  { section: "integration", page: "integrations", selector: "data-serve" },
  { section: "integration", page: "integrations", selector: 'id="serve-on"', async: "serve" },
  { section: "integration", page: "integrations", selector: 'id="serve-port"', async: "serve" },
  { section: "integration", page: "integrations", selector: "data-serve-port", async: "serve" },
  { section: "integration", page: "integrations", selector: "data-serve-token-new", async: "serve" },
  { section: "integration", page: "integrations", selector: 'id="serve-browsers"', async: "serve" },
  { section: "integration", page: "integrations", selector: "data-hooks" },
  { section: "integration", page: "integrations", selector: 'id="hooks-on"', async: "hooks" },
  { section: "integration", page: "integrations", selector: "data-hook-add", async: "hooks" },
  { section: "integration", page: "integrations", selector: "data-hooks-save", async: "hooks" },
  { section: "integration", page: "integrations", selector: "data-widgets" },
  { section: "integration", page: "integrations", selector: 'id="widgets-on"', async: "widgets" },
  { section: "integration", page: "integrations", selector: 'id="widgets-identity"', async: "widgets" },
  { section: "integration", page: "integrations", selector: 'id="widgets-max"', async: "widgets" },
  { section: "integration", page: "integrations", selector: "data-widgets-delete", async: "widgets" },
  { section: "config", page: "files", selector: "data-pick-config" },
  { section: "config", page: "files", selector: "data-reset-config" },
  { section: "folder", page: "files", selector: "data-pick" },
  { section: "folder", page: "files", selector: "data-reset" },
  { section: "folder", page: "files", selector: "data-open-dir" },
  { section: "credentials", page: "files", selector: "data-vault" },
  { section: "about", page: "about", selector: "data-libraries" },
  { section: "updates", page: "about", selector: 'id="update-automatic"' },
  { section: "updates", page: "about", selector: "data-update-check" },
];

export const pageById = (id) => PAGES.find((p) => p.id === id) ?? null;

/** The page to show: the requested one, else the remembered one, else General. */
export const resolvePage = (...candidates) => candidates.map(pageById).find(Boolean) ?? PAGES[0];

/** The page that holds `anchor` (a section id), or null. */
export const pageOfAnchor = (anchor) => PAGES.find((p) => p.sections.includes(anchor)) ?? null;

/** Pages listed in the sidebar right now, in its order: Advanced pages only while the group is open, except the
 *  current page, which is always reachable. */
export const visiblePages = (current, advancedOpen) =>
  PAGES.filter((p) => p.group !== ADVANCED || advancedOpen || p.id === current);

/** RF8.1: arrow keys move between the sidebar entries (pages and the Advanced toggle), wrapping around. */
export function nextEntry(entries, focused, key) {
  const step = { ArrowDown: 1, ArrowUp: -1 }[key];
  if (key === "Home") return entries[0] ?? null;
  if (key === "End") return entries[entries.length - 1] ?? null;
  if (!step || !entries.length) return null;
  const at = entries.indexOf(focused);
  return entries[(at + step + entries.length) % entries.length];
}

// `title` is the tooltip the icon rail needs when the labels are hidden (RF8.4).
const entryHtml = (p, current) => `<li><button type="button" class="set-nav-item${p.id === current ? " on" : ""}" data-page="${esc(p.id)}"
    ${p.id === current ? 'aria-current="page"' : ""} title="${te(p.label)}">${icon(p.icon)}<span>${te(p.label)}</span></button></li>`;

/** RF8.4: below 560 px the sidebar becomes this page selector above the content (CSS shows one or the other). */
const selectHtml = (current) => {
  const option = (p) => `<option value="${esc(p.id)}" ${p.id === current ? "selected" : ""}>${te(p.label)}</option>`;
  return `<select class="set-select set-nav-select" data-page-select aria-label="${te("settings.nav.label")}">
    ${PAGES.filter((p) => !p.group && !p.bottom).map(option).join("")}
    <optgroup label="${te("settings.page.advanced")}">${PAGES.filter((p) => p.group === ADVANCED).map(option).join("")}</optgroup>
    ${PAGES.filter((p) => p.bottom).map(option).join("")}</select>`;
};

/** RF2.5: the sidebar (a navigation landmark, DS-CMP-SEG-4) with the current page marked and the Advanced group as
 *  a disclosure whose open state the caller remembers. */
export function sidebarHtml(current, advancedOpen) {
  const shown = visiblePages(current, advancedOpen);
  const top = shown.filter((p) => !p.group && !p.bottom);
  const advanced = shown.filter((p) => p.group === ADVANCED);
  const bottom = shown.filter((p) => p.bottom);
  return `${selectHtml(current)}<nav class="set-nav" aria-label="${te("settings.nav.label")}">
    <ul class="set-nav-list">${top.map((p) => entryHtml(p, current)).join("")}</ul>
    <div class="set-nav-group">
      <button type="button" class="set-nav-item set-nav-toggle set-nav-grouplabel" data-advanced-toggle aria-expanded="${advancedOpen}" aria-controls="set-nav-advanced">
        ${icon("chev", "set-nav-chev")}${icon("wrench", "set-nav-rail")}<span>${te("settings.page.advanced")}</span></button>
      <ul class="set-nav-list set-nav-sub" id="set-nav-advanced">${advanced.map((p) => entryHtml(p, current)).join("")}</ul>
    </div>
    <ul class="set-nav-list set-nav-bottom">${bottom.map((p) => entryHtml(p, current)).join("")}</ul>
  </nav>`;
}
