import { DEFAULT_THEME, GROUP_ORDER, THEMES, effectiveMode, themeById } from "./themes.js";
import { FALLBACK, LOCALES, PSEUDO, currentLocale, intlTag, pendingLocales, setLocale, t, te, tl } from "./i18n.js";
import { esc, fold, uiPath } from "./util.js";
import { desktopAction } from "./desktop-wording.js";
import { flagCountry, flagSvg } from "./flags.js";
import { crossFade } from "./motion.js";
import { searchCatalog } from "./demo-search.js";
export { esc };
// Shared layer (no framework, no bundler). Mirrors cuotai_core::model.

// ---------- util ----------
// Icons (Lucide paths, ISC).
const paths = {
  refresh: '<path d="M3 12a9 9 0 0 1 9-9 9.75 9.75 0 0 1 6.74 2.74L21 8"/><path d="M21 3v5h-5"/><path d="M21 12a9 9 0 0 1-9 9 9.75 9.75 0 0 1-6.74-2.74L3 16"/><path d="M8 16H3v5"/>',
  edit: '<path d="M21.17 6.81a1 1 0 0 0-3.99-3.99L3.84 16.17a2 2 0 0 0-.5.83l-1.32 4.35a.5.5 0 0 0 .62.62l4.35-1.32a2 2 0 0 0 .83-.5z"/>',
  trash: '<path d="M3 6h18"/><path d="M19 6v14a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V6"/><path d="M8 6V4a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v2"/>',
  eye: '<path d="M2.06 12.35a1 1 0 0 1 0-.7 10.75 10.75 0 0 1 19.88 0 1 1 0 0 1 0 .7 10.75 10.75 0 0 1-19.88 0"/><circle cx="12" cy="12" r="3"/>',
  eyeOff: '<path d="M10.73 5.08A10.43 10.43 0 0 1 12 5c7 0 10 7 10 7a13.16 13.16 0 0 1-1.67 2.68"/><path d="M6.61 6.61A13.53 13.53 0 0 0 2 12s3 7 10 7a9.74 9.74 0 0 0 5.39-1.61"/><line x1="2" x2="22" y1="2" y2="22"/>',
  key: '<path d="m15.5 7.5 2.3 2.3a1 1 0 0 0 1.4 0l2.1-2.1a1 1 0 0 0 0-1.4L19 4"/><path d="m21 2-9.6 9.6"/><circle cx="7.5" cy="15.5" r="5.5"/>',
  xcircle: '<circle cx="12" cy="12" r="10"/><path d="m15 9-6 6"/><path d="m9 9 6 6"/>',
  gauge: '<path d="m12 14 4-4"/><path d="M3.34 19a10 10 0 1 1 17.32 0"/>',
  wrench: '<path d="M14.7 6.3a1 1 0 0 0 0 1.4l1.6 1.6a1 1 0 0 0 1.4 0l3.77-3.77a6 6 0 0 1-7.94 7.94l-6.91 6.91a2.12 2.12 0 0 1-3-3l6.91-6.91a6 6 0 0 1 7.94-7.94l-3.76 3.76z"/>',
  ban: '<circle cx="12" cy="12" r="10"/><path d="m4.9 4.9 14.2 14.2"/>',
  battery: '<rect width="16" height="10" x="2" y="7" rx="2" ry="2"/><line x1="22" x2="22" y1="11" y2="13"/><line x1="6" x2="6" y1="11" y2="13"/>',
  alert: '<circle cx="12" cy="12" r="10"/><line x1="12" x2="12" y1="8" y2="12"/><line x1="12" x2="12.01" y1="16" y2="16"/>',
  grid: '<rect width="7" height="7" x="3" y="3" rx="1"/><rect width="7" height="7" x="14" y="3" rx="1"/><rect width="7" height="7" x="14" y="14" rx="1"/><rect width="7" height="7" x="3" y="14" rx="1"/>',
  list: '<path d="M8 6h13"/><path d="M8 12h13"/><path d="M8 18h13"/><path d="M3 6h.01"/><path d="M3 12h.01"/><path d="M3 18h.01"/>',
  window: '<rect width="20" height="16" x="2" y="4" rx="2"/><path d="M2 9h20"/><path d="M6 6.5h.01"/><path d="M9 6.5h.01"/>',
  sort: '<path d="m21 16-4 4-4-4"/><path d="M17 20V4"/><path d="m3 8 4-4 4 4"/><path d="M7 4v16"/>',
  plus: '<path d="M5 12h14"/><path d="M12 5v14"/>',
  grip: '<circle cx="9" cy="6" r="1.3"/><circle cx="15" cy="6" r="1.3"/><circle cx="9" cy="12" r="1.3"/><circle cx="15" cy="12" r="1.3"/><circle cx="9" cy="18" r="1.3"/><circle cx="15" cy="18" r="1.3"/>',
  clock: '<circle cx="12" cy="12" r="10"/><path d="M12 6v6l4 2"/>',
  x: '<path d="M18 6 6 18"/><path d="m6 6 12 12"/>',
  search: '<circle cx="11" cy="11" r="8"/><path d="m21 21-4.3-4.3"/>',
  gear: '<path d="M12.22 2h-.44a2 2 0 0 0-2 2v.18a2 2 0 0 1-1 1.73l-.43.25a2 2 0 0 1-2 0l-.15-.08a2 2 0 0 0-2.73.73l-.22.38a2 2 0 0 0 .73 2.73l.15.1a2 2 0 0 1 1 1.72v.51a2 2 0 0 1-1 1.74l-.15.09a2 2 0 0 0-.73 2.73l.22.38a2 2 0 0 0 2.73.73l.15-.08a2 2 0 0 1 2 0l.43.25a2 2 0 0 1 1 1.73V20a2 2 0 0 0 2 2h.44a2 2 0 0 0 2-2v-.18a2 2 0 0 1 1-1.73l.43-.25a2 2 0 0 1 2 0l.15.08a2 2 0 0 0 2.73-.73l.22-.39a2 2 0 0 0-.73-2.73l-.15-.08a2 2 0 0 1-1-1.74v-.5a2 2 0 0 1 1-1.74l.15-.09a2 2 0 0 0 .73-2.73l-.22-.38a2 2 0 0 0-2.73-.73l-.15.08a2 2 0 0 1-2 0l-.43-.25a2 2 0 0 1-1-1.73V4a2 2 0 0 0-2-2z"/><circle cx="12" cy="12" r="3"/>',
  folder: '<path d="M20 20a2 2 0 0 0 2-2V8a2 2 0 0 0-2-2h-7.9a2 2 0 0 1-1.69-.9L9.6 3.9A2 2 0 0 0 7.93 3H4a2 2 0 0 0-2 2v13a2 2 0 0 0 2 2Z"/>',
  copy: '<rect width="14" height="14" x="8" y="8" rx="2" ry="2"/><path d="M4 16c-1.1 0-2-.9-2-2V4c0-1.1.9-2 2-2h10c1.1 0 2 .9 2 2"/>',
  back: '<path d="m15 18-6-6 6-6"/>',
  chev: '<path d="m9 18 6-6-6-6"/>',
  check: '<circle cx="12" cy="12" r="10"/><path d="m9 12 2 2 4-4"/>',
  warn: '<path d="m21.73 18-8-14a2 2 0 0 0-3.48 0l-8 14A2 2 0 0 0 4 21h16a2 2 0 0 0 1.73-3"/><path d="M12 9v4"/><path d="M12 17h.01"/>',
  settings: '<circle cx="12" cy="12" r="3"/><path d="M12 2v2M12 20v2M4.93 4.93l1.41 1.41M17.66 17.66l1.41 1.41M2 12h2M20 12h2M4.93 19.07l1.41-1.41M17.66 6.34l1.41-1.41"/>',
  power: '<path d="M12 2v10"/><path d="M18.4 6.6a9 9 0 1 1-12.77.04"/>',
  sun: '<circle cx="12" cy="12" r="4"/><path d="M12 2v2M12 20v2M4.93 4.93l1.41 1.41M17.66 17.66l1.41 1.41M2 12h2M20 12h2M4.93 19.07l1.41-1.41M17.66 6.34l1.41-1.41"/>',
  moon: '<path d="M12 3a6 6 0 0 0 9 9 9 9 0 1 1-9-9Z"/>',
  monitor: '<rect width="20" height="14" x="2" y="3" rx="2"/><path d="M8 21h8M12 17v4"/>',
  bell: '<path d="M10.27 21a2 2 0 0 0 3.46 0"/><path d="M3.26 15.33A1 1 0 0 0 4 17h16a1 1 0 0 0 .74-1.67C19.41 13.96 18 12.5 18 8A6 6 0 0 0 6 8c0 4.5-1.41 5.96-2.74 7.33"/>',
  bellOff: '<path d="M10.27 21a2 2 0 0 0 3.46 0"/><path d="M17 17H4a1 1 0 0 1-.74-1.67C4.59 13.96 6 12.5 6 8a6 6 0 0 1 .26-1.74"/><path d="M8.67 3.21A6 6 0 0 1 18 8c0 2.43.41 3.94.98 5"/><path d="m2 2 20 20"/>',
  // PRD 0020 RF2.5: settings sidebar pages (Data & privacy, Integrations, Local cost, About).
  shield: '<path d="M20 13c0 5-3.5 7.5-7.66 8.95a1 1 0 0 1-.67-.01C7.5 20.5 4 18 4 13V6a1 1 0 0 1 1-1c2 0 4.5-1.2 6.24-2.72a1.17 1.17 0 0 1 1.52 0C14.51 3.81 17 5 19 5a1 1 0 0 1 1 1z"/>',
  plug: '<path d="M12 22v-5"/><path d="M9 8V2"/><path d="M15 8V2"/><path d="M18 8v5a4 4 0 0 1-4 4h-4a4 4 0 0 1-4-4V8Z"/>',
  coins: '<circle cx="8" cy="8" r="6"/><path d="M18.09 10.37A6 6 0 1 1 10.34 18"/><path d="M7 6h1v4"/><path d="m16.71 13.88.7.71-2.82 2.82"/>',
  info: '<circle cx="12" cy="12" r="10"/><path d="M12 16v-4"/><path d="M12 8h.01"/>',
  // PRD 0026: the Stats view entry.
  chart: '<path d="M3 3v18h18"/><path d="M8 17v-5"/><path d="M13 17V7"/><path d="M18 17v-8"/>',
  // PRD 0022 RF1.8 (DS-TOK-ICO-1): glyphs of the refresh, same 24-unit grid and 2 px round stroke.
  chevDown: '<path d="m6 9 6 6 6-6"/>',
  chevUp: '<path d="m18 15-6-6-6 6"/>',
  tick: '<path d="M20 6 9 17l-5-5"/>',
  chartBar: '<path d="M3 3v16a2 2 0 0 0 2 2h16"/><path d="M18 17V9"/><path d="M13 17V5"/><path d="M8 17v-3"/>',
  download: '<path d="M12 15V3"/><path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4"/><path d="m7 10 5 5 5-5"/>',
  puzzle: '<path d="M15.39 4.39a1 1 0 0 0 1.68-.474 2.5 2.5 0 1 1 3.014 3.015 1 1 0 0 0-.474 1.68l1.683 1.682a2.414 2.414 0 0 1 0 3.414L19.61 15.39a1 1 0 0 1-1.68-.474 2.5 2.5 0 1 0-3.014 3.015 1 1 0 0 1 .474 1.68l-1.683 1.682a2.414 2.414 0 0 1-3.414 0L8.61 19.61a1 1 0 0 0-1.68.474 2.5 2.5 0 1 1-3.014-3.015 1 1 0 0 0 .474-1.68l-1.683-1.682a2.414 2.414 0 0 1 0-3.414L4.39 8.61a1 1 0 0 1 1.68.474 2.5 2.5 0 1 0 3.014-3.015 1 1 0 0 1-.474-1.68l1.683-1.682a2.414 2.414 0 0 1 3.414 0z"/>',
  sliders: '<line x1="21" x2="14" y1="4" y2="4"/><line x1="10" x2="3" y1="4" y2="4"/><line x1="21" x2="12" y1="12" y2="12"/><line x1="8" x2="3" y1="12" y2="12"/><line x1="21" x2="16" y1="20" y2="20"/><line x1="12" x2="3" y1="20" y2="20"/><line x1="14" x2="14" y1="2" y2="6"/><line x1="8" x2="8" y1="10" y2="14"/><line x1="16" x2="16" y1="18" y2="22"/>',
  file: '<path d="M15 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V7Z"/><path d="M14 2v4a2 2 0 0 0 2 2h4"/>',
  menubar: '<rect width="18" height="18" x="3" y="3" rx="2"/><path d="M3 9h18"/>',
  history: '<path d="M3 12a9 9 0 1 0 9-9 9.75 9.75 0 0 0-6.74 2.74L3 8"/><path d="M3 3v5h5"/><path d="M12 7v5l4 2"/>',
  zap: '<path d="M4 14a1 1 0 0 1-.78-1.63l9.9-10.2a.5.5 0 0 1 .86.46l-1.92 6.02A1 1 0 0 0 13 10h7a1 1 0 0 1 .78 1.63l-9.9 10.2a.5.5 0 0 1-.86-.46l1.92-6.02A1 1 0 0 0 11 14z"/>',
  globe: '<circle cx="12" cy="12" r="10"/><path d="M12 2a14.5 14.5 0 0 0 0 20 14.5 14.5 0 0 0 0-20"/><path d="M2 12h20"/>',
  ext: '<path d="M15 3h6v6"/><path d="M10 14 21 3"/><path d="M18 13v6a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V8a2 2 0 0 1 2-2h6"/>',
  more: '<circle cx="12" cy="12" r="1"/><circle cx="19" cy="12" r="1"/><circle cx="5" cy="12" r="1"/>',
  dollar: '<line x1="12" x2="12" y1="2" y2="22"/><path d="M17 5H9.5a3.5 3.5 0 0 0 0 7h5a3.5 3.5 0 0 1 0 7H6"/>',
  terminal: '<polyline points="4 17 10 11 4 5"/><line x1="12" x2="20" y1="19" y2="19"/>',
  db: '<ellipse cx="12" cy="5" rx="9" ry="3"/><path d="M3 5V19A9 3 0 0 0 21 19V5"/><path d="M3 12A9 3 0 0 0 21 12"/>',
  arrowUp: '<path d="m5 12 7-7 7 7"/><path d="M12 19V5"/>',
  filter: '<path d="M3 6h18"/><path d="M7 12h10"/><path d="M10 18h4"/>',
  rocket: '<path d="M4.5 16.5c-1.5 1.26-2 5-2 5s3.74-.5 5-2c.71-.84.7-2.13-.09-2.91a2.18 2.18 0 0 0-2.91-.09z"/><path d="m12 15-3-3a22 22 0 0 1 2-3.95A12.88 12.88 0 0 1 22 2c0 2.72-.78 7.5-6 11a22.35 22.35 0 0 1-4 2z"/><path d="M9 12H4s.55-3.03 2-4c1.62-1.08 5 0 5 0"/><path d="M12 15v5s3.03-.55 4-2c1.08-1.62 0-5 0-5"/>',
  returns: '<path d="M3 12a9 9 0 1 0 9-9 9.75 9.75 0 0 0-6.74 2.74L3 8"/><path d="M3 3v5h5"/>',
  arrowDown: '<path d="M12 5v14"/><path d="m19 12-7 7-7-7"/>',
  arrowRight: '<path d="M5 12h14"/><path d="m12 5 7 7-7 7"/>',
  mapPin: '<path d="M20 10c0 6-8 12-8 12s-8-6-8-12a8 8 0 0 1 16 0Z"/><circle cx="12" cy="10" r="3"/>',
  // PRD 0044 re-validation V-12: generic glyphs for the Slack (a hash) and Discord (a speech bubble) channels, no logos.
  hash: '<path d="M4 9h16"/><path d="M4 15h16"/><path d="M10 3 8 21"/><path d="M16 3l-2 18"/>',
  message: '<path d="M21 15a2 2 0 0 1-2 2H7l-4 4V5a2 2 0 0 1 2-2h14a2 2 0 0 1 2 2z"/>',
};
export const icon = (name, cls = "") => `<svg class="i ${cls}" viewBox="0 0 24 24" aria-hidden="true">${paths[name]}</svg>`;

/** PRD 0052 RF7.2 (DS-CMP-MSG-2): the core's server-clock advice as a main-window banner while the clocks disagree;
 *  it only advises, never changes a reading, and goes away by itself once two responses agree again. */
export const clockSkewBannerHtml = (view) => view?.clock_skew
  ? `<div class="banner bad" role="status" data-clock-skew>${icon("warn")} ${esc(tl(view.clock_skew))}</div>` : "";

// ---------- API (Tauri ou demo no navegador) ----------
const T = window.__TAURI__;
export const isDesktop = !!T;
const call = (cmd, args) => (isDesktop ? T.core.invoke(cmd, args) : demo[cmd](args ?? {}));
export const getView = () => call("get_view");
export const refreshAll = () => call("refresh_all");
export const refreshAccount = (index, sub = null) => call("refresh_account", { index, sub });
/** PRD 0036 RF8.5: hide one aggregator connection (`hidden`) or keep it beside its direct account (`show_both`). */
export const setConnectionFlag = (index, key, sub, on) => call("set_connection_flag", { index, key, sub, on });
export const listProviders = () => call("list_providers");
// PRD 0027: the Add-account catalog (core search, ranking and grouping), its modes and the consented detection.
export const searchProviders = (query, locale) => call("search_providers", { query, locale });
/** V-02 (PRD 0005 review): why a search with no result names a provider the picker leaves out (core
 *  `account_edit::picker_hidden_notice`), or null. */
export const pickerHiddenNotice = (query, locale) => call("picker_hidden_notice", { query, locale });
export const providerModes = (provider) => call("provider_modes", { provider });
export const detectSources = () => call("detect_sources");
export const setDetectEnabled = (on) => call("set_detect_enabled", { on });
export const dismissDetected = (kind) => call("dismiss_detected", { kind });
/** The new-provider issue template (allow-listed by the shell's `open_external`). */
export const NEW_PROVIDER_URL = "https://github.com/jmarcon/cuotai/issues/new?template=new-provider.yml";
export const listExtensions = () => call("list_extensions");
export const extensionDisclosure = (id) => call("extension_disclosure", { id });
// PRD 0050 RF2.5: only the id; the shell discloses, asks in a native dialog and keeps the candidate.
export const enableExtension = (id) => call("enable_extension", { id });
export const disableExtension = (id) => call("disable_extension", { id });
export const testExtension = (id) => call("test_extension", { id });
export const extensionSecrets = (id) => call("extension_secrets", { id });
export const setExtensionOverride = (index, id, on) =>
  call("set_extension_override", { index, id, on });
export const setExtensionSecret = (id, name, value) =>
  call("set_extension_secret", { id, name, value });
/** PRD 0035 RF11.5: where a machine-translated language asks for review; `TRANSLATION_GUIDE` in about.rs. */
export const TRANSLATION_GUIDE_URL = "https://github.com/jmarcon/cuotai/blob/main/apps/desktop/ui/locales/README.md";
export const EXTENSIONS_GUIDE_URL = "https://github.com/jmarcon/cuotai/blob/main/docs/extensions.md";
/** PRD 0019 RF7.7: the full changelog the What's new dialog links (`CHANGELOG_URL` in about.rs, the only other copy). */
export const CHANGELOG_URL = "https://github.com/jmarcon/cuotai/blob/main/CHANGELOG.md";
/** PRD 0046 RF5.2: the backup recipes (`BACKUP_GUIDE` in about.rs, the only other copy). */
export const BACKUP_GUIDE_URL = "https://github.com/jmarcon/cuotai/blob/main/docs/advanced.md#backup-restore-and-self-check";
export const openExtensionsFolder = () => (isDesktop ? call("open_extensions_folder") : Promise.resolve());
export const forgetExtensionSecrets = (id) => call("forget_extension_secrets", { id });
export const formSchema = (provider) => call("form_schema", { provider });
export const visibleFields = (provider, choices, authMode, loginParams = {}) => call("visible_fields", { provider, choices, authMode, loginParams });
export const fieldChoices = (provider, choices, authMode) => call("field_choices", { provider, choices, authMode });
/** PRD 0005 RF3: how the draft's usage is read (level, notes, cost warning, save confirmation), from the core. */
export const draftAccess = (provider, authMode, fields, sessionSlots = []) => call("draft_access", { provider, authMode, fields, sessionSlots });
// RF2: login orchestrated by the core; the UI only renders events and sends input.
export const loginStart = (request) => call("login_start", { request });
export const loginSubmitCode = (code) => call("login_submit_code", { code });
export const loginCancel = (attempt) => call("login_cancel", { attempt });
/** Subscribes to the typed login events (no secrets). Returns the subscription's cancel function. */
const demoLoginListeners = new Set();
export const onLogin = (cb) => {
  if (isDesktop) return T.event.listen("login", (e) => cb(e.payload));
  demoLoginListeners.add(cb);
  return Promise.resolve(() => demoLoginListeners.delete(cb));
};
const demoEmit = (ev) => demoLoginListeners.forEach((cb) => cb(ev));
export const getAccount = (index) => call("get_account", { index });
/** PRD 0001 C1: where the account with this stable id is now (`null` when it is gone), after a reload. */
export const accountIndex = (accountId) => call("account_index", { accountId });
/** PRD 0016 RF1.2: move the account's session variables into the system vault (after an explicit confirmation).
 *  PRD 0047 re-validation S-01: `accountId` (the dialog's) binds it to that account wherever the accounts moved. */
export const migrateSession = (index, accountId = null) => call("migrate_session", { index, accountId });
/** PRD 0016 RF4.2: forget the account's web session, deleting its vault entry (after an explicit confirmation). */
export const forgetSession = (index, accountId = null) => call("forget_session", { index, accountId });
/** PRD 0029 RF6.2: the vault entries Cuotai created, each with the core's status; the removal of exactly the confirmed unused keys; one entry by its key. */
export const vaultAudit = () => call("vault_audit");
export const vaultPrune = (keys) => call("vault_prune", { keys });
export const vaultRemove = (key) => call("vault_remove", { key });
/** PRD 0046 RF4.4: the core's read-only, offline self-check (statuses, details and fixes, never a value). */
export const doctorReport = () => call("doctor_report");
/** PRD 0046 RF2.1: what Back up… would write (files in, what stays out and why), then the write itself. */
export const backupPreview = (withHistory) => call("backup_preview", { withHistory });
/** PRD 0050 RF9: the shell opens the native save dialog itself (no path crosses from here); null when cancelled. */
export const backupWrite = (withHistory) => call("backup_write", { withHistory });
/** PRD 0046 RF3.3: the shell opens the native open dialog and plans the chosen backup ({ token, plan }, null when
 *  cancelled); the application names that file by its token (`accept`: the replacements chosen). */
export const restorePlan = () => call("restore_plan");
export const restoreApply = (token, accept, acceptHosts = false) => call("restore_apply", { token, accept, acceptHosts });
/** PRD 0046 RF3.3: restart the app so restored settings apply. */
export const restartApp = () => call("restart_app");
/** PRD 0016 RF2.2: what a paste keeps (names, counts, expiry), never the value. */
export const previewSession = (provider, slot, paste) => call("preview_session", { provider, slot, paste });
/** PRD 0016 RF2.1: one read-only query of the draft with its pasted sessions; nothing is stored. */
export const testSession = (input) => call("test_session", { input });
/** PRD 0048 RF1: "Sign in and capture session" — the app's own sign-in window; resolves with its outcome. */
/** RF3.4: `region` is the dialog's `region` choice value (or null); the core picks that region's capture. */
export const captureSession = (provider, slot, region = null) => call("capture_session", { provider, slot, region });
/** PRD 0048 RF6.2: closes a capture window still waiting for its sign-in. */
export const cancelSessionCapture = () => call("cancel_session_capture");
/** PRD 0056 RF2.4: destroys the hidden shared Google capture window; resolves `true` when one was there. */
export const forgetSharedCapture = () => call("forget_shared_capture");
export const testAggregator = (input) => call("test_aggregator", { input });
export const openAggregatorDashboard = (index) => call("open_aggregator_dashboard", { index });
export const saveAccount = (input) => call("save_account", { input });
/** PRD 0001 C1: `accountId` binds the removal to the account the user picked; another account in that slot is never removed. */
export const deleteAccount = (index, purge = false, accountId = null) => call("delete_account", { index, accountId, purge });
/// RF4: re-read the configuration from disk and rebuild the live
/// state (monitor/cycle/tokens/RF3 recovery). Returns the new
/// view. Used after an `edit_error.conflict` so the next save can
/// succeed against the fresh snapshot, and as the manual path for
/// external edits the UI detected via `configChangedOnDisk`.
export const reloadConfig = () => call("reload_config");
// PRD 0051 RF5.2: the data folder for the next start (`null`: the OS default).
export const setDataDir = (path) => call("set_data_dir", { path });
// PRD 0051 RF5.4: use (`adopt`) or decline the history of a moved configuration file.
export const historyRelocation = (adopt) => call("history_relocation", { adopt });
/// RF4: cheapest detector for "the file changed under us". Byte
/// comparison in the core, so it's safe to call on every focus /
/// before opening the editor without a noticeable cost.
export const configChangedOnDisk = () => call("config_changed_on_disk");
/// RF3 retry: re-attempt the vault purge for a previous
/// `delete_account` that returned a recovery handle (kept,
/// kept_shared, partial_failure). The backend runs the core's
/// shared-key guard against the *current* configuration; the
/// promise resolves to a fresh `DeleteResult` describing the
/// outcome (purged / kept_shared / partial_failure with the same
/// handle, ready for another retry).
export const retryPurge = (handle) => call("retry_purge", { handle });
export const setRefreshSeconds = (seconds) => call("set_refresh_seconds", { seconds });
export const moveAccount = (from, to) => call("move_account", { from, to });
export const getSettings = () => call("get_settings");
// PRD 0025: the menu bar (style, colored variant, selection, popover view), the Advanced group's state and the first
// run. Figures and texts of the preview come from the core's `tray::summary`.
export const trayPreview = (style, accounts, colored = false) => call("tray_preview", { style, accounts, colored });
export const trayAccounts = () => call("tray_accounts");
/** Tauri maps command arguments from camelCase; `popover_view` (as `get_settings` names it) is accepted too. */
export const setTrayPrefs = ({ style, colored, accounts = null, popover_view = null, popoverView = popover_view }) =>
  call("set_tray_prefs", { style, colored, accounts, popoverView });
export const setAdvancedOpen = (open) => call("set_advanced_open", { open });
/** PRD 0026 §7: the Stats view's 7 d / 30 d toggle (`range`) and the reset timeline's 24 h / 7 d toggle (`timeline`),
 *  remembered in `desktop.json` `stats`; `null` keeps the stored choice. `statsPrefs()` never fails (defaults: `{}`). */
export const setStatsPrefs = ({ range = null, timeline = null } = {}) => call("set_stats_prefs", { range, timeline });
export const statsPrefs = () => getSettings().then((s) => s?.stats ?? {}, () => ({}));
/** PRD 0019 RF7.7: "Show what's new after an update" (Settings > About), and the note that this version's notes were
 *  shown. The shell decides when they are due (`whats_new_due` in `get_settings`: a changed version, not a first
 *  install, the switch on). */
export const setWhatsNewAuto = (on) => call("set_whats_new_auto", { on });
export const whatsNewSeen = () => call("whats_new_seen");
/** PRD 0022 MONO-4: a monogram override is empty (derived from the name) or one or two visible characters. The
 *  shell checks the same rule; `desktop.json` keeps it by account id and never the TOML. */
export const validMonogram = (text) => { const v = String(text ?? "").trim(); return [...v].length <= 2 && !/[\s\p{C}]/u.test(v); };
export const setAccountMonogram = (accountId, monogram) => call("set_account_monogram", { accountId, monogram });
/** PRD 0025 RF9.5: the row display (`tray`, `window`, `separate`). */
export const setRows = (rows) => call("set_rows", { rows });
// "Start at login" (PRD 0022, audit #37): the system owns the state; both calls return what it reports.
export const autostartStatus = () => call("autostart_status");
export const autostartSet = (enabled) => call("autostart_set", { enabled });
// PRD 0054 RF4.3: the login entry starts a moved or missing copy; Repair rewrites it (never creates one).
export const autostartStale = () => call("autostart_stale");
export const autostartRepair = () => call("autostart_repair");
export const firstRunTurnOn = () => call("first_run_turn_on");
export const firstRunRecord = (again = false) => call("first_run_record", { again });
export const setUserDir = (path) => call("set_user_dir", { path });
// Animations (PRD 0004 RF1.2): `desktop.json` is the source; localStorage only mirrors it for the synchronous read in the <head>.
export const MOTIONS = ["system", "always", "reduced", "off"];
const applyMotion = (m) => { m = MOTIONS.includes(m) ? m : "system"; document.documentElement.dataset.motion = m; put("cuotai.motion", m); };
export const setMotion = async (motion) => { await call("set_motion", { motion }); applyMotion(motion); };
/** PRD 0020 RF7.2: `desktop.json` is the single source of theme, mode, language, animations, account order and
 *  grouping (and pace). `localStorage` only mirrors them so the <head> script can paint before the first IPC answer;
 *  no window listens to `storage` events for them any more. */
export const setPreference = async (key, value) => { await call("set_preference", { key, value }); mirrorPreference(key, value); };
const MIRROR_KEYS = { theme: "cuotai.theme", theme_mode: "cuotai.themeMode", sort: "cuotai.sort", group_recharging: "cuotai.group-recharging", motion: "cuotai.motion", locale: "cuotai.locale", aggregators_in_all: "cuotai.aggregators-in-all" };
const mirrorPreference = (key, value) => { if (MIRROR_KEYS[key]) put(MIRROR_KEYS[key], key === "group_recharging" ? (value ? "on" : "off") : String(value)); };
/** The values the pre-RF7.2 versions kept in `localStorage`, for the one-time migration. */
const legacyLocalValues = () => {
  const out = {};
  for (const [key, k] of Object.entries(MIRROR_KEYS)) { const v = get(k, ""); if (v) out[key] = v; }
  if (!out.theme && get("cuotai.darkTheme", "")) out.theme = get("cuotai.darkTheme", ""); // the oldest key
  return out;
};
/** Reads `desktop.json` (running the one-time `localStorage` migration first), applies what differs here and
 *  returns the set of changed keys. `boot`: before the language loads, so only the mirror of the language moves. */
export async function syncPreferences({ boot = false } = {}) {
  try {
    let s = await getSettings();
    if (s.local_storage_migrated === false) s = await call("migrate_local_storage", { values: legacyLocalValues() });
    // PRD 0022 RF15.1: Cuotai is the default for new installs only. The shell knows the profile existed before this
    // build ran, so a profile with no stored theme keeps Dracula even when its web storage was cleared (the
    // pre-paint script cannot tell that from a new install). Stored once in desktop.json: never null again.
    if (s.profile_existed === true && !s.theme) {
      try { await call("set_preference", { key: "theme", value: "dracula" }); s = await getSettings(); } catch { /* keeps the default */ }
    }
    return await applyPreferences(s, { boot });
  } catch { return new Set(); } finally {
    document.documentElement.removeAttribute?.("data-theme-pending");
  }
}
export const openUserDir = () => call("open_user_dir");
// PRD 0006 RF4: measurement history retention (desktop.json), export and erasure; the core does the work.
export const setHistoryDays = (days) => call("set_history_days", { days });
// PRD 0006 RF4.4: one account (`index`) is named with the configuration `generation` its index was read from.
/** PRD 0050 RF9: the shell opens the native save dialog itself; { rows, path }, or null when cancelled. */
export const historyExport = (index, days, format, generation = null) => call("history_export", { index, generation, days, format });
export const historyErase = (index, generation = null) => call("history_erase", { index, generation });
// PRD 0052 RF5.3: Erase all Cuotai data. The plan is the core's ordered inventory plus the word to type; the erase
// sends only the typed word and the accounts-file choice (Rust re-checks the word and asks natively).
export const eraseAllPlan = () => call("erase_all_plan");
export const eraseAllData = (includeConfig, word) => call("erase_all_data", { includeConfig, word });
// PRD 0006 RF5: the recording switch (desktop.json), the file's size on disk and an account's recorded history
// (`range`: day, week or month; `metricKey` null = every metric), bound to the view's `generation` like the models.
export const setHistoryRecording = (on) => call("set_history_recording", { on });
export const historySize = () => call("history_size");
export const accountHistory = (index, generation, range, metricKey = null) => call("account_history", { index, generation, range, metricKey });
// PRD 0026 RF4.6/RF5: the usage statistics (`range`: days7, days30; `account` null = all) with the recent journal
// events, and the reset timeline (`range`: hours24, days7). Both are bound to the view's `generation` like the
// history: another generation fails with "stale" and the answer is discarded.
export const stats = (range, account, generation) => call("stats", { range, account, generation });
export const resetTimeline = (range, generation) => call("reset_timeline", { range, generation });
// PRD 0003 RF3: reset notices (desktop.json; the backend schedules and sends them). `setNotifyEnabled` asks the
// system permission when turning on and resolves to `granted`, `denied`, `prompt` or `off`.
export const setNotifyEnabled = (on) => call("set_notify_enabled", { on });
export const setNotifyOptions = (leadMinutes, when, includeOther) => call("set_notify_options", { leadMinutes, when, includeOther });
export const setAccountNotify = (accountKey, on) => call("set_account_notify", { accountKey, on });
// PRD 0032: lock-screen-safe texts (RF7.1), quiet hours (RF2) and the "crossed while closed" marker seen (RF8.2).
export const setNotifyPrivate = (on) => call("set_notify_private", { on });
export const setQuietHours = (quiet) => call("set_quiet_hours", { quiet });
export const acknowledgeMissed = (accountKey) => call("acknowledge_missed", { accountKey });
export const sendTestNotification = (title, body) => call("send_test_notification", { title, body });
// PRD 0043: limit-change notices, days off and the weekly digest (the `notify` section of desktop.json).
export const setInsightPrefs = ({ limitChanges, daysOff, digestEnabled, digestWeekday, digestHour }) =>
  call("set_insight_prefs", { limitChanges, daysOff, digestEnabled, digestWeekday, digestHour });
export const sendTestDigest = () => call("send_test_digest");
// PRD 0044: notification channels. The address goes to the shell once (consent, save) and never comes back; the
// view carries the host, the filters in words and the last delivery only.
export const channelsView = () => call("channels_view");
export const channelConsent = (draft, url) => call("channel_consent", { draft, url });
export const channelAdd = (draft, url) => call("channel_add", { draft, url });
export const channelEdit = (id, draft, url) => call("channel_edit", { id, draft, url });
export const channelSetEnabled = (id, on) => call("channel_set_enabled", { id, on });
export const channelRemove = (id) => call("channel_remove", { id });
export const channelTest = (id) => call("channel_test", { id });
// PRD 0041 RF5.1: the core's live sentences and problems of the dialog's budget and floor rows.
export const checkBudgets = (index, provider, budgets, floors, locale) => call("check_budgets", { index, provider, budgets, floors, locale });
/** PRD 0041 RF6: the draft of "Set a budget…" (plan-price prefill, "Last 30 days" line) for one metric; saves nothing. */
export const budgetSuggestion = (index, label) => call("budget_suggestion", { index, label });
/** PRD 0043 RF4.2: the last 24 hours of the account the menu-bar strip follows (`null` without history). */
export const hourlyStrip = () => call("hourly_strip");
// PRD 0008 RF3: usage alerts (desktop.json; the backend judges each reading and sends them). `setAlertsEnabled` asks
// the same system permission as reset notices and resolves to `granted`, `denied`, `prompt` or `off`.
export const setAlertsEnabled = (on) => call("set_alerts_enabled", { on });
/** PRD 0024 RF4.1: the projected-exhaustion warning, `minutes` (15, 30 or 60) before the stop; resolves to the
 *  permission like `setAlertsEnabled` (turning it on stays off unless `granted`). */
export const setProjectedAlert = (on, minutes) => call("set_projected_alert", { on, minutes });
export const setAlertOptions = (thresholds, exhaustion, recovery, overage) => call("set_alert_options", { thresholds, exhaustion, recovery, overage });
export const setMetricAlert = (accountKey, label, on) => call("set_metric_alert", { accountKey, label, on });
export const setPoolAlerts = (accountKey, each) => call("set_pool_alerts", { accountKey, each });
// PRD 0045 RF5.2: "Alert on breakdown rows" for one account.
export const setBreakdownAlerts = (accountKey, on) => call("set_breakdown_alerts", { accountKey, on });
// PRD 0011 RF4.3: public provider status queries (desktop.json, off by default; the backend asks the core).
export const setStatusEnabled = (on) => call("set_status_enabled", { on });
// PRD 0012 RF5: the local server (desktop.json, off by default); the backend runs the core's server over this app's
// own refresh cycle. Each call answers the server's new state; the token never comes back except to be copied.
export const serveStatus = () => call("serve_status");
export const setServe = (enabled, port, allowBrowsers, allowedOrigins = []) => call("set_serve", { enabled, port, allowBrowsers, allowedOrigins });
// PRD 0031 RF11.2.5: "Share usage statistics" (`serve.stats`, /v1/stats), answered only with a token.
export const setServeStats = (stats) => call("set_serve_stats", { stats });
// PRD 0050 RF6.1/RF6.3: the private access log (`serve.access_log`: off by default, errors, all) and its erasure.
export const setServeAccessLog = (mode) => call("set_serve_access_log", { mode });
export const eraseServeAccessLog = () => call("erase_serve_access_log");
export const generateServeToken = () => call("generate_serve_token");
export const revokeServeToken = () => call("revoke_serve_token");
export const copyServeToken = () => call("copy_serve_token");
// PRD 0013 RF4: hooks (desktop.json, off by default); the backend runs the core's hooks over this app's own refresh
// cycle. Each call answers the hooks' new state; `onHookRun` fires after every run so the log is read again.
export const hooksStatus = () => call("hooks_status");
export const setHooksEnabled = (on) => call("set_hooks_enabled", { on });
// PRD 0050 RF2.2: the shell lists the commands in a native dialog and approves only on its answer.
export const confirmHooks = () => call("confirm_hooks");
export const saveHooks = (revision, rules) => call("save_hooks", { revision, rules });
export const testHook = (name) => call("test_hook", { name });
// PRD 0014 RF4.1: the widget snapshot (desktop.json, off by default). Each call answers the new state (the
// switch, whether identity travels with it, how many accounts, where the file is and whether it is the app
// group an extension can read). `deleteWidgetData` removes widget.json now.
export const widgetsStatus = () => call("widgets_status");
export const setWidgets = (enabled, includeIdentity, maxAccounts) => call("set_widgets", { enabled, includeIdentity, maxAccounts });
export const deleteWidgetData = () => call("delete_widget_data");
export const onHookRun = (cb) => {
  if (isDesktop) return T.event.listen("hooks", () => cb());
  demoHookListeners.add(cb);
  return Promise.resolve(() => demoHookListeners.delete(cb));
};
// PRD 0015 RF1/RF2/RF4: local cost from the logs of command-line tools (off by default; the core's preferences, index,
// background scan and summary). `costEnable` sends only the source: Rust asks in a system dialog; each settings call
// answers the new state. `costUsage` is the core's CostSummary the cards and details show.
export const costStatus = () => call("cost_status");
export const costUsage = () => call("cost_usage");
// PRD 0030 RF1/RF6: the plan prices of the configured accounts, the display currency and the plan value lines (the
// core's `PlansView`); typed by the person, kept on this computer, never exported. `price: null` clears a plan,
// `code: null` clears the display currency.
export const costPlans = () => call("cost_plans");
export const costSetPlan = (id, price, currency, cycleDay) => call("cost_set_plan", { id, price, currency, cycleDay });
export const costSetDisplay = (code, rate) => call("cost_set_display", { code, rate });
export const costEnable = (source) => call("cost_enable", { source });
export const costDisable = (source) => call("cost_disable", { source });
export const costSetPaths = (source, paths) => call("cost_set_paths", { source, paths });
export const costSetOptions = (period, retentionDays) => call("cost_set_options", { period, retentionDays });
export const costPrices = () => call("cost_prices");
export const costErase = () => call("cost_erase");
// PRD 0042 RF6.1: one tab of the breakdown (`day`, `model`, `session`, `block`, `project`), labels, hidden projects and
// each source's block length and folder-name hint. `offset` skips that many sessions ("Show older sessions").
export const costBreakdown = (by, offset = 0) => call("cost_breakdown", { by, offset });
export const costSetLabel = (target, digest, label) => call("cost_set_label", { target, digest, label });
export const costSetHidden = (digest, hidden) => call("cost_set_hidden", { digest, hidden });
export const costSetSourceOptions = (source, blockHours, folderHint) => call("cost_set_source_options", { source, blockHours, folderHint });
// PRD 0018 RF2.6/RF3: updater state and user actions. The UI never receives an
// endpoint or signature; the backend returns only what it presents (consent,
// phase, version, notes, progress and the off reason). `onUpdate` listens to the
// `updater` event from both windows.
export const updaterStatus = () => call("updater_status");
export const setUpdaterAutomatic = (on) => call("updater_set_automatic", { on });
export const updaterAnswerAutomatic = (on) => call("updater_answer_automatic", { on });
export const updaterCheckNow = () => call("updater_check_now");
export const updaterInstall = () => call("updater_install");
export const updaterCancelDownload = () => call("updater_cancel_download");
export const updaterDismiss = () => call("updater_dismiss");
export const updaterRestart = () => call("updater_restart");
const demoUpdateListeners = new Set();
export const onUpdate = (cb) => {
  if (isDesktop) return T.event.listen("updater", (e) => cb(e.payload));
  demoUpdateListeners.add(cb);
  return Promise.resolve(() => demoUpdateListeners.delete(cb));
};
const demoUpdateEmit = (view) => demoUpdateListeners.forEach((cb) => cb(view));
/** RF3.4: "Next notice: Claude 5h · today, 14:30" from the view, or null when none is ahead. */
export function nextNoticeText(view) {
  return nextNoticeParts(view)?.text ?? null;
}
/** RF3.4: the next notice as its full sentence, split around the time so the tray can let the name shrink and keep
 *  the time whole (DS-MSG-15): `head` is everything before `{when}`, `when` the time and what follows it. */
export function nextNoticeParts(view, now = new Date()) {
  const n = view?.notify?.next;
  if (!n) return null;
  const key = `card.binding.${n.window}`;
  const window = t(key) === key ? tl(n.label_text ?? n.label) : t(key);
  const when = fmtNoticeTime(n.at, now);
  if (!when) return null;
  const text = t("notify.next", { name: `${brand(n.provider).label} ${window}`, when });
  const at = text.lastIndexOf(when);
  return at < 0 ? { text, head: text, when: "" } : { text, head: text.slice(0, at), when: text.slice(at) };
}
/** DS-MSG-8 for a time ahead: today the time, within 6 days the short weekday and time, otherwise the short date and
 *  time; the joins are translated patterns (DS-MSG-12). */
export function fmtNoticeTime(iso, now = new Date()) {
  if (!iso) return null;
  const d = new Date(iso);
  if (Number.isNaN(d.getTime())) return null;
  const time = fmtLocal(d, HHMM);
  if (d.toDateString() === now.toDateString()) return t("time.today", { time });
  const startOfDay = (x) => new Date(x.getFullYear(), x.getMonth(), x.getDate()).getTime();
  const days = Math.round((startOfDay(d) - startOfDay(now)) / 86400000);
  if (days > 0 && days <= 6) return t("time.weekdayTime", { day: fmtLocal(d, { weekday: "short" }), time });
  return t("time.dateTime", { date: fmtLocal(d, { day: "numeric", month: "short" }), time });
}
/** RF3.2: the account's bell (card and details), only while reset notices or usage alerts (PRD 0008 RF3.2) are on;
 *  it mutes both. `aria-pressed` is its state. */
export function bellHtml(view, r) {
  if (!(view?.notify?.enabled || view?.notify?.alerts) || !r.account_key) return "";
  const label = t("notify.bell", { account: accountTitle(r) });
  const state = t(r.notify ? "notify.bell.on" : "notify.bell.off");
  return `<button class="icon bell${r.notify ? " on" : ""}" aria-pressed="${r.notify}" aria-label="${esc(label)}" title="${esc(`${label}: ${state}`)}" data-bell="${esc(r.account_key)}" data-bell-on="${r.notify}">${icon(r.notify ? "bell" : "bellOff")}</button>`;
}
/** PRD 0008 RF3.2: one metric's usage alerts in the details (only while alerts are on and the metric can alert);
 *  pressed = alerts on, unpressed = muted. */
export function metricAlertHtml(r, m) {
  if (!m.alert || !r.account_key) return "";
  const on = !m.alert.muted;
  // PRD 0034 RF2: the name reads in the interface language; `data-metric-label` keeps the raw label (the key).
  const label = t("alerts.metric", { metric: tl(m.label_text ?? m.label) });
  const state = t(on ? "notify.bell.on" : "notify.bell.off");
  return `<button class="icon bell${on ? " on" : ""}" aria-pressed="${on}" aria-label="${esc(label)}" title="${esc(`${label}: ${state}`)}" data-metric-alert="${esc(r.account_key)}" data-metric-label="${esc(m.label)}" data-metric-on="${on}">${icon(on ? "bell" : "bellOff")}</button>`;
}
/** "About": project identity and open-source library list (comes from the core). */
export const aboutInfo = () => call("about_info");
/** PRD 0019 RF7.7: the changelog history already parsed by the core (no network). */
export const whatsNew = () => call("whats_new");
/** Opens in the browser a repository from the "About" list (the shell refuses any other address). */
export const reportProblem = (index, generation) => call("report_problem", { index: index ?? null, generation });
/** PRD 0028 RF2.3 (re-validation S-01): the shell rebuilds the draft and opens it; no URL leaves the page. */
export const openProblemReport = (index, generation) => call("open_problem_report", { index: index ?? null, generation });
export const openExternal = (url) => (isDesktop ? call("open_external", { url }) : Promise.resolve(window.open(url, "_blank", "noopener,noreferrer")));
/** PRD 0020 RF1.3: every entry point opens (or focuses) the one Settings window, optionally on a page of its
 *  sidebar and an anchor inside it (`{ page: "cost", anchor: "cost-sources" }`). The browser demo opens
 *  `settings.html` in a named window, so a second click reuses it (RF1.5). */
export function openSettingsWindow({ page = null, anchor = null } = {}) {
  if (isDesktop) return T.core.invoke("open_settings", { page, anchor }).catch(() => {});
  const query = new URLSearchParams(Object.entries({ page, anchor }).filter(([, v]) => v)).toString();
  window.open(`settings.html${query ? `?${query}` : ""}`, "cuotai-settings");
  return Promise.resolve();
}
/** PRD 0020 RF1.3: `⌘,` on macOS, `Ctrl+,` elsewhere, in every window. */
export function bindSettingsShortcut(open = () => openSettingsWindow()) {
  addEventListener("keydown", (e) => {
    if (e.key !== "," || e.altKey || e.shiftKey) return;
    const mac = /Mac/i.test(navigator.platform || navigator.userAgent || "");
    if (mac ? !e.metaKey || e.ctrlKey : !e.ctrlKey || e.metaKey) return;
    e.preventDefault();
    open();
  });
}
/** PRD 0020 RF7.1 (minimum for RF1): the Settings window says what it changed; the other windows repaint as the
 *  old in-window dialog made them do. Preference changes carry their key (RF7.2). */
export const SETTINGS_CHANGED = "settings-changed";
// In the browser demo the windows share one origin, so a BroadcastChannel stands in for the shell's event.
const demoChannel = () => { try { return new BroadcastChannel("cuotai-settings-changed"); } catch { return null; } };
export const emitSettingsChanged = (key = "") => {
  if (isDesktop) return T.event.emit(SETTINGS_CHANGED, { key }).catch(() => {});
  const ch = demoChannel(); ch?.postMessage({ key }); ch?.close();
  return Promise.resolve();
};
/** PRD 0025 RF5.2: Settings › Dashboard asks the main window, which owns the account dialogs, to add, edit or
 *  remove an account (`action`: "add" | "edit" | "remove"; `index`: the account). */
export const ACCOUNT_ACTION = "account-action";
const actionChannel = () => { try { return new BroadcastChannel("cuotai-account-action"); } catch { return null; } };
/** `extra` carries plain ids only, e.g. `{ secretsOf }`: the extension whose stored secrets the removal offers to delete. */
export const requestAccountAction = async (action, index = null, extra = {}) => {
  if (isDesktop) {
    await T.core.invoke("show_main_window").catch(() => {});
    return T.event.emit(ACCOUNT_ACTION, { action, index, ...extra }).catch(() => {});
  }
  const ch = actionChannel(); ch?.postMessage({ action, index, ...extra }); ch?.close();
};
export const onAccountAction = (cb) => {
  if (isDesktop) return T.event.listen(ACCOUNT_ACTION, (e) => cb(e.payload ?? {}));
  const ch = actionChannel();
  if (ch) ch.onmessage = (e) => cb(e.data ?? {});
  ch?.unref?.();
  return Promise.resolve(() => ch?.close());
};
export const onSettingsChanged = (cb) => {
  if (isDesktop) return T.event.listen(SETTINGS_CHANGED, (e) => cb(e.payload?.key ?? ""));
  const ch = demoChannel();
  if (ch) ch.onmessage = (e) => cb(e.data?.key ?? "");
  // Node (the UI smokes) keeps a process alive while a channel listens; `unref` lets it exit. Browsers lack it.
  ch?.unref?.();
  return Promise.resolve(() => ch?.close());
};
/** Native folder picker (Tauri dialog plugin). */
export async function pickFolder(defaultPath) {
  if (isDesktop && T.dialog) return T.dialog.open({ directory: true, multiple: false, defaultPath: defaultPath || undefined });
  return "/Users/demo/CuotaiUser"; // navegador (demo)
}
/// RF1.2: applies the chosen `cuotai.toml` (`null` = OS default); reloads the accounts and returns the new view.
export const setConfigFile = (path) => call("set_config_file", { path });
/** Native TOML file picker. */
export async function pickTomlFile(defaultPath) {
  if (isDesktop && T.dialog) return T.dialog.open({ multiple: false, directory: false, defaultPath: defaultPath || undefined, filters: [{ name: "TOML", extensions: ["toml"] }] });
  return "/Users/demo/cuotai.toml"; // navegador (demo)
}
/** Effective path of `cuotai.toml` with its origin (variable, chosen or OS default); unescaped text. */
export const configLabel = (view) => view?.config_path ? `${view.config_path} · ${t(`settings.config.source.${view.config_source}`)}` : "";
/** RF1.2: notice when the TUI last used another file; unescaped text. */
/** PRD 0051 RF5.4: the moved-file question (core text) with its two answers, "Use this history" and "Keep apart",
 *  and a dismiss that only hides it for this run (DS-CMP-MSG-2): the offer comes back at the next start until it is
 *  answered. Empty without an offer, or once hidden for that same question. */
let hiddenRelocation = null;
const relocationKey = (view) => JSON.stringify(view?.history_relocation ?? null);
export const hideRelocation = (view) => { hiddenRelocation = relocationKey(view); };
export const relocationBannerHtml = (view) => view?.history_relocation && relocationKey(view) !== hiddenRelocation
  ? `<div class="banner dismissable" role="status" data-relocation>${icon("info")}<span>${esc(tl(view.history_relocation))} <button class="link" data-relocation-adopt>${te("relocation.use")}</button> <button class="link" data-relocation-keep>${te("relocation.keep")}</button></span><button class="icon" data-relocation-hide title="${te("common.dismiss")}" aria-label="${te("common.dismiss")}">${icon("x")}</button></div>`
  : "";
/** PRD 0051 RF1.2/RF1.3: the configuration notices. An account set aside (`issue`) is an error (`bad`); an unknown
 *  key kept and ignored (`warning`) is information (`info`). */
export const configNoticesHtml = (view) => (view?.config_notices ?? []).map((note) => note?.kind === "warning"
  ? `<div class="banner info" role="status" data-config-notice="warning">${icon("info")} ${esc(tl(note.text))}</div>`
  : `<div class="banner bad" role="status" data-config-notice="issue">${icon("warn")} ${esc(tl(note?.text ?? note))}</div>`).join("");
export const otherConfigNotice = (view) => view?.other_config ? t("config.other", { tui: view.other_config, app: view.config_path }) : "";
/** RF1.2 banner of the main window (DS-CMP-MSG-2, dismissable): each (TUI path, app path) pair shows in one session
 *  only. The first render while the window is visible records the pair in localStorage, so a later launch does not show
 *  it again; within the session it stays until the close button hides it, also after switching files and back. A
 *  different pair shows again. Settings › Advanced › Files keeps the hint (`otherConfigNotice`). */
const OTHER_SEEN_KEY = "cuotai.otherConfigSeen";
const OTHER_SEEN_MAX = 20;
const otherShown = new Set();
const otherDismissed = new Set();
const otherPair = (view) => JSON.stringify([view.other_config, view.config_path]);
export function otherConfigBanner(view) {
  const text = otherConfigNotice(view);
  if (!text) return "";
  const pair = otherPair(view);
  if (otherDismissed.has(pair)) return "";
  if (otherShown.has(pair)) return text;
  let seen = [];
  try { const v = JSON.parse(localStorage.getItem(OTHER_SEEN_KEY) || "[]"); if (Array.isArray(v)) seen = v; } catch {}
  if (seen.includes(pair)) return "";
  // A hidden window renders too: nobody saw it yet, so it is not recorded (the next visible render does).
  if (globalThis.document?.hidden) return text;
  otherShown.add(pair);
  try { localStorage.setItem(OTHER_SEEN_KEY, JSON.stringify([...seen.filter((x) => x !== pair), pair].slice(-OTHER_SEEN_MAX))); } catch {}
  return text;
}
export function dismissOtherConfig(view) { if (view?.other_config) otherDismissed.add(otherPair(view)); }
export const setTrayMenu = (payload) => isDesktop && T.core.invoke("set_tray_menu", { payload });
export const openMainWindow = () => isDesktop && T.core.invoke("show_main_window");
export const quitApp = () => isDesktop && T.core.invoke("quit_app");
// PRD 0002 RF6.1: per-account model catalog. `generation` comes from the `View` the UI read; if the configuration changed
// since then the backend refuses with "stale" and the result (from the old identity) is discarded.
export const STALE = "stale";
export const modelState = (index, generation) => call("model_state", { index, generation });
export const loadModels = (index, generation) => call("load_models", { index, generation });
export const syncModels = (index, generation) => call("sync_models", { index, generation });
/** Catalog source or reason for not listing, for an account not saved yet (no network). */
export const modelSupport = (provider) => call("model_support", { provider });
// RF8: backend errors arrive as JSON `{ id, params, fallback }` or plain text.
export const errText = (e) => {
  if (e && typeof e === "object" && "id" in e) return tl(e);
  if (typeof e === "string") {
    try {
      const parsed = JSON.parse(e);
      if (parsed && typeof parsed === "object" && "id" in parsed) return tl(parsed);
    } catch {}
    return e;
  }
  if (e?.message) return errText(e.message);
  return t("common.unexpected");
};

/**
 * R-0018-01: an updater command's refusal (`login_in_progress`, `unwritable`, `stale`, …) or failure code
 * (`network`, `io`, …) through its `update.error.*` sentence; anything else goes through `errText`.
 */
export const updateErrText = (e) => {
  if (typeof e === "string" && /^[a-z_]+$/.test(e)) {
    const key = `update.error.${e}`;
    const text = t(key);
    if (text !== key) return text;
  }
  return errText(e);
};

// ---------- formatting ----------
/** Remaining percentage (0–100) per the core's classification; null = no measure/limit. */
export function remainingPct(m) {
  if (m.state === "exhausted") return 0;
  if (m.state === "available" && !m.uncapped && m.ratio != null) return m.ratio * 100;
  return null;
}
export const usedPct = (m) => {
  const r = remainingPct(m);
  return r == null ? null : 100 - r;
};
export const level = (p) => (p == null ? "unknown" : p >= 70 ? "good" : p >= 30 ? "warn" : "bad");
// Intl formatters by kind, locale and options. Building one costs far more than formatting with it, and one
// render of the dashboard formats hundreds of numbers and times (the profile showed them at a tenth of a render).
const formatters = new Map();
export function intlFormatter(kind, locale, options) {
  const key = `${kind}|${locale}|${JSON.stringify(options)}`;
  let f = formatters.get(key);
  if (!f) { f = new Intl[kind](intlTag(locale), options); formatters.set(key, f); } // a bad tag formats as en-US (S-05)
  return f;
}
/** `date.toLocaleTimeString(locale, options)` / `toLocaleDateString` with the formatter cached (same output when
 *  `options` names the fields, which every caller here does). */
// PRD 0034 re-validation V-10: Chrome's `Intl` writes a plain space before the day-period mark ("2:03 PM"); a no-break
// space keeps the time on one line with its mark, as the core writes it.
export const fmtLocal = (date, options, locale = currentLocale()) =>
  intlFormatter("DateTimeFormat", locale, options).format(date).replace(/(\d)[ \u202f](?=[AaPp]\.?[Mm]\.?(?!\p{L}))/gu, "$1\u00a0");
const HHMM = { hour: "2-digit", minute: "2-digit" };
export const fmtNumber = (n, max = 2) => (n == null ? "—" : intlFormatter("NumberFormat", currentLocale(), { maximumFractionDigits: max }).format(n));
/** PRD 0034 re-validation V-17: a share already in percent (`64` for 64 %) in the current language: "64%", "64 %"
 *  (de, fr), "%64" (tr). Up to `max` decimals. */
export const fmtPercent = (p, max = 0) =>
  p == null ? "—" : intlFormatter("NumberFormat", currentLocale(), { style: "percent", maximumFractionDigits: max }).format(p / 100);
/** PRD 0030 L-13: `amount` in `currency` (an ISO code) in the current language ("R$ 550,50", "$20.00"); an unknown
 *  code reads "ZZZ 550.50". */
export const fmtMoney = (amount, currency) => {
  if (amount == null) return "—";
  try { return intlFormatter("NumberFormat", currentLocale(), { style: "currency", currency }).format(amount); } catch { return `${currency} ${Number(amount).toFixed(2)}`; }
};
/** PRD 0030 L-01: a typed rate with up to six significant digits ("0.0000099", "5.4321"), never rounded to zero. */
export const fmtRate = (n) => (n == null ? "—" : intlFormatter("NumberFormat", currentLocale(), { maximumSignificantDigits: 6 }).format(n));
// PRD 0037 re-validation (V-03): the English singular of each counted unit, the twin of the core's
// `metric_text::UNITS` (there is no en-US core catalog, so English reads the core's `fallback`, its plural form).
const UNIT_ONE_EN = {
  "metric.unit.credits": "credit",
  "metric.unit.requests": "request",
  "metric.unit.requisitions": "request",
  "metric.unit.voices": "voice",
  "metric.unit.units": "unit",
  "metric.unit.queries": "query",
  "metric.unit.members": "member",
  "metric.unit.models": "model",
  "metric.unit.calls": "call",
  "metric.unit.provider_units": "provider unit",
  "metric.unit.tokens": "token",
  "metric.unit.images": "image",
  "metric.unit.frames": "frame",
};
/** PRD 0034 re-validation V-05: the metric's unit as a person reads it (`unit_text`, the core's display text); the raw
 *  `unit` is an id (often Portuguese) and is shown only when the core sent no text. PRD 0037 re-validation (V-03):
 *  with the `figure` printed right before it, a counted unit takes that figure's plural form ("1 voice",
 *  "4 000 кредитов"); without one it reads in the plural (`other`) form. */
export const unitName = (m, figure) => {
  if (!m?.unit_text) return m?.unit ?? "";
  if (figure == null || !Number.isFinite(Number(figure))) return tl(m.unit_text);
  const one = UNIT_ONE_EN[m.unit_text.id];
  const n = Math.abs(Number(figure));
  const singular = one && new Intl.PluralRules("en-US").select(n) === "one";
  return tl(singular ? { ...m.unit_text, fallback: one } : m.unit_text, { count: n });
};
// Units that describe "request count": left implicit. Others (USD, credits, tokens) are shown. The test keys on ids
// (the raw unit's symbols or the core's `metric.unit.*` id of `unit_text`), never on a word of some language.
const QUIET_UNITS = new Set(["", "%", "—", "requests"]);
const QUIET_UNIT_IDS = new Set(["metric.unit.requests", "metric.unit.requisitions", "metric.unit.queries", "metric.unit.units"]);
const isQuietUnit = (m) => QUIET_UNITS.has(m.unit ?? "") || QUIET_UNIT_IDS.has(m.unit_text?.id);
// A no-break space: the figure never wraps away from its unit (PRD 0034 re-validation V-10).
// The unit agrees with the figure printed right before it (`figure`).
const unitSuffix = (m, figure) => (isQuietUnit(m) ? "" : `\u00a0${unitName(m, figure)}`);
/** PRD 0038 F-03: the twin of the core's `Metric::is_balance_shaped`: no `used`, no `limit` and a finite `remaining`
 *  (a prepaid balance). Its main figure is the remaining amount with its sign, never "left". */
export const isBalanceShaped = (m) =>
  m?.used == null && m?.limit == null && m?.remaining != null && Number.isFinite(Number(m.remaining));
// PRD 0038 F-14: an amount in an ISO 4217 currency reads as money ("$4.50"), as the TUI's `format_money` prints it.
const isCurrencyUnit = (m) => /^[A-Z]{3}$/.test(m?.unit ?? "");
/** `n` in the unit of metric `m`: money for a currency, else the number and its unit in the figure's plural form. */
const fmtFigure = (m, n) => (isCurrencyUnit(m) ? fmtMoney(n, m.unit) : `${fmtNumber(n)}${unitSuffix(m, n)}`);
/** PRD 0041 D1: a budget whose period was not fully recorded (the backend's `partial`) shows its use as a lower bound,
 *  "≥ $45.00 / $50.00", as the TUI and `--once` print it. */
const atLeast = (m, text) => (m?.partial && m.used != null ? `≥ ${text}` : text);
export function fmtAmount(m) {
  if (isBalanceShaped(m)) return fmtFigure(m, m.remaining);
  if (m.limit == null) return m.used == null ? "—" : atLeast(m, fmtFigure(m, m.used));
  if (isCurrencyUnit(m)) return atLeast(m, `${fmtMoney(m.used, m.unit)} / ${fmtMoney(m.limit, m.unit)}`);
  return atLeast(m, `${fmtNumber(m.used)} / ${fmtNumber(m.limit)}${unitSuffix(m, m.limit)}`);
}
/** One recorded value of metric `m` (history; its `unit` and `unit_text`): "60%", "$12.50", or a bare count. */
export const fmtValue = (v, m) => (m.unit === "%" ? fmtPercent(v, 2) : fmtFigure(m, v));
/** Metric without a usable limit: text in place of the bar (never invents "100% free"). PRD 0037 RF5: a metric the
 *  provider stated has no cap (`unlimited`) says "No cap" after its amount. */
export function textualMetric(m) {
  // PRD 0038 F-13: a balance at or below zero still says how much ("Exhausted · −1.50 credits").
  if (m.state === "exhausted" && m.no_cap)
    return isBalanceShaped(m) ? t("metric.exhaustedAmount", { amount: fmtFigure(m, m.remaining) }) : t("metric.exhausted");
  if (m.state === "available" && m.uncapped) {
    const amount = m.remaining ?? m.used;
    if (amount == null) return m.unlimited ? t("metric.noCap") : null;
    const figure = m.remaining != null ? fmtFigure(m, amount) : atLeast(m, fmtFigure(m, amount));
    const text = t(m.remaining != null ? "metric.balance" : "metric.usage", { amount: figure });
    return m.unlimited ? `${text} · ${t("metric.noCap")}` : text;
  }
  return null;
}
/** PRD 0038 RF4.2: a capped `usage` counter at its limit (every voice slot taken, the `:free` requests of a paid key)
 *  is "All in use", never "Exhausted": a usage counter does not block the account. */
export const isFullCounter = (m) => m?.kind === "usage" && m.state === "exhausted" && !m.no_cap;
/** PRD 0024 RF1.4/RF6.1: "Resets 19:30 · in 1h 35m", worded by the core (`resets_label`); only a reading without it
 *  (the browser demo, a window already past) falls back to the local countdown. Null without a reset time. */
export function resetsText(m) {
  if (m.resets_label) return tl(m.resets_label);
  const left = legacyCountdown(m.resets_at);
  return left ? t("quota.resetsIn", { time: left }) : null;
}
/** PRD 0038 F-09: a metric's period ("30 completed days (UTC)") as the core words it (`period_text`), or "". */
export const periodText = (m) => (m?.period_text ? tl(m.period_text) : "");
/** PRD 0038 F-09: a daily window that resets at 00:00 UTC, seen from a zone where that is not midnight, says so in
 *  words: "Resets at UTC midnight (21:00) · in 3h 5m", the time in the viewer's zone. Null otherwise (a zero offset,
 *  another window, another hour), so the core's `resets_label` stays the reset line. `offset` (minutes, the viewer's
 *  zone at the reset) is injectable for tests. */
export function utcMidnightResetText(m, now = new Date(), offset = (d) => d.getTimezoneOffset()) {
  if (m?.window !== "daily" || !m.resets_at) return null;
  const d = new Date(m.resets_at);
  if (Number.isNaN(d.getTime()) || d.getTime() <= now.getTime()) return null;
  if (d.getUTCHours() || d.getUTCMinutes() || d.getUTCSeconds() || !offset(d)) return null;
  const time = d.toDateString() === now.toDateString() ? fmtLocal(d, HHMM) : fmtReset(m.resets_at);
  return `${t("metric.resetsUtcMidnight", { time })} · ${tl(countdownNote(m.resets_at))}`;
}
// PRD 0035 RF10.3: durations are catalog patterns (the core's `text.duration.*`, with their English fallbacks), never
// concatenated letters, so each language words them (`{h}時間{m}分`, `{h} ч {m} мин`).
const DURATION_EN = {
  "text.duration.minutes": "{m}\u00a0min",
  "text.duration.hours": "{h}h",
  "text.duration.hours_minutes": "{h}h {m}m",
  "text.duration.days": "{d}d",
  "text.duration.days_hours": "{d}d {h}h",
};
// PRD 0034 re-validation (S-02): the raw English pattern is the fallback, so `tl` substitutes the params once.
const durationPart = (id, params) => tl({ id, params, fallback: DURATION_EN[id] });
/** A span of `secs` seconds in the core's words (the twin of `cuotai_core::text::duration`): under an hour in whole
 *  minutes rounded up, under a day in hours and minutes, otherwise in days and hours. */
export function durationText(secs) {
  secs = Math.max(0, Math.floor(secs));
  if (secs < 3600) return durationPart("text.duration.minutes", { m: String(Math.min(59, Math.ceil(secs / 60))) });
  const minutes = Math.floor(secs / 60);
  const d = Math.floor(minutes / 1440), h = Math.floor(minutes / 60) % 24, m = minutes % 60;
  if (d === 0) return m === 0 ? durationPart("text.duration.hours", { h: String(h) }) : durationPart("text.duration.hours_minutes", { h: String(h), m: String(m) });
  return h === 0 ? durationPart("text.duration.days", { d: String(d) }) : durationPart("text.duration.days_hours", { d: String(d), h: String(h) });
}
/** The core's `text.duration.*` note of `secs` seconds, as the shell sends it (demo fixtures only). */
function durationNote(secs) {
  secs = Math.max(0, Math.floor(secs));
  // Interpolated once here, as the core does (its values are digits, so no placeholder can come from them).
  const note = (id, params) => ({ id, params, fallback: DURATION_EN[id].replace(/\{(\w+)\}/g, (m, n) => (n in params ? String(params[n]) : m)) });
  if (secs < 3600) return note("text.duration.minutes", { m: String(Math.min(59, Math.ceil(secs / 60))) });
  const minutes = Math.floor(secs / 60);
  const d = Math.floor(minutes / 1440), h = Math.floor(minutes / 60) % 24, m = minutes % 60;
  if (d === 0) return m === 0 ? note("text.duration.hours", { h: String(h) }) : note("text.duration.hours_minutes", { h: String(h), m: String(m) });
  return h === 0 ? note("text.duration.days", { d: String(d) }) : note("text.duration.days_hours", { d: String(d), h: String(h) });
}
/** The core's `text.countdown` note until `iso` (demo fixtures only). */
function countdownNote(iso) {
  const secs = (new Date(iso).getTime() - Date.now()) / 1000;
  if (!(secs >= 1)) return { id: "text.countdown.now", params: {}, fallback: "now" };
  const duration = durationNote(secs);
  return { id: "text.countdown", params: { duration }, fallback: `in ${duration.fallback}` };
}
function legacyCountdown(iso) {
  if (!iso) return null;
  const s = Math.floor((new Date(iso).getTime() - Date.now()) / 1000);
  if (Number.isNaN(s)) return null;
  if (s <= 0) return t("time.now");
  return durationText(s);
}
/** Countdown to the next query: "4m 20s", "1h 05m", "2d 3h". `ms` is an absolute instant (Unix, ms). */
export function fmtNext(ms, now = Date.now()) {
  let s = Math.ceil((ms - now) / 1000);
  if (!Number.isFinite(s)) return "";
  if (s <= 0) return t("time.now");
  const d = Math.floor(s / 86400); s -= d * 86400;
  const h = Math.floor(s / 3600); s -= h * 3600;
  const m = Math.floor(s / 60); s -= m * 60;
  if (d > 0) return durationPart(h ? "text.duration.days_hours" : "text.duration.days", { d: String(d), h: String(h) });
  if (h > 0) return durationPart("text.duration.hours_minutes", { h: String(h), m: String(m).padStart(2, "0") });
  return m > 0 ? t("time.duration.minutesSeconds", { m, s: String(s).padStart(2, "0") }) : t("time.duration.seconds", { s });
}
/** Date without a time, localized for release and other calendar-date labels (DS-MSG-8). */
export function fmtDate(iso) {
  if (!iso) return null;
  const d = new Date(iso);
  return Number.isNaN(d.getTime())
    ? null
    : fmtLocal(d, { day: "numeric", month: "short", year: "numeric", timeZone: "UTC" });
}
/** Day and clock time of `iso` in the viewer's zone, always with the day ("22 Oct, 18:10"). */
export function fmtDateTime(iso) {
  if (!iso) return null;
  const d = new Date(iso);
  if (Number.isNaN(d.getTime())) return null;
  const time = fmtLocal(d, HHMM);
  return `${fmtLocal(d, { day: "numeric", month: "short" })}, ${time}`;
}
export function fmtReset(iso) {
  if (!iso) return null;
  const d = new Date(iso);
  if (Number.isNaN(d.getTime())) return null;
  const time = fmtLocal(d, HHMM);
  if (d.toDateString() === new Date().toDateString()) return t("time.today", { time });
  return `${fmtLocal(d, { day: "2-digit", month: "short" })}, ${time}`;
}
/** PRD 0033 re-validation (V-09): `iso` as the core's `text::clock` words it (the time today, the weekday and time within
 *  the next six days, the short date and time otherwise), in the UI locale's hour cycle ("2:30 PM", "14:30"), so the
 *  renewed and "data as of" texts read like the card's other times. Null for a missing or malformed instant. */
export function coreClock(iso) {
  if (!iso) return null;
  const d = new Date(iso);
  if (Number.isNaN(d.getTime())) return null;
  const locale = currentLocale();
  // `format_time`: a 24-hour language pads the hour ("09:05"), a 12-hour one never does ("9:05 AM").
  const h12 = /h1[12]/.test(intlFormatter("DateTimeFormat", locale, { hour: "numeric" }).resolvedOptions().hourCycle ?? "");
  const time = { hour: h12 ? "numeric" : "2-digit", minute: "2-digit" };
  const days = Math.round((new Date(d.toDateString()) - new Date(new Date().toDateString())) / 864e5);
  if (days === 0) return fmtLocal(d, time, locale);
  if (days >= 1 && days <= 6) return fmtLocal(d, { weekday: "long", ...time }, locale);
  return fmtLocal(d, { day: "2-digit", month: "2-digit", ...time }, locale);
}
/** PRD 0033 RF2.1: a preserved or restored metric whose window renewed; the core sends it without values. */
export const isRenewedWaiting = (m) => m.expired === true;
/** "Renewed at 2:30 PM; waiting for the next reading", in the viewer's local time. */
export const renewedText = (m) => t("metric.renewedWaiting", { time: coreClock(m.resets_at) ?? "—" });
export const statusLabel = (status) => t(`status.${status}`);
/** Quotas that define the account's summary and ordering: without the short rate-limit window (it is not the
 *  plan quota) and without the ones the user hid. */
export const planMetrics = (r) => r.metrics.filter((m) => m.kind !== "rate_limit" && !isOverage(m) && !isMetricHidden(r, m));

// ---------- PRD 0023: paid extra usage (overage) ----------
/** Billed on top of the plan: never a plan meter, never the summary, always a textual figure. */
export const isOverage = (m) => m.billing === "overage";
/** The core's overage reading of a current report; null for a failed, pending or preserved one. */
// PRD 0034 re-validation (G13): `unit` is the adapter's id (often Portuguese: `créditos`); `unit_name` is how a person
// reads it, the `unit_text` of the overage metric that carries that unit (the raw unit when no metric has one).
// Resolved on each call: the name follows a language switch.
// `figure`: the amount printed right before the unit (PRD 0037 re-validation V-03); none for the bare unit.
const overageUnitName = (r, o, figure) => {
  if (!o.unit || o.currency) return o.unit ?? "";
  const m = (r.metrics ?? []).find((x) => x.billing === "overage" && x.unit === o.unit);
  return m ? unitName(m, figure) : o.unit;
};
// `unit_name` is the bare unit; `unit_name_of(figure)` is the unit printed after `figure`.
export const overageOf = (r) =>
  r.status === "ready" && !r.pending && r.overage
    ? { ...r.overage, unit_name: overageUnitName(r, r.overage), unit_name_of: (figure) => overageUnitName(r, r.overage, figure) }
    : null;
/** The overage while money is being billed (`active`) or the extra budget is used up (`cap_reached`). */
export const billingOverage = (r) => {
  const o = overageOf(r);
  return o && (o.state === "active" || o.state === "cap_reached") ? o : null;
};
/** One amount: money with the locale's currency format, provider units as the core words them, anything else with
 *  its unit. A missing amount is "—", never zero (DS-MSG-5). */
/** `shown` is the unit as a person reads it (a metric's `unitName`, an overage's `unit_name`), or a function of the
 *  value that words it in the value's plural form (`unit_name_of`); `unit` stays the id the provider-units test reads. */
export function fmtExtra(value, unit, currency, shown = unit) {
  if (value == null) return "—";
  if (typeof shown === "function") shown = shown(value);
  if (currency) {
    try { return new Intl.NumberFormat(currentLocale(), { style: "currency", currency }).format(value); } catch { /* unknown code */ }
  }
  if (unit === "unid. provedor") {
    const n = fmtNumber(value);
    return tl({ id: "overage.amount.provider_units", params: { value: n }, fallback: `${n} provider units` });
  }
  return `${fmtNumber(value)}${shown ? `\u00a0${shown}` : ""}`;
}
/** PRD 0023 re-validation (V-01): the card figure of an extra amount: the number alone in the figure, its unit word
 *  on the small line (money: "extra"; provider units: the core's translated word; other units: the unit), so the
 *  one-third column never clips the number in any language. */
export function extraFigure(o) {
  if (o.spent == null) return { fig: "—", unit: t("figure.extra") };
  if (o.currency) return { fig: fmtExtra(o.spent, o.unit, o.currency), unit: t("figure.extra") };
  const unit = o.unit === "unid. provedor"
    ? tl({ id: "metric.unit.provider_units", params: {}, fallback: "provider units" })
    : (o.unit_name_of?.(o.spent) ?? o.unit_name ?? o.unit) || t("figure.extra");
  return { fig: fmtNumber(o.spent), unit };
}
/** Figure first (DS-MSG-4): "$6.25 extra of $50" for one overage metric. */
export function fmtMetricExtra(m) {
  if (m.used == null) return "—";
  const amount = fmtExtra(m.used, m.unit, m.kind === "spend" ? m.unit : null, unitName(m, m.used));
  return m.limit > 0 ? t("metric.overageCap", { amount, cap: fmtExtra(m.limit, m.unit, m.kind === "spend" ? m.unit : null, unitName(m, m.limit)) })
    : t("metric.overage", { amount });
}
/** The account-level line: the figure while billing, the core's words for the other states. */
export function overageLineText(o) {
  if (o.state === "disabled") return "";
  if (o.state === "active" || o.state === "cap_reached") {
    if (o.spent == null) return tl(o.text);
    const amount = fmtExtra(o.spent, o.unit, o.currency, o.unit_name_of ?? o.unit_name);
    return o.cap > 0 ? t("metric.overageCap", { amount, cap: fmtExtra(o.cap, o.unit, o.currency, o.unit_name_of ?? o.unit_name) }) : t("metric.overage", { amount });
  }
  return tl(o.text);
}
/** DS-CMP-EXTRA-2: the chip role of each overage state; disabled has no chip. */
const OVERAGE_CHIP = { active: "warn", cap_reached: "bad", armed: "muted", unknown: "muted unknown" };
/** Card chip: the core's summary ("Limit hit · on extra $6.25", "Extra budget used up", "Extra usage on · nothing
 *  spent", "Extra usage amount unknown"), icon plus text: warning while money is charged, critical at the cap
 *  (PRD 0023 re-validation V-03), muted when armed and muted dashed when the amount is unknown (V-05). */
export function overageChipHtml(r) {
  const o = overageOf(r);
  const role = o && OVERAGE_CHIP[o.state];
  if (!role) return "";
  const text = tl(o.summary);
  return `<span class="mini ${role}" data-overage-chip data-overage-state="${esc(o.state)}" title="${esc(text)}" aria-label="${esc(`${t("card.extra.title")}: ${text}`)}">${icon("coins")}<span class="mini-t">${esc(text)}</span></span>`;
}
/** Card row under the plan meters: textual, never a meter that implies "left" (DS-CMP-MTR-5). */
export function overageLineHtml(r) {
  const o = overageOf(r);
  if (!o || !["active", "cap_reached", "unknown"].includes(o.state)) return "";
  const text = overageLineText(o);
  return text ? `<p class="extra-line" data-overage-row title="${esc(tl(o.text))}">${icon("coins")}<span>${esc(text)}</span></p>` : "";
}
/** Details section: state, amount, cap, cycle end and the provider-units caveat. */
export function overageDetailHtml(r) {
  const o = overageOf(r);
  if (!o) return "";
  const cycle = fmtReset(o.cycle_ends_at);
  const cap = o.cap > 0 ? fmtExtra(o.cap, o.unit, o.currency, o.unit_name_of ?? o.unit_name) : null;
  return `<div class="metric extra-detail" data-overage-detail>
    <div class="metric-top"><div><h3>${te("card.extra.title")}</h3>${o.state === "active" ? "" : `<span class="sub">${esc(tl(o.text))}</span>`}</div></div>
    ${o.state === "active" || o.state === "cap_reached" ? `<div class="balance">${esc(overageLineText(o))}</div>` : ""}
    ${cap && !(o.state === "active" || o.state === "cap_reached") ? `<div class="resets">${te("detail.extra.cap", { cap })}</div>` : ""}
    ${cycle ? `<div class="resets">${te("detail.extra.cycle", { when: cycle })}</div>` : ""}
    ${o.caveat ? `<p class="note">${esc(tl(o.caveat))}</p>` : ""}
  </div>`;
}
export const worstRemaining = (r) => {
  const v = planMetrics(r).map(remainingPct).filter((x) => x != null);
  return v.length ? Math.min(...v) : null;
};
/** PRD 0009 RF2: % still available on the core's binding metric (`r.binding`); null without one. */
export const bindingPct = (r) => (r.status === "ready" && r.binding ? (1 - r.binding.used_ratio) * 100 : null);
/** Short name of the binding window ("Week"); windows the UI does not name use the metric's translated label
 *  (`label_text`, PRD 0034 RF2), never the adapter's raw text when the core sent the translation. */
export const bindingLabel = (r) => {
  const w = r.binding?.window;
  if (w && w !== "other") return t(`card.binding.${w}`);
  return r.binding?.label_text ? tl(r.binding.label_text) : r.binding?.label ?? "";
};
/** A metric's display name: its translated label (`label_text`) when the core sent one, else the raw label. */
export const metricName = (m) => (m?.label_text ? tl(m.label_text) : m?.label ?? "");
/** DS-CMP-MTR-4: a share in % rounded honestly: above 0 and below 1 reads "< 1%", below 100 never reads "100%". */
export const fmtShare = (p) => (p > 0 && p < 1 ? `< ${fmtPercent(1)}` : fmtPercent(p < 100 ? Math.min(99, Math.round(p)) : Math.round(p)));
export const isBlocked = (r) => r.status === "ready" && r.binding?.reason === "exhausted";
/** PRD 0037 RF4: the reset was already past when the provider answered (the core's `renewal_due`): the core's
 *  "Renewal due since 22:00; waiting for …" (the binding metric's `resets_label`), never a past "blocked until". */
export const renewalDueText = (r) => {
  const m = r.binding?.renewal_due ? r.metrics?.[r.binding.index] : null;
  return m?.resets_label ? tl(m.resets_label) : null;
};
/** "Blocked until Today 14:30" (unknown return stays unknown, never "now"); null when not blocked. */
export const blockedText = (r) => {
  if (!isBlocked(r)) return null;
  const due = renewalDueText(r);
  if (due) return due;
  const at = fmtReset(r.binding.blocked_until);
  return at ? t("card.blockedUntil", { time: at }) : t("card.blocked");
};
/** Chip text "Week · 100%" (consumed share of the binding window, rounded honestly). */
export const bindingChipText = (r) => `${bindingLabel(r)} · ${fmtShare(r.binding.used_ratio * 100)}`;
export const isBindingMetric = (r, m) => r.binding != null && r.metrics[r.binding.index] === m;
/** PRD 0010 RF4.1: the current reading is exhausted or blocked; a failure (even with a preserved reading) is not.
 *  An account usable now on paid extra usage (the core's overage state `armed` or `active`) is not recharging;
 *  `cap_reached`, `disabled`, `unknown` (amount missing, usability not confirmed) or no overage keep it in. */
export const isRecharging = (r) =>
  r.status === "ready" && (r.class === "exhausted" || isBlocked(r)) && !["armed", "active"].includes(overageOf(r)?.state);
/** "Returns Today 14:30", or "Return unknown" when the core knows no `blocked_until`. */
export const returnText = (r) => {
  const due = isBlocked(r) ? renewalDueText(r) : null;
  if (due) return due;
  const at = isBlocked(r) ? fmtReset(r.binding.blocked_until) : null;
  return at ? t("group.returnsAt", { time: at }) : t("group.returnUnknown");
};
// ---------- PRD 0010 RF3: measurement age and the preserved reading ----------
/** The current query failed, or none answered yet (`r.pending`, a reading restored from the history on opening),
 *  and the core kept the last valid reading (`r.last_good`): always shown as stale. */
export const isStaleReading = (r) => (r.pending || r.status !== "ready") && r.last_good != null;
/** Instant the reading on screen was measured: the preserved one while the current query failed. PRD 0040
 *  re-validation H-1: the core's `measured_at` (an agent feed's own time), never the query time beside it. */
export const shownAt = (r) => {
  const shown = isStaleReading(r) ? r.last_good : r;
  return shown.measured_at ?? shown.checked_at;
};
/** "3 min ago" in minute steps; a future instant (clock adjusted backwards) reads "now", never an error. */
export function fmtAge(iso, now = Date.now()) {
  const min = Math.floor((now - new Date(iso).getTime()) / 60000);
  if (Number.isNaN(min)) return "";
  if (min < 1) return t("time.now");
  const rtf = intlFormatter("RelativeTimeFormat", currentLocale(), { style: "short" });
  if (min < 60) return rtf.format(-min, "minute");
  return min < 1440 ? rtf.format(-Math.floor(min / 60), "hour") : rtf.format(-Math.floor(min / 1440), "day");
}
/** Core freshness of the reading on screen; a failure without a reading has none (only the attempt's age). */
const freshnessOf = (r) => (isStaleReading(r) ? "stale" : r.status === "ready" ? r.freshness : null);
/** Plain text: "Stale · 42 min ago", "Fresh · 3 min ago", or just the age; then "data as of …" when the provider's data
 *  lags the query, unless `asOf` is false (the card's status line already says it). */
export const ageText = (r, { asOf: withAsOf = true } = {}) => {
  const f = freshnessOf(r);
  const age = f ? `${t(`freshness.${f}`)} · ${fmtAge(shownAt(r))}` : fmtAge(shownAt(r));
  // PRD 0033 RF3.2: the provider states its data is older than the query ("data as of 06:00").
  const asOf = withAsOf && !isStaleReading(r) && r.data_as_of ? coreClock(r.data_as_of) : null;
  return asOf ? `${age} · ${t("freshness.data_as_of", { time: asOf })}` : age;
};
const FRESHNESS_ICON = { fresh: "check", aging: "clock", stale: "warn" };
/** Age chip with the freshness icon and text (never color alone); the absolute time is in the tooltip. */
export function ageHtml(r, opts) {
  const f = freshnessOf(r);
  const title = [t("age.measuredAt", { time: fmtReset(shownAt(r)) ?? "—" }), (isStaleReading(r) ? r.last_good.clock_adjusted : r.clock_adjusted) ? t("age.clockAdjusted") : ""]
    .filter(Boolean).join(" · ");
  return `<span class="age ${f ?? ""}" data-age data-freshness="${f ?? ""}" title="${esc(title)}">${icon(FRESHNESS_ICON[f] ?? "clock")}<span>${esc(ageText(r, opts))}</span></span>`;
}
/** The report with the preserved metrics in place of the failed ones, for drawing only: the status stays the
 *  failure and nothing from the old reading feeds the binding, alerts or ordering. */
export const staleView = (r) => ({ ...r, metrics: r.last_good.metrics, notes: [], notes_l10n: [], binding: null });
// PRD 0036 RF8: a 9router connection is keyed by the core's own key (`nine_router:<instance>/<id>`), never by a
// provider and name another account could share.
export const reportKey = (r) => (r.connection ? r.account_key : `${r.provider}::${r.account}`);
/** Resolve the structured note sidecar by the same index as the legacy fallback. PRD 0047 G1: `tl` swaps the core
 *  texts that name a terminal variable for the desktop's own wording (`desktop-wording.js`). */
export const noteText = (report, index) =>
  report.notes_l10n?.[index] ? tl(report.notes_l10n[index]) : report.notes[index] ?? "";
/** PRD 0047 G1: the account's first note asks for a web session (or the core offers a new paste): the card and the
 *  details offer the button that opens the account dialog on the session box. */
export const sessionAction = (r) => (r.paste_session ? "paste" : r.status !== "ready" && desktopAction(r.notes_l10n?.[0]) === "session" ? "connect" : null);
/** PRD 0005 RF4.2: what a failed account shows first: the core's explanation (an Unavailable account's reason, a
 *  changed undocumented interface), else the adapter's first note, else the status. */
/** PRD 0038 re-validation (F-05): a provider's `Retry-After` replaces the generic first note with the core's line
 *  that says when the account is queried again (`retry_note`). */
export const causeText = (r) => (r.pending ? t("card.waiting") : r.access?.explanation ? tl(r.access.explanation) : r.retry_note ? tl(r.retry_note) : r.notes.length ? noteText(r, 0) : statusLabel(r.status));
/** The tray row's failure line: the wait a `Retry-After` imposes, the first note, or the status. */
export const trayCauseText = (r) => (r.retry_note ? tl(r.retry_note) : noteText(r, 0) || statusLabel(r.status));
/** Adapter notes still to list after `causeText` (all of them when the core's explanation took the first place). */
export const restNotes = (r) => r.notes.map((_, index) => noteText(r, index)).slice(r.access?.explanation ? 0 : 1);
/** PRD 0016 RF4.1: an account whose query uses a web session shows the `UserSession` level and the core's warning.
 *  The integration's access level and summary stay out of the UI: they live in docs/providers.md. */
export const accessHtml = (r) => (r.access ? sessionNoticeHtml(r.access.session) : "");
/** PRD 0016 RF4.1: the `UserSession` level and the core's warning (card, details and the paste box). */
export const sessionNoticeHtml = (notice) => notice
  ? `<p class="access-line session-notice" data-session-notice><span class="lv">${esc(tl(notice.level))}</span> · <span>${esc(tl(notice.warning))}</span></p>`
  : "";
// ---------- PRD 0011 RF4: the provider's public status ----------
// Tone and icon per core state (never color alone: the text is the core's label).
const INCIDENT_LOOK = { degraded: ["warn", "warn"], partial_outage: ["bad", "alert"], major_outage: ["bad", "xcircle"],
  maintenance: ["info", "wrench"], operational: ["good", "check"], unknown: ["muted", "clock"] };
const isNotice = (s) => s.state !== "operational" && s.state !== "unknown";
/** What the card and tray flag: a current incident/maintenance, a failed
 * status read, or a retained reading that expired to unknown. */
export function incidentBadge(r) {
  const s = r.incident;
  if (!s || (!isNotice(s) && !s.stale && !s.expired)) return null;
  // Mid-line after " · ", so the lowercase form (PRD 0011 re-validation, V-04).
  const checked = s.read_at ? t("incident.checkedInline", { age: fmtAge(s.read_at) }) : "";
  if (s.expired) {
    return { tone: "muted", glyph: "clock", text: [tl(s.label), checked].filter(Boolean).join(" · "),
      url: s.page_url, outage: false };
  }
  const [tone, glyph] = isNotice(s) ? INCIDENT_LOOK[s.state] : ["muted", "warn"];
  const text = isNotice(s)
    ? [tl(s.label), s.incident?.title ?? "", s.stale ? t("incident.lastKnown") : "", s.stale ? checked : ""].filter(Boolean).join(" · ")
    : [t("incident.unavailable"), checked].filter(Boolean).join(" · ");
  return { tone, glyph, text, url: s.incident?.url ?? s.page_url, outage: s.state === "partial_outage" || s.state === "major_outage" };
}
/** PRD 0016 RF2.1: the "Open {site}" link of a session paste box; only the core descriptor's HTTPS `open_url`
 * (the shell's `open_external` opens no other address). */
export function sessionOpenLinkHtml(slot, region = null) {
  // PRD 0048 RF3.4: an account of another region opens that region's console.
  const url = slot?.region_open_urls?.find((r) => r.value === region)?.url ?? slot?.open_url;
  if (typeof url !== "string" || !url.startsWith("https://")) return "";
  let host;
  try { host = new URL(url).host; } catch { return ""; }
  return `<button type="button" class="link session-open" data-session-url="${esc(url)}">${icon("window")}<span>${te("dialog.session.open", { site: host })}</span></button>`;
}
/** PRD 0056 RF1.3: the shared sign-in (`"google"`) of a slot's capture for `region`, or null for a private window. */
export function captureSharedOf(slot, region = null) {
  const entry = slot?.region_open_urls?.find((r) => r.value === region);
  return (entry ? entry.capture_shared : slot?.capture_shared) ?? null;
}
/** PRD 0048 RF1 and 0056 RF6: what the capture window does, under "Sign in and capture session". A shared Google
 * window says it is kept hidden and reused, that passkeys do not work in it, and offers to forget it. */
export function sessionCaptureHintHtml(slot, region = null) {
  if (!slot?.capture) return "";
  const shared = captureSharedOf(slot, region) === "google";
  return `<div class="session-capture-hint" data-capture-hint="${esc(slot.slot)}">`
    + (shared
      ? `<small class="hint">${te("dialog.session.captureSharedHint")}</small> `
        + `<small class="hint">${te("dialog.session.capturePasskeyHint")}</small>`
        // PRD 0056 re-validation V-01/V-02: the design-system link button (icon + label) on its own row, not one more
        // line of the hints, with its own status line for the answer.
        + `<div class="session-capture-forget"><button type="button" class="link" data-capture-forget>${icon("trash")}`
        + `<span>${te("dialog.session.captureForget")}</span></button>`
        + `<small class="hint" data-capture-forget-status role="status" aria-live="polite"></small></div>`
      : `<small class="hint">${te("dialog.session.captureHint")}</small>`)
    + "</div>";
}
/** Owner request: "Create key on <host>" under a typed key, for the page the core declares (`SecretInput.key_page`);
 * the shell opens only those exact addresses (`open_external`). */
export function keyPageLinkHtml(input) {
  const url = input?.key_page;
  if (typeof url !== "string" || !url.startsWith("https://")) return "";
  let host;
  try { host = new URL(url).host; } catch { return ""; }
  return `<button type="button" class="link key-page" data-key-url="${esc(url)}">${icon("window")}<span>${te("dialog.secret.createKey", { site: host })}</span></button>`;
}
/** The official install page of the CLI a login mode runs, which the core declares only when that CLI is missing. */
export function installLinkHtml(mode) {
  const url = mode?.install?.url;
  if (typeof url !== "string" || !url.startsWith("https://")) return "";
  return `<button type="button" class="link key-page install-link" data-key-url="${esc(url)}">${icon("window")}<span>${tl(mode.install.label, {}, true)}</span></button>`;
}
/** "Open status page": the shell opens only the core's verified status hosts (`open_external`). */
const statusLinkHtml = (url) => `<button class="status-link" type="button" data-status-url="${esc(url)}">${icon("chev")}<span>${te("incident.open")}</span></button><span class="status-err" data-status-err role="alert"></span>`;
/** Card badge (color, icon and text) with the status page link; `link: false` inside a clickable row. */
export function incidentHtml(r, { link = true } = {}) {
  const b = incidentBadge(r);
  if (!b) return "";
  return `<div class="incident-line ${b.tone}" data-incident="${esc(r.incident.state)}" title="${esc(b.text)}">${icon(b.glyph)}<span class="incident-t">${esc(b.text)}</span>${link ? statusLinkHtml(b.url) : ""}</div>`;
}
/** Details: the provider's full status (operational and unknown too), its incident, when it was read and the link. */
export function incidentDetailHtml(r) {
  const s = r.incident;
  if (!s) return "";
  const [tone, glyph] = INCIDENT_LOOK[s.state] ?? INCIDENT_LOOK.unknown;
  const facts = [s.incident?.started_at ? t("incident.since", { time: fmtReset(s.incident.started_at) }) : "",
    // A failed query with no earlier reading has no "previous reading" to point at (V-01).
    s.read_at ? t("incident.checked", { age: fmtAge(s.read_at) }) : t(s.stale ? "incident.failedUnread" : "incident.notRead"),
    s.read_at && s.stale ? t("incident.stale") : ""].filter(Boolean);
  return `<div class="incident-detail" data-incident="${esc(s.state)}"><h4>${te("incident.title")}</h4>
    <p class="incident-line ${tone}">${icon(glyph)}<span class="incident-t"><b>${esc(tl(s.label))}</b>${s.incident?.title ? ` · ${esc(s.incident.title)}` : ""}</span></p>
    <p class="hint">${esc(facts.join(" · "))}</p><p class="incident-link">${statusLinkHtml(s.incident?.url ?? s.page_url)}</p></div>`;
}
/** Clicks on "Open status page" inside `root`; a refusal shows next to the link. */
export function bindStatusLinks(root) {
  root.addEventListener("click", (e) => {
    const link = e.target.closest?.("[data-status-url]");
    if (!link) return;
    const slot = link.parentElement.querySelector("[data-status-err]");
    slot.textContent = "";
    openExternal(link.dataset.statusUrl).catch(() => { slot.textContent = t("incident.openFailed"); });
  });
}

export const metricKey = (r, m) => `${reportKey(r)}::${m.label}`;

// ---------- providers ----------
// PRD 0001 re-validation (L3): the desktop keeps only each provider's color and mark; the visible name is the
// core's (`Provider::label`, translated through the core catalogs' `provider.<slug>`), the same one the terminal
// and the menu bar show.
const S = (color, mark) => ({ color, mark });
const brandStyles = {
  open_ai_api: S("#10a37f", "O"), anthropic_api: S("#d97757", "A"), chat_gpt: S("#10a37f", "C"),
  claude: S("#d97757", "✻"), gemini_api: S("#4285f4", "G"), vertex_ai: S("#4285f4", "V"),
  aws_bedrock: S("#ff9900", "B"), azure_open_ai: S("#0078d4", "Az"), cursor: S("#8b8b8b", "Cu"),
  copilot: S("#6e40c9", "Co"), windsurf: S("#09b6a2", "W"), mini_max: S("#e73b77", "M"),
  kimi: S("#1783ff", "K"), qwen: S("#615ced", "Q"), groq: S("#f55036", "g"),
  perplexity: S("#20b8cd", "P"), mistral: S("#fa520f", "Mi"), cloudflare_ai: S("#f6821f", "CF"),
  ollama_cloud: S("#9a9a9a", "Ol"), xiaomi_mimo: S("#ff6900", "Mi"), open_code_go: S("#3b82f6", "OC"),
  qoder: S("#22c55e", "Qo"), gemini_ai: S("#4285f4", "G"), grok: S("#9a9a9a", "X"),
  nvidia_nim: S("#76b900", "N"), poolside: S("#a855f7", "Po"), kiro: S("#8a5cf6", "Ki"),
  cline: S("#22d3ee", "Cl"), code_buddy: S("#6d5cf6", "CB"), zed: S("#3b82f6", "Z"),
  alibaba_model_studio: S("#ff6a00", "Al"), alibaba_coding_plan: S("#ff6a00", "Al"), alibaba_token_plan: S("#ff6a00", "Al"),
  alibaba_cloud: S("#ff6a00", "Al"), antigravity: S("#7c6cf0", "Ag"), kimchi: S("#ef4444", "Km"),
  meta: S("#0866ff", "Me"), open_router: S("#6467f2", "OR"), jetbrains_ai: S("#fc801d", "JB"),
  nine_router: S("#0369a1", "9r"), litellm: S("#14b8a6", "LL"), extension: S("#8b5cf6", "Ex"),
};
// The core's English provider names (`list_providers`), loaded once per window by `loadProviderLabels`.
const coreLabels = new Map();
/** Loads the core's provider names; `boot` awaits it before the first render. */
export async function loadProviderLabels() {
  try {
    for (const p of await listProviders()) coreLabels.set(p.id, p.label);
  } catch { /* the slug stays the fallback */ }
}
/** The provider's display name: the core catalog's `provider.<slug>` in the current language, else the core's English. */
export const providerName = (p) => tl({ id: `provider.${p}`, params: {}, fallback: coreLabels.get(p) ?? p });
export const brand = (p) => ({ ...(brandStyles[p] ?? S("#888888", String(p ?? "?")[0].toUpperCase())), label: providerName(p) });
/** PRD 0029 RF3.4 (V-02, V-03, V-07): the core's `display_title` rule: `[title, provider label shown beside it or null]`.
 * An account without a name of its own, or named exactly like its provider, is titled by its provider once, never "Groq · Groq". */
export function titleParts(provider, name) {
  const label = brand(provider).label;
  const own = String(name ?? "").trim();
  return !own || own === label ? [label, null] : [own, label];
}
/** The account's own name from the core's `named`/`display_name` (an unnamed extension account: its manifest name); "" when it has none. */
const ownName = (r) => r.named === false ? (r.provider === "extension" ? r.display_name ?? "" : "") : (r.display_name ?? r.account ?? "");
/** The account's display name: its own name, else its provider's. Every surface that names an account uses this or the two below. */
export const accountTitle = (r) => titleParts(r.provider, ownName(r))[0];
/** The account's own name, or "" for an account titled by its provider (a subtitle under the provider). */
export const accountName = (r) => (titleParts(r.provider, ownName(r))[1] ? titleParts(r.provider, ownName(r))[0] : "");
/** "Provider · name", or the provider alone for an account without a name of its own. */
export const accountFullLabel = (r, sep = " · ") => {
  const [title, provider] = titleParts(r.provider, ownName(r));
  return provider ? `${provider}${sep}${title}` : title;
};
// Provider logos: monochrome SVG glyphs (lobehub/icons, MIT; zed: simple-icons, CC0; see img/providers/NOTICE.md)
// painted by CSS mask over a tile in the brand color — sharp at any size and with no white background.
// [file, tile background, glyph color]; an omitted background uses the brand color and the glyph, white.
const glyphs = {
  open_ai_api: ["openai"], anthropic_api: ["anthropic"], chat_gpt: ["codex", "#111113"], claude: ["claude"],
  gemini_api: ["gemini"], vertex_ai: ["vertexai"], aws_bedrock: ["bedrock", "#232f3e", "#ff9900"], azure_open_ai: ["azure"],
  cursor: ["cursor", "#1c1c1e"], copilot: ["githubcopilot"], windsurf: ["windsurf"], mini_max: ["minimax"],
  kimi: ["kimi"], qwen: ["qwen"], groq: ["groq"], perplexity: ["perplexity"], mistral: ["mistral"],
  cloudflare_ai: ["cloudflare"], ollama_cloud: ["ollama", "#f4f4f5", "#111113"], xiaomi_mimo: ["xiaomimimo"],
  open_code_go: ["opencode", "#111113"], qoder: ["qoder", "#0c0c0e", "#22c55e"], gemini_ai: ["gemini"], grok: ["grok", "#111113"],
  nvidia_nim: ["nvidia", "#0c0c0e", "#76b900"], poolside: ["poolside"], kiro: ["kiro"], cline: ["cline", "#18181b"],
  code_buddy: ["codebuddy"], zed: ["zed", "#084ccf"], alibaba_model_studio: ["bailian"],
  alibaba_coding_plan: ["bailian"], alibaba_token_plan: ["bailian"], alibaba_cloud: ["alibabacloud"],
  antigravity: ["antigravity"],
};
/** Cuotai logotype (DS-ID-2): the four-arc mark is the "C" and "AI" is set apart in the logo's pink.
 *  Only the logo is styled "CuotAI"; assistive technology reads the product name "Cuotai".
 *  `attrs` is repeated on each part (e.g. ` data-tauri-drag-region`, which does not inherit). */
export const wordmark = (attrs = "") =>
  `<span class="wm-c" aria-hidden="true"${attrs}></span><span aria-hidden="true"${attrs}>uot<span class="wm-ai"${attrs}>AI</span></span><span class="wm-name">Cuotai</span>`;

export const logo = (provider, size = 40) => {
  const b = brand(provider);
  const style = `width:${size}px;height:${size}px;font-size:${size * 0.4}px`;
  if (provider === "kimchi") // own artwork, already colored and circular: no tile or mask
    return `<span class="logo art" style="${style}" aria-hidden="true"><img src="${uiPath("img/providers/kimchi.svg")}" alt="" draggable="false" /></span>`;
  const g = glyphs[provider];
  if (!g) return `<span class="logo fb" style="${style};--c:${b.color}" aria-hidden="true"><b>${esc(b.mark)}</b></span>`;
  const [file, bg = b.color, fg = "#fff"] = g;
  return `<span class="logo" style="${style};--c:${bg};--fg:${fg}" aria-hidden="true"><i class="glyph" style="-webkit-mask-image:url(${uiPath(`img/providers/${file}.svg`)});mask-image:url(${uiPath(`img/providers/${file}.svg`)})"></i></span>`;
};

// ---------- tema (compartilhado entre janelas via localStorage + evento storage) ----------
// Registry and mode rules in js/themes.js; tokens in css/themes/<id>.css.
const get = (k, d) => { try { return localStorage.getItem(k) || d; } catch { return d; } };
const put = (k, v) => { try { localStorage.setItem(k, v); } catch {} };
const systemDark = () => matchMedia("(prefers-color-scheme: dark)").matches;
export const theme = {
  mode: get("cuotai.themeMode", "system"),
  id: get("cuotai.theme", get("cuotai.darkTheme", DEFAULT_THEME)), // raw: user themes only exist after boot; darkTheme: old key
};
export function applyTheme() {
  const t = themeById(theme.id);
  const root = document.documentElement;
  root.dataset.theme = t.id;
  root.dataset.mode = effectiveMode(t, theme.mode, systemDark());
  reportCallbackStyle();
}
/** The browser page shown after a sign-in callback uses the active theme's roles and the interface language. */
function reportCallbackStyle() {
  const invoke = window.__TAURI__?.core?.invoke;
  if (!invoke) return;
  const defer = typeof requestAnimationFrame === "function" ? requestAnimationFrame : (f) => setTimeout(f, 0);
  defer(() => {
    try {
      const root = document.documentElement;
      const cs = getComputedStyle(root);
      const role = (n) => cs.getPropertyValue(n).trim();
      const colors = { bg: role("--bg"), surface: role("--surface"), border: role("--border"), fg: role("--fg"), fg_muted: role("--fg-muted"), bad: role("--bad") };
      invoke("set_callback_style", { locale: root.lang || "pt-BR", scheme: root.dataset.mode === "light" ? "light" : "dark", colors }).catch(() => {});
    } catch {}
  });
}
document.addEventListener?.("cuotai:locale", reportCallbackStyle);
applyTheme();
export function initTheme(onChange = () => {}) {
  matchMedia("(prefers-color-scheme: dark)").addEventListener("change", () => { applyTheme(); onChange(); });
  // RF7.1/RF7.2: another window changed a preference; `desktop.json` says what, no `storage` events.
  onPreferencesChanged((changed) => {
    if (!["theme", "theme_mode", "locale", "motion"].some((k) => changed.has(k))) return;
    applyTheme();
    onChange();
  });
}
const groupLabel = (g) => (GROUP_ORDER.includes(g) ? t(`theme.group.${g}`) : g);
/** Quick appearance (auto/light/dark). `themeSelect` adds the theme choice (settings screen only). */
export function themeSwitchHtml({ themeSelect = false, labels = false, selectOnly = false } = {}) {
  const th = themeById(theme.id);
  const modes = [["system", "monitor"], ["light", "sun"], ["dark", "moon"]];
  const groups = [...new Set(THEMES.map((x) => x.group))].sort((a, b) => {
    const ia = GROUP_ORDER.indexOf(a), ib = GROUP_ORDER.indexOf(b);
    return (ia < 0 ? 99 : ia) - (ib < 0 ? 99 : ib) || a.localeCompare(b, currentLocale());
  });
  const field = `<div class="theme-select theme-field">
      <button type="button" class="lang-field-btn theme-field-btn" data-theme-toggle aria-haspopup="menu" aria-expanded="${themeOpen}" aria-label="${te("theme.label")}: ${esc(th.label)}"><span class="swatch" aria-hidden="true"><i></i><i></i><i></i><i></i></span><span>${esc(th.label)}</span>${icon("chevDown")}</button>
      <div class="lang-menu theme-menu${menuPlaceClass(themeOpen)}" role="menu" aria-label="${te("theme.label")}" ${themeOpen ? menuPlaceStyle() : "hidden"}>${groups.map((g) => `<div class="theme-menu-group" role="presentation">${esc(groupLabel(g))}</div>${THEMES.filter((x) => x.group === g)
        .map((x) => `<button type="button" role="menuitemradio" aria-checked="${x.id === th.id}" class="${x.id === th.id ? "on" : ""}" data-theme-pick="${esc(x.id)}"><span>${esc(x.label)}</span>${x.id === th.id ? icon("tick", "sel-tick") : ""}</button>`).join("")}`).join("")}</div>
    </div>`;
  if (selectOnly) return field;
  return `<div class="theme-switch">
    <div class="segmented" role="radiogroup" aria-label="${te("theme.mode.label")}">${modes.map(([id, ic]) => {
      const off = id !== "system" && !th.modes.includes(id);
      const label = t(`theme.mode.${id}`) + (off ? ` (${t("theme.mode.unavailable")})` : "");
      return `<button role="radio" aria-checked="${theme.mode === id}" class="${theme.mode === id ? "on" : ""}" title="${esc(label)}" data-theme-mode="${id}" ${off ? "disabled" : ""}>${icon(ic)}${labels ? `<span>${esc(t(`theme.mode.${id}`))}</span>` : ""}</button>`;
    }).join("")}</div>
    ${themeSelect ? field : ""}
  </div>`;
}
let langOpen = false;
let themeOpen = false;
const flagOf = (l) => flagSvg(flagCountry(l.code, l.flag));
// PRD 0035 re-validation S-03: a user pack is never mistaken for a shipped language. Its item and the field carry a
// marker from a protected text ("user pack · fr.json"); the file name comes from the core already stripped of control
// and direction characters, is escaped here and isolated in <bdi>.
const packFile = (l) => (l.user && typeof l.file === "string" ? l.file : null);
const packMarkHtml = (l) => (packFile(l)
  ? `<small class="lang-pack">${te("userContent.packMarker", { file: "\u0000" }).replace("\u0000", `<bdi>${esc(packFile(l))}</bdi>`)}</small>` : "");
const packMarkText = (l) => (packFile(l) ? ` (${t("userContent.packMarker", { file: packFile(l) })})` : "");
/** Button with the current language's flag and a menu (flag + name). `inline` shows the open list (settings screen). */
export function langMenuHtml({ inline = false, field = false } = {}) {
  const cur = LOCALES.find((l) => l.code === currentLocale()) ?? LOCALES[0];
  const curName = esc(cur.name + packMarkText(cur));
  // PRD 0022 RF9.6 (DS-CMP-SEL-3): flag (decorative) + the endonym in its own language; the selected one is marked
  // by a check and weight, not by color alone.
  const role = inline ? "radio" : "menuitemradio";
  const items = LOCALES.map((l) => `<button role="${role}" aria-checked="${l.code === cur.code}" class="${l.code === cur.code ? "on" : ""}" data-locale="${esc(l.code)}"${inline ? ` tabindex="${l.code === cur.code ? 0 : -1}"` : ""}>${flagOf(l)}<span lang="${esc(l.code)}">${esc(l.name)}</span>${packMarkHtml(l)}${l.code === cur.code ? icon("tick", "sel-tick") : ""}</button>`).join("")
    // PRD 0035 RF1/RF3.3: registry languages without catalogs yet stay visible, in the fixed order, but cannot be chosen.
    + pendingLocales().map((l) => `<button type="button" role="${role}" aria-checked="false" aria-disabled="true" class="pending" data-locale-pending="${esc(l.code)}" tabindex="-1">${flagOf(l)}<span lang="${esc(l.code)}">${esc(l.name)}</span><small>${te("lang.inProgress")}</small></button>`).join("");
  if (inline) return `<div class="lang-list" role="radiogroup" aria-label="${te("lang.label")}">${items}</div>`;
  // DS-CMP-SEL-3 (mockup 50-settings-general-language): one field with the flag and the endonym, opening the list of
  // every language; it scales to the 14 languages of PRD 0035.
  if (field) return `<div class="lang lang-field">
    <button type="button" class="lang-field-btn" data-lang-toggle aria-haspopup="menu" aria-expanded="${langOpen}" aria-label="${te("lang.label")}: ${curName}">${flagOf(cur)}<span lang="${esc(cur.code)}">${esc(cur.name)}</span>${packMarkHtml(cur)}${icon("chevDown")}</button>
    <div class="lang-menu${menuPlaceClass(langOpen)}" role="menu" ${langOpen ? menuPlaceStyle() : "hidden"}>${items}</div>
  </div>`;
  return `<div class="lang">
    <button class="icon lang-btn" data-lang-toggle title="${te("lang.label")}: ${curName}" aria-label="${te("lang.label")}: ${curName}" aria-haspopup="menu" aria-expanded="${langOpen}">${flagOf(cur)}</button>
    <div class="lang-menu${menuPlaceClass(langOpen)}" role="menu" ${langOpen ? menuPlaceStyle() : "hidden"}>${items}</div>
  </div>`;
}

// ---------- menus: language, sort and theme (DS-CMP-SEL-2, PRD 0035 RF3.3) ----------
// One behavior for the three menu buttons: opening moves focus to the checked item (else the first enabled one) once
// the window has been drawn again; ArrowUp/ArrowDown move with wrap-around, Home/End go to the ends, typed letters
// jump to the next item whose label starts with them (folded, 600 ms buffer); Enter/Space choose; Esc and a click
// outside close and give focus back to the button. Pending items (`aria-disabled`) are skipped. On opening, the list
// opens upward when it does not fit below and there is more room above, and its height is clamped to the room left.
const MENU_TOGGLES = ["[data-lang-toggle]", "[data-sort-toggle]", "[data-theme-toggle]"];
const MENU_GAP = 6, MENU_MARGIN = 8, TYPEAHEAD_MS = 600;
let menuPlace = { up: false, max: 0 };
let typeahead = { text: "", at: 0 };
const anyMenuOpen = () => langOpen || sortOpen || themeOpen;
const menuPlaceClass = (open) => (open && menuPlace.up ? " up" : "");
const menuPlaceStyle = () => (menuPlace.max ? `style="max-height:${menuPlace.max}px"` : "");
const menuItems = (menu) => [...menu.querySelectorAll("[role=menuitemradio]")];
const menuEnabled = (b) => b.getAttribute("aria-disabled") !== "true" && !b.disabled;
const menuLabel = (b) => fold((b.querySelector("span")?.textContent ?? b.textContent).trim());
/** The menu button of an item (the list sits right after its button). */
const menuToggleOf = (el) => el.closest("[role=menu]")?.previousElementSibling ?? null;
const openMenuToggle = (root) => [...root.querySelectorAll(MENU_TOGGLES.join(", "))].find((b) => b.getAttribute("aria-expanded") === "true") ?? null;
/** Where a button sits among its kind, so the same button is found again after the window is drawn anew. */
function menuToggleRef(root, toggle) {
  const sel = toggle && MENU_TOGGLES.find((s) => toggle.matches(s));
  return sel ? { sel, n: [...root.querySelectorAll(sel)].indexOf(toggle) } : null;
}
const menuToggleAt = (root, ref) => (ref ? root.querySelectorAll(ref.sel)[Math.max(0, ref.n)] ?? null : null);

/** The part of the window the menu can use: the viewport cut by every scrolling or clipping ancestor. */
function menuBounds(el) {
  let top = 0, bottom = innerHeight;
  for (let p = el.parentElement; p && p !== document.documentElement; p = p.parentElement) {
    const cs = getComputedStyle(p);
    if (cs.overflowY === "visible" && cs.overflowX === "visible") continue;
    const r = p.getBoundingClientRect();
    top = Math.max(top, r.top); bottom = Math.min(bottom, r.bottom);
  }
  return { top, bottom };
}
/** Measures the open menu under its button and decides its direction and height (kept for the next draws). */
function placeMenu(toggle, menu) {
  menu.classList.remove("up");
  menu.style.maxHeight = "";
  const wanted = menu.getBoundingClientRect().height;
  const r = toggle.getBoundingClientRect(), b = menuBounds(toggle);
  const below = b.bottom - r.bottom - MENU_GAP - MENU_MARGIN, above = r.top - b.top - MENU_GAP - MENU_MARGIN;
  const up = wanted > below && above > below;
  const room = Math.floor(Math.max(0, up ? above : below));
  menuPlace = { up, max: wanted > room ? room : 0 };
  menu.classList.toggle("up", up);
  if (menuPlace.max) menu.style.maxHeight = `${menuPlace.max}px`;
}
function focusMenuItem(menu, item) {
  item?.focus({ preventScroll: true });
  item?.scrollIntoView?.({ block: "nearest" });
}
/** Draws the window again, then settles the menu: placed and focused when it opened, focus back on its button when it
 *  closed (`onlyIfLost`: a click outside keeps focus where that click put it). */
async function menuRerender(root, rerender, toggle, opened, { onlyIfLost = false } = {}) {
  const ref = menuToggleRef(root, toggle);
  typeahead = { text: "", at: 0 };
  if (opened) menuPlace = { up: false, max: 0 };
  await rerender();
  const tg = menuToggleAt(root, ref);
  if (!tg) return;
  if (opened) {
    const menu = tg.nextElementSibling;
    if (!menu || menu.hidden) return;
    tg.scrollIntoView?.({ block: "nearest" }); // measured from where the button is seen
    placeMenu(tg, menu);
    const items = menuItems(menu).filter(menuEnabled);
    focusMenuItem(menu, items.find((b) => b.getAttribute("aria-checked") === "true") ?? items[0]);
    return;
  }
  menuPlace = { up: false, max: 0 };
  const lost = !document.activeElement || document.activeElement === document.body || !document.activeElement.isConnected;
  if (!onlyIfLost || lost) tg.focus({ preventScroll: true });
}
/** Keys inside an open menu (and ArrowDown/ArrowUp on its button, which open it). */
function menuKeydown(e) {
  if (e.altKey || e.ctrlKey || e.metaKey) return;
  const toggle = e.target.closest?.(MENU_TOGGLES.join(", "));
  if (toggle && (e.key === "ArrowDown" || e.key === "ArrowUp") && toggle.getAttribute("aria-expanded") !== "true") {
    e.preventDefault();
    toggle.click();
    return;
  }
  const item = e.target.closest?.("[role=menu] [role=menuitemradio]");
  const menu = item?.closest("[role=menu]");
  if (!menu || menu.hidden) return;
  const items = menuItems(menu).filter(menuEnabled);
  if (!items.length) return;
  const at = items.indexOf(item);
  let next = null;
  if (e.key === "ArrowDown") next = items[(at + 1) % items.length];
  else if (e.key === "ArrowUp") next = items[(at - 1 + items.length) % items.length];
  else if (e.key === "Home") next = items[0];
  else if (e.key === "End") next = items[items.length - 1];
  else if (e.key === " " && !(Date.now() - typeahead.at < TYPEAHEAD_MS && typeahead.text)) return; // Space chooses, like Enter (the button's own activation)
  else if (e.key.length === 1) {
    const now = Date.now();
    const text = (now - typeahead.at < TYPEAHEAD_MS ? typeahead.text : "") + fold(e.key);
    typeahead = { text, at: now };
    // A new search starts after the current item; a longer one may still match it.
    const from = text.length === 1 ? at + 1 : Math.max(at, 0);
    for (let k = 0; k < items.length; k++) {
      const cand = items[(from + k) % items.length];
      if (menuLabel(cand).startsWith(text)) { next = cand; break; }
    }
    e.preventDefault();
    if (!next) return;
  } else return;
  e.preventDefault();
  focusMenuItem(menu, next);
}

/** Selector event delegation; `rerender` is called after the switch (theme and language with cross-fade, RF3.3). */
export function bindThemeSwitch(root, rerender) {
  root.addEventListener("click", (e) => {
    const b = e.target.closest("[data-theme-mode]");
    if (!b || b.disabled) return;
    theme.mode = b.dataset.themeMode; setPreference("theme_mode", theme.mode).catch(() => {}); crossFade(() => { applyTheme(); return rerender(); });
  });
  root.addEventListener("change", async (e) => {
    if (e.target.matches("[data-theme-id]")) {
      theme.id = themeById(e.target.value).id; setPreference("theme", theme.id).catch(() => {}); crossFade(() => { applyTheme(); return rerender(); });
    }
  });
  root.addEventListener("click", async (e) => {
    const toggle = e.target.closest("[data-sort-toggle], [data-lang-toggle], [data-theme-toggle]");
    if (toggle) {
      const kind = toggle.matches("[data-sort-toggle]") ? "sort" : toggle.matches("[data-lang-toggle]") ? "lang" : "theme";
      const open = !{ sort: sortOpen, lang: langOpen, theme: themeOpen }[kind];
      sortOpen = open && kind === "sort"; langOpen = open && kind === "lang"; themeOpen = open && kind === "theme";
      return menuRerender(root, rerender, toggle, open);
    }
    const sortItem = e.target.closest("[data-sort]");
    if (sortItem) { const tg = menuToggleOf(sortItem); setSort(sortItem.dataset.sort); sortOpen = false; return menuRerender(root, rerender, tg, false); }
    const pick = e.target.closest("[data-theme-pick]");
    if (pick) {
      const tg = menuToggleOf(pick);
      themeOpen = false; theme.id = themeById(pick.dataset.themePick).id; setPreference("theme", theme.id).catch(() => {});
      return menuRerender(root, () => crossFade(() => { applyTheme(); return rerender(); }), tg, false);
    }
    const item = e.target.closest("[data-locale]");
    if (item) { // loads first: the transition does not freeze the screen
      const tg = menuToggleOf(item);
      langOpen = false; await setLocale(item.dataset.locale);
      return menuRerender(root, () => crossFade(rerender), tg, false);
    }
    if (anyMenuOpen() && !e.target.closest(".lang, .theme-field")) {
      const tg = openMenuToggle(root);
      langOpen = false; sortOpen = false; themeOpen = false;
      return menuRerender(root, rerender, tg, false, { onlyIfLost: true });
    }
  });
  addEventListener("keydown", (e) => {
    if (e.key !== "Escape" || !anyMenuOpen() || document.querySelector("dialog[open]")) return;
    // Taken here, so the Settings window's own Esc (close the window) does not also run.
    e.preventDefault();
    const tg = openMenuToggle(root);
    langOpen = false; sortOpen = false; themeOpen = false;
    menuRerender(root, rerender, tg, false);
  });
  root.addEventListener("keydown", (e) => menuKeydown(e));
  // DS-A11Y-4: the inline language list is one radio group; arrow keys move between languages.
  root.addEventListener("keydown", (e) => {
    const item = e.target.closest?.(".lang-list [role=radio]");
    const step = { ArrowDown: 1, ArrowRight: 1, ArrowUp: -1, ArrowLeft: -1 }[e.key];
    if (!item || !step) return;
    e.preventDefault();
    const list = [...item.parentElement.querySelectorAll("[role=radio]:not([aria-disabled=true])")];
    list[(list.indexOf(item) + step + list.length) % list.length]?.focus();
  });
}

// ---------- conjuntos persistentes (contas desativadas, quotas ocultas) ----------
export function persistentSet(key, onExternal = () => {}) {
  let set;
  try { set = new Set(JSON.parse(localStorage.getItem(key) || "[]")); } catch { set = new Set(); }
  addEventListener("storage", (e) => {
    if (e.key !== key) return;
    try { set.clear(); JSON.parse(e.newValue || "[]").forEach((x) => set.add(x)); } catch {}
    onExternal();
  });
  return {
    has: (x) => set.has(x),
    toggle(x) { set.has(x) ? set.delete(x) : set.add(x); put(key, JSON.stringify([...set])); },
  };
}

// Quotas hidden by the user (eye on the details screen): shared between windows and outside the summary/order.
const hiddenListeners = new Set();
export const hiddenMetrics = persistentSet("cuotai.hiddenMetrics", () => hiddenListeners.forEach((f) => f()));
export const onHiddenChange = (f) => hiddenListeners.add(f);
export const isMetricHidden = (r, m) => hiddenMetrics.has(metricKey(r, m));

// Accounts turned off in the panel (card switch). PRD 0025 re-validation V-08/S-01: the set lives in `desktop.json`
// (`set_hidden_accounts`, validated in Rust), keyed by the stable account id (a 9router connection by its own key), so
// every window and the menu bar leave them out, and a rename keeps an account hidden.
export const hideKey = (r) => (r.connection ? r.account_key : r.account_id ?? reportKey(r));
const LEGACY_DISABLED = "cuotai.disabledAccounts";
const disabledListeners = new Set();
let hiddenKeys = new Set();
const followHidden = (keys) => {
  const next = new Set(Array.isArray(keys) ? keys : []);
  if (next.size === hiddenKeys.size && [...next].every((k) => hiddenKeys.has(k))) return;
  hiddenKeys = next;
  disabledListeners.forEach((f) => f());
};
export const setHiddenAccounts = (keys) => call("set_hidden_accounts", { keys });
export const disabledAccounts = {
  has: (r) => hiddenKeys.has(hideKey(r)),
  toggle(r) {
    const next = new Set(hiddenKeys);
    const k = hideKey(r);
    if (next.has(k)) next.delete(k); else next.add(k);
    followHidden([...next]);
    return setHiddenAccounts([...next]).catch(() => getSettings().then((s) => followHidden(s.hidden_accounts), () => {}));
  },
};
export const onDisabledChange = (f) => disabledListeners.add(f);
/** PRD 0025 RF2.3: the menu-bar accounts (`keys`, account ids) first, in the menu bar's order; the others apart.
 *  Everything in the bar (or nothing matching) keeps one list. */
export function splitByMenuBarKeys(reports, keys) {
  const inBar = (keys ?? []).map((k) => reports.find((r) => r.account_id === k)).filter(Boolean);
  if (!inBar.length || inBar.length === reports.length) return { inBar: reports, rest: [] };
  return { inBar, rest: reports.filter((r) => !inBar.includes(r)) };
}
/** Reads the stored set and follows later changes from any window. */
export async function initHiddenAccounts() {
  try { followHidden((await getSettings()).hidden_accounts); } catch {}
  onSettingsChanged((key) => { if (key === "hidden_accounts" || !key) getSettings().then((s) => followHidden(s.hidden_accounts), () => {}); });
}
/** The one-time move of the old web-storage set (`provider::name`, or a connection's key) to `desktop.json`, mapped to
 *  ids through the current `reports`; entries that match no account are dropped. Runs once the reports are known. */
export async function migrateHiddenAccounts(reports) {
  let legacy = null;
  try { legacy = localStorage.getItem(LEGACY_DISABLED); } catch {}
  if (legacy == null || !reports?.length) return;
  let old = [];
  try { old = JSON.parse(legacy) ?? []; } catch {}
  const moved = (Array.isArray(old) ? old : []).map((k) => reports.find((r) => reportKey(r) === k)).filter(Boolean).map(hideKey);
  try {
    const s = await getSettings();
    const keys = [...new Set([...(s.hidden_accounts ?? []), ...moved])];
    if (moved.length) await setHiddenAccounts(keys);
    followHidden(keys);
    localStorage.removeItem(LEGACY_DISABLED);
  } catch {}
}

// ---------- data state ----------
// The backend is the only source: per-account schedule (RefreshCycle) and emits "snapshot".
export function createStore(render) {
  const s = { view: null, error: null };
  const set = (v) => { s.view = v; s.error = v?.error ?? null; render(); };
  s.refresh = () => refreshAll().catch((e) => { s.error = String(e); render(); });
  // The browser demo has no "snapshot" event: re-read the view after a refresh so its recovery shows.
  // PRD 0033 re-validation V-06: resolves with the core's note when it refused a billed query (else null).
  s.refreshOne = (i) => refreshAccount(i).then(async (note) => { if (!isDesktop) await getView().then(set); return note ?? null; })
    .catch((e) => { s.error = String(e); render(); return null; });
  /** Re-read the view now: the browser demo's stand-in for the "snapshot" event after a local change. */
  s.reload = () => getView().then(set).catch((e) => { s.error = String(e); render(); });
  if (isDesktop) T.event.listen("snapshot", (e) => set(e.payload));
  queueMicrotask(() => getView().then(set).catch((e) => { s.error = String(e); render(); }));
  // Countdowns and the age text (PRD 0010 RF3.1): the view is re-read so the core's freshness, computed when the
  // view is built, follows the clock. No query: only the backend's in-memory state.
  setInterval(() => getView().then(set, render), 30000);
  return s;
}

// ---------- demo (browser only; same shape as the Tauri commands) ----------
const h = (n) => new Date(Date.now() + n * 3600e3).toISOString();
// PRD 0034 re-validation (G9): the demo's metrics carry what the backend sends: the adapters' own raw label and unit
// (Portuguese ids, as `Sessão (5 horas) (%)` and `solicitações`) beside their `label_text`/`unit_text`, so a screen
// that shows a raw id reads Portuguese in every other language (locale-smoke and the en-XA scan then catch it).
const demoText = (id, fallback, params = {}) => ({ id, params, fallback });
const demoPercentOf = (id, english) => demoText("metric.label.percent_of", `${english} (%)`, { label: demoText(id, english) });
const DEMO_RAW_LABELS = {
  "session (5h)": ["Sessão (5 horas) (%)", demoPercentOf("metric.label.session_5h", "Session (5 hours)")],
  "weekly (7d)": ["Semana (7 dias) (%)", demoPercentOf("metric.label.week_7d", "Week (7 days)")],
  credit: ["Crédito", demoText("metric.label.credit", "Credit")],
  "budget (30d)": ["Orçamento do provedor (30d)", demoText("metric.label.provider_budget_period", "Provider budget (30d)", { period: "30d" })],
};
const DEMO_RAW_UNITS = {
  requests: ["solicitações", demoText("metric.unit.requests", "requests")],
  credits: ["créditos", demoText("metric.unit.credits", "credits")],
};
/** The raw `label`/`unit` and their display texts for a demo metric named `label` in `unit` (English demo names). */
const demoTexts = (label, unit) => {
  const [rawLabel, labelText] = DEMO_RAW_LABELS[label] ?? [label, demoText("metric.verbatim", label)];
  const [rawUnit, unitText] = DEMO_RAW_UNITS[unit] ?? [unit, demoText("metric.verbatim", unit)];
  return { label: rawLabel, label_text: labelText, unit: rawUnit, unit_text: unitText };
};
const dm = (label, used, limit, resetH, kind, window, unit = "requests") => {
  const unmeasured = used === 0 && limit === 0;
  const exhausted = !unmeasured && limit > 0 && used >= limit;
  return { ...demoTexts(label, unit), used, limit, remaining: limit - used, kind, window, period: null, resets_at: h(resetH),
    state: unmeasured ? "unmeasured" : exhausted ? "exhausted" : "available",
    ratio: unmeasured || exhausted ? (exhausted ? 0 : null) : (limit - used) / limit, uncapped: false, no_cap: false };
};
const ago = (min) => new Date(Date.now() - min * 60e3).toISOString();
const dr = (account, provider, metrics, notes = [], status = "ready") => ({
  account, provider, checked_at: ago(3), status, metrics, notes, source: "demo", freshness: "fresh", clock_adjusted: false, last_good: null,
  class: "available", refresh_seconds: 300, uses_oauth: false, identity: { email: null, badges: [], copy: null }, model_count: null });
// Demo notes are catalog texts (`demo.note.*`), so every language renders its own.
const demoNote = (id, fallback) => ({ id: `demo.note.${id}`, params: {}, fallback });
const drn = (account, provider, metrics, note, status) => ({ ...dr(account, provider, metrics, [note.fallback], status), notes_l10n: [note] });
// PRD 0034 re-validation (V-24): a note the real adapter writes, with its core id, so the demo shows it translated.
const coreNote = (id, fallback, params = {}) => ({ id, params, fallback });
const coreNotes = (notes) => ({ notes: notes.map((n) => n.fallback), notes_l10n: notes });
// The core's amount (PRD 0023 re-validation V-02): machine params, formatted by `tl` in the UI locale.
// Its English fallback is the en-US currency format, like the core's.
const demoMoney = (value, currency = "USD") => ({ id: "amount.money", params: { value: String(value), currency }, fallback: new Intl.NumberFormat("en-US", { style: "currency", currency }).format(value) });
const demoReports = [
  dr("alex@example.com", "antigravity", [dm("Gemini (5h)", 15, 1000, 0.37, "quota", "window_5h"), dm("Claude & GPT (5h)", 0, 1000, 5, "quota", "window_5h"), dm("Gemini (Weekly)", 120, 1000, 151, "quota", "weekly"), dm("Claude & GPT (Weekly)", 0, 1000, 151.9, "quota", "weekly")]),
  // PRD 0023: the exhausted week is also paying for extra usage ("Limit hit · on extra $6.25").
  { ...dr("Account 2", "claude", [dm("session (5h)", 0, 100, 4.5, "quota", "window_5h", "%"), dm("weekly (7d)", 100, 100, 24.6, "quota", "weekly", "%"),
      { label: "Extra usage", used: 6.25, limit: 50, remaining: null, unit: "USD", kind: "spend", window: "monthly", period: null, resets_at: h(300), billing: "overage",
        state: "available", ratio: null, uncapped: true, no_cap: false }]),
    class: "exhausted", model_count: 1, plan: "Max",
    overage: { state: "active", spent: 6.25, unit: "USD", currency: "USD", cap: 50, cycle_ends_at: h(300),
      text: { id: "overage.on_extra", params: { amount: demoMoney(6.25) }, fallback: "On extra · $6.25" },
      summary: { id: "overage.limit_hit_on_extra", params: { amount: demoMoney(6.25) }, fallback: "Limit hit · on extra $6.25" }, caveat: null } },
  // PRD 0010 RF3 fixtures: an aging reading (10 min against 300 s) and a failure that kept its last valid reading.
  { ...dr("alex@example.com", "chat_gpt", [dm("5h", 6, 100, 67.6, "quota", "window_5h", "%")]), checked_at: ago(10), freshness: "aging", plan: "Plus" },
  { ...dr("octo-dev", "copilot", [dm("chat", 0, 0, 2.6, "quota", "other"), dm("Premium requests", 7026, 7000, 2.6, "quota", "other")]), class: "exhausted" },
  { ...dr("Personal", "groq", []), model_count: 4,
    ...coreNotes([coreNote("note.router_keys.groq_rate_limit_headers_missing", "Groq response without `x-ratelimit-*` headers in this request; the limit window is unknown.")]) },
  // PRD 0023 RF10.3 demo: the video model is at its limit (the core's `scope_blocks`); the account keeps its quota.
  { ...dr("Personal", "mini_max", [dm("M-series (5h)", 6, 100, 2.6, "quota", "window_5h"), dm("M-series (7d)", 60, 100, 98.6, "quota", "weekly"), dm("Video (5h)", 0, 100, 2.6, "quota", "window_5h")]),
    scope_blocks: [{ index: 2, label: "Video", resets_at: h(2.6), text: { id: "metrics.scope_blocked", params: { model: "Video", at: new Date(h(2.6)).toTimeString().slice(0, 5) }, fallback: `Video at its limit until ${new Date(h(2.6)).toTimeString().slice(0, 5)}` } }] },
  dr("Account 2", "code_buddy", [dm("Monthly", 0, 100, 757, "quota", "monthly"), dm("Bonus Pack 1", 0, 250, 62, "quota", null)]),
  // The exhausted week is the binding limit, so the core classifies the account as exhausted.
  { ...dr("Account 1", "kimi", [dm("Weekly", 100, 100, 109, "quota", "weekly"), dm("Ratelimit", 0, 100, 4.8, "rate_limit", "other")]), class: "exhausted" },
  dr("Personal", "ollama_cloud", [dm("Monthly", 0, 100, 248, "quota", "monthly")]),
  drn("Personal", "anthropic_api", [], demoNote("auth", "Credential missing or expired."), "authentication_error"),
  drn("Personal", "mistral", [], demoNote("rate", "HTTP 429: rate limit reached."), "rate_limited"),
  { ...drn("Dev", "cursor", [], demoNote("down", "Service unavailable right now."), "error"), checked_at: ago(1),
    last_good: { checked_at: ago(42), metrics: [dm("Fast requests", 380, 500, 120, "quota", "monthly")] } },
  drn("Work", "windsurf", [], demoNote("config", "Add the API key to this account to finish setting it up."), "needs_configuration"),
  { ...dr("Account 3", "kiro", [dm("credit", 596.15, 10000, 360, "quota", "other", "credits")]),
    identity: { email: null, copy: { label: { id: "identity.profile_arn", params: {}, fallback: "Profile ARN" }, value: "arn:aws:codewhisperer:us-east-1:123456789012:profile/EXAMPLEPROFILE" },
      badges: [{ tone: "accent", text: "IAM Identity Center" }, { tone: "info", text: "us-east-1" }, { tone: "good", text: { id: "demo.kiro.badge.active", params: {}, fallback: "active" } }] } },
  { ...dr("alex@example.com", "grok", [dm("On-demand", 1, 1, 100, "quota", "monthly")]), class: "exhausted", identity: { email: "alex@example.com", badges: [], copy: null } },
  dr("Personal", "deepseek_demo", [{ label: "Balance", used: null, limit: null, remaining: 0, unit: "USD", kind: "balance", window: null, period: null, resets_at: null, state: "available", ratio: null, uncapped: true, no_cap: false }]),
  // PRD 0005 RF4.2 fixtures: an Unavailable provider (no query) and an Undocumented one whose response broke the schema.
  drn("Work", "poolside", [], demoNote("poolside", "Poolside publishes no usage or balance endpoint per key."), "unsupported"),
  // PRD 0028 RF1.3 fixture (DS-CMP-DIAG-1): the response changed, so the details show "What changed".
  { ...dr("Personal", "zed", [], ["Response is incompatible with the query interface; no value was estimated.", "The provider changed models[].id (string → number)."], "error"),
    notes_l10n: [{ id: "query.schema", params: {}, fallback: "Response is incompatible with the query interface; no value was estimated." },
      { id: "diagnostic.schema_drift", params: { path: "models[].id", expected: "string", found: "number" }, fallback: "The provider changed models[].id (string → number)." }],
    drift: {
      heading: { id: "diagnostic.what_changed", params: {}, fallback: "What changed" },
      sentence: { id: "diagnostic.schema_drift", params: { path: "models[].id", expected: "string", found: "number" }, fallback: "The provider changed models[].id (string → number)." },
      paths: [{ path: "models[].id", expected: "string", found: "number" }, { path: "models[].display_name", expected: "string", found: "missing" }],
      privacy: { id: "diagnostic.values_never_recorded", params: {}, fallback: "Values are never recorded." },
    } },
  // PRD 0010 RF2.3 fixture: just opened, no answer yet; the reading recorded 3 h ago shows as stale.
  { ...dr("Restored", "chat_gpt", []), pending: true, class: "unmeasured", checked_at: ago(180),
    last_good: { checked_at: ago(180), metrics: [dm("5h", 30, 100, 2, "quota", "window_5h", "%")] } },
  // PRD 0016 RF1.2 / PRD 0047 G5 fixture: a session set up in the terminal, offered for the vault;
  // RF3.2: its pasted session expires in 2 days (the card warns).
  { ...dr("Personal", "xiaomi_mimo", [dm("Weekly", 6, 100, 96, "quota", "weekly", "%")]),
    session_expiry: { expires_at: new Date(Date.now() + 2.5 * 86400000).toISOString(), days: 2, soon: true,
      label: { id: "user_session.expiry.days", params: { days: 2 }, fallback: "Session expires in 2 days." } } },
  // DS-CMP-ROW-8 demo: per-model daily quotas (the tray names the two tightest models) and a day + month pair whose
  // month limits the account (its thin bar warns while the day is free).
  // The Gemini row reads the CLI login (`product = "cli"`), the mode the core classes as Undocumented.
  { ...dr("Personal", "gemini_ai", [{ ...dm("Gemini 2.5 Flash", 35, 100, 9, "quota", "daily"), scope: { kind: "model", id: "gemini-2.5-flash", label: "Gemini 2.5 Flash" } },
    { ...dm("Gemini 2.5 Pro", 82, 100, 9, "quota", "daily"), scope: { kind: "model", id: "gemini-2.5-pro", label: "Gemini 2.5 Pro" } }]), demo_level: "undocumented" },
  dr("Work", "alibaba_coding_plan", [dm("Weekly", 800, 2000, 9, "quota", "weekly"), dm("Monthly", 21000, 30000, 300, "quota", "monthly")]),
];
// PRD 0036 RF8 demo: a local 9router instance with four connections (two Claude, one of them exhausted; a Codex one
// off in 9router; an unmapped GLM one whose query failed). `?aggregator=off` leaves it out.
// PRD 0036 RF8.5 demo: the `hidden` and `show_both` lists of the 9router account (sub ids).
const demoConnFlags = { hidden: new Set(), show_both: new Set() };
const demoConnection = (id, label, upstream, mapped, metrics, extra = {}, instance = "home", parent = "nine_router") => ({
  ...dr(`${instance}/${id}`, mapped ?? parent, metrics), account: label, demo_id: id,
  connection: { label, upstream, mapped, enabled_upstream: true, duplicate_of: null, plan: null,
    via: parent === "litellm" ? "docs.litellm.ai/docs/proxy/provider_budget_routing" : `open-sse/services/usage/${upstream}.js`, sub: id, show_both: false }, ...extra,
});
if (new URLSearchParams(location.search).get("aggregator") !== "off") {
  const conns = [
    demoConnection("a1b2c3", "work", "claude", "claude", [dm("session (5h)", 22, 100, 3, "quota", "window_5h", "%"), dm("weekly (7d)", 40, 100, 80, "quota", "weekly", "%")],
      { plan: "Max", connection: { label: "work", upstream: "claude", mapped: "claude", enabled_upstream: true, duplicate_of: new URLSearchParams(location.search).has("dup") ? "claude:Account 2" : null, plan: "Max", via: "open-sse/services/usage/claude.js", sub: "a1b2c3", show_both: false } }),
    { ...demoConnection("d4e5f6", "personal", "claude", "claude", [dm("session (5h)", 100, 100, 2, "quota", "window_5h", "%")]), class: "exhausted" },
    demoConnection("0718aa", "team", "codex", "chat_gpt", [], { status: "ready", ...coreNotes([coreNote("note.nine_router.off", "Off in 9router.")]),
      connection: { label: "team", upstream: "codex", mapped: "chat_gpt", enabled_upstream: false, duplicate_of: null, plan: null, via: null, sub: "0718aa", show_both: false } }),
    { ...demoConnection("9bc0de", "zhipu", "glm", null, [], { status: "error", ...coreNotes([coreNote("note.nine_router.reconnect", "Reconnect this account in 9router.")]), class: "unmeasured" }) },
  ];
  demoReports.push({ ...dr("home", "nine_router", []),
    ...coreNotes([coreNote("note.nine_router.summary", "2 of 4 9router connections with usage.", { ready: "2", count: "4" }),
      coreNote("note.nine_router.version", "9router 0.5.109.", { version: "0.5.109" })]),
    class: "available", connections: conns,
    // PRD 0036 RF2.3/RF2.4: `?aggwarn` shows both standing warnings (the demo instance itself is loopback).
    ...(new URLSearchParams(location.search).has("aggwarn") ? { connection_warnings: { exposed: true, private_http: true } } : {}),
    aggregator: { kind: "9router", version: "0.5.109", host: "127.0.0.1:20128", auth: "password", tls: "off", available: 1, total: 3, connections: conns.length,
      pools: [
        { upstream: "claude", provider: "claude", available: 1, exhausted: 1, total: 2, returns_at: null,
          summary: { id: "pool.available", params: { upstream: "Claude", available: "1", total: "2" }, fallback: "Claude · 1 of 2 available" } },
        { upstream: "glm", provider: null, available: 0, exhausted: 0, total: 1, returns_at: null,
          summary: { id: "pool.unmeasured", params: { upstream: "glm", count: "1" }, fallback: "glm · 1 connection with unknown usage" } },
      ],
      alert_each: false, dashboard: true } });
}
// PRD 0036 re-validation (V-12): `?agg=0,1,12,40,exhausted,litellm` adds more aggregator instances after "home", each
// shaped like the backend's view: a 9router with no connection, with one, with 12 (one of them a duplicate of the
// direct "Account 2"), with 40 (long labels), one whose only pool is exhausted, and a LiteLLM proxy with per-provider
// budgets. Pools and their sentences follow the core's rule (`aggregators::pools`, `Pool::summary`).
// The core's names of the providers used below (`DEMO_CORE_LABELS` is declared later in the module).
const DEMO_AGG_NAMES = { claude: "Claude", chat_gpt: "ChatGPT / Codex", copilot: "GitHub Copilot", gemini_ai: "Gemini CLI", kimi: "Kimi",
  open_ai_api: "OpenAI API", anthropic_api: "Anthropic API", azure_open_ai: "Azure OpenAI", nine_router: "9router", litellm: "LiteLLM proxy" };
const DEMO_UPSTREAMS = [["claude", "claude"], ["codex", "chat_gpt"], ["github", "copilot"], ["gemini-cli", "gemini_ai"], ["kimi", "kimi"], ["glm", null]];
const demoPools = (conns) => {
  const pools = [];
  for (const c of conns) {
    const k = c.connection;
    if (k.enabled_upstream === false || k.duplicate_of) continue;
    let p = pools.find((x) => x.upstream === k.upstream);
    if (!p) pools.push(p = { upstream: k.upstream, provider: k.mapped, available: 0, exhausted: 0, total: 0, returns_at: null });
    p.total += 1;
    if (c.class === "exhausted") p.exhausted += 1; else if (c.class !== "unmeasured" && c.metrics.length) p.available += 1;
  }
  return pools.map((p) => {
    const upstream = p.provider ? DEMO_AGG_NAMES[p.provider] : p.upstream;
    const summary = p.total && p.exhausted === p.total ? coreNote("pool.exhausted", `${upstream} · all exhausted`, { upstream })
      : !p.available ? coreNote("pool.unmeasured", `${upstream} · ${p.total} connections with unknown usage`, { upstream, count: String(p.total) })
        : coreNote("pool.available", `${upstream} · ${p.available} of ${p.total} available`, { upstream, available: String(p.available), total: String(p.total) });
    return { ...p, summary };
  });
};
const demoInstance = (provider, name, conns, { version = null, host, notes = [], dashboard = true, auth = provider === "litellm" ? "key" : "password", tls = "off" } = {}) => {
  const pools = demoPools(conns);
  return { ...dr(name, provider, []), ...coreNotes(notes), class: pools.some((p) => p.available) ? "available" : conns.length ? "exhausted" : "unmeasured", connections: conns,
    aggregator: { kind: DEMO_AGG_NAMES[provider], version, host, auth, tls, available: pools.reduce((n, p) => n + p.available, 0), total: pools.reduce((n, p) => n + p.total, 0),
      connections: conns.length, pools, alert_each: false, dashboard } };
};
/** One 9router connection of instance `name` (the `n`-th): an upstream from the list above and a 5-hour window. */
const demoRouterConn = (name, n, label, used, extra = {}) => {
  const [upstream, mapped] = DEMO_UPSTREAMS[n % DEMO_UPSTREAMS.length];
  const id = `${name.slice(0, 3)}${String(n).padStart(3, "0")}`;
  const metrics = mapped ? [dm("session (5h)", used, 100, 1 + (n % 5), "quota", "window_5h", "%")] : [];
  return { ...demoConnection(id, label, upstream, mapped, metrics, {}, name), ...(used >= 100 ? { class: "exhausted" } : mapped ? {} : { class: "unmeasured" }), ...extra };
};
const demoRouterNotes = (conns, version) => [
  coreNote("note.nine_router.summary", `${conns.filter((c) => c.metrics.length).length} of ${conns.length} 9router connections with usage.`,
    { ready: String(conns.filter((c) => c.metrics.length).length), count: String(conns.length) }),
  coreNote("note.nine_router.version", `9router ${version}.`, { version }),
];
for (const size of (new URLSearchParams(location.search).get("agg") ?? "").split(",").filter(Boolean)) {
  if (size === "litellm") {
    const budget = (used, limit) => [dm("budget (30d)", used, limit, 240, "spend", "monthly", "USD")];
    const conns = [["openai", "open_ai_api", 42.5, 100], ["anthropic", "anthropic_api", 180, 200], ["azure", "azure_open_ai", 12, 50], ["deepinfra", null, 3.2, 20]]
      .map(([upstream, mapped, used, limit], n) => demoConnection(`lit${n}`, upstream, upstream, mapped, budget(used, limit), {}, "gateway", "litellm"));
    demoReports.push(demoInstance("litellm", "gateway", conns, { host: "127.0.0.1:4000",
      notes: [coreNote("note.litellm.summary", `${conns.length} providers with a budget in LiteLLM.`, { count: String(conns.length) })] }));
    continue;
  }
  if (size === "exhausted") {
    const conns = ["day", "night", "weekend"].map((label, n) => ({ ...demoRouterConn("night", n * DEMO_UPSTREAMS.length, label, 100) }));
    demoReports.push(demoInstance("nine_router", "night", conns, { version: "0.5.109", host: "127.0.0.1:20129", notes: demoRouterNotes(conns, "0.5.109") }));
    continue;
  }
  const count = Number(size);
  if (!Number.isInteger(count) || count < 0) continue;
  const name = { 0: "empty", 1: "solo", 12: "lab" }[count] ?? `fleet${count}`;
  const conns = Array.from({ length: count }, (_, n) => demoRouterConn(name, n,
    count >= 40 && n % 7 === 3 ? `Very long connection label for the shared subscription number ${n}` : `conn ${n + 1}`, (n * 17) % 101));
  // The 12-connection instance has a duplicate of the direct "Account 2" and a connection that is off in 9router.
  if (count === 12) {
    conns[0].connection = { ...conns[0].connection, duplicate_of: "claude:Account 2" };
    conns[1].connection = { ...conns[1].connection, enabled_upstream: false };
  }
  // The 40-connection instance is a remote one over HTTPS with a pinned certificate and a pasted SSO session.
  const remote = count >= 40 ? { host: "router.example.com", auth: "session", tls: "pinned" } : { host: "192.0.2.10:20128" };
  demoReports.push(demoInstance("nine_router", name, conns, { version: "0.5.109", ...remote, notes: demoRouterNotes(conns, "0.5.109") }));
}
// PRD 0029 re-validation (V-01, V-03): `?unnamed` leaves a plain account (Groq, the only one of its provider) and the
// 9router instance without a name, as a configuration may; the core then shows them by their provider.
if (new URLSearchParams(location.search).has("unnamed")) {
  for (const r of demoReports) if (r.provider === "groq" || r.provider === "nine_router") r.account = "";
}
// PRD 0016 RF2.1 demo: the platform session slot the core declares for Xiaomi MiMo.
// PRD 0027 RF6.4: the native-login detections of the core table (Kimi CLI, Grok CLI, Zed) for the demo picker.
export const DEMO_NATIVE_DETECT = [["claude", "claude_cli", "Claude Code", "Claude Code"], ["chat_gpt", "codex_cli", "Codex CLI", "OpenAI Codex"], ["kimi", "kimi_cli", "Kimi Code CLI", "Kimi"], ["grok", "grok_cli", "Grok CLI", "Grok (xAI)"], ["zed", "zed", "Zed", "Zed"]];
/** The demo's stable account id (PRD 0029) of the account at `index`: kept by the account itself, never by its position
 * (PRD 0029 re-validation V-12), so a removal or a move never shifts another account's hidden or menu-bar selection. */
const demoIds = new WeakMap();
let demoNextId = 0;
const demoAccountId = (index) => {
  for (const r of demoReports) if (!demoIds.has(r)) demoIds.set(r, `acc_demo${String(demoNextId++).padStart(8, "0")}`);
  return demoIds.get(demoReports[index]);
};
/** PRD 0001 C1 / PRD 0047 re-validation S-01: like the shell, a command aimed at `index` with the dialog's account id
 *  is refused when that slot now holds another account. */
const demoCheckSlot = (index, accountId) => {
  if (accountId != null && demoAccountId(index) !== accountId) {
    throw JSON.stringify({ id: "edit_error.account_moved", params: {},
      fallback: "The account being edited is no longer at this place in the configuration file; reload the file and open the account again." });
  }
};
const DEMO_KEY_INPUT = { slot: "credential_env", label: { id: "secret_input.credential.label", params: {}, fallback: "API key or access token" },
  help: { id: "secret_input.help", params: {}, fallback: "Paste the value. Cuotai keeps it in the system vault and sends it only to this provider; the configuration file keeps just a reference." },
  required: true, cookie: false, field: null };
const DEMO_SESSION_SLOT = { slot: "platform_session_env", open_url: "https://platform.xiaomimimo.com/console/plan-manage",
  label: { id: "user_session.slot.xiaomi_mimo.platform_session_env", params: {}, fallback: "platform.xiaomimimo.com web session (balance and Token Plan)" },
  instructions: { id: "user_session.instructions", params: { site: "platform.xiaomimimo.com" }, fallback: "Sign in to platform.xiaomimimo.com in your browser. In the developer tools (F12), Network tab, reload the page, pick a request to platform.xiaomimimo.com and copy its Cookie header or use \"Copy as cURL\"; the cookie table of the Application tab works too. Only this session's cookies are kept; the rest of the pasted text is discarded." },
  hosts: ["platform.xiaomimimo.com"], capture: true, capture_shared: "google" };
// ponytail: fixed demo fixtures name their core-selected metric and rank explicitly; add a mocked backend when
// the browser demo needs dynamic reports. This keeps binding choice rules out of the interface.
const DEMO_BINDINGS = new Map([
  [0, [2, 5]], [1, [1, 0]], [2, [0, 6]], [3, [1, 1]], [4, [null, 10]], [5, [1, 4]], [6, [0, 8]], [7, [0, 2]],
  [8, [0, 9]], [9, [null, 11]], [10, [null, 12]], [11, [null, 13]], [12, [null, 14]], [13, [0, 7]], [14, [0, 3]], [15, [null, 15]],
  [19, [0, 18]], [20, [null, 16]], [21, [1, 17]],
].map(([index, binding]) => [demoReports[index], binding]));
// ponytail: the core's tray pair (`windows`, `metrics::window_pair`) of each demo account, fixed like the bindings:
// [thick, thin, by model]. Accounts without an entry keep the single summary bar.
const DEMO_WINDOWS = new Map([
  [0, [0, 2]], [1, [1, 0]], [2, [0]], [3, [1]], [5, [0, 1]], [6, [0]], [7, [0]], [8, [0]], [13, [0]], [14, [0]], [19, [0]],
  [20, [1, 0, true]], [21, [0, 1]],
].map(([index, w]) => [demoReports[index], w]));
const demoTrayPair = (r) => {
  const w = DEMO_WINDOWS.get(r);
  return w ? { primary: w[0], secondary: w[1] ?? null, by_model: !!w[2] } : null;
};
const demoBinding = (r) => {
  const [metricIndex] = DEMO_BINDINGS.get(r) ?? [];
  if (metricIndex == null) return null;
  const m = r.metrics[metricIndex];
  const exhausted = m.state === "exhausted";
  const usedRatio = exhausted ? 1 : 1 - m.ratio;
  const renewalDue = demoRenewalDue(r, m);
  return { index: metricIndex, label: m.label, label_text: m.label_text, window: m.window, used_ratio: usedRatio, remaining: m.limit * (1 - usedRatio),
    resets_at: m.resets_at, reason: exhausted ? "exhausted" : "tightest", blocked_until: exhausted && !renewalDue ? m.resets_at : null, renewal_due: renewalDue };
};
// ponytail: fixed core pace output per demo metric (one per stage), same shape as `MetricView.pace`; the browser
// demo never estimates pace itself. Add a mocked backend when the demo needs paces that follow the clock.
/** The core's `metrics::renewal_due`: the reset was at or before the reading (demo values). */
const demoRenewalDue = (r, m) => !!m.resets_at && new Date(m.resets_at) <= new Date(r.checked_at);
/** The core's `metric.renewal_due` text (`metrics::renewal_due_text`), as the shell sends it (demo values). */
const demoRenewalDueLabel = (r, iso) => {
  const at = demoAt(iso), provider = providerName(r.provider);
  return { id: "metric.renewal_due", params: { at, provider }, fallback: `Renewal due since ${at.fallback}; waiting for ${provider} to renew the window.` };
};
/** The core's "Resets {at} · {in}" pair (`resets::pair_label`), as the shell sends it (demo values). */
const demoResetsLabel = (iso) => {
  const at = demoAt(iso), left = countdownNote(iso);
  return { id: "resets.pair", params: { at, in: left }, fallback: `Resets ${at.fallback} · ${left.fallback}` };
};
// `source` "recent" plays a projection the recent readings drove (the muted marker, RF1.5; the forecast needs it).
const dp = (stage, tone, expected, used, resetH, runsOutH = null, source = runsOutH != null && used >= 0.25 ? "recent" : "average") => {
  const delta = (used - expected) * 100, points = String(Math.round(Math.abs(delta)));
  const label = { reserve: { id: "pace.reserve", params: { delta: points }, fallback: `${points} points in reserve` },
    on_pace: { id: "pace.on_pace", fallback: "On pace" }, ahead: { id: "pace.ahead", params: { delta: points }, fallback: `${points} points above the pace` },
    exhausted: { id: "pace.exhausted", fallback: "Limit reached until the reset" } }[stage];
  const at = runsOutH == null ? null : h(runsOutH);
  // The core's margin is a duration note and its time a clock note, so both follow the UI language (V-10).
  const margin = runsOutH == null ? null : durationNote((resetH - runsOutH) * 3600);
  const when = at ? demoAt(at) : null;
  const outlook = stage === "exhausted" ? null : at
    ? { id: "pace.runs_out_at", params: { at: when, margin }, fallback: `runs out at ${when.fallback}, ${margin.fallback} before the reset` }
    : { id: "pace.lasts_until_reset", fallback: "lasts until the reset" };
  // PRD 0024 RF1: the core's stage chip in both units, as `PaceView.chips` carries it (demo values).
  const pct = String(Math.min(999, Math.round((Math.abs(used - expected) / Math.max(expected, 0.01)) * 100)));
  const chip = (glyph, id, params, compact) => ({ glyph, text: { id, params, fallback: id }, compact, hint: null });
  const chips = {
    exhausted: { points: chip("✕", "pace.chip.exhausted", {}, null), percent: chip("✕", "pace.chip.exhausted", {}, null) },
    on_pace: { points: chip("→", "pace.chip.on_pace", {}, null), percent: chip("→", "pace.chip.on_pace", {}, null) },
    ahead: { points: chip("↑", "pace.chip.ahead", { d: points }, null), percent: chip("↑", "pace.chip.ahead_pct", { p: pct }, { id: "pace.chip.percent", params: { p: pct }, fallback: `${pct}%` }) },
    reserve: { points: chip("↓", "pace.chip.reserve", { d: points }, null), percent: chip("↓", "pace.chip.reserve_pct", { p: pct }, { id: "pace.chip.percent", params: { p: pct }, fallback: `${pct}%` }) },
  }[stage];
  // The core's line texts (`Pace::stage_text`, `Pace::summary`, `Pace::shows_stage`, `Pace::glyph`), as `PaceView` sends them.
  const ranked = stage === "ahead" || stage === "reserve";
  const stage_text = { points: label, percent: ranked ? chips.percent.text : label };
  const shows_stage = !(stage === "on_pace" && at);
  const line = (u) => (!shows_stage
    ? { id: "pace.runs_out_at_alone", params: { at: when, margin }, fallback: `Runs out at ${when.fallback}, ${margin.fallback} before the reset` }
    : outlook ? { id: "pace.line", params: { stage: stage_text[u], outlook }, fallback: `${stage_text[u].fallback} · ${outlook.fallback}` } : stage_text[u]);
  return { elapsed_fraction: expected, expected_used_fraction: expected, used_fraction: used, delta_points: delta, stage, tone, source,
    resets_at: h(resetH), projected_exhaustion_at: at, lasts_until_reset: stage !== "exhausted" && !at, label, outlook, chips,
    stage_text, summary: { points: line("points"), percent: line("percent") }, shows_stage, glyph: shows_stage ? chips.points.glyph : "↑",
    ...(source === "recent" ? { marker: { id: "pace.source.recent", params: {}, fallback: "recent" } } : {}) };
};
/** PRD 0024 re-validation (V-01, V-10): the core clock the shell sends (`at.time`, `at.weekday`, `at.date` with
 *  machine params); `short` is the chip form (the weekday or the date alone beyond today). */
function demoAt(iso, short = false) {
  const d = new Date(iso);
  const days = Math.round((new Date(d.toDateString()) - new Date(new Date().toDateString())) / 864e5);
  const hm = { hour: String(d.getHours()), minute: String(d.getMinutes()) };
  const time = { hour: "numeric", minute: "2-digit" };
  const en = (shape) => fmtLocal(d, shape, "en-US");
  if (days === 0) return { id: "at.time", params: hm, fallback: en(time) };
  if (days >= 1 && days <= 6) {
    const weekday = String(((d.getDay() + 6) % 7) + 1);
    return short ? { id: "at.weekday", params: { weekday }, fallback: en({ weekday: "short" }) }
      : { id: "at.weekday", params: { weekday, ...hm }, fallback: en({ weekday: "long", ...time }) };
  }
  const dm = { day: String(d.getDate()), month: String(d.getMonth() + 1) };
  return short ? { id: "at.date", params: dm, fallback: en({ month: "2-digit", day: "2-digit" }) }
    : { id: "at.date", params: { ...dm, ...hm }, fallback: en({ month: "2-digit", day: "2-digit", ...time }) };
}
/** PRD 0026 re-validation (V-02, V-03, V-09, V-11): the core's past clock, day text, day heading and hour text the shell
 *  sends for Stats and the journal (`text::clock::past`, `day`, `day_heading`, `hour`), with machine params. */
const demoDaysBack = (d) => Math.round((new Date(new Date().toDateString()) - new Date(d.toDateString())) / 864e5);
// The demo stands in for the core, so its en-US fallbacks reuse the cached formatters: built per text, they were
// most of the tray Stats drill-in's first frame in the browser demo (perf probe `tray-open-stats`).
const demoEn = (d, shape) => fmtLocal(d, shape, "en-US");
function demoTime(iso) {
  const d = new Date(iso);
  return { id: "at.time", params: { hour: String(d.getHours()), minute: String(d.getMinutes()) }, fallback: demoEn(d, { hour: "numeric", minute: "2-digit" }) };
}
function demoPast(iso) {
  const d = new Date(iso), back = demoDaysBack(d);
  const hm = { hour: String(d.getHours()), minute: String(d.getMinutes()) }, time = { hour: "numeric", minute: "2-digit" };
  if (back === 0) return demoTime(iso);
  if (back >= 1 && back <= 6) return { id: "at.weekday", params: { weekday: String(((d.getDay() + 6) % 7) + 1), ...hm }, fallback: demoEn(d, { weekday: "long", ...time }) };
  return { id: "at.date", params: { day: String(d.getDate()), month: String(d.getMonth() + 1), ...hm }, fallback: demoEn(d, { month: "2-digit", day: "2-digit", ...time }) };
}
function demoDay(day) {
  const d = new Date(`${day}T12:00:00`), back = demoDaysBack(d);
  if (back >= 0 && back <= 6) return { id: "at.weekday", params: { weekday: String(((d.getDay() + 6) % 7) + 1) }, fallback: demoEn(d, { weekday: "short" }) };
  return { id: "at.date", params: { day: String(d.getDate()), month: String(d.getMonth() + 1) }, fallback: demoEn(d, { month: "2-digit", day: "2-digit" }) };
}
function demoHeading(iso) {
  const d = new Date(iso), back = demoDaysBack(d);
  if (back === 0) return { id: "cost.period.today", params: {}, fallback: "Today" };
  if (back === 1) return { id: "cost.view.yesterday", params: {}, fallback: "Yesterday" };
  return demoDay(`${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, "0")}-${String(d.getDate()).padStart(2, "0")}`);
}
const demoHour = (h) => ({ id: "at.hour", params: { hour: String(h) }, fallback: demoEn(new Date(2024, 0, 1, h), { hour: "numeric" }) });
/** PRD 0024 RF4.3: the forecast the shell adds to a metric the core's strict forecast says runs out before the reset
 *  (RF2.2: a recent slope and at least 25% used; demo values). */
const demoForecast = (m, pace) => {
  if (!pace?.projected_exhaustion_at || pace.source !== "recent" || !(pace.used_fraction >= 0.25)) return null;
  const at = demoAt(pace.projected_exhaustion_at);
  const short = demoAt(pace.projected_exhaustion_at, true);
  const margin = durationNote((new Date(m.resets_at) - new Date(pace.projected_exhaustion_at)) / 1000);
  return { at,
    callout: { id: "alerts.body.projected", params: { metric: m.label_text ?? m.label, at, margin }, fallback: `At this rate ${m.label_text?.fallback ?? m.label} runs out ≈${at.fallback}, ${margin.fallback} before the reset` },
    chip: { glyph: "↑", text: { id: "pace.chip.runs_out", params: { at: short }, fallback: `Runs out ≈${short.fallback}` },
      compact: { id: "pace.chip.approx", params: { at: short }, fallback: `≈${short.fallback}` }, hint: null } };
};
// report index → [pace_index, { metric index: pace }]; windows that just opened (or not yet) have none, as in the core.
const DEMO_PACES = new Map([
  [0, [2, { 0: dp("reserve", "good", 0.926, 0.015, 0.37), 2: dp("on_pace", "neutral", 0.101, 0.12, 151, 124.7), 3: dp("reserve", "good", 0.096, 0, 151.9) }]],
  [1, [1, { 0: dp("reserve", "good", 0.1, 0, 4.5), 1: dp("exhausted", "bad", 0.854, 1, 24.6) }]],
  [5, [1, { 0: dp("reserve", "good", 0.48, 0.06, 2.6), 1: dp("ahead", "warn", 0.413, 0.6, 98.6, 46.3), 2: dp("reserve", "good", 0.48, 0, 2.6) }]],
  [7, [0, { 0: dp("exhausted", "bad", 0.351, 1, 109) }]],
  [8, [0, { 0: dp("reserve", "good", 0.656, 0, 248) }]],
  [14, [0, { 0: dp("exhausted", "bad", 0.866, 1, 100) }]],
  [20, [null, { 0: dp("reserve", "good", 0.6, 0.35, 9), 1: dp("ahead", "warn", 0.6, 0.82, 9, 4.5) }]],
  [21, [1, { 0: dp("reserve", "good", 0.62, 0.4, 9), 1: dp("ahead", "warn", 0.45, 0.7, 300, 150) }]],
].map(([index, pace]) => [demoReports[index], pace]));
// PRD 0033 re-validation (V-15): an AWS Bedrock account as the core reports it (`providers::cloud::fetch_bedrock`): the
// month's Cost Explorer spend with its raw label and the translated `label_text`/`period_text`, the two Cost Explorer
// notes and the hourly metered interval (example.com only, demo figures).
const demoBedrock = () => {
  const now = new Date();
  const monthEnd = new Date(Date.UTC(now.getUTCFullYear(), now.getUTCMonth() + 1, 1)).toISOString();
  const cost = { label: "Custo AWS Bedrock (mês corrente)", label_text: { id: "metric.label.bedrock_cost", params: {}, fallback: "AWS Bedrock cost (current month)" },
    used: 18.42, limit: null, remaining: null, unit: "USD", unit_text: { id: "metric.verbatim", params: {}, fallback: "USD" }, kind: "spend", window: "monthly",
    period: "mês corrente", period_text: { id: "metric.period.current_month", params: {}, fallback: "current month" }, resets_at: monthEnd,
    state: "available", ratio: null, uncapped: true, no_cap: false };
  const notes = [
    { id: "note.cloud.aws_cost_scope", params: {}, fallback: "Cost Explorer filtered by service \"Amazon Bedrock\", monthly granularity, current month. AWS cost data updates ~1x/day and may lag by up to 24h." },
    { id: "note.cloud.aws_query_cost", params: {}, fallback: "Each Cost Explorer request is billed by AWS (about US$ 0.01); Cuotai makes at most one per interval for all its windows and commands." },
  ];
  return { ...dr("billing@example.com", "aws_bedrock", [cost], notes.map((n) => n.fallback)), notes_l10n: notes, refresh_seconds: 3600,
    source: "https://docs.aws.amazon.com/cli/latest/reference/ce/get-cost-and-usage.html" };
};
// PRD 0022 RF18.2: `?fixture=six` swaps the demo accounts for the mockups' set (the six accounts plus a second Claude
// account, example.com identities only), so the captures compare with `assets/0022/after/` (01, 03, 10, 11). With
// `&bedrock` it adds an AWS Bedrock account (PRD 0027 RF3.1 "Billed per query" chip). `?mono=XY` (any demo) gives the
// first account a monogram override (DS-CMP-MONO-4), as `desktop.json` would.
const DEMO_QUERY = new URLSearchParams(globalThis.location?.search ?? "");
if (DEMO_QUERY.get("fixture") === "six") {
  const overage = (amount) => ({ state: "active", spent: amount, unit: "USD", currency: "USD", cap: 50, cycle_ends_at: h(300),
    text: { id: "overage.on_extra", params: { amount: demoMoney(amount) }, fallback: `On extra · $${amount.toFixed(2)}` },
    summary: { id: "overage.limit_hit_on_extra", params: { amount: demoMoney(amount) }, fallback: `Limit hit · on extra $${amount.toFixed(2)}` }, caveat: null });
  const six = [
    [{ ...dr("work@example.com", "claude", [dm("session (5h)", 78, 100, 1.2, "quota", "window_5h", "%"), dm("weekly (7d)", 41, 100, 70, "quota", "weekly", "%")]), plan: "Max", model_count: 3 },
      [0, 0], [0, { 0: dp("ahead", "warn", 0.62, 0.78, 1.2, 0.4), 1: dp("ahead", "warn", 0.3, 0.41, 70, 30) }], [0, 1]],
    [{ ...dr("side@example.com", "claude", [dm("session (5h)", 29, 100, 3.1, "quota", "window_5h", "%"), dm("weekly (7d)", 12, 100, 120, "quota", "weekly", "%")]), plan: "Pro" },
      [0, 3], [0, { 0: dp("on_pace", "good", 0.3, 0.29, 3.1) }], [0, 1]],
    [{ ...dr("me@example.com", "chat_gpt", [dm("5h", 37, 100, 2.4, "quota", "window_5h", "%"), dm("weekly", 20, 100, 90, "quota", "weekly", "%")]), plan: "Plus" }, [0, 2], [0, { 0: dp("reserve", "good", 0.5, 0.37, 2.4) }], [0, 1]],
    [{ ...dr("dev@example.com", "cursor", [dm("Fast requests", 500, 500, 200, "quota", "monthly"),
      { label: "Extra usage", used: 6.25, limit: 50, remaining: null, unit: "USD", kind: "spend", window: "monthly", period: null, resets_at: h(200), billing: "overage",
        state: "available", ratio: null, uncapped: true, no_cap: false }]), class: "exhausted", plan: "Pro", overage: overage(6.25) }, [0, 1], null, [0]],
    [dr("ops@example.com", "kimi", [dm("Weekly", 41, 100, 50, "quota", "weekly"), dm("Ratelimit", 0, 100, 4.8, "rate_limit", "other")]), [0, 4], [0, { 0: dp("on_pace", "neutral", 0.42, 0.41, 50) }], [0]],
    [{ ...drn("team@example.com", "groq", [], demoNote("down", "Service unavailable right now."), "error"), checked_at: ago(2),
      last_good: { checked_at: ago(38), metrics: [dm("Requests (day)", 26, 100, 9, "quota", "daily")] } }, [null, 5], null],
    [drn("api@example.com", "anthropic_api", [], demoNote("auth", "Credential missing or expired."), "authentication_error"), [null, 6], null],
    ...(DEMO_QUERY.has("bedrock") ? [[demoBedrock(), [null, 7], null]] : []),
  ];
  demoReports.splice(0, demoReports.length, ...six.map(([r]) => r));
  // PRD 0024 re-validation (V-10): the core's best account of the Claude pair and the advice of the one that runs out,
  // so `check.mjs` and the smokes render the Best now chip, the advice with its Show action and a projection on
  // another day (Work's week, above).
  const [work, side] = demoReports;
  side.best_now = { id: "advice.best", params: {}, fallback: "Best now" };
  side.group_label = "Coding";
  const at = demoAt(h(0.4));
  work.advice = { best_index: 1, best_name: side.account,
    text: { id: "advice.switch", params: { from: work.account, at, to: side.account, pct: "71" }, fallback: `${work.account} runs out ≈${at.fallback} · ${side.account} has 71% left` } };
  for (const [r, binding, pace, windows] of six) {
    DEMO_BINDINGS.set(r, binding);
    if (pace) DEMO_PACES.set(r, pace);
    if (windows) DEMO_WINDOWS.set(r, windows);
  }
}
// PRD 0023 re-validation (V-01, V-03, V-05, V-09): `?overage` adds the extra-usage states the default demo lacks (a
// Claude account billing in provider units, a capped budget, an unknown amount and an armed one; example.com only), so
// `check.mjs` renders them for overflow, zoom and contrast.
if (DEMO_QUERY.has("overage")) {
  const units = (v) => ({ id: "overage.amount.provider_units",
    params: { value: { id: "amount.number", params: { value: String(v) }, fallback: v.toLocaleString("en-US") } }, fallback: `${v.toLocaleString("en-US")} provider units` });
  const fixed = (id, fallback) => ({ id, params: {}, fallback });
  const extra = (used, limit, unit, kind) => ({ label: "Extra usage", used, limit, remaining: null, unit, kind, window: "monthly", period: null,
    resets_at: h(300), billing: "overage", state: "available", ratio: null, uncapped: true, no_cap: false });
  const spentWeek = () => [dm("session (5h)", 0, 100, 4.5, "quota", "window_5h", "%"), dm("weekly (7d)", 100, 100, 24.6, "quota", "weekly", "%")];
  const cap = fixed("overage.cap_reached", "Extra budget used up");
  const unknown = fixed("overage.unknown", "Extra usage amount unknown");
  const armed = fixed("overage.armed", "Extra usage on · nothing spent");
  const variants = [
    [{ ...dr("units@example.com", "claude", [...spentWeek(), extra(1234.5, 5000, "unid. provedor", "quota")]), class: "exhausted", plan: "Max",
      overage: { state: "active", spent: 1234.5, unit: "unid. provedor", currency: null, cap: 5000, cycle_ends_at: h(300),
        text: { id: "overage.on_extra", params: { amount: units(1234.5) }, fallback: "On extra · 1,234.5 provider units" },
        summary: { id: "overage.limit_hit_on_extra", params: { amount: units(1234.5) }, fallback: "Limit hit · on extra 1,234.5 provider units" },
        caveat: fixed("overage.units_caveat", "Amounts are in provider units, not money.") } }, [1, 30], [0, 1]],
    [{ ...dr("capped@example.com", "cursor", [dm("Fast requests", 500, 500, 200, "quota", "monthly"), extra(50, 50, "USD", "spend")]), class: "exhausted", plan: "Pro",
      overage: { state: "cap_reached", spent: 50, unit: "USD", currency: "USD", cap: 50, cycle_ends_at: h(300), text: cap, summary: cap, caveat: null } }, [0, 31], [0]],
    [{ ...dr("unknown@example.com", "claude", [dm("session (5h)", 20, 100, 3, "quota", "window_5h", "%"), dm("weekly (7d)", 40, 100, 90, "quota", "weekly", "%"),
      extra(null, null, "unid. provedor", "quota")]), plan: "Pro",
      overage: { state: "unknown", spent: null, unit: "unid. provedor", currency: null, cap: null, cycle_ends_at: null, text: unknown, summary: unknown, caveat: null } }, [1, 32], [0, 1]],
    [{ ...dr("armed@example.com", "claude", [dm("session (5h)", 30, 100, 2, "quota", "window_5h", "%"), dm("weekly (7d)", 55, 100, 70, "quota", "weekly", "%"),
      extra(0, 50, "USD", "spend")]), plan: "Max",
      overage: { state: "armed", spent: 0, unit: "USD", currency: "USD", cap: 50, cycle_ends_at: h(300), text: armed, summary: armed, caveat: null } }, [1, 33], [0, 1]],
  ];
  for (const [r, binding, windows] of variants) {
    demoReports.push(r);
    DEMO_BINDINGS.set(r, binding);
    DEMO_WINDOWS.set(r, windows);
  }
}
// PRD 0037 re-validation (V-09): `?eleven` swaps the demo accounts for an ElevenLabs set shaped like the adapter's
// reports (`providers::eleven_labs`: raw labels and units with the core's texts, its note ids): a normal plan, extra
// usage armed, active, at its cap and with no cap (`unlimited`), a plan whose limit was not reported, the disabled free
// plan with its zero limit (exhausted, PRD 0037 RF3), a reset already past at the reading (renewal due, RF4), a key
// without the subscription permission, a changed response, and an EU account during an incident on its own status
// page beside a US account on the operational one (one status per source, RF9). example.com identities only.
if (DEMO_QUERY.has("eleven")) {
  // The core's `metrics::classify_metric` of each metric, with the explicit zero allowance of a quota (RF3).
  const metric = (label, labelId, english, unit, unitText, fields) => {
    const { used = null, limit = null, remaining = null } = fields;
    const zero = limit === 0 && remaining === 0 && fields.kind === "quota";
    const cap = limit > 0 ? limit : null;
    const state = used == null && remaining == null ? { state: "unmeasured" }
      : zero || (remaining != null && remaining <= 0) ? { state: "exhausted", no_cap: cap == null }
        : cap != null && used >= cap ? { state: "exhausted", no_cap: false }
          : cap != null ? { state: "available", ratio: Math.min(1, Math.max(0, (cap - used) / cap)), uncapped: false }
            : { state: "available", ratio: 0, uncapped: true };
    return { label, label_text: demoText(labelId, english), unit, ...(unitText ? { unit_text: unitText } : {}), used, limit, remaining,
      period: null, billing: "included", scope: null, derived_from: null, unlimited: false, no_cap: false, ...fields, ...state };
  };
  const CREDITS = demoText("metric.unit.credits", "credits");
  const credits = (used, limit, resetH = 300) => metric("Créditos do plano", "metric.label.plan_credits", "Plan credits", "créditos", CREDITS,
    { used, limit, remaining: limit == null ? null : Math.max(0, limit - used), kind: "quota", window: "monthly", resets_at: h(resetH) });
  const extraCredits = (used, cap) => metric("Créditos extras usados", "metric.label.extra_credits_used", "Extra credits used", "créditos", CREDITS,
    { used, limit: cap, remaining: cap == null ? null : Math.max(0, cap - used), kind: "usage", window: "monthly", resets_at: h(300), billing: "overage", unlimited: cap == null });
  const extraCost = (amount) => metric("Custo do uso extra", "metric.label.extra_usage_cost", "Extra usage cost", "USD", null,
    { used: amount, kind: "spend", window: "monthly", resets_at: h(300), billing: "overage" });
  const voices = (used, limit) => metric("Vozes personalizadas", "metric.label.voices_custom", "Custom voices", "vozes", demoText("metric.unit.voices", "voices"),
    { used, limit, remaining: Math.max(0, limit - used), kind: "usage", window: null, resets_at: null });
  const SOURCE = coreNote("note.eleven_labs.source", "Read with GET /v1/user/subscription using the API key; the query does not generate audio.");
  const PAYG = coreNote("note.eleven_labs.payg_not_reported", "Pay As You Go balance: not reported by this endpoint.");
  const tier = (name) => coreNote("note.eleven_labs.tier", `Plan reported by ElevenLabs: ${name}.`, { tier: name });
  const notes = (...extra) => coreNotes([SOURCE, ...extra, PAYG]);
  const fixed = (id, fallback) => ({ id, params: {}, fallback });
  // The core's `overage::assess` of each report: the headline is the money metric (the cost, which has no cap), and an
  // extra-credit counter at its cap escalates it to "cap reached".
  const overage = (state, spent) => {
    const onExtra = (id, lead) => ({ id, params: { amount: demoMoney(spent) }, fallback: `${lead} $${spent.toFixed(2)}` });
    const text = state === "active" ? onExtra("overage.on_extra", "On extra ·")
      : state === "cap_reached" ? fixed("overage.cap_reached", "Extra budget used up") : fixed("overage.armed", "Extra usage on · nothing spent");
    const summary = state === "active" ? onExtra("overage.limit_hit_on_extra", "Limit hit · on extra") : text;
    return { state, spent, unit: "USD", currency: "USD", cap: null, cycle_ends_at: h(300), text, summary, caveat: null };
  };
  const report = (account, metrics, extra = {}) => ({ ...dr(account, "eleven_labs", metrics), demo_region: "default", ...extra });
  const failed = (account, status, list, extra = {}) => ({ ...dr(account, "eleven_labs", [], list.map((n) => n.fallback), status), notes_l10n: list, class: "unmeasured", demo_region: "default", ...extra });
  const drift = { id: "diagnostic.schema_drift", params: { path: "character_count", expected: "number", found: "missing" }, fallback: "The provider changed character_count (number → missing)." };
  const eleven = [
    [report("voice@example.com", [credits(41200, 100000), voices(3, 10)], notes(tier("creator"))), [0, 0], [0]],
    [report("armed@example.com", [credits(88000, 100000), extraCredits(0, 20000), extraCost(0), voices(1, 10)], { ...notes(tier("pro")), overage: overage("armed", 0) }), [0, 1], [0]],
    [report("active@example.com", [credits(100000, 100000), extraCredits(3000, 20000), extraCost(4.2)],
      { ...notes(tier("pro")), class: "exhausted", overage: overage("active", 4.2) }), [0, 2], [0]],
    [report("capped@example.com", [credits(100000, 100000), extraCredits(20000, 20000), extraCost(28)],
      { ...notes(tier("pro")), class: "exhausted", overage: overage("cap_reached", 28) }), [0, 3], [0]],
    [report("unlimited@example.com", [credits(500000, 500000), extraCredits(64000, null), extraCost(89.6)],
      { ...notes(tier("scale"), coreNote("note.eleven_labs.overage_unlimited", "Extra usage has no credit cap.")), class: "exhausted", overage: overage("active", 89.6) }), [0, 4], [0]],
    [report("nolimit@example.com", [credits(42000, null)],
      notes(coreNote("note.eleven_labs.limit_missing", "ElevenLabs did not report the credit limit; only consumption is shown."))), [null, 5], null],
    [report("free@example.com", [credits(0, 0, 200), voices(0, 3)],
      { ...notes(tier("free"), coreNote("note.eleven_labs.status.free_disabled", "The ElevenLabs free plan is disabled; the credits shown may no longer be usable.")),
        class: "exhausted" }), [0, 6], [0]],
    [report("renewal@example.com", [credits(100000, 100000, -2)], { ...notes(tier("creator")), class: "exhausted" }), [0, 7], [0]],
    [failed("scope@example.com", "authentication_error", [coreNote("note.eleven_labs.scope_missing",
      "The key is valid but may not read the subscription; check its restriction (and, for Enterprise, its IP allow-list or workspace).")]), [null, 8], null],
    [failed("drift@example.com", "error", [fixed("query.schema", "Response is incompatible with the query interface; no value was estimated."), drift],
      { drift: { heading: fixed("diagnostic.what_changed", "What changed"), sentence: drift, paths: [{ path: "character_count", expected: "number", found: "missing" }],
        privacy: fixed("diagnostic.values_never_recorded", "Values are never recorded.") } }), [null, 9], null],
    [report("eu@example.com", [credits(12500, 100000), voices(2, 10)], { ...notes(tier("creator")), demo_region: "eu" }), [0, 10], [0]],
    [report("us@example.com", [credits(30000, 100000)], { ...notes(tier("creator")), demo_region: "us" }), [0, 11], [0]],
  ];
  demoReports.splice(0, demoReports.length, ...eleven.map(([r]) => r));
  for (const [r, binding, windows] of eleven) {
    DEMO_BINDINGS.set(r, binding);
    if (windows) DEMO_WINDOWS.set(r, windows);
  }
}
// PRD 0038 re-validation: `?openrouter` swaps the demo accounts for an OpenRouter set shaped like the adapter's reports
// (`providers::open_router`: raw labels and units with the core's text ids, its note ids). The key view: a key with a
// monthly limit, spend by day, week and month and the free-model counter; a key without a limit. The account view
// (management key): a positive credit balance with spend by upstream provider over completed UTC days (the period
// line), a negative balance under the low-balance threshold, and a key rate-limited with the instant `Retry-After`
// allows (`retry_note`, the core's wait line). Management-key amounts are in credits (no `$`, PRD 0038 RF5.3 and O2); the key view
// is in USD. example.com identities only.
if (DEMO_QUERY.has("openrouter")) {
  const metric = (label, labelText, unit, unitText, fields) => {
    const { used = null, limit = null, remaining = null } = fields;
    const cap = limit > 0 ? limit : null;
    const state = used == null && remaining == null ? { state: "unmeasured" }
      : remaining != null && remaining <= 0 ? { state: "exhausted", no_cap: cap == null }
        : cap != null && used >= cap ? { state: "exhausted", no_cap: false }
          : cap != null ? { state: "available", ratio: Math.min(1, Math.max(0, (cap - used) / cap)), uncapped: false }
            : { state: "available", ratio: 0, uncapped: true };
    return { label, label_text: labelText, unit, ...(unitText ? { unit_text: unitText } : {}), used, limit, remaining,
      window: null, period: null, resets_at: null, billing: "included", scope: null, derived_from: null, unlimited: false, no_cap: false, ...fields, ...state };
  };
  const CREDITS = demoText("metric.unit.credits", "credits");
  const REQUESTS = demoText("metric.unit.requisitions", "requests");
  const keyLimit = (used, limit) => metric("Limite da chave", demoText("metric.label.key_limit", "Key limit"), "USD", null,
    { used, limit, remaining: Math.max(0, limit - used), kind: "quota", window: "monthly", resets_at: h(300) });
  const spend = (label, id, english, used, window, resetH) => metric(label, demoText(id, english), "USD", null,
    { used, kind: "spend", window, resets_at: resetH == null ? null : h(resetH) });
  const keySpend = (today, week, month) => [spend("Gasto hoje", "metric.label.today_spend", "Spending today", today, "daily", 9),
    spend("Gasto na semana", "metric.label.week_spend", "Spending in the week", week, "weekly", 81),
    spend("Gasto no mês", "metric.label.month_spend", "Spending in the month", month, "monthly", 300)];
  const freeToday = (used, limit) => metric("Modelos gratuitos (:free): requisições hoje", demoText("metric.label.free_models_today", "Free models (:free): requests today"),
    "requisições", REQUESTS, { used, limit, remaining: Math.max(0, limit - used), kind: "quota", window: "daily", resets_at: h(9) });
  const balance = (value) => metric("Saldo de créditos", demoText("metric.label.credit_balance", "Credit balance"), "créditos", CREDITS,
    { remaining: value, kind: "balance" });
  const creditsUsed = (value) => metric("Créditos usados (total)", demoText("metric.label.credits_used_total", "Credits used (total)"), "créditos", CREDITS,
    { used: value, kind: "spend" });
  // Spend by upstream provider: completed UTC days, so the metric carries the adapter's period and no reset.
  const PERIOD = { period: "30 dias concluídos (UTC)", period_text: demoText("metric.period.last_30_days_utc", "30 completed days (UTC)") };
  const byProvider = (name, used) => metric(`Gasto 30 dias — ${name}`, demoText("metric.label.spend_30d", `30-day spending — ${name}`, { name }), "créditos", CREDITS,
    { used, kind: "spend", window: "other", ...PERIOD });
  const others = (used) => metric("Gasto 30 dias — Outros", demoText("metric.label.spend_30d_other", "30-day spending — Other"), "créditos", CREDITS,
    { used, kind: "spend", window: "other", ...PERIOD });
  const KEY_SCOPE = coreNote("note.open_router.key_scope", "These numbers describe this key, not the account balance; use the account product for the balance.");
  const FREE_TIER = coreNote("note.open_router.free_tier", "Free models allow 50 or 1000 requests per day depending on credits ever purchased (the tier follows the purchased total, not is_free_tier) and 20 per minute, which the API does not report.");
  const UNIT = coreNote("note.open_router.credits_unit", "Balance = total_credits - total_usage as OpenRouter reports it; the endpoint documents only \"credits\", so no currency is assumed.");
  const WINDOW = coreNote("note.open_router.activity_window", "Spend by origin covers completed UTC days only and updates after midnight UTC.");
  const LOW = coreNote("note.open_router.low_balance", "The credit balance is at or below the low-balance threshold of 5.", { threshold: "5" });
  const report = (account, metrics, notes, extra = {}) => ({ ...dr(account, "open_router", metrics), ...coreNotes(notes), ...extra });
  const RATE = coreNote("query.open_router.rate_limited", "OpenRouter rate-limited the queries.");
  const RETRY = coreNote("query.retry_after_until", "OpenRouter asked Cuotai to wait; this account is queried again at 14:30.", { provider: "OpenRouter", time: "14:30" });
  const openrouter = [
    [report("key@example.com", [keyLimit(12.4, 50), ...keySpend(0.8, 4.1, 12.4), freeToday(12, 50)], [KEY_SCOPE, FREE_TIER]), [0, 0], [0, 4]],
    [report("nolimit@example.com", [...keySpend(0, 2.35, 9.1), freeToday(50, 50)], [KEY_SCOPE, FREE_TIER], { class: "exhausted" }), [3, 1], [3]],
    [report("balance@example.com", [balance(37.25), creditsUsed(162.75), byProvider("OpenAI", 80.1), byProvider("Anthropic", 52.4), others(12)], [UNIT, WINDOW]), [null, 2], null],
    [report("negative@example.com", [balance(-3.4), creditsUsed(103.4)], [UNIT, LOW], { class: "exhausted" }), [null, 3], null],
    [{ ...dr("limited@example.com", "open_router", [], [RATE.fallback], "rate_limited"), notes_l10n: [RATE], class: "unmeasured", retry_note: RETRY }, [null, 4], null],
  ];
  demoReports.splice(0, demoReports.length, ...openrouter.map(([r]) => r));
  for (const [r, binding, windows] of openrouter) {
    DEMO_BINDINGS.set(r, binding);
    if (windows) DEMO_WINDOWS.set(r, windows);
  }
}
// PRD 0045 re-validation (V-07): `?breakdown` swaps the demo accounts for two accounts with a breakdown, shaped like the
// backend's `BreakdownView` (the core's spend order, its percentage order, the card's "Top: …" line and the truncation
// note): a LiteLLM proxy whose 26 virtual keys fill 24 rows plus "Others (2)" (one capped at 92 %, one exhausted, one
// disabled with its note, one with the fallback label, a budget offered on every uncapped row), and an OpenRouter
// management key with 5 key rows. example.com identities only; no label looks like a key.
if (DEMO_QUERY.has("breakdown")) {
  const usd = demoText("metric.verbatim", "USD");
  const monthEnd = (() => { const d = new Date(); return new Date(d.getFullYear(), d.getMonth() + 1, 1).toISOString(); })();
  const spendM = (used) => ({ label: "Gasto total", label_text: demoText("metric.label.total_spend", "Total spending"), used, limit: null, remaining: null,
    unit: "USD", unit_text: usd, kind: "spend", window: null, period: null, resets_at: null, state: "available", ratio: null, uncapped: true, no_cap: false });
  const capM = (label, id, english, used, limit) => ({ label, label_text: demoText(id, english), used, limit, remaining: Math.max(0, limit - used),
    unit: "USD", unit_text: usd, kind: "quota", window: "monthly", period: null, resets_at: monthEnd, resets_label: demoResetsLabel(monthEnd),
    state: used >= limit ? "exhausted" : "available", ratio: Math.max(0, (limit - used) / limit), uncapped: false, no_cap: false });
  const DISABLED = coreNote("note.breakdown.disabled", "Disabled on the provider: it cannot be used until it is enabled again.");
  /** One row of the view: its figure is the capped metric, else the spend; a budget is offered on an uncapped row. */
  const row = (scope, id, label, metric, notes = []) => ({ scope, id, label: demoText("metric.verbatim", label), others: false, figure: metric,
    metrics: [metric], notes, demo_label: label,
    ...(metric.limit == null ? { budget_draft: { kind: "spend", label_contains: "Total spending", amount: 0, unit: "USD", period: "monthly", row: { scope, id } } } : {}) });
  /** The backend's view of `rows` (in spend order) and the "Others" row, if any, last. */
  const view = (rows, others) => {
    const frac = (r) => (r.figure?.limit ? r.figure.used / r.figure.limit : null);
    const byPercent = rows.map((_, i) => i).sort((a, b) => (frac(rows[b]) ?? -1) - (frac(rows[a]) ?? -1) || a - b);
    // `breakdown::amount_text`: whole dollars from $100 or for a whole amount, cents otherwise.
    const amount = (v) => (v >= 100 || Number.isInteger(v) ? v.toFixed(0) : v.toFixed(2));
    const items = rows.slice(0, 2).map((r) => `${r.demo_label} $${amount(r.figure.used)}`).join(" · ");
    return { rows: (others ? [...rows, others] : rows).map(({ demo_label, ...r }) => r), by_percent: others ? [...byPercent, rows.length] : byPercent,
      top: coreNote("breakdown.top", `Top: ${items}`, { items }), alert_rows: false,
      truncated: others ? coreNote("breakdown.truncated", `Showing the top ${rows.length} of ${rows.length + 2}; the rest are summed in Others.`,
        { shown: String(rows.length), total: String(rows.length + 2) }) : null };
  };
  const NAMES = ["proj-alpha", "ci-runner", "search-api", "batch-nightly", "support-bot", "docs-site", "eval-suite", "mobile-app", "data-sync",
    "billing-svc", "lab-notebooks", "ingest", "staging", "translator", "summarizer", "web-chat", "qa-tools", "reports", "partner-a", "partner-b",
    "intern-sandbox", "metrics"];
  const vk = (n) => `vk${String(n).padStart(2, "0")}`;
  const keys = NAMES.map((name, n) => row("virtual_key", vk(n), name, spendM(Number((412 / (1 + n * 0.45)).toFixed(2)))));
  keys[1] = row("virtual_key", vk(1), "ci-runner", capM("Orçamento da chave", "metric.label.key_budget", "Key budget", 276, 300));
  keys[3] = row("virtual_key", vk(3), "batch-nightly", capM("Orçamento da chave", "metric.label.key_budget", "Key budget", 150, 150));
  keys[5] = row("virtual_key", vk(5), "docs-site", spendM(keys[5].figure.used), [DISABLED]);
  keys.push(row("virtual_key", vk(22), "Virtual key …7f3a", spendM(1.25)), row("virtual_key", vk(23), "legacy-import", spendM(0.4)));
  const keyOthers = { scope: "virtual_key", id: "others", label: coreNote("breakdown.others", "Others (2)", { count: "2" }), others: true,
    figure: spendM(0.35), metrics: [spendM(0.35)], notes: [] };
  const budget = (used, limit) => [dm("budget (30d)", used, limit, 240, "spend", "monthly", "USD")];
  const conns = [["openai", "open_ai_api", 42.5, 100], ["anthropic", "anthropic_api", 180, 200]]
    .map(([upstream, mapped, used, limit], n) => demoConnection(`lit${n}`, upstream, upstream, mapped, budget(used, limit), {}, "gateway", "litellm"));
  const litellm = { ...demoInstance("litellm", "gateway", conns, { host: "127.0.0.1:4000",
    notes: [coreNote("note.litellm.summary", `${conns.length} providers with a budget in LiteLLM.`, { count: String(conns.length) })] }), breakdown: view(keys, keyOthers) };
  const orRows = [
    row("key", "ork1", "prod-backend", spendM(86.4)),
    row("key", "ork2", "agents", capM("Limite da chave", "metric.label.key_limit", "Key limit", 46, 50)),
    row("key", "ork3", "playground", spendM(12.1)),
    row("key", "ork4", "Key …91c2", spendM(3.05), [DISABLED]),
    row("key", "ork5", "experiments", spendM(0.8)),
  ];
  const balanceM = { label: "Saldo de créditos", label_text: demoText("metric.label.credit_balance", "Credit balance"), unit: "créditos",
    unit_text: demoText("metric.unit.credits", "credits"), used: null, limit: null, remaining: 37.25, kind: "balance", window: null, period: null,
    resets_at: null, state: "available", ratio: 0, uncapped: true, no_cap: false };
  const openrouter = { ...dr("org@example.com", "open_router", [balanceM]), breakdown: view(orRows, null) };
  demoReports.splice(0, demoReports.length, litellm, openrouter);
}
// PRD 0037 re-validation (V-09): `?every-provider` adds one card for each provider the demo set lacks, so every
// provider of the core (`Provider::ALL`, the keys of `demo-forms.json`) renders a card; `scripts/test-demo-fixture-ui.mjs`
// fails when a new provider has none. Shapes follow each adapter (its kind, window and unit); a provider without a
// usage query shows its core reason; usage a provider does not report stays absent (null), never zero. example.com
// identities only.
if (DEMO_QUERY.has("every-provider")) {
  const metric = (label, unit, fields) => {
    const { used = null, limit = null, remaining = limit != null && used != null ? Math.max(0, limit - used) : null } = fields;
    const cap = limit > 0 ? limit : null;
    const state = used == null && remaining == null ? { state: "unmeasured", ratio: null, uncapped: false }
      : cap != null && used >= cap ? { state: "exhausted", ratio: 0, uncapped: false }
        : cap != null ? { state: "available", ratio: (cap - used) / cap, uncapped: false }
          : { state: "available", ratio: null, uncapped: true };
    return { ...demoTexts(label, unit), used, limit, remaining, window: null, period: null, resets_at: null, no_cap: false, ...fields, ...state };
  };
  const unsupported = (account, provider, reason) =>
    ({ ...dr(account, provider, [], [reason], "unsupported"), notes_l10n: [coreNote(`access.reason.${provider}`, reason)], class: "unmeasured" });
  const perMinute = (label, used, limit) => metric(label, "units", { used, limit, kind: "quota", window: "other" });
  const pct = (label, used, window, resetH) => metric(label, "%", { used, limit: 100, kind: "quota", window, resets_at: h(resetH) });
  const balance = (label, value, unit = "USD") => metric(label, unit, { remaining: value, kind: "balance" });
  const spend = (label, used, extra = {}) => metric(label, "USD", { used, kind: "spend", window: "monthly", resets_at: h(300), ...extra });
  const cards = [
    [dr("api@example.com", "open_ai_api", [spend("Costs (current month)", 23.8),
      metric("Input tokens", "tokens", { used: 1250000, kind: "usage", window: "monthly", resets_at: h(300) })]), null],
    [demoBedrock(), null],
    [dr("voice@example.com", "eleven_labs", [metric("Plan credits", "credits", { used: 41200, limit: 100000, kind: "quota", window: "monthly", resets_at: h(300) }),
      metric("Custom voices", "voices", { used: 3, limit: 10, kind: "usage" })]), 0],
    [dr("project@example.com", "gemini_api", [perMinute("Gemini API: online_prediction_requests / per_minute (us-central1)", 12, 60),
      metric("Gemini API: recent online_prediction_requests (us-central1)", "units", { used: 9, kind: "usage", window: "other" })]), 0],
    [dr("project@example.com", "vertex_ai", [perMinute("Vertex AI: online_prediction_requests / per_minute (us-central1)", 40, 300)]), 0],
    [dr("deploy@example.com", "azure_open_ai", [perMinute("Tokens per minute (gpt-4o)", 12000, 150000)]), 0],
    [unsupported("key@example.com", "qwen", "The DashScope key only authenticates inference; there is no public quota query for it."), null],
    [unsupported("sonar@example.com", "perplexity", "The Sonar API has no public balance or usage query per key; Perplexity Computer uses the Analytics API (product = computer)."), null],
    [unsupported("worker@example.com", "cloudflare_ai", "Workers AI Neuron usage is only shown in the dashboard; there is no public query per key."), null],
    [dr("go@example.com", "open_code_go", [pct("Rolling (5h)", 18, "window_5h", 3.2), pct("Weekly", 42, "weekly", 90), pct("Monthly", 61, "monthly", 410)]), 2],
    [dr("dev@example.com", "qoder", [metric("Credits", "credits", { used: 320, limit: 2000, kind: "quota", window: "monthly", resets_at: h(520) })]), 0],
    [unsupported("nim@example.com", "nvidia_nim", "Billing lives in the NVIDIA Cloud account; there is no public balance query per inference key."), null],
    [dr("team@example.com", "cline", [balance("Credit balance", 18.4)]), null],
    [unsupported("studio@example.com", "alibaba_model_studio", "The Model Studio inference key gives no access to a public balance or quota query."), null],
    [dr("token@example.com", "alibaba_token_plan", [pct("Token Plan: 5h", 25, "window_5h", 2.1), pct("Token Plan: weekly", 48, "weekly", 120)]), 1],
    [dr("billing@example.com", "alibaba_cloud", [balance("Account balance", 120.5), spend("Spending this month", 34.2)]), null],
    [dr("team@example.com", "kimchi", [spend("Budget — shared team", 42.5, { limit: 100, window: "other", resets_at: null })]), 0],
    [dr("api@example.com", "meta", [spend("Spending", 12.3), metric("Requests", "requests", { used: 1840, kind: "usage", window: "monthly", resets_at: h(300) })]), null],
    [dr("org@example.com", "jetbrains_ai", [metric("Organization AI credits (included quota)", "credits", { used: 1200, limit: 5000, kind: "usage", window: "monthly", resets_at: h(300) })]), 0],
    [dr("key@example.com", "open_router", [metric("Key limit", "USD", { used: 12.4, limit: 50, kind: "quota", window: "monthly", resets_at: h(300) }), spend("Spending in the month", 12.4)]), 0],
  ];
  let rank = 40;
  for (const [r, binding] of cards) {
    if (demoReports.some((x) => x.provider === r.provider)) continue;
    demoReports.push(r);
    if (binding != null) DEMO_BINDINGS.set(r, [binding, rank++]);
  }
  if (!demoReports.some((r) => r.provider === "litellm")) {
    const budget = (used, limit) => [dm("budget (30d)", used, limit, 240, "spend", "monthly", "USD")];
    const conns = [["openai", "open_ai_api", 42.5, 100], ["anthropic", "anthropic_api", 180, 200]]
      .map(([upstream, mapped, used, limit], n) => demoConnection(`lit${n}`, upstream, upstream, mapped, budget(used, limit), {}, "gateway", "litellm"));
    demoReports.push(demoInstance("litellm", "gateway", conns, { host: "127.0.0.1:4000",
      notes: [coreNote("note.litellm.summary", `${conns.length} providers with a budget in LiteLLM.`, { count: String(conns.length) })] }));
  }
}
if (DEMO_QUERY.get("mono") && demoReports[0]) demoReports[0].monogram = DEMO_QUERY.get("mono");
// `?paste-session`: the core offers the MiMo account a new paste, so the card and the details show Paste session.
if (DEMO_QUERY.has("paste-session")) for (const r of demoReports) if (r.provider === "xiaomi_mimo") r.paste_session = true;
// PRD 0033 re-validation (V-15): the states of PRD 0033 the default demo lacks, first in the list so the captures show
// them (example.com only). `?renewed`: a preserved reading whose 5 h window renewed beside a current week (the card's
// renewed slot, RF2.1), a restored reading whose only quota renewed (the tray's "Renewed at …") and an exhausted binding
// week beside a paced 5 h window (the card's labelled pace line, RF4.1). `?lagging`: a fresh and an aging reading whose
// provider data is older than the query by more than 15 minutes (`data_as_of`, RF3). `?metered`: the AWS Bedrock
// account (`demoBedrock`). They combine with each other and with the default set.
const demoRenewed = (label, window, renewedMin) => ({ ...demoTexts(label, "%"), used: null, limit: null, remaining: null, kind: "quota", window, period: null,
  resets_at: ago(renewedMin), state: "unmeasured", ratio: null, uncapped: false, no_cap: false, expired: true });
const demoSpend = (label, used) => ({ label, used, limit: null, remaining: null, unit: "USD", kind: "spend", window: "monthly", period: null,
  resets_at: h(300), state: "available", ratio: null, uncapped: true, no_cap: false });
const demoFirst = [];
if (DEMO_QUERY.has("renewed")) {
  const mixed = { ...drn("mixed@example.com", "claude", [], demoNote("down", "Service unavailable right now."), "error"), checked_at: ago(1), plan: "Max",
    last_good: { checked_at: ago(50), metrics: [demoRenewed("session (5h)", "window_5h", 20), dm("weekly (7d)", 40, 100, 70, "quota", "weekly", "%")] } };
  const restored = { ...dr("renewed@example.com", "kimi", []), pending: true, class: "unmeasured", checked_at: ago(300),
    last_good: { checked_at: ago(300), metrics: [demoRenewed("Weekly", "weekly", 45)] } };
  const paceOther = { ...dr("pace@example.com", "claude", [dm("session (5h)", 50, 100, 2.5, "quota", "window_5h", "%"), dm("weekly (7d)", 100, 100, 30, "quota", "weekly", "%")]),
    class: "exhausted", plan: "Pro" };
  demoFirst.push(mixed, restored, paceOther);
  DEMO_BINDINGS.set(paceOther, [1, 0]);
  DEMO_WINDOWS.set(paceOther, [0, 1]);
  DEMO_PACES.set(paceOther, [0, { 0: dp("reserve", "good", 0.58, 0.5, 2.5) }]);
}
if (DEMO_QUERY.has("lagging")) {
  demoFirst.push({ ...dr("lagging@example.com", "open_ai_api", [demoSpend("Costs (month)", 12.4)]), data_as_of: ago(6 * 60 + 10) },
    { ...dr("aging@example.com", "open_ai_api", [demoSpend("Costs (month)", 3.1)]), checked_at: ago(25), freshness: "aging", data_as_of: ago(9 * 60) });
}
if (DEMO_QUERY.has("metered")) demoFirst.push(demoBedrock());
// PRD 0037 RF4: `?renewal-due`: a fresh reading whose exhausted week reset two hours before the provider answered (it had
// not renewed): the card, the tray and the details say "Renewal due since …", never a past "blocked until".
if (DEMO_QUERY.has("renewal-due")) {
  const due = { ...dr("renewal@example.com", "claude", [dm("weekly (7d)", 100, 100, -2, "quota", "weekly", "%"), dm("session (5h)", 20, 100, 3, "quota", "window_5h", "%")]),
    class: "exhausted", plan: "Pro" };
  demoFirst.push(due);
  DEMO_BINDINGS.set(due, [0, 0]);
  DEMO_WINDOWS.set(due, [1, 0]);
}
// PRD 0040 re-validation (V-10): agent-fed accounts as the backend reports them, first in the list (example.com only).
// `?feed=` takes a comma list: `fresh` (a Claude account fed by Claude Code, read a minute ago, and a hybrid one that
// fell back to the query), `stale` (its newest reading is two hours old), `absent` (no reading yet), `invalid` (a
// ChatGPT / Codex account fed by Codex CLI's logs in an unknown shape) and `one-window` (only the 5 h window: the week
// is unknown, not zero). `?live=N` gives the first fed Claude account (else the first Claude account) N live sessions,
// the context at 50, 75 and 95 % in turn (five shown, then "and N more"); `?feed-banner` shows the unbound Claude Code
// banner (`feed_banners`), whose Add account opens the Claude form prefilled with the Claude Code mode.
const DEMO_FEED = new Set((DEMO_QUERY.get("feed") ?? "").split(",").filter(Boolean));
const demoFed = (account, metrics, notes, extra = {}) => ({ ...dr(account, "claude", metrics), ...coreNotes(notes), source: "feed:claude_code",
  checked_at: ago(0), demo_access: "claude_code", demo_level: "official", ...extra });
const FED_NOTE = coreNote("note.claude_code.fed", "Fed by Claude Code's documented status line: no network request, no token reuse.");
// PRD 0040 re-validation V-09: the core's `provenance` of a ready fed report, aged from when Claude Code wrote it.
const demoAgeNote = (min) => min < 1 ? coreNote("freshness.age.now", "now")
  : min < 60 ? coreNote("freshness.age.minutes", `${min} min ago`, { n: String(min) })
  : coreNote("freshness.age.hours", `${Math.floor(min / 60)} h ago`, { n: String(Math.floor(min / 60)) });
const demoProvenance = (min) => {
  const age = demoAgeNote(min);
  return coreNote("feed.provenance.claude_code", `From Claude Code · ${age.fallback}`, { age });
};
const DEMO_FELL_BACK = coreNote("feed.provenance.api", "From the Claude query: no recent agent reading", { provider: "Claude" });
const demoFedWindows = () => [dm("session (5h)", 23, 100, 2.5, "quota", "window_5h", "%"), dm("weekly (7d)", 41, 100, 70, "quota", "weekly", "%")];
const demoFeedFirst = [];
if (DEMO_FEED.has("fresh")) {
  const fed = demoFed("fed@example.com", demoFedWindows(), [FED_NOTE], { data_as_of: null, measured_at: ago(1), provenance: demoProvenance(1) });
  const hybrid = { ...dr("hybrid@example.com", "claude", demoFedWindows()), ...coreNotes([coreNote("note.claude_code.answered_by_api",
    "Answered by the Claude query: no recent reading from Claude Code.")]), demo_access: "claude_code_or_api", demo_level: "official", plan: "Pro", provenance: DEMO_FELL_BACK };
  demoFeedFirst.push(fed, hybrid);
  for (const r of [fed, hybrid]) { DEMO_BINDINGS.set(r, [1, 0]); DEMO_WINDOWS.set(r, [0, 1]); }
}
if (DEMO_FEED.has("stale")) {
  // Queried now, the reading two hours old: the backend's `measured_at` is when Claude Code wrote it, and the reading
  // is presented as stale.
  const stale = demoFed("stale@example.com", demoFedWindows(), [FED_NOTE], { freshness: "stale", data_as_of: ago(120), measured_at: ago(120), provenance: demoProvenance(120) });
  demoFeedFirst.push(stale);
  DEMO_BINDINGS.set(stale, [1, 0]);
  DEMO_WINDOWS.set(stale, [0, 1]);
}
if (DEMO_FEED.has("absent")) {
  demoFeedFirst.push(demoFed("waiting@example.com", [], [coreNote("note.claude_code.no_feed",
    "No reading from Claude Code yet: add the status line (see docs) and send one prompt.")], { status: "needs_configuration", class: "unmeasured" }));
}
if (DEMO_FEED.has("invalid")) {
  demoFeedFirst.push({ ...demoFed("codex@example.com", [], [coreNote("query.schema", "Response is incompatible with the query interface; no value was estimated.")],
    { status: "error", class: "unmeasured" }), provider: "chat_gpt", source: "feed:codex_cli", demo_access: "codex_cli", demo_level: "undocumented" });
}
if (DEMO_FEED.has("one-window")) {
  const one = demoFed("one-window@example.com", [demoFedWindows()[0]], [FED_NOTE, coreNote("note.claude_code.window_absent",
    "Claude Code drops a window once it resets and sends it again after the next prompt; until then it is unknown, not zero.")],
    { provenance: demoProvenance(0) });
  demoFeedFirst.push(one);
  DEMO_BINDINGS.set(one, [0, 0]);
  DEMO_WINDOWS.set(one, [0]);
}
demoFirst.push(...demoFeedFirst);
const DEMO_LIVE = Number.parseInt(DEMO_QUERY.get("live") ?? "", 10);
if (DEMO_LIVE > 0) {
  const target = demoFeedFirst.find((r) => r.provider === "claude" && r.status === "ready") ?? demoReports.find((r) => r.provider === "claude");
  const ctx = (pct) => ({ context: coreNote("feed.live.ctx", `ctx ${pct}%`, { pct: String(pct) }), level: pct >= 90 ? "bad" : pct >= 70 ? "warn" : "good" });
  const sessions = Array.from({ length: Math.min(DEMO_LIVE, 5) }, (_, i) => ({
    model_name: ["Opus", "Sonnet", "Haiku"][i % 3], ...ctx([50, 75, 95][i % 3]),
    cost: coreNote("feed.live.cost", `≈ $${(0.42 * (i + 1)).toFixed(2)} this session`, { usd: demoMoney(Number((0.42 * (i + 1)).toFixed(2))) }),
    written_at: ago(i * 2) }));
  const more = DEMO_LIVE - sessions.length;
  if (target) target.live = { title: coreNote("feed.live.title", "Live session"), caveat: coreNote("feed.live.caveat", "Agent estimate at list price, not a charge."),
    more: more > 0 ? coreNote("feed.live.more", `and ${more} more`, { n: more }) : null, chip: sessions[0].context, chip_level: sessions[0].level, sessions };
}
const demoFeedBanners = () => (DEMO_QUERY.has("feed-banner") ? [{ source: "claude_code", provider: "claude", query_mode: "claude_code",
  text: coreNote("feed.banner.claude_code", "Claude Code is sending readings; add a Claude account fed by Claude Code."),
  action: coreNote("feed.banner.add", "Add account") }] : []);
// PRD 0041 re-validation (F-23): `?budget` adds the budget and floor states, shaped like the backend's views
// (`metric_view`, `report_view`): a spend account with a monthly budget and its allowance, a budget over its limit, a
// partial budget (part of its period unrecorded: `partial`, its used value is a lower bound), a budget in another
// currency than its metric (the core's note names both) and a usage metric that offers Set a budget…; a nearly used
// budget whose allowance reads "less than"; a balance with two floors; and a balance below a quarter of its floor.
// Every text is the core's (id, params, English fallback); the demo only carries them.
if (DEMO_QUERY.has("budget")) {
  const money = (v, currency = "USD") => demoMoney(v, currency);
  const usd = demoText("metric.verbatim", "USD");
  const monthEnd = (() => { const d = new Date(); return new Date(d.getFullYear(), d.getMonth() + 1, 1).toISOString(); })();
  const monthStart = (() => { const d = new Date(); return new Date(d.getFullYear(), d.getMonth(), 1).toISOString(); })();
  const spendLabel = demoText("metric.verbatim", "Spending this month");
  const budgetOf = demoText("metric.label.budget_of", "Budget · Spending this month", { label: spendLabel });
  const spendM = (used) => ({ label: "Gasto no mês", label_text: spendLabel, used, limit: null, remaining: null, unit: "USD", unit_text: usd,
    kind: "spend", window: "monthly", period: null, resets_at: monthEnd, state: "available", ratio: null, uncapped: true, no_cap: false,
    budget_draft: { kind: "spend", label_contains: "Spending this month", amount: 0, unit: "USD", period: "monthly" } });
  const budgetM = ({ label = "Orçamento · Gasto no mês", label_text = budgetOf, used, limit, unit = "USD", window = "monthly", resetH, allowance = null,
    partial = false, budget = 0 }) => ({
    label, label_text, used, limit, remaining: used == null ? null : Math.max(0, limit - used), unit, unit_text: demoText("metric.verbatim", unit),
    kind: "quota", window, period: monthStart, resets_at: resetH == null ? monthEnd : h(resetH), billing: "included", derived: true,
    derived_from: { kind: "spend", label: "Gasto no mês", budget, ...(partial ? { partial: true } : {}) }, ...(partial ? { partial: true } : {}),
    state: used == null ? "unmeasured" : used >= limit ? "exhausted" : "available", ratio: used == null ? null : Math.max(0, (limit - used) / limit),
    uncapped: false, no_cap: false, budget_badge: demoText("budget.badge", "Your budget"), ...(allowance ? { allowance } : {}) });
  const usageM = { label: "Requests today", label_text: demoText("metric.verbatim", "Requests today"), used: 1200, limit: null, remaining: null,
    unit: "requests", unit_text: demoText("metric.verbatim", "requests"), kind: "usage", window: "daily", period: null, resets_at: h(10),
    state: "available", ratio: null, uncapped: true, no_cap: false,
    budget_draft: { kind: "usage", label_contains: "Requests today", amount: 0, unit: "requests", period: "daily" } };
  const named = (id, fallback, params) => coreNote(id, fallback, { budget: "Budget · Spending this month", ...params });
  const spendAcc = { ...dr("budget@example.com", "open_ai_api", [
      budgetM({ used: 45, limit: 50, allowance: coreNote("pace.allowance.budget_day_recent_amount", "≈ $0.38/day left in the budget · recently $1.20/day",
        { value: money(0.38), recent: money(1.2) }) }),
      budgetM({ label: "Team cap", label_text: demoText("metric.verbatim", "Team cap"), used: 31, limit: 30, window: "weekly", resetH: 5 * 24, budget: 1 }),
      budgetM({ label: "Daily cap", label_text: demoText("metric.verbatim", "Daily cap"), used: 2.1, limit: 3, window: "daily", resetH: 14, partial: true, budget: 2 }),
      budgetM({ label: "Euro cap", label_text: demoText("metric.verbatim", "Euro cap"), used: null, limit: 40, unit: "EUR", budget: 3 }),
      spendM(45), usageM]),
    ...coreNotes([
      named("note.budget.partial", "Daily cap: part of this period's use was not recorded, so at least this much was used.", { budget: "Daily cap" }),
      named("note.budget.unit_mismatch_named", "Euro cap: the metric is reported in USD and this budget is in EUR, so it cannot count.",
        { budget: "Euro cap", metric_unit: "USD", budget_unit: "EUR" }),
    ]),
    // The TOML rows the account dialog opens with (`get_account`).
    demo_budgets: [
      { kind: "spend", amount: 50, unit: "USD", period: "monthly" },
      { kind: "spend", amount: 30, unit: "USD", period: "weekly", starts: "monday", label: "Team cap" },
      { kind: "spend", amount: 3, unit: "USD", period: "daily", label: "Daily cap" },
      { kind: "spend", amount: 40, unit: "EUR", period: "monthly", label: "Euro cap" },
    ] };
  const lowAcc = { ...dr("budget-low@example.com", "open_ai_api", [
      budgetM({ used: 49.995, limit: 50, allowance: coreNote("pace.allowance.budget_day_less", "less than $0.01/day left in the budget", { value: money(0.01) }) }),
      spendM(49.995)]),
    demo_budgets: [{ kind: "spend", amount: 50, unit: "USD", period: "monthly" }] };
  const bal = (left) => ({ label: "Saldo", label_text: demoText("metric.verbatim", "Balance"), used: null, limit: null, remaining: left, unit: "USD",
    unit_text: usd, kind: "balance", window: null, period: null, resets_at: null, state: "available", ratio: null, uncapped: true, no_cap: false,
    offers_floor: true });
  const cred = { label: "Créditos", label_text: demoText("metric.verbatim", "Credits"), used: null, limit: null, remaining: 74.75, unit: "credits",
    unit_text: demoText("metric.verbatim", "credits"), kind: "credits", window: null, period: null, resets_at: null, state: "available", ratio: null,
    uncapped: true, no_cap: false, offers_floor: true, budget_draft: { kind: "credits", label_contains: "Credits", amount: 0, unit: "credits", period: "monthly" } };
  const floor = (left, amount, role) => ({ label: "Saldo", label_text: demoText("metric.verbatim", "Balance"), left, amount, unit: "USD", role,
    chip: role === "ok" ? null : coreNote("budget.floor.chip", `Balance low · ${money(left).fallback} left`, { left: money(left) }),
    line: coreNote("budget.floor.line", `Floor ${money(amount).fallback} · ${money(left).fallback} left`, { floor: money(amount), left: money(left) }) });
  // Two floors on one balance: the higher one warns, the lower one is only a line.
  const floorAcc = { ...dr("floor@example.com", "open_router", [{ ...bal(7.5), offers_floor: false }, cred]), floors: [floor(7.5, 10, "warn"), floor(7.5, 5, "ok")],
    demo_floors: [{ kind: "balance", amount: 10, unit: "USD" }, { kind: "balance", amount: 5, unit: "USD" }] };
  const badAcc = { ...dr("floor-bad@example.com", "kimi", [{ ...bal(2), offers_floor: false }]), floors: [floor(2, 10, "bad")],
    demo_floors: [{ amount: 10, unit: "USD" }] };
  demoFirst.push(spendAcc, lowAcc, floorAcc, badAcc);
}
demoReports.unshift(...demoFirst);
// The core's binding of a demo connection: its most consumed quota (demo only; the app gets it from the core).
const demoConnectionBinding = (c) => {
  const ratio = (m) => (m.limit ? m.used / m.limit : 0);
  const index = c.metrics.reduce((best, m, i) => (best < 0 || ratio(m) > ratio(c.metrics[best]) ? i : best), -1);
  if (index < 0) return null;
  const m = c.metrics[index], used = Math.min(1, ratio(m));
  return { index, label: m.label, label_text: m.label_text, window: m.window, used_ratio: used, remaining: m.limit * (1 - used), resets_at: m.resets_at,
    reason: used >= 1 ? "exhausted" : "tightest", blocked_until: used >= 1 ? m.resets_at : null };
};
// Demo account names are catalog texts (`demo.account.*`), so every language shows its own; the report keeps the raw
// name for keys and look-ups, and only what the screens receive is translated.
const DEMO_NAMES = { Personal: "demo.account.personal", Work: "demo.account.work", Dev: "demo.account.dev", Restored: "demo.account.restored" };
const demoName = (name) => {
  const numbered = /^Account (\d+)$/.exec(name ?? "");
  if (numbered) return t("demo.account.numbered", { n: numbered[1] });
  return DEMO_NAMES[name] ? t(DEMO_NAMES[name]) : name;
};
// ---- PRD 0041 re-validation (F-05, F-07, F-10, F-11, F-25): the demo's mirror of `budget::editor::check_rows` ----
const DEMO_FIELD_LABELS = { kind: "Metric", amount: "Amount", unit: "Unit", period: "Period", starts: "Starts", label: "Name",
  label_contains: "Label contains", row: "Breakdown row" };
const demoFieldLabel = (path) => {
  if (path === "budgets") return demoText("budget.field.budgets", "Budgets");
  if (path === "floors") return demoText("budget.field.floors", "Balance floors");
  const key = path.split(".").pop();
  return DEMO_FIELD_LABELS[key] ? demoText(`budget.field.${key}`, DEMO_FIELD_LABELS[key]) : demoText("metric.verbatim", path);
};
/** The core's `text::number::parse_amount`, reduced: `{ value }`, `{ ambiguous: [whole, decimal] }` or `{}` (not an amount). */
function demoParseAmount(text) {
  const t = String(text ?? "").trim();
  const spaced = /[   ]/.test(t);
  const c = t.replace(/[   ]/g, "");
  if (!/^-?[\d.,]+$/.test(c)) return {};
  const seps = c.match(/[.,]/g) ?? [];
  const num = (int, frac) => { const v = Number(`${int || "0"}${frac ? `.${frac}` : ""}`); return Number.isFinite(v) ? { value: v } : {}; };
  if (!seps.length) return num(c, "");
  const last = Math.max(c.lastIndexOf("."), c.lastIndexOf(","));
  const [int, frac] = [c.slice(0, last), c.slice(last + 1)];
  if (seps.length === 1) {
    if (frac.length === 3 && !spaced && /[1-9]/.test(int)) {
      return { ambiguous: [Number(int + frac), Number(`${int}.${frac}`)] };
    }
    return num(int, frac);
  }
  const decimal = c[last];
  const both = new Set(seps).size === 2;
  if (both && seps.filter((x) => x === decimal).length !== 1) return {};
  const groups = (both ? int : c).split(/[.,]/);
  if (groups.slice(1).some((g) => g.length !== 3)) return {};
  return both ? num(groups.join(""), frac) : num(groups.join(""), "");
}
/** The core's `amount_input`: the UI language's decimal sign, no grouping, a fourth digit after exactly three. */
const demoAmountInput = (v, locale) => {
  const plain = v.toLocaleString("en-US", { useGrouping: false, maximumFractionDigits: 20 });
  const [int, frac] = plain.split(".");
  if (!frac) return int;
  const decimal = new Intl.NumberFormat(locale).formatToParts(1.5).find((p) => p.type === "decimal")?.value ?? ".";
  return `${int}${decimal}${frac.length === 3 ? `${frac}0` : frac}`;
};
const DEMO_TARGET_KINDS = { spend: "spend", account: "spend", usage: "usage", credits: "credits" };
function demoCheckBudgets({ index = null, budgets = [], floors = [], locale = "en-US" }) {
  const reading = index == null ? null : demoReports[index]?.metrics.filter((m) => !m.derived) ?? null;
  const metricOf = (row) => reading?.find((m) => m.kind === DEMO_TARGET_KINDS[row.target]) ?? null;
  const problems = [];
  const push = (field, rule, id, fallback, params = {}) => {
    const label = demoFieldLabel(field);
    problems.push({ field, rule, note: demoText(id, fallback.replace("{field}", label.fallback), { field: label, ...params }) });
  };
  if (budgets.length > 8) push("budgets", "too_many", "budget.error.too_many", "{field}: an account can have at most 8 budgets and 4 floors.");
  if (floors.length > 4) push("floors", "too_many", "budget.error.too_many", "{field}: an account can have at most 8 budgets and 4 floors.");
  const amounts = (list, rows) => rows.forEach((row, i) => {
    const field = `${list}[${i}].amount`;
    const read = demoParseAmount(row.amount);
    if (read.ambiguous) {
      const [whole, decimal] = read.ambiguous.map((v) => demoAmountInput(v, locale));
      const text = String(row.amount).trim();
      push(field, "amount_ambiguous", "budget.error.amount_ambiguous", `{field}: ${text} could be ${whole} or ${decimal}. Type ${whole} or ${decimal} instead.`,
        { text, whole, decimal });
    } else if (!(read.value > 0 && read.value <= 1e9)) {
      push(field, "amount", "budget.error.amount", "{field}: the amount must be greater than 0 and at most 1,000,000,000.");
    }
  });
  amounts("budgets", budgets);
  amounts("floors", floors);
  budgets.forEach((row, i) => {
    const unit = String(row.unit ?? "").trim();
    const m = metricOf(row);
    if (unit && m && !reading.some((x) => x.kind === m.kind && x.unit.toLowerCase() === unit.toLowerCase())) {
      push(`budgets[${i}].unit`, "unit_mismatch", "budget.error.unit_mismatch",
        `{field}: the metric is reported in ${m.unit} and this budget is in ${unit}; set it in ${m.unit}.`, { metric_unit: m.unit, budget_unit: unit });
    }
  });
  const units = budgets.map((row) => metricOf(row)?.unit ?? null);
  const sentences = budgets.map((row, i) => {
    const unit = String(row.unit ?? "").trim() || units[i] || "";
    const value = demoParseAmount(row.amount).value ?? 0;
    const amount = /^[A-Z]{3}$/.test(unit) ? demoMoney(value, unit) : `${value} ${unit}`.trim();
    const shown = typeof amount === "string" ? amount : amount.fallback;
    const PERIOD = { daily: "a day, from midnight", weekly: "a week, starting on {start}", monthly: "a month, starting on day {start}", window: "over the provider's own cycle" };
    const start = row.period === "weekly" ? demoText(`budget.weekday.${row.start ?? "monday"}`, (row.start ?? "monday").replace(/^./, (x) => x.toUpperCase()))
      : row.period === "monthly" ? String(row.start ?? 1) : null;
    const startText = typeof start === "string" ? start : start?.fallback;
    return demoText(`budget.describe.${row.period}`, `Cuotai will show a Budget quota of ${shown} ${PERIOD[row.period].replace("{start}", startText ?? "")}.`,
      { amount, ...(start ? { start } : {}) });
  });
  return { targets: ["spend", "usage", "credits", "account"], floor_targets: ["any", "balance", "credits"], problems, sentences, units };
}

const demoEraseWord = () => tl({ id: "privacy.erase.word", params: {}, fallback: "ERASE" });
// PRD 0052 re-validation V-04: `?skew` shows the core's clock advice while the clocks disagree.
const DEMO_SKEW = { id: "network.clock_ahead", params: { minutes: 12 }, fallback: "This computer's clock is about {minutes} min ahead of the providers' servers. Times, ages and resets may look wrong: turn on automatic date and time, then refresh." };
const DEMO_CONFIG_NOTICES = [
  { kind: "issue", text: { id: "config.account.requires_newer", params: { number: 3, path: "accounts.quota_mode" }, fallback: "Account 3 uses accounts.quota_mode, which this version of Cuotai does not know; it is skipped and the other accounts still work. Update Cuotai, or run cuotai --check-config to find a typo." } },
  { kind: "warning", text: { id: "config.warning.unknown_key", params: { path: "surprise" }, fallback: "surprise is not known to this version of Cuotai; it is ignored and kept when the file is saved." } },
];
const DEMO_RELOCATION = { id: "history.relocation.prompt", params: { path: "~/Documents/cuotai.toml", count: 1240, accounts: 3 },
  fallback: "The configuration file moved: ~/Documents/cuotai.toml no longer exists and its history (readings: 1240; accounts: 3) has exactly the accounts of this file. Use that history here?" };
// PRD 0051 re-validation V-04: `?datadir=env|error|pending` shows the data-folder row locked by CUOTAI_DATA_DIR, with
// an unusable chosen folder, or with a change that applies at the next start.
const DEMO_DATA_DIRS = {
  env: { data_dir: "/Users/demo/cuotai-data", data_dir_source: "environment", data_dir_custom: null, data_dir_env_locked: true, data_dir_error: null },
  error: { data_dir: null, data_dir_source: "desktop", data_dir_custom: "/Volumes/Gone/Cuotai", data_dir_env_locked: false,
    data_dir_error: { id: "data_dir.error.setting", params: {}, fallback: "The data folder chosen in Settings must be an absolute path to a directory of your own that is not a link; Cuotai keeps no data until it is fixed or reset." } },
  pending: { data_dir: "~/Library/Application Support/cuotai", data_dir_source: "default", data_dir_custom: "/Users/demo/Cuotai data", data_dir_env_locked: false, data_dir_error: null },
};
// Own keys only, copied, so a crafted `?datadir=__proto__` never reaches Object.prototype.
const DEMO_DATA_DIR = Object.hasOwn(DEMO_DATA_DIRS, DEMO_QUERY.get("datadir") ?? "") ? { ...DEMO_DATA_DIRS[DEMO_QUERY.get("datadir")] } : { data_dir: "~/Library/Application Support/cuotai", data_dir_source: "default", data_dir_custom: null, data_dir_env_locked: false, data_dir_error: null };
const demoView = () => {
  const now = new Date().toISOString();
  const reports = demoReports.map((r, index) => {
    const [paceIndex = null, paces = {}] = r.status === "ready" ? DEMO_PACES.get(r) ?? [] : [];
    const notify = !demoSettings.notify.off_accounts.includes(demoKey(r));
    const metrics = r.metrics.map((m, i) => ({ ...m, pace: paces[i] ?? null, forecast: demoForecast(m, paces[i]), resets_label: demoRenewalDue(r, m) ? demoRenewalDueLabel(r, m.resets_at) : m.resets_at && new Date(m.resets_at) > Date.now() ? demoResetsLabel(m.resets_at) : null, alert: demoMetricAlert(r, m, notify) }));
    const incident = demoIncident(r);
    // RF3 mirror: a failed query during an incident gains the core's note, its status unchanged.
    const note = incident && r.status !== "ready" && ["degraded", "partial_outage", "major_outage"].includes(incident.state) && DEMO_INCIDENT_NOTE;
    // PRD 0036 RF8: a connection keeps its aggregator's index (actions act on it) and its own core key.
    // PRD 0036 re-validation (V-06, V-14): each connection names its aggregator and points at the direct account it repeats.
    const connections = (r.connections ?? []).filter((c) => !demoConnFlags.hidden.has(c.demo_id))
      .map((c) => ({ ...c, connection: { ...c.connection, show_both: demoConnFlags.show_both.has(c.demo_id),
        duplicate_of: demoConnFlags.show_both.has(c.demo_id) ? null : c.connection.duplicate_of } }))
      .map((c) => ({ ...c, connection: { ...c.connection, aggregator: r.aggregator.kind, aggregator_provider: r.provider,
        duplicate_index: c.connection.duplicate_of ? demoReports.findIndex((x) => demoKey(x) === c.connection.duplicate_of) : null } }))
      .map((c) => ({ ...c, index, binding: demoConnectionBinding(c), binding_rank: 0, pace_index: null,
      metrics: c.metrics.map((m) => ({ ...m, pace: null, alert: null })), next_refresh_at_ms: Date.now() + 90e3, access: demoAccess(r),
      account_key: `${r.provider}:${r.account}/${c.demo_id}`, notify: true, usage_alerts: [], incident: null }));
    // PRD 0029 RF3.4: the backend's `named` and `display_name`.
    const named = Boolean((r.account ?? "").trim());
    return { ...r, ...(r.aggregator ? { aggregator: { ...r.aggregator, discovered_at: r.checked_at } } : {}), account: demoName(r.account), named, display_name: named ? demoName(r.account).trim() : providerName(r.provider), connections, index, binding: demoBinding(r), binding_rank: DEMO_BINDINGS.get(r)?.[1] ?? DEMO_BINDINGS.size + index,
      metrics, pace_index: paceIndex, windows: r.status === "ready" ? demoTrayPair(r) : null,
      next_refresh_at_ms: Date.now() + (40 + index * 37) * 1000, uses_oauth: r.provider === "kiro" || r.uses_oauth, access: demoAccess(r),
      // PRD 0025 re-validation V-19: the stable id the real backend sends (the popover's menu-bar split reads it).
      account_id: demoAccountId(index), account_key: demoKey(r), notify, usage_alerts: metrics.filter((m) => m.alert?.active).map((m) => m.label_text ?? m.label), incident,
      // PRD 0022 MONO-4: the shell adds the override kept by account id in desktop.json.
      ...(demoSettings.monograms[demoAccountId(index)] ? { monogram: demoSettings.monograms[demoAccountId(index)] } : {}),
      ...(note ? { notes: [...r.notes, note.fallback], notes_l10n: [...r.notes.map((n, i) => r.notes_l10n?.[i] ?? null), note] } : {}) };
  });
  return { config_path: demoSettings.config ?? "~/.config/cuotai/config.toml (demo)", config_source: demoSettings.config ? "chosen" : "system", refresh_seconds: demoSettings.refresh ?? 300, config_exists: true, account_count: demoReports.length, loaded: true,
    started_at: now, finished_at: now, reports, busy: [], error: null, generation: demoSettings.generation,
    // `?history-warning` shows the size-ceiling warning (PRD 0006 RF4.3) the core would send.
    history_warning: new URLSearchParams(location.search).has("history-warning")
      ? { id: "history.warning.ceiling", params: { max: "64", removed: "1200" }, fallback: "The history went over 64 MiB: its 1200 oldest measurements were deleted." }
      : null,
    // `?no-tray` shows the notice for a system without a tray icon (PRD 0054 RF5.1).
    tray_unavailable: DEMO_QUERY.has("no-tray"),
    history_epoch: demoSettings.historyEpoch,
    // PRD 0051 re-validation V-04: `?confignotes` shows an account set aside and an unknown key kept (the core's notes);
    // `?relocation` the moved-file question until it is answered.
    config_notices: DEMO_QUERY.has("confignotes") ? DEMO_CONFIG_NOTICES : [],
    clock_skew: DEMO_QUERY.has("skew") ? DEMO_SKEW : null,
    history_relocation: DEMO_QUERY.has("relocation") && !demoSettings.relocationAnswered ? DEMO_RELOCATION : null,
    feed_banners: demoFeedBanners(),
    notify: { enabled: demoSettings.notify.enabled, next: demoNextNotice(reports), alerts: demoSettings.alerts.enabled },
    status: { enabled: demoSettings.statusQueries, domains: [...new Set(demoReports.map((r) => DEMO_STATUS_HOSTS[r.provider]).filter(Boolean))],
      outage: reports.some((r) => ["partial_outage", "major_outage"].includes(r.incident?.state)) },
    serve: demoServeView(), hooks: { state: demoHooks.enabled ? demoHooksView().state : "off" },
    tray: { style: demoSettings.tray.style, selected: demoTraySelection().map((a) => a.key), popover_view: demoSettings.tray.popover_view ?? "list" } };
};
// ponytail: a rough demo of the core's `tray::summary` (PRD 0025 RF1); the app always asks the core.
// PRD 0036 RF5.3 (V-05): demo mirror of the core's `tray::summary_units`. An aggregator gives its active connections
// (on in the aggregator, not a duplicate, not kept in both places), or itself when it failed or has none; Settings ›
// "Aggregator connections in All: Hidden" leaves every aggregator out (`tray::without_aggregators`).
const demoTrayAccounts = () => demoReports.flatMap((r, index) => {
  const b = demoBinding(r);
  const own = { key: demoAccountId(index), provider: r.provider, provider_label: brand(r.provider).label, name: demoName(r.account),
    remaining: b ? Math.max(0, 1 - b.used_ratio) : null, exhausted: b?.reason === "exhausted" };
  if (!r.aggregator) return [own];
  if (aggMode === "hidden") return [];
  const flags = demoConnFlags;
  const conns = r.status !== "ready" ? [] : (r.connections ?? []).filter((c) => !flags.hidden.has(c.demo_id) && !flags.show_both.has(c.demo_id)
    && c.connection.enabled_upstream !== false && !c.connection.duplicate_of);
  if (!conns.length) return [{ ...own, remaining: null, exhausted: false, stale: r.status !== "ready" }];
  return conns.map((c) => {
    const cb = demoConnectionBinding(c);
    return { key: `${own.key}/${c.demo_id}`, parent: own.key, provider: c.provider, provider_label: brand(c.provider).label, name: c.account,
      remaining: cb ? Math.max(0, 1 - cb.used_ratio) : null, exhausted: cb?.reason === "exhausted", stale: c.status !== "ready" };
  });
});
function demoTraySelection(style = demoSettings.tray.style, accounts = demoSettings.tray.accounts) {
  const all = demoTrayAccounts();
  // An aggregator's id that is not an item picks its connections (`TraySelection::pick`).
  if (accounts) return [...new Set(accounts.flatMap((k) => { const a = all.find((x) => x.key === k); return a ? [a] : all.filter((x) => x.parent === k); }))];
  return all.slice(0, style === "meters" ? 4 : style === "logos_percent" ? 6 : all.length);
}
function demoTrayPreview({ style, accounts }) {
  const picked = demoTraySelection(style, accounts);
  const pct = (a) => (a.remaining == null ? "—" : fmtPercent(Math.round(a.remaining * 100)));
  const items = picked.map((a) => ({ ...a, level: a.remaining == null ? null : level(a.remaining * 100), figure: pct(a) }));
  const tight = [...items].filter((a) => a.remaining != null).sort((a, b) => a.remaining - b.remaining)[0];
  const title = style === "tightest_percent" ? (tight ? `${tight.figure} · ${tight.name}`.slice(0, 24) : items.length ? "—" : null)
    : style === "logos_percent" ? items.slice(0, 3).map((a) => `${a.provider_label[0].toUpperCase()} ${a.figure}`).join(" · ") : null;
  // V-12: the core's `tray::summary` reads each account's display name (its provider's when unnamed).
  const label = items.map((a) => `${titleParts(a.provider, a.name)[0]} ${a.figure}`).join("; ");
  return { style, items: style === "meters" ? items.slice(0, 4) : items, hidden: 0, title, badge: "none",
    accessible_label: { id: "demo", params: {}, fallback: label }, tooltip: { id: "demo", params: {}, fallback: `Cuotai — ${label}` } };
}
// ponytail: demo mirror of the backend's local server state (PRD 0012 RF5); nothing listens in the browser.
// `?serve=taken` plays port 6738 held by a `cuotai --serve` instance (the core's probe says so in the app).
const DEMO_SERVE_TAKEN = new URLSearchParams(location.search).get("serve") === "taken";
function demoServeView() {
  const s = demoSettings.serve;
  const taken = s.enabled && DEMO_SERVE_TAKEN && s.port === 6738;
  const state = !s.enabled ? "off" : taken ? "error" : "on";
  return { state, enabled: s.enabled, port: s.port, allow_browsers: s.allow_browsers, token: s.secret != null, stats: !!s.stats,
    allowed_origins: s.allowed_origins ?? [], origins_notice: false, token_notice: false,
    access_log: s.access_log ?? "off", access_log_bytes: s.access_log_bytes ?? 0,
    url: state === "on" ? `http://127.0.0.1:${s.port}` : null,
    error: taken ? { id: "serve.error.port_taken", params: { port: s.port }, fallback: `Port ${s.port} is in use.` } : null,
    cuotai_on_port: taken, default_port: 6738, port_min: 1024, port_max: 65535,
    browsers_warning: { id: "serve.allow_browsers.warning", params: {}, fallback: "With browsers allowed, the pages of the trusted origins that know the access token can read your usage from the local server. Add only origins you control." } };
}
// ponytail: demo mirror of the widget snapshot (PRD 0014 RF4.1); nothing is written in the browser. `?widgets`
// starts with publishing on, so the file path and the macOS prerequisites are visible as in the app.
const DEMO_WIDGETS = new URLSearchParams(location.search).has("widgets");
function demoWidgetsView() {
  const w = demoSettings.widgets;
  return { prefs: { enabled: w.enabled, include_identity: w.include_identity, max_accounts: w.max_accounts },
    path: "~/.local/share/cuotai/widget.json (demo)", extension_reads: DEMO_WIDGETS, macos: true, accounts_min: 1, accounts_max: 24 };
}
// ponytail: demo mirror of the backend's hooks (PRD 0013 RF4) with the core's validation, listing and digest reduced
// to what the screen needs; nothing runs in the browser: a test logs a made-up successful run after a moment.
// `?hooks=changed` starts with the hooks on and a rule changed since its confirmation (the main-window banner).
const DEMO_HOOK_EVENTS = ["quota_threshold_crossed", "quota_exhausted", "quota_recovered", "quota_reset", "account_blocked",
  "account_unblocked", "refresh_failed", "refresh_recovered", "auth_required", "provider_incident", "usage_updated", "overage_started", "overage_ended", "session_expiring", "quota_reset_soon",
  "pool_exhausted", "pool_recovered", "balance_below_floor", "balance_recovered", "limit_changed", "notification_sent"];
const DEMO_HOOKS_CHANGED = new URLSearchParams(location.search).get("hooks") === "changed";
const demoHooks = { enabled: DEMO_HOOKS_CHANGED, path: "~/.config/cuotai/hooks.toml (demo)", revision: null, confirmed: null, runs: [],
  rules: DEMO_HOOKS_CHANGED ? [{ name: "sound", events: ["quota_exhausted"], command: ["/usr/bin/afplay", "/System/Library/Sounds/Glass.aiff"] }] : null };
if (DEMO_HOOKS_CHANGED) demoHooks.revision = "r1";
const demoHookListeners = new Set();
const demoDigest = (rules) => [...JSON.stringify(rules)].reduce((h, c) => (Math.imul(h, 31) + c.charCodeAt(0)) >>> 0, 7).toString(16);
const demoL10n = (id, params, fallback) => ({ id, params, fallback });
function demoListing(rules) {
  const lines = [demoL10n("hooks.confirm.header", { path: demoHooks.path }, `Os hooks de ${demoHooks.path} executam estes comandos quando os eventos indicados acontecem:`)];
  rules.forEach((r, i) => {
    const command = JSON.stringify(r.command), events = r.events.join(", ");
    lines.push(demoL10n("hooks.confirm.rule", { index: i + 1, name: r.name }, `${i + 1}. ${r.name}`),
      demoL10n("hooks.confirm.events", { events }, `events: ${events}`),
      demoL10n("hooks.confirm.timeout", { seconds: r.timeout_seconds ?? 10 }, `time limit: ${r.timeout_seconds ?? 10} s`),
      demoL10n("hooks.confirm.command", { command }, `command: ${command}`));
  });
  return lines;
}
/** The core's `hooks.error.*` for the cases the editor can produce. */
function demoHooksError(rules) {
  if (!rules.length) return demoL10n("hooks.error.empty", {}, "hooks.toml has no [[hooks]] rule.");
  const seen = new Set();
  for (const [i, r] of rules.entries()) {
    const name = r.name.trim();
    if (!name || name.length > 64) return demoL10n("hooks.error.name", { index: i + 1, max: 64 }, `A regra ${i + 1} precisa de um name de 1 a 64 caracteres, sem caracteres de controle.`);
    if (!r.events.length) return demoL10n("hooks.error.no_events", { name }, `Rule "${name}" lists no event.`);
    const program = r.command[0] ?? "";
    if (!program || !(program.startsWith("/") || !program.includes("/"))) return demoL10n("hooks.error.command", { name }, `The command of rule "${name}" must be a list of arguments whose first item is an absolute path or a name looked up in PATH, such as ["/path/script", "--flag"].`);
    if (seen.has(name)) return demoL10n("hooks.error.duplicate", { name }, `More than one rule is named "${name}".`);
    seen.add(name);
  }
  return null;
}
function demoHooksView() {
  const h = demoHooks, rules = h.rules;
  const digest = rules ? demoDigest(rules) : null, confirmed = digest != null && h.confirmed === digest;
  const state = !rules ? "missing" : !h.enabled ? "off" : confirmed ? "on" : "unconfirmed";
  return { state, enabled: h.enabled, path: h.path, revision: h.revision, rules: structuredClone(rules ?? []), error: null, confirmed,
    digest: rules && !confirmed ? digest : null, listing: rules && !confirmed ? demoListing(rules) : [],
    running: state === "on" ? rules.map((r) => r.name) : [], runs: h.runs.slice(0, 20), warnings: [], events: DEMO_HOOK_EVENTS,
    default_timeout: 10, max_timeout: 60 };
}
// ponytail: demo mirror of the core's provider statuses (PRD 0011 RF4); the app reads them from the backend. `?status`
// starts with the queries on and incidents simulated; `?status=offline` plays sources that could not be read.
const DEMO_STATUS = new URLSearchParams(location.search).get("status");
const DEMO_STATUS_HOSTS = { claude: "status.claude.com", anthropic_api: "status.claude.com", chat_gpt: "status.openai.com",
  copilot: "www.githubstatus.com", groq: "groqstatus.com", mini_max: "status.minimax.io", kimi: "status.moonshot.cn",
  cursor: "status.cursor.com", windsurf: "status.windsurf.com", grok: "status.x.ai", eleven_labs: "status.elevenlabs.io" };
// PRD 0037 re-validation (V-01, V-09): ElevenLabs has one status page per region (`incidents::source_for`), so each
// account reads its own source; the demo plays an incident on the EU page only.
const DEMO_ELEVEN_SOURCES = { default: "united-states", us: "united-states", eu: "eu", in: "india", sg: "singapore" };
const DEMO_ELEVEN_INCIDENTS = { eu: ["partial_outage", "Elevated error rates on the API (EU)", "https://status.elevenlabs.io/eu/incidents/demo5", 0.6] };
const DEMO_STATE_EN = { operational: "Operational", degraded: "Degraded performance", partial_outage: "Partial outage",
  major_outage: "Major outage", maintenance: "Under maintenance", unknown: "Unknown status" };
const DEMO_INCIDENTS = {
  claude: ["partial_outage", "Elevated errors on claude.ai & Claude Code", "https://status.claude.com/incidents/demo1", 0.8],
  cursor: ["major_outage", "Cursor IDE requests failing", "https://status.cursor.com/incidents/demo2", 0.3],
  groq: ["degraded", "Increased latency on the API", "https://groqstatus.com/incident/demo3", 2],
  kimi: ["maintenance", "Scheduled database maintenance", "https://status.moonshot.cn/incidents/demo4", 0.5],
  grok: ["unknown"],
};
const DEMO_INCIDENT_NOTE = { id: "note.incident.ongoing", params: {}, fallback: "The provider reports an ongoing incident; the failure may not be from your account." };
function demoIncident(r) {
  const host = DEMO_STATUS_HOSTS[r.provider];
  if (!demoSettings.statusQueries || !host) return null;
  const offline = DEMO_STATUS === "offline";
  const source = r.provider === "eleven_labs" ? DEMO_ELEVEN_SOURCES[r.demo_region ?? "default"] : null;
  const [state, title, url, startedH] = offline ? ["unknown"] : (source ? DEMO_ELEVEN_INCIDENTS[source] : DEMO_INCIDENTS[r.provider]) ?? ["operational"];
  return { provider: r.provider, source, state, label: dl(`incident.state.${state}`, DEMO_STATE_EN[state]), page_url: `https://${host}/${source ?? ""}`,
    incident: title ? { title, started_at: h(-startedH), url } : null, read_at: offline ? null : ago(2), stale: offline };
}
// ponytail: demo mirror of `alerts::eligible` and of an active alert (a capped quota at or above the lowest
// threshold, bell and metric not muted); the app reads both from the backend's alert state.
function demoMetricAlert(r, m, notify) {
  const a = demoSettings.alerts;
  const used = m.used ?? (m.limit != null && m.remaining != null ? m.limit - m.remaining : null);
  // `?missed` also turns the alerts on, so the marker can be seen without touching the settings first.
  if (!(a.enabled || new URLSearchParams(location.search).has("missed")) || r.status !== "ready" || m.kind !== "quota" || !(m.limit > 0) || used == null) return null;
  const muted = (a.muted[demoKey(r)] ?? []).includes(m.label);
  const pct = (used / m.limit) * 100;
  // PRD 0032 RF8: `?missed` plays a first reading above a threshold that nobody was told about (until opened).
  const reached = pct >= 100 ? 100 : Math.max(0, ...a.thresholds.filter((t) => pct >= t));
  const missed = new URLSearchParams(location.search).has("missed") && reached > 0 && !demoSettings.missedSeen.has(demoKey(r)) ? { missed: reached } : {};
  return { muted, active: notify && !muted && (pct >= 100 || a.thresholds.some((t) => pct >= t)), ...missed };
}
// ponytail: demo mirror of `Account::key` and of the core planner's next notice (first future quota reset with a
// window, "always" filter, lead applied); the app reads both from the backend.
const demoKey = (r) => `${r.provider}:${r.account}`;
function demoNextNotice(reports) {
  const n = demoSettings.notify;
  if (!n.enabled) return null;
  const lead = n.lead_minutes * 60000;
  const next = reports.filter((r) => r.status === "ready" && r.notify)
    .flatMap((r) => r.metrics.filter((m) => m.kind === "quota" && m.window && (m.window !== "other" || n.include_other) && m.resets_at)
      .map((m) => ({ index: r.index, provider: r.provider, account: r.account, window: m.window, label: m.label, label_text: m.label_text, resets_at: m.resets_at, at: new Date(new Date(m.resets_at).getTime() - lead).toISOString() })))
    .filter((e) => new Date(e.at).getTime() > Date.now())
    .sort((a, b) => a.at.localeCompare(b.at, "en")); // ISO instants: a fixed, language-free order
  return next[0] ?? null;
}
// The core's summaries of the agent-fed modes (`access::classify`), English fallbacks of their catalog texts.
const DEMO_FEED_ACCESS = {
  claude_code: "Claude plan windows handed over by Claude Code's documented status line, with no network request and no token reuse.",
  claude_code_or_api: "Claude Code's documented status line while it is recent, otherwise the internal interface the official clients use.",
  codex_cli: "Codex limit windows read from Codex CLI's own local session logs, with no network request.",
};
// ponytail: demo mirror of `access::describe` + `UsageAccess::explain` (default modes, generic pt summary and
// evidence); the app reads both from the core.
const DEMO_ROUTER = "https://github.com/decolua/9router/blob/f01fb909e37189008080632ddaf404f096345cde/open-sse/services/usage/";
function demoAccess(r) {
  const level = r.demo_level ?? demoLevelOf(r.provider);
  // PRD 0040: an agent-fed mode has its own access entry (`access.summary.claude_code`, the status-line page).
  const slug = r.demo_access ?? r.provider;
  const reason = level === "unavailable" ? dlx(`access.reason.${slug}`, "demo.access.reason", "No public usage query for this credential (demo).") : null;
  const url = slug === "claude_code" || slug === "claude_code_or_api" ? "https://code.claude.com/docs/en/statusline"
    : slug === "codex_cli" ? "https://github.com/openai/codex/blob/86ac0916597244a2e67a197822a5483924660ff1/codex-rs/protocol/src/protocol.rs"
    : level === "undocumented" ? `${DEMO_ROUTER}${r.provider}.js` : "https://example.com/docs (demo)";
  const changed = level === "undocumented" && r.status === "error" && r.notes_l10n?.some((n) => n.id === "query.schema")
    ? { id: "access.interface_changed", params: {}, fallback: "The response does not follow the known internal interface: it may have changed without notice." }
    : null;
  return { level, label: demoLevel(level), summary: DEMO_FEED_ACCESS[slug] ? dl(`access.summary.${slug}`, DEMO_FEED_ACCESS[slug]) : dlx(`access.summary.${slug}`, "demo.access.summary", "Demo usage query."), reason, metered: r.provider === "aws_bedrock",
    evidence: { kind: level === "undocumented" ? "reference" : "documentation", url },
    explanation: r.status === "ready" ? null : reason ?? changed, reference: changed ? url : null,
    reference_label: { id: "access.reference", params: {}, fallback: "Reference contract" }, session: demoSessionNotice(demoSessionState(r)) };
}
// PRD 0016 RF4 demo: the MiMo account reads its platform session from a variable until it is migrated (or pasted), then
// from the vault until it is forgotten; the card mirrors `user_session::notice`.
const demoSessionState = (r) => r.provider !== "xiaomi_mimo" || demoSettings.sessionForgotten.has(r) ? null
  : demoSettings.sessionMigrated.has(r) ? "vault" : "env";
const demoSessionNotice = (state) => state && { level: dl("access.credential.user_session", "User session"),
  warning: state === "vault"
    ? dl("user_session.warning", "This is a full login session. Cuotai stores it in the system vault and uses it only to query this service.")
    : dl("user_session.warning_env", "This is a full login session. Cuotai reads it from an environment variable and uses it only to query this service; move it to the system vault in the account wizard.") };
// The demo's stand-in for `desktop.json` `dashboard.hidden_accounts`, shared by the demo's windows.
const DEMO_HIDDEN = "cuotai.demo.hiddenAccounts";
const demoHidden = () => { try { const v = JSON.parse(localStorage.getItem(DEMO_HIDDEN) || "[]"); return Array.isArray(v) ? v : []; } catch { return []; } };
const DEMO_ROWS = ["minimal", "detailed"].find((p) => new URLSearchParams(location.search).get("rows") === p) ?? null;
function demoTraySel() {
  const n = Number(new URLSearchParams(location.search).get("traysel"));
  if (!Number.isInteger(n) || n <= 0) return null;
  return demoReports.map((_, index) => demoAccountId(index)).slice(-n).reverse();
}
/** PRD 0032 re-validation R-03: `?quiet` starts with quiet hours on (weeknights, longer weekend nights and a Monday
 *  lunch break), `?quiet=always` with every day whole (the always-on note), so the harness can draw the sub-page. */
function demoQuiet() {
  const mode = new URLSearchParams(location.search).get("quiet");
  if (mode == null) return { enabled: false, days: {}, digest: true };
  const days = ["mon", "tue", "wed", "thu", "fri", "sat", "sun"];
  if (mode === "always") return { enabled: true, digest: true, days: Object.fromEntries(days.map((d) => [d, [{ from: "00:00", to: "00:00" }]])) };
  const night = (d) => (d === "sat" || d === "sun" ? { from: "23:00", to: "09:00" } : { from: "22:00", to: "07:00" });
  const out = Object.fromEntries(days.map((d) => [d, [night(d)]]));
  out.mon.push({ from: "12:00", to: "13:00" });
  return { enabled: true, digest: true, days: out };
}
/** The demo's twin of the core's strict quiet-hours reading (`QuietSchedule::from_value_strict`, PRD 0032
 *  re-validation C-06): the refusal as the JSON object `set_quiet_hours` sends, or null. */
function demoQuietRefusal(quiet) {
  const keys = ["mon", "tue", "wed", "thu", "fri", "sat", "sun"];
  const time = /^([01]\d|2[0-3]):[0-5]\d$/;
  if (typeof quiet?.enabled !== "boolean" || typeof quiet?.digest !== "boolean") return { code: "invalid_quiet" };
  const span = (iv) => {
    const m = (x) => Number(x.slice(0, 2)) * 60 + Number(x.slice(3));
    return [m(iv.from), m(iv.to) > m(iv.from) ? m(iv.to) : m(iv.to) + 1440];
  };
  for (const [day, list] of Object.entries(quiet.days ?? {})) {
    if (!keys.includes(day)) return { code: "invalid_quiet" };
    if (!Array.isArray(list) || list.length > 3) return { code: "invalid_quiet", day };
    for (const [index, iv] of list.entries()) {
      for (const field of ["from", "to"]) if (!time.test(iv?.[field])) return { code: "invalid_quiet_time", day, index, field };
      if (iv.from === iv.to && iv.from !== "00:00") return { code: "invalid_quiet_empty", day, index, field: "to" };
      const [a, b] = span(iv);
      for (const [other, earlier] of list.slice(0, index).entries()) {
        if (earlier.from === iv.from && earlier.to === iv.to) return { code: "invalid_quiet_duplicate", day, index, field: "from", with: other };
        const [c, d] = span(earlier);
        if (a < d && c < b) return { code: "invalid_quiet_overlap", day, index, field: "from", with: other };
      }
    }
  }
  return null;
}
const demoSettings = { monograms: {}, profileExisted: new URLSearchParams(location.search).has("profile-existed"), whatsNewDue: new URLSearchParams(location.search).has("whats-new"), // Re-validation V-19: `?traysel=N` starts with a custom menu-bar selection of the last N accounts (reversed), and
  // `?rows=minimal|detailed` with that row display, so the harness can draw them.
  tray: { style: "tightest_percent", colored: false, accounts: demoTraySel(), popover_view: null }, advancedOpen: false,
  rows: DEMO_ROWS ? { tray: { preset: DEMO_ROWS } } : undefined, stats: {},
  // `?first-run` shows the first-run step (PRD 0025 RF6) in the demo.
  onboarding: new URLSearchParams(location.search).has("first-run") ? 0 : 1,
  custom: null, config: null, generation: 1, historyRecording: true, historyEpoch: 0, historyErased: new Set(), sessionMigrated: new Set(), sessionForgotten: new Set(),
  notify: { enabled: false, lead_minutes: 0, when: "used", include_other: false, off_accounts: [], private: true, days_off: ["sat", "sun"], limit_changes: false, digest: { enabled: false, weekday: 0, hour: 9 } },
  quiet: demoQuiet(), missedSeen: new Set(),
  alerts: { enabled: false, thresholds: [75, 90], exhaustion: true, recovery: true, overage: true, muted: {} }, statusQueries: DEMO_STATUS != null || DEMO_QUERY.has("eleven"), detect: { enabled: new URLSearchParams(location.search).get("detect") === "ask" ? null : true, dismissed: [] },
  serve: { enabled: DEMO_SERVE_TAKEN, port: 6738, allow_browsers: false, secret: DEMO_SERVE_TAKEN ? "cuotai_demo_taken" : null },
  widgets: { enabled: DEMO_WIDGETS, include_identity: false, max_accounts: 6, written: false } };
// `?autostart-stale` starts with Open at login on and its entry naming a copy of the app that moved (PRD 0054 RF4.3).
if (DEMO_QUERY.has("autostart-stale")) demoSettings.autostart = demoSettings.autostartStale = true;
const demoPermission = (on) => (!on ? "off" : new URLSearchParams(location.search).has("notify-denied") ? "denied" : "granted");
// Models in the demo: same validation (index + generation) and the `unsupported` state with an explicit demonstration reason;
// no invented catalog is shown as real. `?models=failed` (the ui-shots overflow check) shows a failed sync over a catalog
// whose ids and source say it is a demo fixture, so the sync bar and the status role render.
const DEMO_MODELS_FAILED = new URLSearchParams(location.search).get("models") === "failed";
const demoModels = async ({ index, generation }) => {
  if (generation !== demoSettings.generation) throw STALE;
  if (!demoReports[index]) throw JSON.stringify({ id: "models.error.invalid_index", params: {}, fallback: "Account does not exist in this configuration." });
  const demo = { id: "models.demo", params: {}, fallback: "Browser demo: the model catalog is only queried in the desktop app." };
  if (!DEMO_MODELS_FAILED) return { state: "unsupported", reason: demo };
  const at = new Date(Date.now() - 2 * 3600e3).toISOString();
  const previous = { models: ["demo-model-a", "demo-model-b", "demo-model-c"].map((id) => ({ id, name: null, description: null })), source: demo, fetched_at: new Date(Date.now() - 3 * 864e5).toISOString() };
  return { state: "failed", status: "authentication_error", error: { id: "query.http", params: { code: 401 }, fallback: "Query returned HTTP 401. Check access and permissions." }, at, previous };
};
// ponytail: demo mirror of `HistoryStore::account_history` + `MetricHistory::{trend, chart}` over synthetic samples:
// every 20 minutes for 10 days with a day missing (days 6 to 5 ago), 5-hour windows that refill, weekly and longer
// windows that only grow. Accounts without metrics (Groq) have none: the empty state. The app reads the real file.
const DEMO_HISTORY_CELLS = 96;
const SPANS = { day: 1, week: 7, month: 30 };
const DAY_MS = 864e5;
const demoWave = (seed) => 0.55 + 0.45 * Math.sin(seed * 12.9898) * Math.sin(seed * 78.233);
function demoSamples(m, metricIndex, now) {
  if (m.used == null) return [];
  const limit = m.limit || 100, out = [];
  const windowMs = { window_5h: 5 * 36e5, weekly: 7 * DAY_MS, monthly: 30 * DAY_MS }[m.window] ?? 30 * DAY_MS;
  for (let at = now - 10 * DAY_MS; at <= now; at += 20 * 6e4) {
    if (at > now - 6 * DAY_MS && at < now - 5 * DAY_MS) continue; // the computer was off
    const start = Math.floor(at / windowMs) * windowMs;
    const peak = Math.min(1, demoWave(start / windowMs + metricIndex)) * limit;
    out.push({ taken_at: at, used: Math.round(peak * ((at - start) / windowMs) * 10) / 10 });
  }
  return out;
}
function demoChart(points, limit, from, to, windows) {
  const span = to - from, heights = new Array(DEMO_HISTORY_CELLS).fill(null);
  const used = points.map((p) => p.used);
  const [min, max] = used.length ? [Math.min(...used), Math.max(...used)] : [0, 0];
  const [floor, ceiling] = limit > 0 ? [Math.min(min, 0), Math.max(limit, max)] : [min, max];
  const height = (v) => (ceiling > floor ? Math.min(1, Math.max(0, (v - floor) / (ceiling - floor))) : 0);
  let previous = null; // readings at most an hour apart carry the earlier value across empty cells, as the core
  for (const p of points) {
    const cell = Math.min(DEMO_HISTORY_CELLS - 1, Math.floor(((p.taken_at - from) * DEMO_HISTORY_CELLS) / span));
    if (previous && p.taken_at - previous.at <= 36e5) for (let c = previous.cell + 1; c < cell; c++) heights[c] = height(previous.used);
    heights[cell] = Math.max(heights[cell] ?? -Infinity, height(p.used));
    previous = { at: p.taken_at, cell, used: p.used };
  }
  const last = points.at(-1);
  return { floor, ceiling, heights,
    last: last ? { taken_at: new Date(last.taken_at).toISOString(), used: last.used, x: (last.taken_at - from) / span, y: height(last.used) } : null,
    resets: windows.slice(1).map((w) => ({ at: w.started_at, x: (Date.parse(w.started_at) - from) / span })) };
}
function demoWindows(points, limit) {
  const windows = [];
  for (const p of points) {
    const w = windows.at(-1);
    const iso = new Date(p.taken_at).toISOString();
    if (!w || (limit > 0 && w.final_used - p.used > 0.2 * limit)) {
      windows.push({ started_at: iso, ended_at: iso, resets_at: null, limit, initial_used: p.used, final_used: p.used, peak_used: p.used, peak_at: iso, samples: 1 });
      continue;
    }
    Object.assign(w, { ended_at: iso, final_used: p.used, samples: w.samples + 1 });
    if (p.used > w.peak_used) Object.assign(w, { peak_used: p.used, peak_at: iso });
  }
  return windows;
}
const demoHistory = async ({ index, generation, range, metricKey }) => {
  if (generation !== demoSettings.generation) throw STALE;
  const r = demoReports[index];
  if (!r) throw JSON.stringify({ id: "models.error.invalid_index", params: {}, fallback: "Account does not exist in this configuration." });
  const to = Date.now(), from = to - SPANS[range] * DAY_MS;
  const erased = demoSettings.historyErased.has("all") || demoSettings.historyErased.has(index);
  const metrics = erased ? [] : (r.metrics.length ? r.metrics : r.last_good?.metrics ?? []).map((m, i) => {
    const points = demoSamples(m, i, to).filter((p) => p.taken_at >= from);
    const limit = m.limit ?? null, windows = demoWindows(points, limit);
    const used = points.map((p) => p.used);
    return { key: `${r.provider}-${i}`, label: m.label, unit: m.unit, label_text: m.label_text, unit_text: m.unit_text, kind: m.kind, window: m.window, limit, samples: points.length,
      trend: points.length ? { min: Math.min(...used), max: Math.max(...used), last: { taken_at: new Date(points.at(-1).taken_at).toISOString(), used: points.at(-1).used } } : null,
      chart: demoChart(points, limit, from, to, windows), windows };
  });
  return { range, from: new Date(from).toISOString(), to: new Date(to).toISOString(), recording: demoSettings.historyRecording,
    metrics: metricKey == null ? metrics : metrics.filter((m) => m.key === metricKey) };
};
// ponytail: demo mirror of `stats::summarize` and `resets::timeline` over the synthetic history above (the wave of
// `demoWave`, a day missing 5 to 6 days ago) and the demo view's binding metrics. The app reads the real files.
const demoRemoved = { id: "journal.removed_account", params: {}, fallback: "Removed account" };
const demoStats = async ({ range, account, generation }) => {
  if (generation !== demoSettings.generation) throw STALE;
  if (account != null && !demoReports[account]) throw JSON.stringify({ id: "models.error.invalid_index", params: {}, fallback: "Account does not exist in this configuration." });
  const days = range === "days30" ? 30 : 7;
  const erased = demoSettings.historyErased.has("all");
  const indexes = account != null ? [account] : demoReports.map((_, i) => i).filter((i) => demoReports[i].metrics.some((m) => m.limit));
  const today = new Date();
  const daily = (seed) => Array.from({ length: days }, (_, k) => {
    const back = days - 1 - k, d = new Date(today.getFullYear(), today.getMonth(), today.getDate() - back);
    const day = `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, "0")}-${String(d.getDate()).padStart(2, "0")}`;
    const gap = erased || back === 5 || back === 6;
    return { day, share: gap ? null : Math.min(1, Math.max(0.05, demoWave(seed + back) * 1.2)), is_today: back === 0, label: demoDay(day) };
  });
  const accounts = indexes.map((i, row) => {
    const d = daily(i * 3.1), have = d.filter((x) => x.share != null);
    return { account_index: row, name: demoName(demoReports[i].account), hits: have.filter((x) => x.share >= 1).length,
      average_peak: have.length ? have.reduce((a, x) => a + x.share, 0) / have.length : null, days_with_data: have.length, days: d };
  });
  const dailyAll = accounts[0] ? accounts[0].days.map((_, k) => ({ ...accounts[0].days[k],
    share: accounts.map((a) => a.days[k].share).filter((x) => x != null).reduce((a, x) => (a == null ? x : Math.max(a, x)), null) })) : daily(0);
  const withData = dailyAll.filter((x) => x.share != null).length;
  const averages = accounts.map((a) => a.average_peak).filter((x) => x != null);
  const now = Date.now(), at = (hoursAgo) => new Date(now - hoursAgo * 36e5).toISOString();
  const events = erased ? [] : [
    { occurred_at: at(2), kind: "quota_threshold_crossed", account: demoReports[0].account, summary: { id: "events.summary.quota_threshold_crossed", params: { metric: "Gemini (5h)", threshold: 90 }, fallback: "Gemini (5h): 90% used" } },
    { occurred_at: at(4), kind: "quota_threshold_crossed", account: "work@example.com", parent: demoReports[0].account, is_connection: true, summary: { id: "events.summary.quota_threshold_crossed", params: { metric: DEMO_RAW_LABELS["weekly (7d)"][1], threshold: 80 }, fallback: "Week (7 days) (%): 80% used" } },
    { occurred_at: at(7), kind: "quota_reset", account: demoReports[1].account, summary: { id: "events.summary.quota_reset", params: { metric: DEMO_RAW_LABELS["session (5h)"][1] }, fallback: "Session (5 hours) (%): window reset" } },
    { occurred_at: at(30), kind: "quota_exhausted", account: demoReports[1].account, summary: { id: "events.summary.quota_exhausted", params: { metric: DEMO_RAW_LABELS["weekly (7d)"][1] }, fallback: "Week (7 days) (%): quota exhausted" } },
    { occurred_at: at(55), kind: "refresh_failed", account: null, summary: { id: "events.summary.refresh_failed", params: { category: { id: "events.category.provider_error", params: {}, fallback: "provider error" } }, fallback: "Refresh failed: provider error" } },
  ].filter((e) => account == null || e.account === demoReports[account].account).map((e) => ({ ...e, account: demoName(e.account), parent: e.parent ? demoName(e.parent) : e.parent,
    when: demoPast(e.occurred_at), time: demoTime(e.occurred_at), day: demoHeading(e.occurred_at) }));
  return { summary: { range, hits: accounts.reduce((a, x) => a + x.hits, 0), average_peak: averages.length ? averages.reduce((a, x) => a + x, 0) / averages.length : null,
      accounts, extra_spent: account == null && !erased ? [{ unit: "USD", amount: 6.25 }] : null, daily: dailyAll, unknown_limits: 0,
      retention_days: 90, first_sample_at: at(24 * 10), coverage: withData < 2 ? { kind: "not_enough" } : withData === days ? { kind: "full" } : { kind: "partial", days_with_data: withData } },
    indexes, history_kept: { id: "stats.history_kept", params: { days: 90 }, fallback: "History kept 90 days on this device" },
    not_enough: { id: "stats.not_enough", params: {}, fallback: "Not enough history" }, removed_account: demoRemoved, events, skipped: 0, recording: demoSettings.historyRecording,
    rhythm: demoRhythm(erased) };
};
// ponytail: PRD 0043 demo rhythm: working hours with a gap at night, a reset at 09:00 and the weekend off.
function demoRhythm(erased) {
  const now = new Date(), hour = now.getHours();
  const at = (h) => new Date(now.getTime() - h * 36e5).toISOString();
  const use = (h) => (h >= 9 && h < 19 ? Math.round(demoWave(h) * 120) / 10 : h < 7 ? null : 0.4);
  const last24 = Array.from({ length: 24 }, (_, i) => {
    const h = (hour - 23 + i + 24) % 24;
    return { start: at(23 - i), hour: h, consumption: erased ? null : use(h), samples: erased || use(h) == null ? 0 : 4, reset: !erased && h === 9, day_off: false, label: demoHour(h) };
  });
  const week = Array.from({ length: 7 }, (_, i) => {
    const d = new Date(now.getFullYear(), now.getMonth(), now.getDate() - (6 - i));
    const day = `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, "0")}-${String(d.getDate()).padStart(2, "0")}`;
    const off = d.getDay() === 0 || d.getDay() === 6;
    return { start: d.toISOString(), day, consumption: erased ? null : off ? 3 : Math.round(demoWave(i) * 600) / 10, samples: erased ? 0 : 40, reset: false, day_off: off, label: demoDay(day) };
  });
  const typical = Array.from({ length: 24 }, (_, h) => ({ start: at(0), hour: h, consumption: erased || use(h) == null ? null : use(h), samples: erased ? 0 : 12, reset: false, day_off: false, label: demoHour(h) }));
  return { last_24h: { range: "last24h", buckets: last24, metrics: [] }, last_7d: { range: "last7d", buckets: week, metrics: [] }, typical: { range: "typical30d", buckets: typical, metrics: [] } };
}
const demoTimeline = async ({ range, generation }) => {
  if (generation !== demoSettings.generation) throw STALE;
  const now = Date.now(), span = range === "days7" ? 7 * DAY_MS : DAY_MS;
  const all = demoView().reports.filter((r) => r.binding && r.status === "ready").map((r) => {
    const at = r.binding.blocked_until ?? r.binding.resets_at;
    const share = Math.min(1, Math.max(0, 1 - r.binding.used_ratio));
    return { account_index: r.index, name: r.account, provider: r.provider, label: r.binding.label ?? "", returns_at: at ?? null, returns_in: at ? durationNote((new Date(at) - now) / 1000) : null, returns_clock: at ? demoAt(at) : null, remaining_share: share,
      level: share >= 0.7 ? "good" : share >= 0.3 ? "warn" : "bad", exhausted: r.binding.reason === "exhausted", stale: r.freshness === "stale" };
  });
  const by = (a, b) => new Date(a.returns_at) - new Date(b.returns_at);
  const known = all.filter((l) => l.returns_at);
  return { range, now: new Date(now).toISOString(), ends_at: new Date(now + span).toISOString(), ends_clock: demoAt(new Date(now + span).toISOString()),
    lanes: known.filter((l) => new Date(l.returns_at) - now <= span).sort(by), later: known.filter((l) => new Date(l.returns_at) - now > span).sort(by), unknown: all.filter((l) => !l.returns_at) };
};
// ponytail: the browser demo has no logs; two fixed sources play the core's disclosure, the consent by digest, turning
// off (erasing what was derived), the chosen directories, the period, the retention, the background scan's last
// summary and the RF4 summary (synthetic days and models; one model without a price). `?cost-paused` plays a computer
// in energy-saving mode and `?cost-noaccess` a Claude Code directory the system refuses. PRD 0030 (L-16): `?cost-age`
// prices the Claude Sonnet model from a user `pricing.json` dated 319 days ago (its age on its table line), and
// `?cost-plans` gives the first Claude account a plan, types a second currency 12 days ago and says one saved plan was
// invalid (with `?fixture=six`: two Claude accounts, one planned). Core texts keep their English fallback here (the
// core's source language, with the core's ids and wording).
const DEMO_COST = {
  claude_code: { label: "Claude Code", pattern: "**/*.jsonl", dirs: ["/Users/demo/.config/claude/projects", "/Users/demo/.claude/projects"], files: 12,
    providers: ["claude"], models: { "claude-sonnet-4-5-20250929": { entry: "claude-sonnet-4-5", input: 3, output: 15, cache_read: 0.3, cache_write: 3.75 }, "claude-haiku-5-5": null } },
  codex_cli: { label: "Codex CLI", pattern: "**/rollout-*.jsonl", dirs: ["/Users/demo/.codex/sessions", "/Users/demo/.codex/archived_sessions"], files: 3,
    providers: ["chat_gpt"], models: { "gpt-5-codex": { entry: "gpt-5-codex", input: 1.25, output: 10, cache_read: 0.125, cache_write: 1.25 } } },
};
const DEMO_PERIODS = { today: "Today", last_7_days: "Last 7 days", last_30_days: "Last 30 days", current_month: "Current month", all: "Everything available" };
const demoCost = { on: {}, paths: {}, period: "last_7_days", retention: 120, erased: new Set(), hours: {}, hints: {}, labels: {}, hidden: new Set() };
// PRD 0042: the core's default block length per source.
const DEMO_BLOCK_HOURS = { claude_code: 5, codex_cli: 5, gemini_cli: 24 };
// `?cost` starts the demo with every local-cost source on, so the cards and the tray show their cost rows (fa gap 3).
if (new URLSearchParams(location.search).has("cost")) for (const id of Object.keys(DEMO_COST)) demoCost.on[id] = "2026-10-01T09:00:00Z";
const demoDirs = (id) => demoCost.paths[id]?.map((p) => p.replace(/^~(?=\/|$)/, "/Users/demo")) ?? DEMO_COST[id].dirs;
const demoDisclosure = (id) => {
  const { label, pattern } = DEMO_COST[id];
  const dirs = demoDirs(id);
  return { source: id, label, directories: dirs, custom: !!demoCost.paths[id], pattern, digest: `demo-${id}-${dirs.join("|")}`,
    purpose: demoL10n("cost.disclosure.purpose", { source: label }, `To estimate, on this computer only, the tokens and the cost at API prices of your ${label} usage per day and per model.`),
    extracted: demoL10n("cost.disclosure.extracted_v2", {}, "Only numbers are extracted: the time, the model, the token counts (input, output, cache read and cache write), an anonymous digest of the session and a one-way fingerprint of the project folder, so you can label it."),
    never: demoL10n("cost.disclosure.never_v2", {}, "Conversation content, prompts, answers, file names and project paths are never read into the index, stored or shown; the last folder name of a project is kept only if you turn that on. Nothing leaves this computer."),
    off: demoL10n("cost.disclosure.off", {}, "Turning the source off stops every read and erases everything derived from it.") };
};
const demoCostView = () => {
  const sources = Object.entries(DEMO_COST).map(([id, s]) => {
    const enabled = id in demoCost.on;
    const files = enabled && !demoCost.erased.has(id) ? s.files : 0;
    return { source: id, label: s.label, enabled, consented_at: demoCost.on[id] ?? null, paths: demoCost.paths[id] ?? [], disclosure: demoDisclosure(id),
      indexed: { files, invalid_lines: files ? 2 : 0 }, reconsent: false, block_hours: demoCost.hours[id] ?? DEMO_BLOCK_HOURS[id],
      default_block_hours: DEMO_BLOCK_HOURS[id], folder_hint: !!demoCost.hints[id],
      summary: enabled ? demoL10n("cost.scan.ready", { source: s.label, files: String(s.files), requests: String(files ? 0 : 41), invalid: "2" },
        `${s.label}: ${s.files} log(s); ${files ? 0 : 41} new request(s); 2 invalid line(s) ignored.`) : null };
  });
  const on = sources.some((s) => s.enabled);
  return { sources, period: demoCost.period, periods: Object.entries(DEMO_PERIODS).map(([id, text]) => ({ id, label: demoL10n(`cost.period.${id}`, {}, text) })),
    retention_days: demoCost.retention, retention_min: 7, retention_max: 730, block_hours_min: 1, block_hours_max: 24,
    state: !on ? "off" : new URLSearchParams(location.search).has("cost-paused") ? "paused" : "waiting", error: null,
    size: on ? 86016 : 0 };
};
const DEMO_EFFECTIVE = "2026-10-21";
const demoNotice = () => demoL10n("cost.estimate.notice", {}, "Estimate at API prices, not a charge; amounts in USD.");
const tokenSum = (t) => t.input + t.output + t.cache_read + t.cache_write + (t.cache_write_1h ?? 0);
const addTokens = (a, b) => ({ input: a.input + b.input, output: a.output + b.output, cache_read: a.cache_read + b.cache_read, cache_write: a.cache_write + b.cache_write, cache_write_1h: (a.cache_write_1h ?? 0) + (b.cache_write_1h ?? 0) });
const ZERO_TOKENS = { input: 0, output: 0, cache_read: 0, cache_write: 0, cache_write_1h: 0 };
const dayKey = (d) => `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, "0")}-${String(d.getDate()).padStart(2, "0")}`;
/** The demo price of `model` (model names are unique across the demo sources); `null`: cost unknown. */
const demoPrice = (model) => Object.values(DEMO_COST).map((s) => s.models[model]).find((p) => p !== undefined) ?? null;
const demoDaysAgo = (days) => { const d = new Date(); d.setDate(d.getDate() - days); return dayKey(d); };
const demoAge = (effective) => Math.max(0, Math.round((new Date(`${dayKey(new Date())}T00:00:00`) - new Date(`${effective}T00:00:00`)) / 864e5));
// `?cost-age`: the user's pricing.json, 319 days old, prices the Claude Sonnet model over the embedded table.
const DEMO_USER_TABLE = DEMO_QUERY.has("cost-age") ? { effective: demoDaysAgo(319), source: null, models: ["claude-sonnet-4-5-20250929"] } : null;
const demoOrigin = (model) => (DEMO_USER_TABLE?.models.includes(model) ? "user" : "embedded");
/** The core's `TableInfo` of one table, with its age and (from 60 days, C-06) "Prices from N days ago". */
const demoTable = (origin) => {
  const effective = origin === "user" ? DEMO_USER_TABLE.effective : DEMO_EFFECTIVE;
  const days = demoAge(effective);
  return { origin, effective, source: origin === "user" ? DEMO_USER_TABLE.source : "https://claude.com/pricing#api", age_days: days,
    age: days >= 60 ? demoL10n("cost.prices.age", { days: String(days) }, `Prices from ${days} days ago`) : null };
};
/** The core's `cost::estimate` of demo rows `{ model, tokens, requests }` at the demo prices. */
const demoEstimate = (rows) => {
  const by = new Map();
  for (const r of rows) {
    const m = by.get(r.model) ?? { tokens: ZERO_TOKENS, requests: 0 };
    by.set(r.model, { tokens: addTokens(m.tokens, r.tokens), requests: m.requests + r.requests });
  }
  let tokens = ZERO_TOKENS, unknown = ZERO_TOKENS, requests = 0, total = 0, priced = false;
  const models = [...by.keys()].sort().map((model) => {
    const { tokens: mt, requests: mr } = by.get(model);
    const p = demoPrice(model);
    const cost = p ? (mt.input * p.input + mt.output * p.output + mt.cache_read * p.cache_read + mt.cache_write * p.cache_write + (mt.cache_write_1h ?? 0) * p.input * 2) / 1e6 : null;
    tokens = addTokens(tokens, mt);
    requests += mr;
    if (cost == null) unknown = addTokens(unknown, mt); else { total += cost; priced = true; }
    return { model, tokens: mt, requests: mr, cost, price: p ? { entry: p.entry, origin: demoOrigin(model), rates: { input: p.input, output: p.output, cache_read: p.cache_read, cache_write: p.cache_write } } : null };
  });
  const origins = ["embedded", "user"].filter((o) => models.some((m) => m.price?.origin === o));
  return { currency: "USD", total: priced || !models.length ? total : null, tokens, requests, unknown_tokens: unknown,
    unknown_share: tokenSum(tokens) ? tokenSum(unknown) / tokenSum(tokens) : 0, models,
    tables: origins.map(demoTable), notice: demoNotice() };
};
/** Synthetic usage of `id` on the day `back` days ago (some days without any). */
const demoDayRows = (id, back) => Object.keys(DEMO_COST[id].models).flatMap((model, i) => {
  const k = (back * 7 + i * 5 + id.length) % 9;
  if (k === 0 || (i > 0 && back % 3)) return [];
  return [{ model, requests: 4 * k, tokens: { input: 3_000 * k, output: 1_500 * k, cache_read: 45_000 * k, cache_write: 2_500 * k, cache_write_1h: 500 * k } }];
});
/** The core's `CostSummary` of the demo sources over the saved period. */
const demoCostUsage = () => {
  const now = new Date();
  const span = demoSpanDays();
  const at = (back) => { const d = new Date(now); d.setDate(d.getDate() - back); return dayKey(d); };
  const noAccess = new URLSearchParams(location.search).has("cost-noaccess");
  const sources = Object.entries(DEMO_COST).map(([id, s]) => {
    const base = { source: id, label: s.label, providers: s.providers };
    if (!(id in demoCost.on)) return { ...base, state: "off", note: demoL10n("cost.scan.off", { source: s.label }, `${s.label}: off; nothing is read.`), usage: null };
    const kept = !demoCost.erased.has(id);
    const rowsOf = (back) => (kept ? demoShownRows(id, back) : []);
    const days = [];
    for (let back = span; back >= 0; back--) {
      const e = demoEstimate(rowsOf(back));
      days.push({ day: at(back), tokens: e.tokens, requests: e.requests, cost: e.total, unknown_tokens: e.unknown_tokens });
    }
    const period = demoEstimate(Array.from({ length: span + 1 }, (_, back) => rowsOf(back)).flat());
    const periodLabel = demoL10n(`cost.period.${demoCost.period}`, {}, DEMO_PERIODS[demoCost.period]);
    const state = id === "claude_code" && noAccess ? "no_access" : "ready";
    const note = state === "no_access"
      ? demoL10n("cost.view.no_access", { source: s.label }, `${s.label}: no access to the log directories; nothing was worked around. The numbers are what the index already held.`)
      : period.models.length ? null : demoL10n("cost.view.empty", { source: s.label, period: periodLabel }, `${s.label}: no usage in the logs read (${periodLabel.fallback}).`);
    // C-06: this source's own models without a price.
    const unpriced = period.models.filter((m) => m.cost == null).map((m) => m.model);
    return { ...base, state, note, usage: { today: demoEstimate(rowsOf(0)), yesterday: demoEstimate(rowsOf(1)), period, days: period.models.length ? days : [],
      unpriced_models: unpriced, unpriced: demoUnpriced(unpriced) } };
  });
  const label = demoL10n(`cost.period.${demoCost.period}`, {}, DEMO_PERIODS[demoCost.period]);
  const all = Object.keys(DEMO_COST).filter((id) => id in demoCost.on && !demoCost.erased.has(id))
    .flatMap((id) => Array.from({ length: span + 1 }, (_, back) => demoShownRows(id, back)).flat());
  const total = demoEstimate(all);
  const unpriced = total.models.filter((m) => m.cost == null).map((m) => m.model);
  const hidden = demoHiddenProjects();
  const oldest = total.tables.reduce((a, b) => (a && a.age_days >= b.age_days ? a : b), null);
  // C-07: the desktop's `display_text`, the core's `DisplayCurrency::text_for` of each source's period.
  const display = demoPlan.display ? Object.fromEntries(sources.filter((s) => s.usage).map((s) => [s.source, demoDisplayText(s.usage.period)]).filter(([, v]) => v)) : {};
  return { currency: "USD", notice: demoNotice(), span: { period: demoCost.period, days: null, label, first_day: demoCost.period === "all" ? null : at(span), last_day: at(0) }, sources,
    total, pricing_error: null, live: demoLive(), prices_effective: oldest?.effective ?? null, prices_age_days: oldest?.age_days ?? null, prices_age: oldest?.age ?? null,
    unpriced_models: unpriced, unpriced: demoUnpriced(unpriced), ...(hidden.count ? { hidden_projects: hidden.count } : {}), hidden_note: hidden.note,
    ...(demoPlan.notes.length ? { settings_notes: demoPlan.notes } : {}), ...(Object.keys(display).length ? { display_text: display } : {}) };
};
const demoUnpriced = (models) => (models.length ? demoL10n("cost.prices.unpriced", { models: listText(models) }, `No price for: ${models.join(", ")}`) : null);
// PRD 0042 demo: sessions, blocks and projects built from the same synthetic days, in the core's shapes (re-validation
// L-12: only states the core can produce). The rows of a day belong to the project `DEMO_PROJECTS[back % 2]`, the
// third project has rows of its own and starts hidden with `?cost` (M-05: a hidden project leaves every figure, and
// each view says so in `hidden_note`). Today is the active block, one session per source: Claude Code's rows are half
// on a model without a price (H-01: the live line keeps "+ unknown cost", the projection is `money_partial`), Codex
// CLI's have too few recent requests for a burn rate (`rate_missing`). Times are written in the computer's time, like
// the core's `local_text`. Digests are fixed placeholders; labels and hidden projects live for the page's life.
const demoOn = () => Object.keys(DEMO_COST).filter((id) => id in demoCost.on && !demoCost.erased.has(id));
const DEMO_PROJECTS = ["3f9a1c2e7b4d5a60", "a81d0c44e2f97b13", "c27e5b90d4a1f836"];
const DEMO_HIDDEN_PROJECT = DEMO_PROJECTS[2];
// K3: the folder names the desktop gets beside the projects page (`hints`), never on a project row.
const DEMO_FOLDERS = { [DEMO_PROJECTS[0]]: ["claude_code", "client-portal-redesign"], [DEMO_PROJECTS[1]]: ["codex_cli", "infra"] };
if (new URLSearchParams(location.search).has("cost")) {
  demoCost.hidden.add(DEMO_HIDDEN_PROJECT);
  demoCost.hints.claude_code = true;
}
const demoShown = (digest) => !demoCost.hidden.has(digest);
/** The active block's requests of `id` (today, in `DEMO_PROJECTS[0]`). */
const demoLiveRows = (id) => (id === "claude_code"
  ? [{ model: "claude-sonnet-4-5-20250929", requests: 30, tokens: { input: 40_000, output: 20_000, cache_read: 320_000, cache_write: 30_000, cache_write_1h: 0 } },
    { model: "claude-haiku-5-5", requests: 26, tokens: { input: 38_000, output: 18_000, cache_read: 330_000, cache_write: 24_000, cache_write_1h: 0 } }]
  : [{ model: "gpt-5-codex", requests: 18, tokens: { input: 25_000, output: 9_000, cache_read: 140_000, cache_write: 0, cache_write_1h: 0 } }]);
/** The hidden project's own requests: Claude Code, three days ago. */
const demoHiddenRows = (id, back) => (id === "claude_code" && back === 3
  ? [{ model: "claude-sonnet-4-5-20250929", requests: 9, tokens: { input: 12_000, output: 6_000, cache_read: 90_000, cache_write: 8_000, cache_write_1h: 0 } }] : []);
/** The rows of `id` on the day `back` days ago that `project` holds. */
const demoProjectRows = (project, id, back) => (project === DEMO_HIDDEN_PROJECT ? demoHiddenRows(id, back)
  : project === DEMO_PROJECTS[back % 2] ? (back === 0 ? demoLiveRows(id) : demoDayRows(id, back)) : []);
/** Every row of `id` on the day `back` days ago, the hidden projects' left out. */
const demoShownRows = (id, back) => DEMO_PROJECTS.filter(demoShown).flatMap((p) => demoProjectRows(p, id, back));
/** The span's days, `back` from 0 (today). */
const demoSpanDays = () => ({ today: 0, last_7_days: 6, last_30_days: 29, current_month: new Date().getDate() - 1, all: 44 }[demoCost.period]);
/** The hidden projects with requests in the span, and the core's `cost.hidden.note` (`null` for none). */
const demoHiddenProjects = () => {
  const days = Array.from({ length: demoSpanDays() + 1 }, (_, back) => back);
  const count = DEMO_PROJECTS.filter((p) => !demoShown(p) && demoOn().some((id) => days.some((back) => demoProjectRows(p, id, back).length))).length;
  const note = count ? demoL10n("cost.hidden.note", { count },
    count === 1 ? "{count} hidden project is not counted in these figures." : "{count} hidden projects are not counted in these figures.") : null;
  return { count, note };
};
const pad2 = (n) => String(n).padStart(2, "0");
/** The core's `local_text` with "%Y-%m-%d %H:%M" (`date`) or "%H:%M". */
const demoLocal = (at, date = false) => { const d = new Date(at); return `${date ? `${dayKey(d)} ` : ""}${pad2(d.getHours())}:${pad2(d.getMinutes())}`; };
const demoDuration = (m) => (m < 60 ? demoL10n("cost.duration.minutes", { m }, "{m}m") : demoL10n("cost.duration.hours", { h: Math.floor(m / 60), m: m % 60 }, "{h}h {m}m"));
/** The core's `compact_tokens`. */
const demoCompact = (n) => {
  if (n < 1000) return String(n);
  const [div, suffix] = n < 1e6 ? [1e3, "K"] : n < 1e9 ? [1e6, "M"] : [1e9, "B"];
  const v = n / div;
  return `${String(Number(v.toFixed(v < 10 ? 2 : v < 100 ? 1 : 0)))}${suffix}`;
};
const demoCents = (usd) => demoMoney(Math.round(usd * 100) / 100);
const demoProjectName = (digest) => {
  const label = demoCost.labels[digest];
  return label ? demoL10n("cost.project.labelled", { label }, "{label}") : demoL10n("cost.project.name", { short: digest.slice(0, 8) }, "Project {short}");
};
/** The core's `BlockSummary` of `rows`; the active one started about 80 minutes ago, on the hour. */
const demoBlock = (id, back, rows) => {
  const now = Date.now();
  const hours = demoCost.hours[id] ?? DEMO_BLOCK_HOURS[id];
  const active = back === 0;
  const start = new Date(active ? now - 80 * 60_000 : now - (back * 24 + 3) * 3_600_000);
  start.setMinutes(0, 0, 0);
  if (active && start.getTime() + hours * 3_600_000 <= now) start.setTime(now - (now % 3_600_000));
  return { source: id, start: start.toISOString(), end: new Date(start.getTime() + hours * 3_600_000).toISOString(), active,
    last_request: new Date(active ? now - 60_000 : start.getTime() + 3_600_000).toISOString(), sessions: 1, projects: 1,
    estimate: demoEstimate(rows), provider_window: null };
};
/** The core's `LiveBlock` of each source on, with the shared contract's fields (K1). */
function demoLive() {
  if (!demoShown(DEMO_PROJECTS[0])) return [];
  return demoOn().map((id) => {
    const block = demoBlock(id, 0, demoLiveRows(id));
    const e = block.estimate;
    const now = Date.now();
    const elapsed = Math.floor((now - Date.parse(block.start)) / 60_000);
    const left = Math.max(0, Math.floor((Date.parse(block.end) - now) / 60_000));
    const tokens = tokenSum(e.tokens), unknown = tokenSum(e.unknown_tokens);
    const at = demoLocal(block.end);
    const base = { block, elapsed_minutes: elapsed, left_minutes: left, burn_rate_tokens_per_min: null, projected_block_cost: null, line: null, projection: null,
      unknown_tokens: unknown, projected_tokens: null, projected_unknown_tokens: null, projection_kind: null, rate_missing: null };
    if (id !== "claude_code") {
      return { ...base, rate_missing: demoL10n("cost.live.rate_few", { min: 3, minutes: 10, count: 2 }, "Burn rate: needs at least {min} requests in the last {minutes} minutes; {count} so far.") };
    }
    // 38K tokens a minute over the last ten minutes, 40 % of them on the model without a price.
    const rate = 38_000, recentUnknown = 0.4;
    const rateText = demoCompact(rate);
    const line = e.total == null
      ? demoL10n("cost.live.line_unknown", { tokens: demoCompact(tokens), rate: rateText }, "This block: {tokens} tokens, unknown cost · {rate} tok/min")
      : unknown
        ? demoL10n("cost.live.line_partial", { cost: demoCents(e.total), unknown: demoCompact(unknown), rate: rateText }, "This block ≈ {cost} + unknown cost of {unknown} tokens · {rate} tok/min")
        : demoL10n("cost.live.line", { cost: demoCents(e.total), rate: rateText }, "This block ≈ {cost} · {rate} tok/min");
    const out = { ...base, burn_rate_tokens_per_min: rate, line };
    const length = Date.parse(block.end) - Date.parse(block.start);
    if (e.total == null || elapsed * 60_000 < 0.2 * length) return out;
    const projectedTokens = Math.round(tokens + rate * left);
    const projectedUnknown = Math.round(unknown + rate * recentUnknown * left);
    const pricedPerToken = e.total / (tokens - unknown);
    const cost = e.total + pricedPerToken * rate * (1 - recentUnknown) * left;
    const kind = projectedUnknown === 0 ? "money" : projectedUnknown / projectedTokens >= 0.5 ? "tokens" : "money_partial";
    const projection = kind === "tokens"
      ? demoL10n("cost.live.projection_tokens", { tokens: demoCompact(projectedTokens), at }, "At this rate ≈ {tokens} tokens by {at}; most of them have no known price, so no cost is projected")
      : kind === "money_partial"
        ? demoL10n("cost.live.projection_partial", { cost: demoCents(cost), unknown: demoCompact(projectedUnknown), at }, "At this rate ≈ {cost} + unknown cost of about {unknown} tokens by {at}")
        : demoL10n("cost.live.projection", { cost: demoCents(cost), at }, "At this rate ≈ {cost} by {at}");
    return { ...out, projected_block_cost: kind === "tokens" ? null : Math.round(cost * 1e6) / 1e6, projection, projected_tokens: projectedTokens,
      projected_unknown_tokens: projectedUnknown, projection_kind: kind };
  });
}
function demoBreakdown(by, offset = 0) {
  const usage = demoCostUsage();
  const hidden = demoHiddenProjects();
  const base = { notice: demoNotice(), span: usage.span, total: usage.total, hidden_note: hidden.note };
  const label = (digest) => demoCost.labels[digest] ?? null;
  if (by === "day" || by === "model") return usage;
  const span = demoSpanDays();
  if (by === "session") {
    // One session per source, day and project with requests (the core lists no session without one).
    const sessions = demoOn().flatMap((id, i) => Array.from({ length: Math.min(span, 3) + 1 }, (_, back) => back).flatMap((back) => DEMO_PROJECTS.filter(demoShown).map((project) => {
      const rows = demoProjectRows(project, id, back);
      if (!rows.length) return null;
      const digest = `${id.length}${i}${back}${DEMO_PROJECTS.indexOf(project)}`.padEnd(64, "c");
      // Today's session runs inside the active block (it started on the hour, at least 80 minutes ago).
      const minutes = back ? 40 + back * 35 : 70 - i * 20;
      const first = new Date(Date.now() - (back ? (back * 24 + 2) * 3_600_000 + DEMO_PROJECTS.indexOf(project) * 5 * 3_600_000 + i * 12 * 60_000 : (75 - i * 20) * 60_000));
      const last = new Date(first.getTime() + minutes * 60_000);
      const times = { start: demoLocal(first, true), end: demoLocal(last), duration: demoDuration(minutes) };
      const name = label(digest) ? demoL10n("cost.session.labelled", { label: label(digest), ...times }, "{label} · {start}–{end} · {duration}")
        : demoL10n("cost.session.name", times, "Session · {start}–{end} · {duration}");
      return { source: id, session: digest, short: digest.slice(0, 8), label: label(digest), name, first: first.toISOString(), last: last.toISOString(),
        duration_minutes: minutes, project, estimate: demoEstimate(rows), project_name: demoProjectName(project) };
    }).filter(Boolean))).sort((a, b) => b.last.localeCompare(a.last) || a.session.localeCompare(b.session));
    return { ...base, count: sessions.length, total: demoEstimate(demoOn().flatMap((id) => Array.from({ length: span + 1 }, (_, back) => demoShownRows(id, back)).flat())), sessions: sessions.slice(offset, offset + 100) };
  }
  if (by === "block") {
    // The active block first (K7), then the closed ones newest first; a block with no shown request is not listed.
    const blocks = demoOn().flatMap((id) => [0, 1, 2].filter((back) => back <= span).map((back) => {
      const rows = back === 0 ? (demoShown(DEMO_PROJECTS[0]) ? demoLiveRows(id) : []) : demoShownRows(id, back);
      return rows.length ? { block: demoBlock(id, back, rows), rows } : null;
    }).filter(Boolean)).sort((a, b) => Number(b.block.active) - Number(a.block.active) || b.block.start.localeCompare(a.block.start) || a.block.source.localeCompare(b.block.source));
    // R-02: the total is the sum of the listed blocks.
    return { ...base, total: demoEstimate(blocks.flatMap((b) => b.rows)), blocks: blocks.map((b) => b.block), live: demoLive() };
  }
  const days = Array.from({ length: span + 1 }, (_, back) => back);
  const rowsOf = (digest) => demoOn().flatMap((id) => days.flatMap((back) => demoProjectRows(digest, id, back)));
  const projects = DEMO_PROJECTS.map((digest) => {
    const rows = rowsOf(digest);
    if (!rows.length) return null;
    const backs = days.filter((back) => demoOn().some((id) => demoProjectRows(digest, id, back).length));
    const at = (back, hours) => new Date(Date.now() - back * 86_400_000 - hours * 3_600_000).toISOString();
    return { project: digest, short: digest.slice(0, 8), label: label(digest), name: demoProjectName(digest), hidden: !demoShown(digest),
      first: at(Math.max(...backs), 6), last: at(Math.min(...backs), 0.05), sessions: 2 + DEMO_PROJECTS.indexOf(digest), estimate: demoEstimate(rows) };
  }).filter(Boolean).sort((a, b) => (b.estimate.total ?? -1) - (a.estimate.total ?? -1) || tokenSum(b.estimate.tokens) - tokenSum(a.estimate.tokens) || a.project.localeCompare(b.project));
  const hints = Object.fromEntries(projects.map((p) => [p.project, DEMO_FOLDERS[p.project]]).filter(([, f]) => f && demoCost.hints[f[0]] && demoOn().includes(f[0])).map(([d, f]) => [d, f[1]]));
  return { ...base, total: demoEstimate(DEMO_PROJECTS.filter(demoShown).flatMap(rowsOf)), hidden: hidden.count, projects, hints };
}
// PRD 0030 RF1/RF6 demo: plan prices by account id and one typed rate, kept for the page's life like the core's
// settings. The value line mirrors `cost::plan_values` (C-05: one account, a single line; several, one combined line
// with a ratio only when every account has a plan in one currency and on one cycle day; the front ends show it only on
// the planned ids) and its texts use the core's ids and English wording with `amount.money` / `amount.number` params.
const demoPlan = { plans: {}, display: null, notes: [] };
const demoNumber = (value) => ({ id: "amount.number", params: { value: String(value) }, fallback: String(value) });
/** The core's `DisplayCurrency::age_text`. */
const demoRateAge = (days) => (days === 0 ? demoL10n("cost.rate.age.today", {}, "set today") : days === 1 ? demoL10n("cost.rate.age.yesterday", {}, "set yesterday")
  : demoL10n("cost.rate.age.days", { count: String(days) }, `set ${days} days ago`));
/** The core's `DisplayCurrency::text_for`: null without usage or a priced amount; "+ unknown" for a partial one. */
const demoDisplayText = (e) => {
  const d = demoPlan.display;
  if (!d || e.total == null || !tokenSum(e.tokens)) return null;
  const money = demoMoney(Math.round(e.total * d.rate * 100) / 100, d.code);
  const amount = tokenSum(e.unknown_tokens) ? demoL10n("cost.display.partial", { amount: money }, `${money.fallback} + unknown`) : money;
  const days = demoAge(d.set_on);
  const age = demoRateAge(days);
  return days >= 30 ? demoL10n("cost.display.line_stale", { amount, age, days: String(days) }, `≈ ${amount.fallback} at your rate (${age.fallback}); update your rate`)
    : demoL10n("cost.display.line", { amount, age, days: String(days) }, `≈ ${amount.fallback} at your rate (${age.fallback})`);
};
/** The core's `ratio_text` (C-04). */
const demoRatioText = (ratio, rate, plural) => {
  const suffix = `${plural ? "_plans" : ""}${rate ? "_rate" : ""}`;
  const what = plural ? "the plans' price" : "the plan price";
  const tail = rate ? ", at your rate" : "";
  if (ratio >= 0.1) { const r = Math.round(ratio * 10) / 10; return demoL10n(`cost.plan.times${suffix}`, { ratio: demoNumber(r) }, `≈ ${r}× ${what}${tail}`); }
  if (ratio >= 0.01) { const pc = Math.max(1, Math.round(ratio * 100)); return demoL10n(`cost.plan.share${suffix}`, { percent: demoNumber(pc) }, `≈ ${pc}% of ${what}${tail}`); }
  return demoL10n(`cost.plan.share_small${suffix}`, {}, `less than 1% of ${what}${tail}`);
};
/** The core's `plan_text`. */
const demoPlanText = ({ label, count, planned, estimate, partial, plans, ratio, atYourRate }) => {
  const combined = count > 1;
  const many = (n) => (n === 1 ? "account" : "accounts");
  if (estimate == null) {
    const source = combined ? demoL10n("cost.plan.combined_label", { source: label, count: String(count) }, `${label}, ${count} ${many(count)} combined`) : label;
    return demoL10n("cost.plan.unknown", { source, plans }, `${typeof source === "string" ? source : source.fallback}: cost at API prices unknown (models without a price) · plan ${plans.fallback}`);
  }
  const amount = demoMoney(estimate);
  const est = partial ? demoL10n("cost.plan.estimate_partial", { amount }, `≈ ${amount.fallback} at API prices for the models with a price (estimate)`)
    : demoL10n("cost.plan.estimate", { amount }, `≈ ${amount.fallback} at API prices (estimate)`);
  if (combined && planned < count) {
    return demoL10n("cost.plan.combined_some", { source: label, count: String(count), planned: String(planned), estimate: est, plans },
      `${label}, ${count} accounts combined: ${est.fallback} · plans of ${planned} of them: ${plans.fallback}; the logs cannot be split, so there is no ratio`);
  }
  const value = ratio == null ? null : demoRatioText(ratio, atYourRate, combined);
  if (combined) {
    return value ? demoL10n("cost.plan.combined", { source: label, count: String(count), estimate: est, plans, value }, `${label}, ${count} accounts combined: ${est.fallback} · plans ${plans.fallback} · ${value.fallback}`)
      : demoL10n("cost.plan.combined_side", { source: label, count: String(count), estimate: est, plans }, `${label}, ${count} accounts combined: ${est.fallback} · plans ${plans.fallback}`);
  }
  return value ? demoL10n("cost.plan.single", { source: label, estimate: est, plans, value }, `${label}: ${est.fallback} · plan ${plans.fallback} · ${value.fallback}`)
    : demoL10n("cost.plan.single_side", { source: label, estimate: est, plans }, `${label}: ${est.fallback} · plan ${plans.fallback}`);
};
/** The core's `plans_text`: one `amount.money` per currency, joined by `cost.plan.list`. */
const demoPlansText = (plans) => plans.map((p) => demoMoney(p.price, p.currency)).reduceRight((rest, first) => (rest ? demoL10n("cost.plan.list", { first, rest }, `${first.fallback} · ${rest.fallback}`) : first), null);
const demoPlansView = () => {
  const today = new Date();
  const accounts = demoReports.flatMap((r, index) => {
    const [source, s] = Object.entries(DEMO_COST).find(([, c]) => c.providers.includes(r.provider)) ?? [];
    const id = demoAccountId(index);
    return source ? [{ id, name: r.account || brand(r.provider).label, provider: r.provider, source, label: s.label, plan: demoPlan.plans[id] ?? null }] : [];
  });
  const usage = demoCostUsage();
  const values = Object.keys(DEMO_COST).flatMap((source) => {
    const of = accounts.filter((a) => a.source === source);
    const planned = of.filter((a) => a.plan);
    const period = usage.sources.find((s) => s.source === source)?.usage?.period;
    if (!planned.length || !period) return [];
    const label = DEMO_COST[source].label;
    const sums = {};
    for (const a of planned) sums[a.plan.currency] = Math.round(((sums[a.plan.currency] ?? 0) + a.plan.price) * 100) / 100;
    const plans = Object.keys(sums).sort().map((currency) => ({ currency, price: sums[currency] }));
    const complete = planned.length === of.length;
    const oneDay = planned.every((a) => (a.plan.cycle_day ?? 1) === (planned[0].plan.cycle_day ?? 1));
    const total = period.total;
    let ratio = null, atYourRate = false;
    if (total != null && complete && oneDay && plans.length === 1) {
      if (plans[0].currency === "USD") ratio = total / plans[0].price;
      else if (demoPlan.display?.code === plans[0].currency) { ratio = (total * demoPlan.display.rate) / plans[0].price; atYourRate = true; }
    }
    const partial = tokenSum(period.unknown_tokens) > 0;
    const text = demoPlanText({ label, count: of.length, planned: planned.length, estimate: total, partial, plans: demoPlansText(plans), ratio, atYourRate });
    return [{ source, label, accounts: planned.map((a) => a.name), account_ids: of.map((a) => a.id), planned_ids: planned.map((a) => a.id), combined: of.length > 1, complete,
      cycle_start: dayKey(new Date(today.getFullYear(), today.getMonth(), 1)), estimate: total, partial, plans, ratio, at_your_rate: atYourRate, text }];
  });
  const d = demoPlan.display;
  const days = d ? demoAge(d.set_on) : 0;
  return { accounts, values, display: d && { code: d.code, rate: d.rate, set_on: d.set_on, age_days: days, age: demoRateAge(days), stale: days >= 30 },
    ...(demoPlan.notes.length ? { settings_notes: demoPlan.notes } : {}) };
};
// `?cost-plans`: the first Claude account has a USD plan, BRL was typed 12 days ago, and one saved plan was invalid.
if (DEMO_QUERY.has("cost-plans")) {
  const index = demoReports.findIndex((r) => r.provider === "claude");
  if (index >= 0) demoPlan.plans[demoAccountId(index)] = { price: 100, currency: "USD", cycle_day: null };
  demoPlan.display = { code: "BRL", rate: 5.4321, set_on: demoDaysAgo(12) };
  demoPlan.notes = [demoL10n("cost.settings.plans_ignored", { count: "1" }, "1 saved plan price is invalid and ignored; type it again.")];
}
const demoPrices = () => {
  const models = Object.values(DEMO_COST).flatMap((s) => Object.values(s.models).filter(Boolean))
    .map((p) => ({ model: p.entry, origin: "embedded", rates: { input: p.input, output: p.output, cache_read: p.cache_read, cache_write: p.cache_write } }))
    .sort((a, b) => a.model.localeCompare(b.model, "en")); // model ids are identifiers, never translated
  return { currency: "USD", notice: demoNotice(), tables: [{ origin: "embedded", effective: DEMO_EFFECTIVE, source: "https://claude.com/pricing#api; https://openai.com/api/pricing/" }],
    pricing_file: { path: "/Users/demo/Library/Application Support/cuotai/cost/pricing.json", used: false, error: null }, models };
};
// PRD 0018 RF2.6/RF3.1/RF2.5 browser demo: the same `UpdateView` fields.
// `?update=consent` shows the question without a request; `off`, `managed`,
// `error` (a failed check) and `download-error` exercise the other states.
const DEMO_UPDATE = new URLSearchParams(location.search).get("update");
// The structured notes the shell sends (PRD 0019 RF7.7): two sections, as after an unpublished tag.
const DEMO_NOTES = {
  releases: [
    { version: "0.72.0", date: "2026-10-22", categories: [
      { label: { id: "changelog.category.added", fallback: "Added" }, entries: ["A new automatic update option."] },
      { label: { id: "changelog.category.fixed", fallback: "Fixed" }, entries: ["Measurement fixes for the weekly quota.", "The tray icon keeps its color after sleep."] },
    ] },
    { version: "0.71.1", date: "2026-10-20", categories: [
      { label: { id: "changelog.category.fixed", fallback: "Fixed" }, entries: ["The account dialog keeps its scroll position."] },
    ] },
  ],
  more: 3,
};
const demoUpdate = {
  automatic: !["off", "consent"].includes(DEMO_UPDATE),
  consentAnswered: DEMO_UPDATE !== "consent",
  lastCheck: null,
  phase: DEMO_UPDATE === "error" || DEMO_UPDATE === "download-error" ? "failed" : DEMO_UPDATE === "fresh" || DEMO_UPDATE === "off" || DEMO_UPDATE === "consent" ? "uptodate" : "available",
  version: "0.72.0", notes: DEMO_NOTES, date: "2026-10-22T00:00:00Z",
  canInstall: DEMO_UPDATE !== "managed", upgradeCommand: DEMO_UPDATE === "managed" ? "brew upgrade --cask cuotai-app" : null,
  progress: null, dismissed: false, error: DEMO_UPDATE === "error" || DEMO_UPDATE === "download-error" ? "network" : null,
  failedStage: DEMO_UPDATE === "error" ? "check" : DEMO_UPDATE === "download-error" ? "download" : null,
  disabledReason: DEMO_UPDATE === "managed" ? "homebrew" : DEMO_UPDATE === "off" ? "preference" : null,
};
const demoWait = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
const demoUpdateView = () => {
  const enabled = demoUpdate.consentAnswered && !demoUpdate.disabledReason;
  return {
    enabled, automatic: demoUpdate.automatic, disabled_reason: demoUpdate.disabledReason,
    channel: "stable", current_version: "0.71.1", build: null, last_check: demoUpdate.lastCheck,
    consent_pending: !demoUpdate.consentAnswered && !demoUpdate.disabledReason,
    phase: demoUpdate.phase,
    version: demoUpdate.phase === "idle" || (demoUpdate.phase === "failed" && demoUpdate.failedStage !== "download") ? null : demoUpdate.version,
    notes: demoUpdate.phase === "available" ? DEMO_NOTES : null, date: demoUpdate.phase === "available" ? demoUpdate.date : null,
    can_install: demoUpdate.canInstall, upgrade_command: demoUpdate.upgradeCommand, progress: demoUpdate.progress,
    dismissed: demoUpdate.dismissed, error: demoUpdate.error,
    failed_stage: demoUpdate.phase === "failed" ? demoUpdate.failedStage : null,
    can_retry: demoUpdate.phase === "failed" && demoUpdate.failedStage === "download",
    manual_url: "https://github.com/jmarcon/cuotai/releases/latest",
  };
};
// Mockup 40: one approved and one not yet approved extension (`cuotai.demoExtras=1` adds the
// secrets and override controls); example.com identities only.
const demoExtensionDisclosure = (id) => {
  const line = (key, params, fallback) => ({ id: `extensions.consent.${key}`, params, fallback });
  return [
    line("header", { id, name: "Acme usage 1.2.0" }, `The extension ${id} (Acme usage 1.2.0) runs a local program with your user's powers:`),
    line("command", { command: '["./quota.py","--json"]' }, 'command: ["./quota.py","--json"]'),
    line("interpreter", { interpreter: "/usr/bin/env python3" }, "interpreter (the program's #! line): /usr/bin/env python3"),
    line("directory", { dir: `~/.local/share/cuotai/extensions/${id}` }, `directory: ~/.local/share/cuotai/extensions/${id}`),
    line("env", { env: "ACME_TOKEN, OPENAI_ADMIN_KEY" }, "environment variables it asks for: ACME_TOKEN, OPENAI_ADMIN_KEY"),
    line("credential_risk", { name: "OPENAI_ADMIN_KEY", accounts: "OpenAI (work)" },
      "May expose another account credential: OPENAI_ADMIN_KEY is the credential variable of OpenAI (work)."),
    line("interval", { seconds: 300, timeout: 30 }, "it runs every 300 s, with a 30 s limit each time"),
    line("risk", {}, "Cuotai passes only the requested values it inherited; it passes neither vault data nor other accounts' credentials and restricts neither network nor files: review the code before approving it."),
  ];
};

const demoExtensions = () => {
  let extras = false;
  try { extras = localStorage.getItem("cuotai.demoExtras") === "1"; } catch { /* storage may be blocked */ }
  const label = (id, fallback) => ({ id, params: {}, fallback });
  return [
    { id: "acme-usage", name: "Acme usage", version: "1.2.0", description: "Reads the Acme AI usage page of the example workspace.",
      state: "enabled", state_label: label("extensions.state.enabled", "Ativada"), approved: true,
      actions: { enable: false, disable: true, test: true, remove: extras },
      last_run: "2026-10-22T18:10:00Z", error: null, catalog: null,
      account: extras ? { index: 0, name: "Acme usage", key: "extension:acme-usage" } : null,
      overrides: extras ? "groq" : null,
      override_targets: extras ? [{ index: 1, name: "Groq", active: true }, { index: 2, name: "Groq (work)", active: false }] : [],
      declares_secrets: extras },
    { id: "lab-quota", name: "Lab quota", version: "0.3.1", description: "Quota of an internal inference gateway.",
      state: "disabled", state_label: label("extensions.state.disabled", "Needs approval"), approved: false,
      actions: { enable: true, disable: false, test: false, remove: false },
      last_run: null, error: null, catalog: null, account: null, overrides: null, override_targets: [], declares_secrets: false },
  ];
};
// ---------- demo: notification channels (PRD 0044) ----------
let demoChannels = [];
const demoChannelLog = [];
// PRD 0044 re-validation V-05: `?channels` starts with two channels (ntfy on this computer, and a webhook with the
// weekly digest that has been failing for over a day) and five deliveries of mixed kinds and outcomes, one from
// yesterday; `?channels=five` adds a Slack, a Discord and a second ntfy channel, so Add is at the maximum. The log
// entries carry the core's labels (`kind_label`, `error_label`) as `Localized` serializes them.
if (DEMO_QUERY.has("channels")) {
  const ago = (minutes) => new Date(Date.now() - minutes * 60000).toISOString();
  const kinds = ["reset", "threshold", "exhausted", "recovered", "projected", "overage", "floor", "session_expiring", "limit_changed"];
  const channel = (id, kind, label, host, extra = {}) => ({ id, kind, label, host, enabled: true, created_at: ago(20000), signed: false,
    filters: { kinds, accounts: [], min_severity: "info", include_names: null }, last: null, ...extra });
  demoChannels = [
    channel("a1b2c3d4e5f60718", "ntfy", "Phone", "127.0.0.1:8080", { last: { at: ago(10), outcome: "delivered", status: 200 } }),
    channel("0f1e2d3c4b5a6978", "webhook", "Team dashboard", "hooks.example.com", { signed: true,
      filters: { kinds: [...kinds, "digest"], accounts: ["acc_demo_work"], min_severity: "warning", include_names: true },
      last: { at: ago(120), outcome: "failed", status: 503 }, failing_since: ago(1700), failing_status: 503, failing_long: true }),
  ];
  if (DEMO_QUERY.get("channels") === "five") demoChannels.push(
    channel("1122334455667788", "slack", "Ops channel", "hooks.slack.com"),
    channel("8877665544332211", "discord", "Release room", "discord.com", { enabled: false }),
    channel("9999aaaabbbbcccc", "ntfy", "Laptop", "ntfy.example.com"));
  const kind = (slug, fallback) => demoL10n(slug === "test" || slug === "rate_limit" || slug === "failing" ? `channels.log_kind.${slug}` : `channels.kind.${slug}`, {}, fallback);
  const entry = (minutes, ch, slug, fallback, outcome, status, attempt, error) => ({ at: ago(minutes), channel: ch, kind: slug, outcome,
    ...(status ? { status } : {}), attempt, ...(error ? { error } : {}), kind_label: kind(slug, fallback),
    error_label: error ? demoL10n(`channels.error_category.${error}`, {}, "the request timed out") : null });
  demoChannelLog.push(
    entry(10, "a1b2c3d4e5f60718", "threshold", "Thresholds", "delivered", 200, 1),
    entry(120, "0f1e2d3c4b5a6978", "threshold", "Thresholds", "failed", 503, 3),
    entry(300, "a1b2c3d4e5f60718", "test", "Test", "delivered", 200, 1),
    entry(480, "0f1e2d3c4b5a6978", "reset", "Resets", "rate_limited", null, 0),
    entry(1560, "0f1e2d3c4b5a6978", "digest", "Weekly digest", "failed", null, 3, "timeout"));
}
const DEMO_NOTICE_KINDS = [["reset", "Resets"], ["threshold", "Thresholds"], ["exhausted", "Exhaustion"], ["recovered", "Recovery"],
  ["projected", "Projected exhaustion"], ["overage", "Extra usage"], ["floor", "Balance floors"], ["session_expiring", "Session expiry"],
  ["limit_changed", "Limit changes"], ["digest", "Weekly digest"]];
function demoFilterWords(filters) {
  // The core's rule: the default set (every kind but the digest) reads "All kinds", the digest named after it when on.
  const defaults = DEMO_NOTICE_KINDS.filter(([k]) => k !== "digest");
  const kindL10n = ([k, l]) => demoL10n(`channels.kind.${k}`, {}, l);
  const kinds = defaults.every(([k]) => filters.kinds.includes(k))
    ? [demoL10n("channels.words.every_kind", {}, "All kinds"), ...DEMO_NOTICE_KINDS.filter(([k]) => k === "digest" && filters.kinds.includes(k)).map(kindL10n)]
    : DEMO_NOTICE_KINDS.filter(([k]) => filters.kinds.includes(k)).map(kindL10n);
  const accounts = filters.accounts?.length
    ? demoL10n("channels.words.accounts", { count: String(filters.accounts.length) }, `of ${filters.accounts.length} accounts`)
    : demoL10n("channels.words.every_account", {}, "of every account");
  const shown = filters.include_names ?? !demoSettings.notify.private;
  const names = shown ? demoL10n("channels.words.names_shown", {}, "names shown") : demoL10n("channels.words.names_hidden", {}, "names hidden");
  const severity = filters.min_severity === "warning" ? demoL10n("channels.severity.warning", {}, "warnings and critical only")
    : filters.min_severity === "critical" ? demoL10n("channels.severity.critical", {}, "critical only") : null;
  return { kinds, accounts, names, severity };
}
function demoChannelConsent(draft, url) {
  let parsed;
  try { parsed = new URL(String(url).trim()); } catch { throw JSON.stringify(demoL10n("channels.error.url_invalid", {}, "That is not a web address (it should start with https://).")); }
  const host = parsed.hostname;
  if (parsed.protocol !== "https:" && !(draft.kind === "ntfy" && /^(localhost|127\.|10\.|192\.168\.)/.test(host)))
    throw JSON.stringify(demoL10n("channels.error.url_http", {}, "Only https:// addresses are accepted (plain http:// only for an ntfy server on this computer or a private network)."));
  if (draft.kind === "slack" && (host !== "hooks.slack.com" || !parsed.pathname.startsWith("/services/")))
    throw JSON.stringify(demoL10n("channels.error.url_slack", {}, "A Slack incoming webhook starts with https://hooks.slack.com/services/."));
  if (draft.kind === "discord" && (!["discord.com", "discordapp.com"].includes(host) || !parsed.pathname.startsWith("/api/webhooks/")))
    throw JSON.stringify(demoL10n("channels.error.url_discord", {}, "A Discord webhook starts with https://discord.com/api/webhooks/."));
  const title = "Claude: 90% of the 5-hour window used";
  const body = (draft.filters.include_names ?? !demoSettings.notify.private) ? "Work: Session (5h) · resets 16:00" : "Resets 16:00";
  const example = draft.kind === "ntfy" ? `Title: ${title}\nPriority: 4\nTags: warning\n\n${body}`
    : JSON.stringify(draft.kind === "slack" ? { text: `${title}\n${body}` } : draft.kind === "discord" ? { content: `**${title}**\n${body}` }
      : { schema: 1, kind: "threshold", severity: "warning", title, body, sample: true }, null, 2);
  return { kind: draft.kind, host: parsed.host, masked: `${parsed.protocol}//${parsed.host}/••••`, warning: parsed.protocol === "http:"
    ? demoL10n("channels.warning.plain_http", {}, "This address uses plain HTTP: anyone on your network can read the notifications. Use it only for a server on this computer or your private network.") : null,
  filters: demoFilterWords(draft.filters), example,
  sentence: demoL10n("channels.consent.sentence", {}, "Cuotai will send these texts to this address when a notification is due. Nothing else leaves this computer.") };
}
function demoChannelsView() {
  return { channels: demoChannels.map((c) => ({ failing_since: null, failing_status: null, failing_long: false, ...c, filters_text: demoFilterWords(c.filters), last: c.last ?? null })),
    max: 5, kinds: ["webhook", "ntfy", "slack", "discord"], notice_kinds: DEMO_NOTICE_KINDS.map(([id, l]) => ({ id, label: demoL10n(`channels.kind.${id}`, {}, l) })),
    accounts: [{ id: "acc_demo_work", name: "Work" }, { id: "acc_demo_home", name: "Home" }], private: demoSettings.notify.private, log: demoChannelLog.slice(0, 30) };
}

/** PRD 0046 demo: what a backup of the demo profile would hold. */
const demoBackup = (withHistory) => ({
  files: [
    { path: "config/cuotai.toml", sha256: "0".repeat(64), kind: "config" },
    { path: "prefs/desktop.json", sha256: "1".repeat(64), kind: "desktop_settings" },
    { path: "user/themes/night.css", sha256: "2".repeat(64), kind: "theme" },
    ...(withHistory ? [{ path: "history/acc_demo00000001.csv", sha256: "3".repeat(64), kind: "history" }] : []),
  ],
  // PRD 0046 V-5: the core's English fallbacks (`bundle::excluded_reason`), as the shell sends them.
  excluded: [
    ["vault", "Vault entries: secrets never leave the system vault."],
    ["serve_token", "Local server token: each computer makes its own."],
    ["cost_index", "Local cost index: each computer rebuilds it from its own logs."],
    ["extensions", "Extensions: you review and approve them on each computer."],
    ["logs", "Logs: they describe this computer only."],
  ].map(([item, fallback]) => ({ id: `bundle.excluded.${item}`, params: {}, fallback })),
  history_bytes: withHistory ? 48_000 : 0,
  history_large: false,
  default_name: "cuotai-backup-2026-11-07.cuotai-backup.zip",
});

// PRD 0037 re-validation (V-09): the ElevenLabs region values (the demo's EU and US accounts edit with theirs).
const DEMO_ELEVEN_REGIONS = [["default", "Default (api.elevenlabs.io)"], ["us", "United States (api.us.elevenlabs.io)"], ["eu", "European Union (data residency)"],
  ["in", "India (data residency)"], ["sg", "Singapore (data residency)"]];
// PRD 0045 RF6.3 (re-validation V-07): the demo mirror of `account_edit::breakdown_options` for the Breakdown field:
// LiteLLM's keys and teams, OpenRouter's keys with the account product (management key), the metered reason for AWS
// Bedrock, and the "nothing to break down" reason (field hidden) for every other draft.
const demoBreakdownOptions = (provider, fields = []) => {
  const mode = (kind, label, requests) => ({ kind, label: dl(`breakdown.kind.${kind}`, label),
    requests: { id: "access.breakdown_requests", params: { count: String(requests) }, fallback: `Extra requests per refresh: up to ${requests}` } });
  const account = (Array.isArray(fields) ? fields : []).find((f) => f.key === "product")?.choice === 1;
  const modes = provider === "litellm" ? [mode("keys", "API keys", 3), mode("teams", "Teams", 1)]
    : provider === "open_router" && account ? [mode("keys", "API keys", 3)] : [];
  const reason = modes.length ? null : provider === "aws_bedrock"
    ? dl("access.breakdown_metered", "This provider bills every query, so Cuotai offers no breakdown for it.")
    : dl("access.breakdown_none", "This account's interface reports no keys, projects, workspaces or teams to break down; per-user data is never read.");
  return { modes, reason };
};
const DEMO_CERT_NEW = "3F:A1:9C:07:D2:5E:88:41:B6:0D:E3:72:1A:C9:54:F0:2B:7D:96:E8:13:4C:A5:6F:D0:39:82:BE:57:0A:C4:1E";
const DEMO_CERT_OLD = "8B:22:E7:4D:01:9A:C3:F6:5D:70:B8:2E:94:1F:A6:3C:D9:05:6B:E1:48:7A:C2:93:0F:5E:B4:27:D8:61:3A:9C";
/** The Test connection results `?aggtest=` selects, as the core words them (`nine_router::test_message`). */
const DEMO_AGG_TESTS = {
  locked: () => ({ ok: false, tone: "warn", field: null, message: coreNote("query.nine_router.locked", "Too many 9router login attempts; wait {seconds} s before trying again.".replace("{seconds}", "300"), { seconds: 300 }) }),
  // Re-validation S-03: a first sighting and a changed pin, with the core's certificate offer.
  "cert-first": () => ({ ok: false, tone: "warn", field: "params.tls_pin_sha256", message: coreNote("test.nine_router.untrusted_certificate", `This computer does not trust the instance's certificate. SHA-256 fingerprint: ${DEMO_CERT_NEW}. Check it on the server before trusting it.`, { fingerprint: DEMO_CERT_NEW }),
    fingerprint: DEMO_CERT_NEW, certificate: { state: "untrusted", host: "router.example.com", port: 443, fingerprint: DEMO_CERT_NEW, previous: null } }),
  "cert-changed": () => ({ ok: false, tone: "warn", field: "params.tls_pin_sha256", message: coreNote("test.nine_router.certificate_changed", `The instance's certificate changed and no longer matches the pinned one. New SHA-256 fingerprint: ${DEMO_CERT_NEW}. Check it on the server before trusting it.`, { fingerprint: DEMO_CERT_NEW }),
    fingerprint: DEMO_CERT_NEW, certificate: { state: "changed", host: "router.example.com", port: 443, fingerprint: DEMO_CERT_NEW, previous: DEMO_CERT_OLD } }),
  insecure: () => ({ ok: false, tone: "error", field: "params.base_url", message: coreNote("query.nine_router.base_url_insecure", "Outside this machine 9router requires HTTPS (HTTP only on a private network, with the explicit option). Nothing was sent.") }),
};
// PRD 0033 re-validation V-06: when the demo's metered account last made a billed query (per account name), and the
// core's `note.metered.refused` with its two clocks as nested core texts (`at.time`).
const demoMeteredAt = new Map();
const demoClock = (ms) => {
  const d = new Date(ms), hh = String(d.getHours()).padStart(2, "0"), mm = String(d.getMinutes()).padStart(2, "0");
  return { id: "at.time", params: { hour: hh, minute: mm }, fallback: `${hh}:${mm}` };
};
const demoMeteredRefused = (queried, next) => ({ id: "note.metered.refused", params: { queried: demoClock(queried), next: demoClock(next) },
  fallback: `Not queried: the last billed AWS Cost Explorer query was at ${demoClock(queried).fallback}; the next one is possible at ${demoClock(next).fallback}.` });
const demo = {
  get_view: async () => demoView(),
  refresh_all: async () => {},
  // Demo of PRD 0010 RF3.2: refreshing the failing account succeeds, so its preserved reading (and badge) goes away.
  set_connection_flag: async ({ key, sub, on }) => {
    const set = demoConnFlags[key];
    if (!set) throw new Error("bad key");
    if (on) set.add(sub); else set.delete(sub);
  },
  refresh_account: async ({ index }) => {
    const r = demoReports[index];
    // PRD 0033 re-validation V-06: the core's 15-minute floor of an explicit refresh of the metered demo account
    // (`?metered`): the first refresh is a (demo) billed query, a second within 15 minutes is refused with the note.
    if (r?.provider === "aws_bedrock") {
      const now = Date.now(), last = demoMeteredAt.get(r.account);
      if (last != null && now - last < 15 * 60e3) return demoMeteredRefused(last, last + 15 * 60e3);
      demoMeteredAt.set(r.account, now);
      demoReports[index] = { ...r, checked_at: ago(0) };
      return null;
    }
    if (r?.last_good) demoReports[index] = { ...r, status: "ready", pending: false, checked_at: ago(0), metrics: r.last_good.metrics, notes: [], last_good: null };
    return null;
  },
  // Same key as the core's `picker_providers` (lowercased label), pickable first.
  list_providers: async () => Object.entries(DEMO_CORE_LABELS).map(([id, label]) => ({ id, label, pickable: !DEMO_NO_QUERY.includes(id) }))
    .sort((a, b) => (b.pickable - a.pickable) || (a.label.toLowerCase() < b.label.toLowerCase() ? -1 : a.label.toLowerCase() > b.label.toLowerCase() ? 1 : 0)),
  // PRD 0027 re-validation C-07: the browser demo has no core, so it runs the JS port of the core search over the
  // core-generated catalog; a node test checks the port gives the core's results.
  search_providers: async ({ query, locale }) =>
    searchCatalog(await demoCatalog(), query, locale ?? currentLocale(), demoResolve).map(demoPickerEntry),
  // ponytail: demo mirror of `picker_hidden_notice` for the providers without any usage query.
  picker_hidden_notice: async ({ query }) => {
    const fold = (v) => String(v ?? "").normalize("NFD").replace(/[\u0300-\u036f]/g, "").toLowerCase();
    const q = fold(query).trim();
    const id = q && DEMO_NO_QUERY.find((p) => fold(DEMO_CORE_LABELS[p]).includes(q) || p.includes(q));
    if (!id) return null;
    const reason = dlx(`access.reason.${id}`, "demo.access.reason", "No public usage query for this credential (demo).");
    return { id: "catalog.hidden_provider", params: { provider: DEMO_CORE_LABELS[id], reason },
      fallback: `${DEMO_CORE_LABELS[id]} has no usage query, so Cuotai does not offer it: ${reason.fallback}` };
  },
  provider_modes: async ({ provider }) => (await demoCatalog()).entries.filter((e) => e.provider === provider)
    .sort((a, b) => b.recommended - a.recommended).map(demoPickerEntry),
  detect_sources: async () => (demoSettings.detect.enabled ? [{ provider: "cursor", kind: "cursor_app", mode: "oauth",
    hint: { id: "detect.hint", params: { tool: "Cursor" }, fallback: "Cursor is installed on this device" },
    consent: { id: "detect.consent.cursor_import", params: {}, fallback: "Cuotai will read the sign-in token from Cursor's local database once, to query your usage. It is stored in your system's credential vault." } },
    ...DEMO_NATIVE_DETECT.map(([provider, kind, tool, name]) => ({ provider, kind, mode: "oauth",
      hint: { id: "detect.hint", params: { tool }, fallback: `${tool} is installed on this device` },
      consent: { id: "detect.consent.native_login", params: { provider: name, tool },
        fallback: `Cuotai will open the ${name} sign-in; nothing is read from ${tool}'s folder.` } }))]
    .filter((d) => !demoSettings.detect.dismissed.includes(d.kind)) : []),
  set_detect_enabled: async ({ on }) => { demoSettings.detect.enabled = on; },
  dismiss_detected: async ({ kind }) => { demoSettings.detect.dismissed.push(kind); },
  list_extensions: async () => demoExtensions(),
  // Demo mirror of core `consent::disclosure` (plus one PRD 0050 RF8.2 risk line), so the consent box has content.
  extension_disclosure: async ({ id } = {}) => ({ lines: demoExtensionDisclosure(id ?? "acme-usage") }),
  enable_extension: async ({ id }) => ({ id: id ?? "demo", message: dl("extensions.action.enabled", "Extension enabled."), account: null }),
  disable_extension: async ({ id }) => ({ id, message: dl("extensions.action.disabled", `Extension ${id} disabled.`), account: null }),
  test_extension: async ({ id }) => ({ source: `extension:${id}`, status: "ready", metrics: [], notes: [], violations: [] }),
  // ponytail: demo mirror of `access` for the browser (default modes only, generic pt summary); the app asks the core.
  draft_access: async ({ provider, fields = [] }) => {
    const level = demoLevelOf(provider);
    const reason = level === "unavailable" ? dlx(`access.reason.${provider}`, "demo.access.reason", "No public usage query for this credential (demo).") : null;
    return {
      level: demoLevel(level),
      summary: dlx(`access.summary.${provider}`, "demo.access.summary", "Demo usage query."),
      notes: [dlx(`access.credential_hint.${level === "official" ? "admin_key" : "inference_key"}`, "demo.access.credential_hint", "Credencial da consulta (demo)."), ...(reason ? [reason] : [])],
      cost_advisory: provider === "aws_bedrock" ? dlx("access.cost_advisory", "demo.access.cost_advisory", "Provedor cobrado por consulta (demo).") : null,
      save_confirmation: reason && { id: "access.confirm_unavailable", params: { reason }, fallback: `${reason.fallback} The account will be saved, but it will show this reason instead of numbers.` },
      breakdown: demoBreakdownOptions(provider, fields),
    };
  },
  // PRD 0047: the desktop types the key (`secret_inputs`); the variable-name fields are terminal-only.
  // PRD 0037 re-validation (V-09): every provider's form is the backend's own `form_schema`, generated into
  // `demo-forms.json` by the desktop test `demo_forms_fixture`; a provider the file lacks gets the generic demo form.
  form_schema: async ({ provider }) => (await demoForms())[provider]?.schema ?? ({ credential_kind: "env",
    interval: provider === "aws_bedrock" ? { min: 300, max: 86400, default_seconds: 3600, effective_min: 900 } : { min: 30, max: 86400, default_seconds: null, effective_min: 30 },
    secret_inputs: [DEMO_KEY_INPUT, ...(provider === "xiaomi_mimo" ? [{ slot: DEMO_SESSION_SLOT.slot, label: DEMO_SESSION_SLOT.label,
      help: DEMO_SESSION_SLOT.instructions, required: false, cookie: true, field: null }] : [])],
    terminal_only_fields: [],
    login: { modes: [{ id: "oauth", label: { id: "demo.login.oauth.label", params: {}, fallback: "OAuth sign-in (demo)" }, help: { id: "demo.login.oauth.help", params: {}, fallback: "The browser demo does not run the sign-in." }, completion_notice: { id: "demo.login.oauth.completion", params: {}, fallback: "Sign-in authorized (demo)." }, credential_store: "cuotai_vault", account_binding: { type: "none" }, registration: [], options: [], unavailable_reason: { id: "demo.login.oauth.unavailable", params: {}, fallback: "Sign-in is not available in the demo." } }], unsupported_reason: null },
    sessions: provider === "xiaomi_mimo" ? [DEMO_SESSION_SLOT] : [],
    session_notice: provider === "xiaomi_mimo" ? demoSessionNotice("vault") : null,
    fields: [{ id: "demo.field.project_id", key: "project_id", label: { id: "demo.field.project_id.label", params: {}, fallback: "Project ID" }, help: { id: "demo.field.project_id.help", params: {}, fallback: "Opcional." }, required: false, options: null },
             { id: "demo.field.mode", key: "mode", label: { id: "demo.field.mode.label", params: {}, fallback: "Mode" }, help: { id: "demo.field.mode.help", params: {}, fallback: "" }, required: false, options: [{ id: "demo.field.mode.option.personal", index: 0, value: "personal", label: { id: "demo.field.mode.option.personal", params: {}, fallback: "Personal" } }, { id: "demo.field.mode.option.org", index: 1, value: "org", label: { id: "demo.field.mode.option.org", params: {}, fallback: "Organization" } }] }] }),
  visible_fields: async ({ provider, choices, authMode }) => demoVisible((await demoForms())[provider], choices, authMode) ?? ["project_id", "mode"],
  field_choices: async ({ provider, choices, authMode }) => {
    const schema = await demo.form_schema({ provider });
    return schema.fields.filter((field) => field.options).map((field) => {
      const options = field.options.filter((option) => !option.auth_modes || option.auth_modes.includes(authMode));
      const current = field.options.find((option) => option.index === choices[field.key]);
      const selected = current && options.some((option) => option.index === current.index) ? current : options[0];
      return {
        key: field.key,
        options,
        choice: selected.index,
        notice: current && current !== selected
          ? dl("edit_notice.option_replaced_for_auth_mode", `${current.label.fallback} is not available in this mode; ${selected.label.fallback} was selected.`)
          : null,
      };
    });
  },
  get_account: async ({ index }) => {
    const r = demoReports[index];
    const state = demoSessionState(r);
    const sessionOffer = state === "env";
    const label = { id: "user_session.slot.xiaomi_mimo.platform_session_env", params: {}, fallback: "platform.xiaomimimo.com web session (balance and Token Plan)" };
    // PRD 0047 G4/G5: the key of every demo account is in the vault; the MiMo session still comes from the terminal setup
    // until it is moved. No value and no variable name reach the form.
    const source = state === "env" ? "environment" : state === "vault" ? "vault" : "unset";
    const states = [{ slot: "credential_env", source: "vault", label: dl("secret_input.state.vault", "Saved in the system vault") },
      ...(r.provider === "xiaomi_mimo" ? [{ slot: "platform_session_env", source, label: dl(`secret_input.state.${source}`, source) }] : [])];
    return { name: r.account, provider: r.provider, credential: "",
      interval: "", group: "", account_id: demoAccountId(index), monogram: demoSettings.monograms[demoAccountId(index)] ?? "", uses_oauth: false, native_login: false, auth_mode: "credential", auth_options: {}, auth_registration: {},
      fields: r.provider === "eleven_labs" ? [{ key: "region", text: null, choice: Math.max(0, DEMO_ELEVEN_REGIONS.findIndex(([value]) => value === r.demo_region)) }]
        : (await demoForms())[r.provider]?.schema.fields.map((f) => ({ key: f.key, text: f.options ? null : "", choice: f.options ? 0 : null }))
          ?? [{ key: "project_id", text: "", choice: null }, { key: "mode", text: null, choice: 0 }],
      session_slots: state === "vault" ? ["platform_session_env"] : [],
      // PRD 0041: the account's budgets and floors as the TOML holds them (`?budget`).
      budgets: r.demo_budgets ?? [], floors: r.demo_floors ?? [],
      credential_stored: true, secret_states: states,
      vault_migration: sessionOffer ? [{ slot: "platform_session_env", label,
        consent: { id: "secret_input.migrate.consent", params: { label },
          fallback: `${label.fallback}: Cuotai copies the value it reads today from the terminal setup into the system vault. From then on this account works without that setup; the configuration file keeps just a reference.` } }] : [] };
  },
  migrate_session: async ({ index, accountId }) => {
    demoCheckSlot(index, accountId);
    demoSettings.sessionMigrated.add(demoReports[index]);
    return demo.get_account({ index });
  },
  vault_audit: async () => ({ config_path: "~/.config/cuotai/config.toml", entries: [
    { key: "00000000-0000-4000-8000-000000000001", created_at: "2026-10-01T12:00:00Z", kind: "oauth", provider: "claude", account_id: "acc_demo00000001", account: "Claude", status: "in_use", orphan: false, present: true },
    { key: "channel_0123456789abcdef", created_at: "2026-10-02T12:00:00Z", kind: "channel", provider: null, account_id: null, account: null, status: "in_use", orphan: false, present: true },
    ...(demoSettings.vaultPruned ? [] : [{ key: "00000000-0000-4000-8000-000000000002", created_at: "2026-09-01T12:00:00Z", kind: "oauth", provider: null, account_id: null, account: null, status: "unused", orphan: true, present: true }]),
    ...(demoSettings.vaultRemoved ? [] : [{ key: "00000000-0000-4000-8000-000000000003", created_at: "2026-08-01T12:00:00Z", kind: null, provider: null, account_id: null, account: null, status: "owner_unknown", orphan: false, present: true }]),
  ] }),
  doctor_report: async () => ({ schema: 1, checks: [
    { id: "version", status: "ok", title: { id: "doctor.title.version", params: { version: "demo", os: "macos", arch: "aarch64", source: "direct" }, fallback: "Cuotai {version} on {os} {arch} ({source})" } },
    { id: "config", status: "ok", title: { id: "doctor.title.config", params: { path: "~/.config/cuotai/config.toml", source: "default" }, fallback: "Configuration file {path} ({source})" },
      detail: { id: "doctor.config.valid", params: { count: 3 }, fallback: "Valid; accounts: {count}." } },
    { id: "account.2", status: "fail", title: { id: "doctor.title.account", params: { number: 2, provider: "groq" }, fallback: "Account {number} ({provider})" },
      detail: { id: "doctor.account.env_missing", params: { variable: "GROQ_API_KEY" }, fallback: "It reads {variable}, which is not set in this environment." },
      fix: { id: "doctor.fix.set_env", params: { variable: "GROQ_API_KEY" }, fallback: "Set {variable} in the environment that starts Cuotai, or store the key in the vault from the account editor." } },
    { id: "cost_index", status: "skip", title: { id: "doctor.title.cost_index", params: {}, fallback: "Local cost index" },
      detail: { id: "doctor.state.absent", params: {}, fallback: "It has not been created yet." } },
    // PRD 0052 RF3: the diagnostics the doctor adds (sanitized proxy, trust store, Cuotai's files).
    { id: "proxy", status: "ok", title: { id: "doctor.title.proxy", params: {}, fallback: "Network proxy" },
      detail: { id: "doctor.proxy.none", params: { bypass: { id: "doctor.proxy.no_bypass", params: {}, fallback: "No NO_PROXY list is set." } },
        fallback: "No proxy variable is set: requests go direct. {bypass}" } },
    { id: "trust_store", status: "ok", title: { id: "doctor.title.trust_store", params: {}, fallback: "System root certificates" },
      detail: { id: "doctor.trust_store.ok", params: { count: 150 }, fallback: "{count} certificates load from the operating system." } },
    { id: "artifacts", status: "ok", title: { id: "doctor.title.artifacts", params: {}, fallback: "Cuotai's files" },
      detail: { id: "doctor.artifacts.summary", params: { present: 2, total: 31, size: "1.5 MiB" }, fallback: "{present} of {total} present, {size} in all." } },
  ], artifacts: [
    { id: "history.db", path: "~/.local/share/cuotai/history/history.sqlite3", kind: "file", exists: true, found: "file", bytes: 1536000, mode: "0600", mode_expected: null, schema: "5" },
    { id: "vault.oauth", path: "cuotai.oauth", kind: "vault_service", exists: null, found: null, bytes: null, mode: null, mode_expected: null, schema: "not_applicable" },
  ] }),
  backup_preview: async ({ withHistory }) => demoBackup(withHistory),
  backup_write: async ({ withHistory }) => ({ ...demoBackup(withHistory), path: `/Users/demo/${demoBackup(withHistory).default_name}` }),
  restore_plan: async () => ({ token: 1, plan: {
    created_at: "2026-11-01T09:30:00Z", cuotai_version: "0.83.1",
    items: [
      { path: "config/cuotai.toml", target: "~/.config/cuotai/config.toml", action: "replace", accounts: { added: 1, changed: 1, removed: 0 } },
      { path: "config/hooks.toml", target: "~/.config/cuotai/hooks.toml", action: "add" },
      { path: "history/acc_demo00000001.csv", target: "history", action: "merge", merge: { rows: 120, new: 80 } },
      { path: "prefs/desktop.json", target: "~/Library/Application Support/cuotai/desktop.json", action: "skip" },
      { path: "user/themes/night.css", target: "~/.config/cuotai/themes/night.css", action: "keep" },
    ],
    needs: [
      { id: "bundle.need.env", params: { number: 1, provider: "groq", variable: "GROQ_API_KEY" }, fallback: "Set {variable} for account {number} ({provider})." },
      { id: "bundle.need.sign_in", params: { number: 2, provider: "claude" }, fallback: "Sign in again to account {number} ({provider})." },
      // PRD 0046 G-0046-1: a consent switch the backup had on stays off here (no button in Next steps).
      { id: "bundle.need.setting", params: { setting: { id: "bundle.setting.serve", params: {}, fallback: "Local server" } },
        fallback: "Local server was on in the backup and stays off on this computer; turn it on in Settings if you want it." },
      { id: "bundle.need.channel", params: { label: "Team", kind: "slack" }, fallback: "Type the address of the notification channel Team (slack) again." },
    ],
    next_steps: [{ kind: "env", account: 0, provider: "groq" }, { kind: "sign_in", account: 1, provider: "claude" },
      { kind: "setting", account: 0, provider: "serve" }, { kind: "channel", account: 0, provider: "slack" }],
    // PRD 0046 M-03: an account that would send its key to a proxy needs the dialog's own checkbox.
    hosts: [{ account: 2, provider: "litellm", kind: "base_url", host: "https://proxy.example.com:8443",
      text: { id: "bundle.host.url", params: { number: 3, provider: "litellm", host: "https://proxy.example.com:8443" },
        fallback: "Account 3 (litellm) sends its key to https://proxy.example.com:8443." } }],
  } }),
  restore_apply: async ({ accept, acceptHosts }) => {
    // The core refuses to write the accounts file before the hosts are confirmed (`bundle.error.hosts`).
    if (!acceptHosts) throw JSON.stringify({ id: "bundle.error.hosts", params: {}, fallback: "This restore would send keys to the addresses its plan lists; confirm them to restore." });
    return { written: 2 + accept.length, restart: accept.includes("prefs/desktop.json") };
  },
  restart_app: async () => location.reload(),
  vault_prune: async ({ keys }) => { const deleted = demoSettings.vaultPruned ? [] : keys.slice(0, 1); demoSettings.vaultPruned = true; return { deleted, skipped: [] }; },
  vault_remove: async () => { demoSettings.vaultRemoved = true; return "owner_unknown"; },
  forget_session: async ({ index, accountId }) => {
    demoCheckSlot(index, accountId);
    demoSettings.sessionForgotten.add(demoReports[index]);
    return demo.get_account({ index });
  },
  // PRD 0016 RF2 demo: the browser keeps only the accepted names, like the core (no value is ever shown).
  preview_session: async ({ paste }) => {
    const names = paste.replace(/^cookie:/i, "").split(";").map((p) => p.split("=")[0].trim()).filter(Boolean);
    const kept = ["api-platform_serviceToken", "userId"].filter((n) => names.includes(n));
    if (kept.length < 2) throw JSON.stringify({ id: "user_session.paste.missing", params: { names: "api-platform_serviceToken, userId" }, fallback: "This session's cookies are missing: api-platform_serviceToken, userId." });
    const discarded = names.length - kept.length;
    const expiry = { id: "user_session.expiry.unknown", params: {}, fallback: "Validity unknown." };
    return { kept, discarded, expiry: null, summary: { id: "user_session.paste.summary", params: { kept: kept.join(", "), discarded, expiry }, fallback: `Kept: ${kept.join(", ")}. Discarded: ${discarded}. Validity unknown.` } };
  },
  // PRD 0048 demo: the browser has no capture window; the dialog falls back to the paste box.
  capture_session: async () => ({ status: "unavailable" }),
  cancel_session_capture: async () => {},
  forget_shared_capture: async () => false,
  open_aggregator_dashboard: async () => {},
  // PRD 0036 RF2.2: the demo instance answers like a local 9router with three connections.
  test_aggregator: async ({ input }) => {
    // PRD 0036 re-validation (V-12): `?aggtest=<outcome>` answers with one of the core's other results (`DEMO_AGG_TESTS`).
    const outcome = new URLSearchParams(location.search).get("aggtest") ?? "";
    const forced = Object.hasOwn(DEMO_AGG_TESTS, outcome) && typeof DEMO_AGG_TESTS[outcome] === "function" ? DEMO_AGG_TESTS[outcome] : null;
    // Once the demo certificate is pinned (after either certificate dialog), the instance connects.
    const pinned = (input.fields ?? []).find((f) => f.key === "tls_pin_sha256")?.text === DEMO_CERT_NEW;
    if (forced && !pinned) return forced(input);
    const base = (input.fields ?? []).find((f) => f.key === "base_url")?.text ?? "";
    if (base && !/^https?:\/\//.test(base)) {
      return { ok: false, tone: "error", field: "params.base_url", message: { id: "test.nine_router.unreachable", params: {}, fallback: "The address could not be reached." } };
    }
    if (input.provider === "litellm") return { ok: true, tone: "ok", field: null, message: { id: "test.litellm.connected", params: { count: "4" }, fallback: "LiteLLM: 4 providers with a budget found." } };
    return { ok: true, tone: "ok", field: null, message: { id: "test.nine_router.connected", params: { count: "3", version: "0.5.109" }, fallback: "9router 0.5.109: 3 connections with usage found." } };
  },
  test_session: async ({ input }) => {
    await demo.preview_session({ paste: input.sessions?.[0]?.paste ?? "" });
    return { ok: true, status: { id: "status.ready", params: {}, fallback: "OK" }, note: null, metrics: 2, expiry: null };
  },
  save_account: async ({ input }) => {
    window.__lastSaveRequest = input;
    // PRD 0029 RF3: the core rule — a name is needed only beside another account of the provider.
    const others = demoReports.filter((x, i) => i !== input.index && x.provider === input.provider);
    // V-12: as the core sends it, on the name field, with the provider's name in the English text.
    if (!input.name.trim() && others.length && input.provider !== "extension") throw JSON.stringify({ id: "edit_error.name_required_same_provider", field: "name", params: { provider: { id: `provider.${input.provider}`, params: {}, fallback: providerName(input.provider) } }, fallback: `Name this account: you have another ${providerName(input.provider)} account.` });
    const peer = input.index == null ? others.find((x) => !x.account.trim()) : null;
    if (peer) {
      if (!input.peerName?.trim()) throw JSON.stringify({ id: "edit_error.name_required", field: "peer_name", params: {}, fallback: "Set a name for the account." });
      peer.account = input.peerName.trim();
    }
    if (input.monogram != null && !validMonogram(input.monogram)) throw JSON.stringify({ id: "desktop.error.invalid_monogram", params: {}, fallback: "A monogram has one or two characters, without spaces." });
    window.__lastSaveMonogram = input.monogram ?? null;
    if (input.index != null && input.monogram != null) {
      const id = demoAccountId(input.index), text = String(input.monogram).trim();
      if (text) demoSettings.monograms[id] = text; else delete demoSettings.monograms[id];
    }
    const r = dr(input.name.trim(), input.provider, [dm("Exemplo", 10, 100, 5, "quota", null)]);
    // PRD 0016 demo: a paste goes to the vault; otherwise the account keeps where its session was.
    const prev = input.index == null ? null : demoReports[input.index];
    if (input.sessions?.some((p) => p.paste.trim())) demoSettings.sessionMigrated.add(r);
    else for (const set of [demoSettings.sessionMigrated, demoSettings.sessionForgotten]) if (set.has(prev)) set.add(r);
    if (input.index != null) demoIds.set(r, demoAccountId(input.index)); // an edit keeps the account's id
    input.index == null ? demoReports.push(r) : (demoReports[input.index] = r);
    demoSettings.generation++;
    return { cleanup_warning: null };
  },
  // Demo login: emits the core's event sequence (prompt → progress → completed).
  login_start: async ({ request }) => {
    window.__lastLoginRequest = request;
    const attempt = 1;
    setTimeout(() => demoEmit({ attempt, type: "prompt", instructions: "Autorize no navegador do sistema.", url: "https://example.test/device", user_code: "ABCD-1234" }), 100);
    setTimeout(() => demoEmit({ attempt, type: "finished", params: {} }), 500);
    return attempt;
  },
  login_submit_code: async () => { throw { id: "demo.login_unavailable", params: {}, fallback: "Sign-in is not available in the browser demo." }; },
  login_cancel: async () => {},
  account_index: async ({ accountId }) => {
    const found = demoReports.findIndex((_, i) => demoAccountId(i) === accountId);
    return found < 0 ? null : found;
  },
  delete_account: async ({ index, purge }) => {
    const oauth = demoReports[index]?.provider === "kiro" || demoReports[index]?.uses_oauth;
    demoReports.splice(index, 1);
    demoSettings.generation++;
    return { outcome: !oauth ? "removed" : purge ? "purged" : "kept", other_account: null, message: null };
  },
  set_refresh_seconds: async ({ seconds }) => {
    if (seconds < 30) throw JSON.stringify({ id: "edit_error.config_bounds", params: {}, fallback: "refresh_seconds must be >= 30." });
    demoSettings.refresh = seconds;
    demoSettings.generation++;
  },
  move_account: async ({ from, to }) => {
    if (from < 0 || to < 0 || from >= demoReports.length || to >= demoReports.length) throw "Account does not exist.";
    const [account] = demoReports.splice(from, 1);
    demoReports.splice(to, 0, account);
    demoSettings.generation++;
  },
  // RF4 demo stubs: no state to reload in the demo, but keep the
  // contract so the wizard/reloder don't have to guard the
  // non-desktop path.
  reload_config: async () => { demoSettings.generation++; return demoView(); },
  history_relocation: async () => { demoSettings.relocationAnswered = true; return null; },
  set_data_dir: async ({ path }) => { if (!DEMO_DATA_DIR.data_dir_env_locked) DEMO_DATA_DIR.data_dir_custom = path ?? null; },
  model_state: demoModels,
  load_models: demoModels,
  sync_models: demoModels,
  model_support: async () => ({ supported: false, text: { id: "models.demo", params: {}, fallback: "Browser demo: the model catalog is only queried in the desktop app." } }),
  config_changed_on_disk: async () => false,
  get_settings: async () => ({ user_dir: demoSettings.custom ?? "~/.config/cuotai", default_dir: "~/.config/cuotai", custom_dir: demoSettings.custom, env_locked: false, ...DEMO_DATA_DIR, config_file: demoSettings.config, config_env_locked: false, motion: get("cuotai.motion", "system"), show_pace: get(PACE_KEY, "on") !== "off",
    theme: get("cuotai.theme", "") || null, theme_mode: get("cuotai.themeMode", "system"), sort: get(SORT_KEY, "custom"), aggregators_in_all: get("cuotai.aggregators-in-all", "grouped"), group_recharging: get(GROUP_KEY, "off") === "on", show_in_dock: demoSettings.showInDock !== false, locale: get("cuotai.locale", "") || null, local_storage_migrated: true,
    live_session: get(LIVE_KEY, "on") !== "off", history_days: demoSettings.historyDays ?? 90, history_days_min: 7, history_days_max: 365, history_recording: demoSettings.historyRecording, status_queries: demoSettings.statusQueries,
    notify: { ...demoSettings.notify }, notify_leads: [0, 5, 15, 60], notify_whens: ["used", "exhausted", "always"],
    alerts: structuredClone(demoSettings.alerts), quiet: structuredClone(demoSettings.quiet), alert_min: 10, alert_max: 99, detect: structuredClone(demoSettings.detect), 
    tray: { ...demoSettings.tray }, advanced_open: !!demoSettings.advancedOpen, stats: { ...demoSettings.stats },
    profile_existed: !!demoSettings.profileExisted, whats_new_auto: demoSettings.whatsNewAuto !== false, whats_new_due: demoSettings.whatsNewDue === true, monograms: { ...demoSettings.monograms }, hidden_accounts: demoHidden(), rows: structuredClone(demoSettings.rows ?? {}), onboarding: demoSettings.onboarding, onboarding_version: 1, version: "demo", license: "MIT OR Apache-2.0" }),
  set_user_dir: async ({ path }) => { demoSettings.custom = path ?? null; },
  set_motion: async ({ motion }) => { if (!MOTIONS.includes(motion)) throw "invalid_motion"; },
  set_show_pace: async () => {},
  set_preference: async ({ key, value }) => {
    const ok = { theme: typeof value === "string" && /^[\w.-]{1,64}$/.test(value), theme_mode: THEME_MODE_IDS.includes(value), sort: SORT_MODES.includes(value), group_recharging: typeof value === "boolean", show_in_dock: typeof value === "boolean", aggregators_in_all: AGG_MODES.includes(value) };
    if (!ok[key]) throw "invalid_preference";
    if (key === "show_in_dock") demoSettings.showInDock = value;
    mirrorPreference(key, value);
    await emitSettingsChanged(key);
  },
  migrate_local_storage: async () => demo.get_settings(),
  tray_preview: async (args) => demoTrayPreview(args),
  tray_accounts: async () => demoTrayAccounts(),
  set_tray_prefs: async ({ style, colored, accounts, popoverView: popover_view }) => {
    if (!["meters", "logos_percent", "tightest_percent", "icon_only"].includes(style)) throw "invalid_tray_style";
    if (popover_view != null && !["list", "rings", "compact", "grid"].includes(popover_view)) throw "invalid_popover_view";
    demoSettings.tray = { style, colored, accounts: accounts ?? null, popover_view: popover_view ?? null };
  },
  set_advanced_open: async ({ open }) => { demoSettings.advancedOpen = open; },
  set_stats_prefs: async ({ range, timeline }) => {
    if (range != null && !["days7", "days30"].includes(range)) throw "invalid_stats_range";
    if (timeline != null && !["hours24", "days7"].includes(timeline)) throw "invalid_timeline_range";
    const next = { ...demoSettings.stats, ...(range != null && { range }), ...(timeline != null && { timeline }) };
    if (next.range === "days7") delete next.range;
    if (next.timeline === "hours24") delete next.timeline;
    demoSettings.stats = next;
  },
  set_whats_new_auto: async ({ on }) => { demoSettings.whatsNewAuto = !!on; },
  whats_new_seen: async () => { demoSettings.whatsNewDue = false; },
  set_hidden_accounts: async ({ keys }) => {
    put(DEMO_HIDDEN, JSON.stringify([...new Set(keys ?? [])].filter((k) => typeof k === "string")));
    await emitSettingsChanged("hidden_accounts");
  },
  set_account_monogram: async ({ accountId, monogram }) => {
    if (!validMonogram(monogram)) throw "invalid_monogram";
    const text = String(monogram).trim();
    if (text) demoSettings.monograms[accountId] = text; else delete demoSettings.monograms[accountId];
    await emitSettingsChanged("monograms");
  },
  set_rows: async ({ rows }) => { demoSettings.rows = structuredClone(rows); },
  autostart_status: async () => !!demoSettings.autostart,
  autostart_set: async ({ enabled }) => { demoSettings.autostart = !!enabled; return demoSettings.autostart; },
  autostart_stale: async () => !!demoSettings.autostart && !!demoSettings.autostartStale,
  autostart_repair: async () => { demoSettings.autostartStale = false; return !!demoSettings.autostart; },
  // Re-validation V-19: `?notify-denied` plays the refused permission (RF6.2): nothing is turned on.
  first_run_turn_on: async () => {
    demoSettings.onboarding = 1;
    if (demoPermission(true) === "denied") return "denied";
    demoSettings.alerts.enabled = true; demoSettings.alerts.overage = true; demoSettings.alerts.projected_lead_minutes = 60;
    return "granted";
  },
  first_run_record: async ({ again }) => { demoSettings.onboarding = again ? 0 : 1; },
  set_advice_enabled: async () => {},
  set_live_session: async () => {},
  dismiss_feed_banner: async ({ source }) => { if (source !== "claude_code" && source !== "codex_cli") throw "invalid_source"; },
  claude_code_snippets: async () => ({
    cli: null, install: null,
    settings: JSON.stringify({ statusLine: { type: "command", command: "cuotai --feed claude-statusline --print" } }, null, 2),
    chained: JSON.stringify({ statusLine: { type: "command", command: `sh -c 'input=$(cat); printf %s "$input" | cuotai --feed claude-statusline; printf %s "$input" | ~/.claude/statusline.sh'` } }, null, 2),
  }),
  forget_claude_code_readings: async () => ({ id: "feed.forget.done", params: { agent: "Claude Code" }, fallback: "The stored Claude Code readings were erased." }),
  set_pace_unit: async ({ unit }) => { if (!["points", "percent"].includes(unit)) throw "invalid_pace_unit"; },
  set_history_days: async ({ days }) => { if (!(Number.isInteger(days) && days >= 7 && days <= 365)) throw "invalid_history_days"; demoSettings.historyDays = days; },
  // ponytail: the browser demo has no history file; fixed counts let the export and erase flows be driven, and an
  // erased account (or everything) leaves the synthetic history of `account_history`.
  history_export: async ({ index, generation, format }) => { if (index != null && generation !== demoSettings.generation) throw STALE; return { rows: index == null ? 42 : 7, path: `/Users/demo/cuotai-history.${format}` }; },
  history_erase: async ({ index, generation }) => { if (index != null && generation !== demoSettings.generation) throw STALE; demoSettings.historyErased.add(index ?? "all"); demoSettings.historyEpoch++; return index == null ? 42 : 7; },
  // The core's word in the UI language (the demo reads the core catalogs), so every locale shows its own word.
  erase_all_plan: async () => ({ word: demoEraseWord(), items: [
    { id: "history.db", location: "~/Library/Application Support/cuotai/history/history.sqlite3", kind: "file", erase: true },
    { id: "desktop.settings", location: "~/Library/Application Support/cuotai/desktop.json", kind: "file", erase: true },
    { id: "vault.oauth", location: "cuotai.oauth", kind: "vault_service", erase: true, label: "Saved sign-ins, keys and channel addresses" },
    { id: "webview.storage", location: "dev.cuotai.desktop (WebKit / WebView2)", kind: "app_managed", erase: true, app_only: true, label: "The app's window storage" },
    { id: "autostart.entry", location: "~/Library/LaunchAgents/<app>.plist", kind: "app_managed", erase: true, app_only: true, label: "Open at login" },
    { id: "config.accounts", location: "~/.config/cuotai/config.toml", kind: "file", erase: false },
  ] }),
  // The demo never erases anything: it answers as a partial erase so the page stays usable.
  erase_all_data: async ({ word }) => {
    if (word.trim().normalize("NFC") !== demoEraseWord().normalize("NFC")) throw JSON.stringify({ id: "privacy.erase.word_mismatch", params: {}, fallback: "The confirmation word does not match; nothing was erased." });
    return { complete: false, removed: 0, absent: 0, failed: ["demo"] };
  },
  set_history_recording: async ({ on }) => { demoSettings.historyRecording = on; demoSettings.historyEpoch++; },
  set_status_enabled: async ({ on }) => { demoSettings.statusQueries = on; },
  serve_status: async () => demoServeView(),
  set_serve: async ({ enabled, port, allowBrowsers, allowedOrigins = [] }) => {
    if (!Number.isInteger(port) || port < 1024 || port > 65535) throw "invalid_port";
    // ponytail: demo mirror of the core's PRD 0050 RF4 refusals; the app's rules live in `serve::Origin`.
    const fail = (id, fallback) => { throw JSON.stringify({ id, params: {}, fallback }); };
    // Re-validation V-02: the token comes first, as in the core, so the first refusal points to Generate token.
    if (allowBrowsers && demoSettings.serve.secret == null) fail("serve.error.browser_token_required", "Allowing browsers needs an access token for the local server: generate one first.");
    const list = allowedOrigins.map((o) => o.trim()).filter(Boolean);
    if (list.some((o) => !/^(https:\/\/[a-z0-9.-]+|http:\/\/(localhost|127\.0\.0\.1|\[::1\]))(:\d{1,5})?\/?$/i.test(o))) {
      fail("serve.origin.error.scheme", "Use https://host[:port], or http:// only for localhost and loopback addresses.");
    }
    if (allowBrowsers && !list.length) fail("serve.origin.error.empty", "Add at least one trusted origin before allowing browsers.");
    Object.assign(demoSettings.serve, { enabled, port, allow_browsers: allowBrowsers, allowed_origins: list.map((o) => o.replace(/\/$/, "").toLowerCase()) });
    // PRD 0012 RF5: turning the server on creates its token when there is none.
    if (enabled && demoSettings.serve.secret == null) demoSettings.serve.secret = `cuotai_demo_${crypto.randomUUID().replaceAll("-", "")}`;
    return demoServeView();
  },
  set_serve_stats: async ({ stats }) => { demoSettings.serve.stats = stats; return demoServeView(); },
  set_serve_access_log: async ({ mode }) => {
    if (!["off", "errors", "all"].includes(mode)) throw "invalid_access_log";
    demoSettings.serve.access_log = mode;
    // ponytail: demo stand-in for one recorded entry; nothing is written in the browser.
    if (mode !== "off" && !demoSettings.serve.access_log_bytes) demoSettings.serve.access_log_bytes = 2048;
    return demoServeView();
  },
  erase_serve_access_log: async () => { demoSettings.serve.access_log_bytes = 0; return demoServeView(); },
  generate_serve_token: async () => { demoSettings.serve.secret = `cuotai_demo_${crypto.randomUUID().replaceAll("-", "")}`; return demoServeView(); },
  // ponytail: demo mirror of the widget snapshot (PRD 0014 RF4.1); nothing is written in the browser. `?widgets`
  // starts with publishing on and an account limit already chosen.
  widgets_status: async () => demoWidgetsView(),
  set_widgets: async ({ enabled, includeIdentity, maxAccounts }) => {
    if (!Number.isInteger(maxAccounts) || maxAccounts < 1 || maxAccounts > 24) throw "invalid_widget_accounts";
    Object.assign(demoSettings.widgets, { enabled, include_identity: includeIdentity, max_accounts: maxAccounts });
    return demoWidgetsView();
  },
  delete_widget_data: async () => { demoSettings.widgets.written = false; },
  // PRD 0012 RF5: revoking turns the server off; it never serves without a token.
  revoke_serve_token: async () => { if (demoSettings.serve.allow_browsers) throw "browsers_need_token"; demoSettings.serve.secret = null; demoSettings.serve.enabled = false; return demoServeView(); },
  copy_serve_token: async () => {
    if (demoSettings.serve.secret == null) throw JSON.stringify({ id: "serve.error.token_missing", params: {}, fallback: "The local server token is no longer in the system vault; generate a new one or revoke the token." });
    return demoSettings.serve.secret;
  },
  cost_status: async () => demoCostView(),
  cost_usage: async () => demoCostUsage(),
  cost_breakdown: async ({ by, offset }) => demoBreakdown(by, offset),
  cost_set_label: async ({ digest, label }) => {
    const text = (label ?? "").trim();
    if (text.length > 48) throw JSON.stringify(demoL10n("cost.error.invalid_label", { max: "48" }, "A label has 1 to 48 characters."));
    // PRD 0042 re-validation S-05: the core refuses control, direction and invisible characters (`text::is_hidden`).
    if (/[\p{Cc}\p{Cf}]/u.test(text)) throw JSON.stringify(demoL10n("cost.error.label_characters", {}, "A label cannot contain invisible or direction characters."));
    if (text) demoCost.labels[digest] = text; else delete demoCost.labels[digest];
  },
  cost_set_hidden: async ({ digest, hidden }) => { if (hidden) demoCost.hidden.add(digest); else demoCost.hidden.delete(digest); },
  cost_set_source_options: async ({ source, blockHours, folderHint }) => {
    if (!(Number.isInteger(blockHours) && blockHours >= 1 && blockHours <= 24)) {
      throw JSON.stringify(demoL10n("cost.error.block_hours", { min: "1", max: "24" }, "The block length ranges from 1 to 24 hours."));
    }
    if (blockHours === DEMO_BLOCK_HOURS[source]) delete demoCost.hours[source]; else demoCost.hours[source] = blockHours;
    demoCost.hints[source] = folderHint;
    return demoCostView();
  },
  cost_prices: async () => demoPrices(),
  cost_set_paths: async ({ source, paths }) => {
    const chosen = [...new Set(paths.map((p) => p.trim()).filter(Boolean))];
    if (chosen.some((p) => !(p.startsWith("/") || p === "~" || p.startsWith("~/")))) {
      throw JSON.stringify(demoL10n("cost.error.relative_path", {}, "Use a directory that is absolute or starts with ~/."));
    }
    if (JSON.stringify(chosen) === JSON.stringify(demoCost.paths[source] ?? [])) return demoCostView();
    if (source in demoCost.on) { delete demoCost.on[source]; demoCost.erased.add(source); }
    if (chosen.length) demoCost.paths[source] = chosen; else delete demoCost.paths[source];
    return demoCostView();
  },
  // The demo stands in for the native dialog by accepting it.
  cost_enable: async ({ source }) => {
    if (!DEMO_COST[source]) {
      throw JSON.stringify(demoL10n("cost.error.consent_changed", {}, "The directories of this source changed since they were shown; review and confirm again."));
    }
    demoCost.on[source] = new Date().toISOString();
    demoCost.erased.delete(source);
    return demoCostView();
  },
  cost_disable: async ({ source }) => { delete demoCost.on[source]; demoCost.erased.add(source); return demoCostView(); },
  cost_set_options: async ({ period, retentionDays }) => {
    if (!(Number.isInteger(retentionDays) && retentionDays >= 7 && retentionDays <= 730)) {
      throw JSON.stringify(demoL10n("cost.error.retention_bounds", { min: "7", max: "730" }, "The cost index retention ranges from 7 to 730 days."));
    }
    Object.assign(demoCost, { period, retention: retentionDays });
    return demoCostView();
  },
  cost_plans: async () => demoPlansView(),
  cost_set_plan: async ({ id, price, currency, cycleDay }) => {
    if (price == null) { delete demoPlan.plans[id]; return demoPlansView(); }
    if (!(price > 0) || !/^[A-Za-z]{3}$/.test(currency) || (cycleDay != null && !(cycleDay >= 1 && cycleDay <= 28))) {
      throw JSON.stringify(demoL10n("cost.error.invalid_plan", {}, "Invalid plan: a price above zero, a three-letter currency and a cycle day from 1 to 28."));
    }
    demoPlan.plans[id] = { price: Math.round(price * 100) / 100, currency: currency.toUpperCase(), cycle_day: cycleDay ?? null };
    return demoPlansView();
  },
  cost_set_display: async ({ code, rate }) => {
    if (code == null) { demoPlan.display = null; return demoPlansView(); }
    if (!/^[A-Za-z]{3}$/.test(code) || code.toUpperCase() === "USD" || !(rate > 0)) {
      throw JSON.stringify(demoL10n("cost.error.invalid_rate", {}, "Invalid rate: a three-letter currency other than USD and a rate above zero."));
    }
    // L-01: six significant digits are kept; a rate under 0.000001 has its own refusal.
    const kept = Number(rate.toPrecision(6));
    if (kept < 0.000001) throw JSON.stringify(demoL10n("cost.error.rate_too_small", {}, "The rate is too small: use at least 0.000001."));
    demoPlan.display = { code: code.toUpperCase(), rate: kept, set_on: dayKey(new Date()) };
    return demoPlansView();
  },
  cost_erase: async () => { for (const id of Object.keys(DEMO_COST)) demoCost.erased.add(id); return 57; },
  // PRD 0018 RF2.6/RF3 browser demo: the same one-time answer and backend
  // request gate, without network access or installation.
  updater_status: async () => demoUpdateView(),
  updater_set_automatic: async ({ on }) => {
    if (!demoUpdate.consentAnswered) throw "updater_consent_required";
    demoUpdate.automatic = on;
    demoUpdate.disabledReason = on ? null : "preference";
    // S-0018-04: off drops the announced update, a download and a failure kept for a retry.
    if (!on && ["available", "downloading", "failed"].includes(demoUpdate.phase)) demoUpdate.phase = "idle";
    demoUpdateEmit(demoUpdateView());
    return demoUpdateView();
  },
  updater_answer_automatic: async ({ on }) => {
    demoUpdate.consentAnswered = true;
    demoUpdate.automatic = on;
    demoUpdate.disabledReason = on ? null : "preference";
    const reply = demoUpdateView();
    if (on && new URLSearchParams(location.search).has("update-consent-race")) {
      demoUpdate.phase = "available";
      demoUpdateEmit(demoUpdateView());
    } else {
      demoUpdateEmit(reply);
    }
    return reply;
  },
  updater_check_now: async () => {
    if (!demoUpdateView().enabled) throw demoUpdate.consentAnswered ? "updater_off" : "updater_consent_required";
    Object.assign(demoUpdate, { phase: "checking" });
    demoUpdateEmit(demoUpdateView());
    await demoWait(600);
    const fresh = new URLSearchParams(location.search).has("update-fresh");
    Object.assign(demoUpdate, { phase: fresh ? "uptodate" : "available", lastCheck: new Date().toISOString(), dismissed: false, progress: null, error: null });
    demoUpdateEmit(demoUpdateView());
    return demoUpdateView();
  },
  updater_install: async () => {
    const view = demoUpdateView();
    if (!view.enabled) throw "updater_off";
    if (view.phase !== "available" && !view.can_retry) throw "no_update";
    Object.assign(demoUpdate, { phase: "downloading", progress: 0, dismissed: false });
    demoUpdateEmit(demoUpdateView());
    for (const progress of [18, 47, 78, 100]) {
      await demoWait(350);
      if (demoUpdate.phase !== "downloading") return demoUpdateView(); // cancelado
      demoUpdate.progress = progress;
      demoUpdateEmit(demoUpdateView());
    }
    await demoWait(400);
    Object.assign(demoUpdate, { phase: "installing", progress: null });
    demoUpdateEmit(demoUpdateView());
    await demoWait(400);
    demoUpdate.phase = "ready";
    demoUpdateEmit(demoUpdateView());
    return demoUpdateView();
  },
  updater_cancel_download: async () => { if (demoUpdate.phase === "downloading") demoUpdate.phase = "available"; demoUpdateEmit(demoUpdateView()); return demoUpdateView(); },
  updater_dismiss: async () => { demoUpdate.dismissed = true; demoUpdateEmit(demoUpdateView()); return demoUpdateView(); },
  updater_restart: async () => { window.__lastUpdateRestart = true; },
  hooks_status: async () => demoHooksView(),
  // ponytail: the app asks in a native dialog (PRD 0050 RF2); the demo has none and accepts, approving what it lists.
  set_hooks_enabled: async ({ on }) => {
    if (on && demoHooks.rules && !demoHooksError(demoHooks.rules)) demoHooks.confirmed = demoDigest(demoHooks.rules);
    demoHooks.enabled = on;
    return demoHooksView();
  },
  confirm_hooks: async () => {
    if (!demoHooks.rules) throw JSON.stringify(demoL10n("hooks.error.changed", {}, "O hooks.toml mudou desde que foi exibido; revise as regras de novo."));
    demoHooks.confirmed = demoDigest(demoHooks.rules);
    return demoHooksView();
  },
  save_hooks: async ({ revision, rules }) => {
    if (revision !== demoHooks.revision) throw JSON.stringify(demoL10n("hooks.error.changed", {}, "O hooks.toml mudou desde que foi exibido; revise as regras de novo."));
    const error = demoHooksError(rules);
    if (error) throw JSON.stringify(error);
    demoHooks.rules = structuredClone(rules);
    demoHooks.revision = `r${Date.now()}`;
    return demoHooksView();
  },
  test_hook: async ({ name }) => {
    const view = demoHooksView();
    if (view.state !== "on") throw "hooks_off";
    const rule = demoHooks.rules.find((r) => r.name === name);
    if (!rule) throw "hook_unknown";
    setTimeout(() => {
      const run = { at: new Date().toISOString(), rule: name, event: rule.events[0], duration_ms: 42, outcome: "exited", exit_code: 0 };
      demoHooks.runs.unshift({ ...run, summary: demoL10n("hooks.run.succeeded", { rule: name, event: run.event, ms: 42 }, `${name} (${run.event}): done in 42 ms`) });
      for (const cb of demoHookListeners) cb();
    }, 300);
    return view;
  },
  // `?notify-denied` plays a system that refuses the permission (PRD 0003 RF3.3, reused by PRD 0008 RF3.3).
  set_notify_enabled: async ({ on }) => {
    const permission = demoPermission(on);
    demoSettings.notify.enabled = permission === "granted";
    return permission;
  },
  set_alerts_enabled: async ({ on }) => {
    const permission = demoPermission(on);
    demoSettings.alerts.enabled = permission === "granted";
    return permission;
  },
  set_projected_alert: async ({ on, minutes }) => {
    const permission = demoPermission(on);
    if (on && ![15, 30, 60].includes(minutes)) throw "invalid_lead";
    if (permission === "granted") demoSettings.alerts.projected_lead_minutes = minutes;
    else delete demoSettings.alerts.projected_lead_minutes;
    return permission;
  },
  set_alert_options: async ({ thresholds, exhaustion, recovery, overage }) => {
    if (!thresholds.every((t) => Number.isInteger(t) && t >= 10 && t <= 99)) throw "invalid_threshold";
    Object.assign(demoSettings.alerts, { thresholds: [...new Set(thresholds)].sort((a, b) => a - b), exhaustion, recovery });
    if (overage != null) demoSettings.alerts.overage = overage;
  },
  set_metric_alert: async ({ accountKey, label, on }) => {
    const muted = (demoSettings.alerts.muted[accountKey] ?? []).filter((l) => l !== label);
    if (!on) muted.push(label);
    if (muted.length) demoSettings.alerts.muted[accountKey] = muted;
    else delete demoSettings.alerts.muted[accountKey];
  },
  set_pool_alerts: async ({ accountKey, each }) => {
    const keys = new Set(demoSettings.alerts.pool_each ?? []);
    if (each) keys.add(accountKey);
    else keys.delete(accountKey);
    demoSettings.alerts.pool_each = [...keys].sort();
  },
  set_breakdown_alerts: async ({ accountKey, on }) => {
    const keys = new Set(demoSettings.alerts.breakdown_each ?? []);
    if (on) keys.add(accountKey);
    else keys.delete(accountKey);
    demoSettings.alerts.breakdown_each = [...keys].sort();
  },
  set_notify_options: async ({ leadMinutes, when, includeOther }) => {
    if (![0, 5, 15, 60].includes(leadMinutes) || !["used", "exhausted", "always"].includes(when)) throw "invalid_notify";
    Object.assign(demoSettings.notify, { lead_minutes: leadMinutes, when, include_other: includeOther });
  },
  set_notify_private: async ({ on }) => { demoSettings.notify.private = on; },
  set_quiet_hours: async ({ quiet }) => {
    const refused = demoQuietRefusal(quiet);
    if (refused) throw JSON.stringify(refused);
    demoSettings.quiet = { enabled: quiet.enabled, digest: quiet.digest, days: structuredClone(quiet.days ?? {}) };
  },
  acknowledge_missed: async ({ accountKey }) => { demoSettings.missedSeen.add(accountKey); },
  set_account_notify: async ({ accountKey, on }) => {
    const off = demoSettings.notify.off_accounts.filter((k) => k !== accountKey);
    demoSettings.notify.off_accounts = on ? off : [...off, accountKey];
  },
  set_locale: async () => {},
  set_callback_style: async () => {},
  send_test_notification: async ({ title, body }) => {
    if (!demoSettings.notify.enabled && !demoSettings.alerts.enabled) throw "notify_off";
    window.__lastTestNotification = { title, body };
  },
  set_insight_prefs: async ({ limitChanges, daysOff, digestEnabled, digestWeekday, digestHour }) => {
    if (digestWeekday > 6 || digestHour > 23) throw "invalid_notify";
    Object.assign(demoSettings.notify, { limit_changes: limitChanges, days_off: daysOff, digest: { enabled: digestEnabled, weekday: digestWeekday, hour: digestHour } });
  },
  send_test_digest: async () => {
    if (!demoSettings.notify.enabled && !demoSettings.alerts.enabled) throw "notify_off";
    window.__lastTestNotification = { title: "Your week in Cuotai", body: "From the readings recorded on this computer." };
  },
  // ponytail: demo mirror of the core's channels (PRD 0044) with the host checks reduced; nothing is sent.
  channels_view: async () => demoChannelsView(),
  channel_consent: async ({ draft, url }) => demoChannelConsent(draft, url),
  channel_add: async ({ draft, url }) => {
    const consent = demoChannelConsent(draft, url);
    if (demoChannels.length >= 5) throw JSON.stringify(demoL10n("channels.error.too_many", { max: 5 }, "At most 5 channels; remove one first."));
    const channel = { id: Math.random().toString(16).slice(2, 18).padEnd(16, "0"), kind: draft.kind, label: draft.label.trim(), host: consent.host,
      filters: draft.filters, enabled: true, created_at: new Date().toISOString(), signed: !!draft.sign && draft.kind === "webhook" };
    demoChannels.push(channel);
    return { channel, signing_secret: channel.signed ? "0".repeat(64) : null };
  },
  channel_edit: async ({ id, draft, url }) => {
    const channel = demoChannels.find((c) => c.id === id);
    if (!channel) throw JSON.stringify(demoL10n("channels.error.unknown", {}, "There is no such channel."));
    if (url) channel.host = demoChannelConsent(draft, url).host;
    Object.assign(channel, { label: draft.label.trim(), filters: draft.filters });
    return { channel, signing_secret: null };
  },
  channel_set_enabled: async ({ id, on }) => { const c = demoChannels.find((x) => x.id === id); if (c) c.enabled = on; },
  channel_remove: async ({ id }) => { demoChannels = demoChannels.filter((c) => c.id !== id); },
  channel_test: async ({ id }) => {
    const c = demoChannels.find((x) => x.id === id);
    demoChannelLog.unshift({ at: new Date().toISOString(), channel: id, kind: "test", outcome: "delivered", status: 200, attempt: 1,
      kind_label: demoL10n("channels.log_kind.test", {}, "Test"), error_label: null });
    c.last = { at: new Date().toISOString(), outcome: "delivered", status: 200 };
    return { outcome: "delivered", status: 200 };
  },
  // Demo: the core's `check_budgets` (PRD 0041 RF5.1, F-05, F-07, F-10, F-11) on the rows as typed, reduced to the
  // amount, ambiguous amount, unit mismatch and too-many rules; same answer shape (problems with field, rule and a
  // note naming the field's label; the sentences; each row's metric unit from the account's reading).
  check_budgets: async (args) => demoCheckBudgets(args),
  // Demo: no history, so no strip; `?strip` shows the 24-hour strip of the first account (PRD 0043 RF4.2).
  hourly_strip: async () => (DEMO_QUERY.has("strip") ? { name: "Claude", rhythm: demoRhythm(false).last_24h } : null),
  // Demo: the core's suggestion for the metric `label` of account `index`: its draft and, for a spend or usage metric,
  // the "Last 30 days" reference line (no plan price in the demo). Unknown account or metric: the core's invalid index.
  budget_suggestion: async ({ index, label }) => {
    const m = demoReports[index]?.metrics.find((x) => x.label === label && !x.derived);
    if (!m) throw JSON.stringify(demoL10n("models.error.invalid_index", {}, "Account does not exist in this configuration."));
    const budget = m.budget_draft ?? { kind: m.kind === "credits" ? "credits" : m.kind === "usage" ? "usage" : "spend", label_contains: m.label_text?.fallback ?? m.label,
      amount: 0, unit: m.unit, period: "monthly" };
    const reference = ["spend", "usage"].includes(m.kind)
      ? { id: "budget.reference.last_30_days", params: { amount: m.unit === "USD" ? demoMoney(38.2) : `38.2 ${m.unit}` }, fallback: `Last 30 days: ${m.unit === "USD" ? "$38.20" : `38.2 ${m.unit}`}` }
      : null;
    return { budget, ...(reference ? { reference } : {}) };
  },
  history_size: async () => (demoSettings.historyErased.has("all") ? 4096 : 1887437),
  account_history: demoHistory,
  stats: demoStats,
  reset_timeline: demoTimeline,
  set_config_file: async ({ path }) => { demoSettings.config = path ?? null; demoSettings.generation++; return demoView(); },
  open_user_dir: async () => {},
  // Demo: one sample section (in the app, the shell picks the running version's section of the embedded changelog).
  whats_new: async () => ({
    current: "demo",
    title: { id: "changelog.title", params: {}, fallback: "What's new" },
    entries_language: { id: "changelog.entries_language", params: {}, fallback: "The entries stay in English, the language of the changelog." },
    release: { version: "demo", date: "2026-10-25", current: true, paragraphs: [], categories: [
      { label: { id: "changelog.category.added", params: {}, fallback: "Added" }, entries: ["Sample entry for the What's new screen in the browser demo."] },
    ] },
    full_changelog: CHANGELOG_URL,
  }),
  // Demo: sample of the real list (in the app, the full list comes from the core).
  // Demo: the report text is the core's; here, an example with no data at all.
  report_problem: async ({ index }) => (index == null
    ? { title: "[Bug] ", body: "## Problem report\n\n- Cuotai version: demo\n- OS and architecture: macos aarch64\n- Install channel: unknown\n\nNo account name, e-mail, organization, credential, URL or response value is included.\n", copy_only: false }
    : {
      title: "[Provider] zed: schema_changed",
      body: "## Provider problem report\n\n- Cuotai version: demo\n- OS and architecture: macos aarch64\n- Install channel: unknown\n- Provider: `zed`\n- Connection mode: `native_login`\n- Access level: undocumented (verified on 2026-10-25)\n- Failure category: `schema_changed`\n- Drifted fields: `models[].id` (string -> number)\n- Response fingerprint (SHA-256): `3f1c0d2a9b8e7f6a5d4c3b2a1908f7e6d5c4b3a291807f6e5d4c3b2a1908f7e6`\n- Last good reading: 2 h ago\n- Provider incident active: unknown\n\nNo account name, e-mail, organization, credential, URL or response value is included.\n",
      copy_only: false,
    }),
  // Demo: the shell opens the draft itself; here, the bare form.
  open_problem_report: async ({ index }) => {
    window.open(`https://github.com/jmarcon/cuotai/issues/new?template=${index == null ? "bug" : "provider-problem"}.yml`, "_blank", "noopener,noreferrer");
  },
  about_info: async () => ({
    name: "Cuotai", version: "demo", license: "MIT OR Apache-2.0", repository: "https://github.com/jmarcon/cuotai",
    libraries: [
      { name: "tokio", license: "MIT", repository: "https://github.com/tokio-rs/tokio", role: "runtime", used_by: ["core", "tui", "desktop"] },
      { name: "tauri", license: "Apache-2.0 OR MIT", repository: "https://github.com/tauri-apps/tauri", role: "runtime", used_by: ["desktop"], from_brazil: true },
      { name: "ratatui", license: "MIT", repository: "https://github.com/ratatui/ratatui", role: "runtime", used_by: ["tui"] },
      { name: "wiremock", license: "MIT OR Apache-2.0", repository: "https://github.com/LukeMathWalker/wiremock-rs", role: "test", used_by: ["core"] },
      { name: "Lobe Icons", license: "MIT", repository: "https://github.com/lobehub/lobe-icons", role: "asset", used_by: ["desktop"] },
      { name: "Dracula", license: "MIT", repository: "https://github.com/dracula/dracula-theme", role: "theme", used_by: ["tui", "desktop"], from_brazil: true },
      { name: "Catppuccin", license: "MIT", repository: "https://github.com/catppuccin/catppuccin", role: "theme", used_by: ["desktop"] },
      { name: "9router", license: "MIT", repository: "https://github.com/decolua/9router", role: "reference", used_by: ["core"] },
    ],
  }),
};

// ---------- account order ----------
// Custom mode follows `report.index`, the order persisted in the TOML by the
// core and shared with the TUI. Only the ordering mode is a local preference.
const SORT_KEY = "cuotai.sort";
// "binding" (PRD 0009 RF2.3): the core's "most restrictive first" order (`binding_rank`); it only changes the
// display, so the manual order in the TOML stays the default and is never rewritten.
export const SORT_MODES = ["custom", "binding", "reset-asc", "reset-desc", "pct-asc", "pct-desc"];
let sortMode = SORT_MODES.includes(get(SORT_KEY, "custom")) ? get(SORT_KEY, "custom") : "custom";
export const currentSort = () => sortMode;
export const setSort = (mode) => { if (SORT_MODES.includes(mode)) { sortMode = mode; setPreference("sort", mode).catch(() => {}); } };
// PRD 0010 RF4.2: optional "Recharging" group, off by default; display only, the TOML order is never rewritten.
const GROUP_KEY = "cuotai.group-recharging";
let groupRecharging = get(GROUP_KEY, "off") === "on";
export const groupingRecharging = () => groupRecharging;
export const setGroupRecharging = (on) => { groupRecharging = on; setPreference("group_recharging", !!on).catch(() => {}); };
/** [others, recharging] in the order already chosen; with grouping off, everything stays in the first list. */
// PRD 0022 RF8.1 (DS-CMP-SHELL-5) with PRD 0025 RF4.3: the tray view is ONE preference, `tray.popover_view` in
// desktop.json: "list" (the default), "rings" or "compact". The 0025 value "grid" is the ring grid, so it reads as
// "rings"; the copy an older version kept in localStorage ("donut" or "list") moves there once.
const TRAY_VIEW_KEY = "cuotai.trayView";
export const TRAY_VIEWS = ["list", "rings", "compact"];
export const trayViewOf = (v) => (v === "grid" || v === "donut" ? "rings" : TRAY_VIEWS.includes(v) ? v : "list");
let trayView = "list";
const trayViewListeners = new Set();
export const trayViewPref = () => trayView;
export const onTrayViewChange = (cb) => trayViewListeners.add(cb);
function followTrayView(v) {
  const next = trayViewOf(v);
  if (next === trayView) return;
  trayView = next;
  trayViewListeners.forEach((f) => f());
}
/** Saves the view with the rest of the menu-bar preferences (one `set_tray_prefs`, which tells every window). */
export async function setTrayView(v) {
  if (!TRAY_VIEWS.includes(v)) return;
  const s = await getSettings();
  const tray = s.tray ?? {};
  await setTrayPrefs({ style: tray.style ?? "tightest_percent", colored: !!tray.colored, accounts: tray.accounts ?? null, popoverView: v });
  followTrayView(v);
  if (!isDesktop) emitSettingsChanged("tray");
}
/** Reads the stored view (running the one-time localStorage move) and follows later changes from any window. */
export async function initTrayView() {
  let legacy = null;
  try { legacy = localStorage.getItem(TRAY_VIEW_KEY); } catch {}
  try {
    const s = await getSettings();
    if (s.tray?.popover_view) followTrayView(s.tray.popover_view);
    else if (legacy) await setTrayView(trayViewOf(legacy));
    try { localStorage.removeItem(TRAY_VIEW_KEY); } catch {}
  } catch {}
  if (isDesktop) T.event.listen("tray-prefs-changed", (e) => followTrayView(e.payload?.popover_view));
  onSettingsChanged((key) => { if (key === "tray" || !key) getSettings().then((s) => followTrayView(s.tray?.popover_view), () => {}); });
}
export const splitRecharging = (list) => (groupRecharging ? [list.filter((r) => !isRecharging(r)), list.filter(isRecharging)] : [list, []]);
// PRD 0007 RF3.4: consumption pace, on by default; `desktop.json` is the source, localStorage mirrors it for windows.
const PACE_KEY = "cuotai.pace";
let showPace = get(PACE_KEY, "on") !== "off";
const applyPace = (on) => { showPace = on !== false; put(PACE_KEY, showPace ? "on" : "off"); };
export const paceShown = () => showPace;
// PRD 0024 RF1.3: "points" (default) or "percent"; the core ships both chips and the view only picks one.
const PACE_UNIT_KEY = "cuotai.pace.unit";
let paceUnitNow = get(PACE_UNIT_KEY, "points") === "percent" ? "percent" : "points";
const applyPaceUnit = (unit) => { paceUnitNow = unit === "percent" ? "percent" : "points"; put(PACE_UNIT_KEY, paceUnitNow); };
export const paceUnit = () => paceUnitNow;
// PRD 0024 RF7.5: the best account of each group of comparable accounts (on unless turned off).
const ADVICE_KEY = "cuotai.advice";
let adviceNow = get(ADVICE_KEY, "on") !== "off";
const applyAdvice = (on) => { adviceNow = on !== false; put(ADVICE_KEY, adviceNow ? "on" : "off"); };
export const adviceShown = () => adviceNow;
export const setAdviceEnabled = async (on) => { await call("set_advice_enabled", { enabled: on }); applyAdvice(on); await emitSettingsChanged("advice_enabled"); };
// PRD 0040 RF7.2: the live agent session on cards and the tray (on unless turned off).
const LIVE_KEY = "cuotai.live-session";
let liveNow = get(LIVE_KEY, "on") !== "off";
const applyLive = (on) => { liveNow = on !== false; put(LIVE_KEY, liveNow ? "on" : "off"); };
export const liveSessionShown = () => liveNow;
export const setLiveSession = async (on) => { await call("set_live_session", { show: on }); applyLive(on); await emitSettingsChanged("live_session"); };
/** PRD 0040 RF8.1: hide an agent's "add an account fed by it" banner for 7 days. */
export const dismissFeedBanner = (source) => call("dismiss_feed_banner", { source });
/** PRD 0040 RF2.4: the `~/.claude/settings.json` snippets (text only; Cuotai never edits the agent's settings). */
export const claudeCodeSnippets = () => call("claude_code_snippets");
/** PRD 0040 RF1.2: erase the stored Claude Code readings; resolves to the core's text saying whether there were any. */
export const forgetClaudeCodeReadings = () => call("forget_claude_code_readings");
const ctxRole = (level) => (level === "bad" ? "bad" : level === "warn" ? "warn" : "");
/** PRD 0040 RF7.1: the card's Live session rows (model, "ctx 8%" in its display-only role, the agent's cost estimate
 *  marked "≈" with the caveat as tooltip), newest first, "and N more"; nothing while there is none or it is turned off.
 *  The context share is a chip, never a meter: it is not a quota and has no alert. */
export function liveSessionHtml(r) {
  const live = r?.live;
  if (!liveNow || !live?.sessions?.length) return "";
  const caveat = tl(live.caveat);
  const rows = live.sessions.map((s) => `<li class="ls-row" data-live-row>${s.model_name ? `<span class="ls-model">${esc(s.model_name)}</span>` : ""}${
    s.context ? `<span class="mini ${ctxRole(s.level)}" data-live-ctx><span class="mini-t">${esc(tl(s.context))}</span></span>` : ""}${
    s.cost ? `<span class="ls-cost" title="${esc(caveat)}">${esc(tl(s.cost))}</span>` : ""}</li>`).join("");
  return `<div class="live-session" data-live role="group" aria-label="${esc(tl(live.title))}"><p class="ls-title">${esc(tl(live.title))}</p><ul class="ls-list">${rows}</ul>${
    live.more ? `<p class="ls-more">${esc(tl(live.more))}</p>` : ""}${live.sessions.some((s) => s.cost) ? `<p class="ls-caveat">${esc(caveat)}</p>` : ""}</div>`;
}
/** PRD 0040 RF7.2: the tray row's single "ctx 8%" chip. */
export const liveChipHtml = (r) => (liveNow && r?.live?.chip
  ? `<span class="mini ${ctxRole(r.live.chip_level)}" data-live-chip title="${esc(tl(r.live.title))}"><span class="mini-t">${esc(tl(r.live.chip))}</span></span>` : "");
/** PRD 0024 RF7.3: the "Best now" chip of the best account in its group, from the core's text; empty otherwise. */
export function bestChipHtml(r) {
  if (!adviceNow || !r.best_now) return "";
  const text = tl(r.best_now);
  const group = r.group_label ? ` · ${r.group_label}` : "";
  return `<span class="mini best-now" title="${esc(text + group)}" aria-label="${esc(text + group)}">${icon("check")}<span class="mini-t">${esc(text)}</span></span>`;
}
/** PRD 0024 RF7.3: what the best account of the group has left while this one runs out, with the one action (focus
 *  the other card). `action` is false where there is no card to focus. */
export function adviceHtml(r, { action = true } = {}) {
  if (!adviceNow || !r.advice) return "";
  // PRD 0024 re-validation (S-01): `te` escapes its values, so the name is passed raw (escaped once).
  const show = action ? `<button type="button" class="btn sm ghost" data-show-account="${r.advice.best_index}" title="${te("advice.show", { name: r.advice.best_name })}">${te("advice.show", { name: r.advice.best_name })}</button>` : "";
  return `<p class="call" role="note">${icon("info")}<span>${esc(tl(r.advice.text))}</span>${show}</p>`;
}
export const setPaceUnit = async (unit) => { await call("set_pace_unit", { unit }); applyPaceUnit(unit); await emitSettingsChanged("pace_unit"); };
export const setShowPace = async (on) => { await call("set_show_pace", { show: on }); applyPace(on); };
/** The core's pace of `m` (never estimated here); null when the core has none or the user hid pace. */
export const paceOf = (m) => (showPace ? m.pace ?? null : null);
/** PRD 0033 RF4.1: the name of `m`'s window, for the pace line ("Week", "5h"); windows the UI does not name use the metric label. */
export const paceLabelOf = (m) => (m.window && m.window !== "other" ? t(`card.binding.${m.window}`) : metricName(m));
/** PRD 0033 RF4.1: the pace line names its metric unless the binding chip next to it already says the same. */
export const paceLineLabel = (r, m) => (r.status === "ready" && r.binding && isBindingMetric(r, m) ? "" : paceLabelOf(m));
/** PRD 0007 RF3.2: the card's metric — the core's most restrictive metric with a pace, unless the user hid it. */
export const cardPace = (r) => {
  const m = r.status === "ready" && r.pace_index != null ? r.metrics[r.pace_index] : null;
  return m && paceOf(m) && !isMetricHidden(r, m) ? m : null;
};
/** "8 points in reserve · lasts until the reset" from the core texts; `expected` adds "Expected 47% used",
 *  `label` prefixes the metric, `short` keeps only the outlook (the stage when exhausted). */
export function paceText(p, { expected = false, label = "", short = false } = {}) {
  // PRD 0024 RF1.3/RF6.1: the stage reads from the core chip in the user's unit (the demo has no chips).
  // D-01 (re-validation): when "On pace" yields to a run-out projection, the core's summary is the projection alone.
  const hidden = p.shows_stage === false && p.summary;
  const stage = hidden ? tl(p.summary[paceUnitNow]) : p.chips ? tl(p.chips[paceUnitNow].text) : tl(p.label);
  const outlook = hidden || !p.outlook ? "" : tl(p.outlook);
  const parts = short ? [outlook || stage]
    : [stage, expected ? t("metric.pace.expected", { pct: Math.round(p.expected_used_fraction * 100) }) : "", outlook];
  const text = parts.filter(Boolean).join(" · ");
  return label ? `${label}: ${text}` : text;
}
const PACE_ICON = { good: "check", neutral: "clock", warn: "warn", bad: "xcircle" };
/** Pace line: tone icon plus text (never color alone); the tooltip says it is a linear estimate. */
export function paceHtml(p, opts) {
  const text = paceText(p, opts);
  return `<p class="pace-line" data-pace="${esc(p.stage)}" title="${esc(`${text}. ${t("card.pace.estimate")}`)}">${icon(PACE_ICON[p.tone] ?? "clock", p.tone)}<span>${esc(text)}</span></p>`;
}
/** PRD 0024 RF4.3 (DS-CMP-CALL-3): the forecast callout of a metric, from the core's text; icon and border carry the
 *  tone, not color alone. Empty unless the core projects a stop before the reset and pace is shown. */
export function forecastHtml(m, r = null) {
  if (!m.forecast || !showPace) return "";
  // Mockup 03 (#16): "Alert me" unmutes this metric's usage alert or, while usage alerts are off, opens their
  // Settings page; once the alert is on, the callout carries no action.
  const label = `${icon("bell")}<span>${te("alerts.alertMe")}</span>`;
  const alertMe = !r?.account_key || (m.alert && !m.alert.muted) ? ""
    : m.alert ? `<button type="button" class="btn ghost sm" data-metric-alert="${esc(r.account_key)}" data-metric-label="${esc(m.label)}" data-metric-on="false">${label}</button>`
    : `<button type="button" class="btn ghost sm" data-settings="alerts">${label}</button>`;
  return `<p class="call warn" role="note">${icon("warn")}<span>${esc(tl(m.forecast.callout))}</span>${alertMe}</p>`;
}
/** DS-CMP-BADGE-1: the plan the core reports ("Max", "Pro"), outlined and neutral; nothing when it states none. */
export const planBadgeHtml = (r) => (r?.plan ? `<span class="plan-badge" data-plan title="${te("common.label_value", { label: t("agg.plan"), value: r.plan })}">${esc(r.plan)}</span>` : "");
/** PRD 0023 RF10.3: each model at its limit with its return ("Opus at its limit until 19:30"), from the core. */
export const scopeBlocksHtml = (r) => (r?.scope_blocks ?? []).map((b) =>
  `<p class="scope-block" data-scope-block="${esc(b.label)}">${icon("clock")}<span>${esc(tl(b.text))}</span></p>`).join("");
/** PRD 0024 RF4.3: the card's outlook chip ("↑ Runs out ≈18:53") from the core chip, text as tooltip and name. */
export function forecastChipHtml(m) {
  if (!m.forecast || !showPace) return "";
  const text = tl(m.forecast.chip.text);
  return `<span class="mini time warn" title="${esc(text)}" aria-label="${esc(text)}"><span aria-hidden="true">${esc(m.forecast.chip.glyph)}</span>${esc(text)}</span>`;
}
export function initOrderSync(onChange) {
  onPreferencesChanged((changed) => {
    if (["sort", "group_recharging", "show_pace", "pace_unit", "advice_enabled", "live_session"].some((k) => changed.has(k))) onChange();
  });
}

/** Applies a `get_settings` answer to this window's state; resolves to the keys whose value changed here. */
async function applyPreferences(s, { boot = false } = {}) {
  const changed = new Set();
  const was = { motion: document.documentElement.dataset.motion || "system", pace: showPace };
  if (s.theme && s.theme !== theme.id) { theme.id = s.theme; changed.add("theme"); }
  if (THEME_MODE_IDS.includes(s.theme_mode) && s.theme_mode !== theme.mode) { theme.mode = s.theme_mode; changed.add("theme_mode"); }
  if (SORT_MODES.includes(s.sort) && s.sort !== sortMode) { sortMode = s.sort; changed.add("sort"); }
  if (typeof s.group_recharging === "boolean" && s.group_recharging !== groupRecharging) { groupRecharging = s.group_recharging; changed.add("group_recharging"); }
  if (AGG_MODES.includes(s.aggregators_in_all) && s.aggregators_in_all !== aggMode) { aggMode = s.aggregators_in_all; changed.add("aggregators_in_all"); }
  applyMotion(s.motion);
  if (was.motion !== (s.motion || "system")) changed.add("motion");
  applyPace(s.show_pace);
  if (was.pace !== showPace) changed.add("show_pace");
  // PRD 0024 RF1.3: the pace unit follows desktop.json like the other preferences (RF7.2).
  if (["points", "percent"].includes(s.pace_unit) && s.pace_unit !== paceUnitNow) { applyPaceUnit(s.pace_unit); changed.add("pace_unit"); }
  // PRD 0024 RF7.5: the best-account advice, the same way.
  if (typeof s.advice_enabled === "boolean" && s.advice_enabled !== adviceNow) { applyAdvice(s.advice_enabled); changed.add("advice_enabled"); }
  // PRD 0040 RF7.2: the live agent session, the same way.
  if (typeof s.live_session === "boolean" && s.live_session !== liveNow) { applyLive(s.live_session); changed.add("live_session"); }
  for (const [key, value] of [["theme", theme.id], ["theme_mode", theme.mode], ["sort", sortMode], ["group_recharging", groupRecharging], ["aggregators_in_all", aggMode]]) mirrorPreference(key, value);
  if (s.locale && boot) mirrorPreference("locale", s.locale); // the language loads right after: it picks the mirror up
  else if (s.locale && s.locale !== currentLocale()) { await setLocale(s.locale); changed.add("locale"); }
  return changed;
}
const THEME_MODE_IDS = ["system", "light", "dark"];
// PRD 0036 RF8.3 under PRD 0020 RF7.2: how the popover's "All" shows aggregators, a desktop.json preference whose
// `localStorage` copy is only the mirror for the first paint.
export const AGG_MODES = ["grouped", "listed", "hidden"];
let aggMode = (() => { const v = get("cuotai.aggregators-in-all", "grouped"); return AGG_MODES.includes(v) ? v : "grouped"; })();
export const aggregatorsInAll = () => aggMode;
export async function setAggregatorsInAll(mode) {
  aggMode = AGG_MODES.includes(mode) ? mode : "grouped";
  await setPreference("aggregators_in_all", aggMode); // the shell tells every window (RF7.1)
}
/** Calls `cb` when another window changed the aggregator display choice. */
export const onAggregatorModeChange = (cb) => onPreferencesChanged((changed) => { if (changed.has("aggregators_in_all")) cb(); });
const prefListeners = new Set();
let prefHooked = false;
/** Calls `cb(changedKeys)` when another window (or the shell) changed a preference that differs from this one's. */
function onPreferencesChanged(cb) {
  prefListeners.add(cb);
  if (prefHooked) return;
  prefHooked = true;
  onSettingsChanged(async () => {
    const changed = await syncPreferences();
    if (changed.size) prefListeners.forEach((f) => f(changed));
  });
}

/** Account summary: the tightest quota (lowest % available; tie → nearest reset), or the text of
 *  a metric without a limit, or the reason there is no summary. */
export function summaryOf(r) {
  const quotas = planMetrics(r);
  const when = (m) => (m.resets_at ? new Date(m.resets_at).getTime() : Infinity);
  const measured = quotas.map((m) => ({ m, pct: remainingPct(m) })).filter((x) => x.pct != null)
    .sort((a, b) => a.pct - b.pct || when(a.m) - when(b.m));
  if (measured.length) return { kind: "bar", m: measured[0].m, pct: measured[0].pct };
  const text = quotas.map((m) => ({ m, text: textualMetric(m) })).find((x) => x.text != null);
  if (text) return { kind: "text", m: text.m, text: text.text };
  const renewed = quotas.filter(isRenewedWaiting).sort((a, b) => when(a) - when(b))[0];
  if (renewed) return { kind: "renewed", m: renewed };
  const planAll = r.metrics.filter((m) => m.kind !== "rate_limit" && !isOverage(m));
  return { kind: planAll.length && !quotas.length ? "hidden" : "none" };
}
/** PRD 0041 RF2: a budget the core derived from the person's `[[accounts.budgets]]` (never a provider limit). */
export const isBudgetMetric = (m) => !!m?.derived_from;
/** The account's 5h and weekly windows (classification comes from the core: `metric.window`). In each window the tightest
 *  quota among the non-hidden ones wins; null when the account has no such window. A budget never takes a provider
 *  window's slot (PRD 0041 re-validation F-08): it keeps its own name in the tightest-quota column. */
export function windowsOf(r) {
  const when = (m) => (m.resets_at ? new Date(m.resets_at).getTime() : Infinity);
  const pick = (w) => planMetrics(r).filter((m) => m.window === w && !isBudgetMetric(m)).map((m) => ({ m, pct: remainingPct(m) })).filter((x) => x.pct != null)
    .sort((a, b) => a.pct - b.pct || when(a.m) - when(b.m))[0] ?? null;
  return { h5: pick("window_5h"), week: pick("weekly") };
}
/** PRD 0033 re-validation (V-01): the renewed metric of each window that has no measured one (`windowsOf` gives it no
 *  slot value), so a card with a renewed 5 h window beside a current week says "Renewed at …" instead of "—". */
export function renewedWindowsOf(r) {
  const when = (m) => (m.resets_at ? new Date(m.resets_at).getTime() : Infinity);
  const pick = (w) => planMetrics(r).filter((m) => m.window === w && !isBudgetMetric(m) && isRenewedWaiting(m)).sort((a, b) => when(a) - when(b))[0] ?? null;
  return { h5: pick("window_5h"), week: pick("weekly") };
}
/** Worst % available among the rate-limit windows (non-hidden); null when there are none. */
export function rateLimitOf(r) {
  const v = r.metrics.filter((m) => m.kind === "rate_limit" && !isMetricHidden(r, m)).map(remainingPct).filter((x) => x != null);
  return v.length ? Math.min(...v) : null;
}

/** Instant (ms) of the nearest reset among the account's metrics; null when none reports one. */
function soonestReset(r) {
  const now = Date.now();
  const s = summaryOf(r);
  if (s.m?.resets_at) { const t0 = new Date(s.m.resets_at).getTime(); if (Number.isFinite(t0) && t0 > now) return t0; } // the same reset that the card shows
  const times = planMetrics(r).map((m) => (m.resets_at ? new Date(m.resets_at).getTime() : NaN)).filter((x) => Number.isFinite(x) && x > now);
  return times.length ? Math.min(...times) : null;
}
/** List in the chosen order. Custom is the file order; outside it, accounts without the datum go to the end. */
export function sortReports(reports) {
  if (sortMode === "custom") return [...reports].sort((a, b) => a.index - b.index);
  if (sortMode === "binding") return [...reports].sort((a, b) => a.binding_rank - b.binding_rank);
  const [field, dir] = sortMode.split("-");
  const value = (r) => (field === "reset" ? soonestReset(r) : r.status === "ready" ? worstRemaining(r) : null);
  const sign = dir === "asc" ? 1 : -1;
  return [...reports].sort((a, b) => {
    const va = value(a), vb = value(b);
    if (va == null && vb == null) return a.index - b.index;
    if (va == null) return 1;
    if (vb == null) return -1;
    return sign * (va - vb) || a.index - b.index;
  });
}

let sortOpen = false;
/** Button + ordering menu (custom, reset, % available). */
export function sortMenuHtml() {
  const label = (m) => (m === "custom" ? t("sort.custom") : t(`sort.${m}`));
  // PRD 0022 re-validation (S-02): a catalog (a user language pack included) is untrusted text, so each item's label
  // is set with `textContent` and serialized by the DOM, never spliced into the markup raw.
  const text = (m) => {
    const span = document.createElement("span");
    span.textContent = label(m);
    return span.outerHTML;
  };
  const items = SORT_MODES.map((m) => `<button role="menuitemradio" aria-checked="${m === sortMode}" class="${m === sortMode ? "on" : ""}" data-sort="${m}">${text(m)}${m === sortMode ? icon("tick", "sel-tick") : ""}</button>`).join("");
  return `<div class="sortmenu lang">
    <button class="btn outline sort-btn" data-sort-toggle title="${te("sort.label")}: ${esc(label(sortMode))}" aria-label="${te("sort.label")}: ${esc(label(sortMode))}" aria-haspopup="menu" aria-expanded="${sortOpen}">${icon("sort")}${icon("chevDown")}</button>
    <div class="lang-menu${menuPlaceClass(sortOpen)}" role="menu" ${sortOpen ? menuPlaceStyle() : "hidden"}>${items}</div>
  </div>`;
}

// ---------- notice categories (cards and popover): dedicated icon and own color, from most to least severe ----------
// `label(r)`: with the account, the category's text for it; without, the category's title.
export const ALERTS = [
  { id: "error", icon: "xcircle", tone: "bad", label: () => t("status.error"), match: (r) => r.status === "error" },
  { id: "auth", icon: "key", tone: "accent", label: () => t("status.authentication_error"), match: (r) => r.status === "authentication_error" },
  { id: "exhausted", icon: "battery", tone: "bad", label: () => t("tray.cat.exhausted"), match: (r) => r.status === "ready" && r.class === "exhausted" },
  // PRD 0008 RF3.4: an active usage alert (the exhausted state above keeps precedence).
  // PRD 0034 RF2: each metric in the reader's language (the core's `metric_text`), listed the locale's way.
  { id: "usage", icon: "bell", tone: "warn", label: (r) => (r ? t("alerts.active", { metrics: usageAlertList(r) }) : t("alerts.activeTitle")),
    match: (r) => r.status === "ready" && r.class !== "exhausted" && r.usage_alerts?.length > 0 },
  { id: "rate", icon: "gauge", tone: "warn", label: () => t("status.rate_limited"), match: (r) => r.status === "rate_limited" },
  { id: "config", icon: "wrench", tone: "info", label: () => t("status.needs_configuration"), match: (r) => r.status === "needs_configuration" },
  { id: "unsupported", icon: "ban", tone: "muted", label: () => t("status.unsupported"), match: (r) => r.status === "unsupported" },
];
export const alertOf = (r) => ALERTS.find((c) => c.match(r)) ?? null;
/** PRD 0008 RF3.4: the metrics with an active usage alert, localized and joined with the locale's list format. */
export const usageAlertList = (r) => listText((r?.usage_alerts ?? []).map((x) => tl(x)));
/** PRD 0034 V-18: translated items listed the UI language's way (en "A, B, C", de "A, B und C", ja "A、B、C"); raw
 *  text, the caller escapes it. A tag `Intl` cannot read, and the pseudo-locale, list like en-US. */
const listFormats = new Map();
export function listText(items) {
  const list = (items ?? []).map((x) => String(x ?? ""));
  const code = currentLocale();
  const tag = code === PSEUDO ? FALLBACK : intlTag(code);
  try {
    if (!listFormats.has(tag)) listFormats.set(tag, new Intl.ListFormat(tag, { style: "narrow", type: "conjunction" }));
    return listFormats.get(tag).format(list);
  } catch {
    return list.join(", ");
  }
}

// Demo Kiro: one login mode (oauth) and the credential fields, as in the core.
const dl = (id, fallback) => ({ id, params: {}, fallback });
// The core's own id when the catalogs know it, else a demo id (so no language shows the Portuguese fallback).
const dlx = (id, demoId, fallback) => dl(t(id) !== id ? id : demoId, fallback);
// Default-mode level per provider, as `access::describe` classifies them (demo only).
const DEMO_LEVELS = {
  derived: ["gemini_api", "vertex_ai", "groq"],
  unavailable: ["qwen", "perplexity", "cloudflare_ai", "gemini_ai", "nvidia_nim", "poolside", "alibaba_model_studio"],
  official: ["open_ai_api", "anthropic_api", "aws_bedrock", "azure_open_ai", "cursor", "copilot", "windsurf", "kimi", "mistral", "grok", "cline", "alibaba_cloud", "open_router", "jetbrains_ai"],
};
// PRD 0027 re-validation C-07: the core catalog of the picker, generated by the core test `catalog_demo_fixture`.
// PRD 0037 re-validation (V-09): the backend's `form_schema` and `visible_fields` answers of every provider, generated by
// the desktop test `demo_forms_fixture` (`demo-forms.json`, keyed by provider slug).
let demoFormsData = null;
const demoForms = () => (demoFormsData ??= fetch(uiPath("js/demo-forms.json")).then((r) => r.json()).then((d) => d.providers));
/** The generated provider list (`Provider::ALL`), for the demo-coverage test. */
export const demoFormProviders = async () => Object.keys(await demoForms());
/** The backend's visible field keys of `entry` for `authMode` and the choice indices `choices`: the first recorded
 *  combination that agrees with every given choice, else the answer for the default choices. */
function demoVisible(entry, choices, authMode) {
  if (!entry) return null;
  const byMode = entry.visible[authMode] ?? entry.visible.credential;
  const given = Object.entries(choices ?? {}).filter(([k, v]) => v != null && entry.schema.fields.some((f) => f.options && f.key === k));
  if (!given.length) return byMode[""];
  const combo = Object.keys(byMode).find((c) => c && given.every(([k, v]) => c.split("&").includes(`${k}=${v}`)));
  return byMode[combo ?? ""];
}
let demoCatalogData = null;
const demoCatalog = () => (demoCatalogData ??= fetch(uiPath("js/demo-catalog.json")).then((r) => r.json()));
// A core text in a language: the current one through the UI's catalogs (core ones included), en-US as the core does.
const demoResolve = (text, locale) => (locale === "en-US" || locale !== currentLocale() ? text.fallback : tl(text));
// The app's `PickerEntry` (desktop backend): the English provider label, as `Provider::label`.
const demoPickerEntry = (e) => ({ provider: e.provider, label: e.label, mode: e.mode, method: e.method,
  method_label: e.method_label, hint: e.hint, metered: e.metered, recommended: e.recommended });
const DEMO_NO_QUERY = ["qwen", "cloudflare_ai", "nvidia_nim", "poolside"];
// ponytail: the core's English provider names (`Provider::label`) for the browser demo, which has no core; the
// app always reads them from `list_providers`.
const DEMO_CORE_LABELS = {
  open_ai_api: "OpenAI API", anthropic_api: "Anthropic API", chat_gpt: "ChatGPT / Codex", claude: "Claude",
  gemini_api: "Gemini (Cloud Monitoring)", vertex_ai: "Vertex AI", aws_bedrock: "AWS Bedrock", azure_open_ai: "Azure OpenAI",
  cursor: "Cursor", copilot: "GitHub Copilot", windsurf: "Windsurf", mini_max: "Minimax.io", kimi: "Kimi", qwen: "Qwen",
  groq: "Groq", meta: "Meta Model API", jetbrains_ai: "JetBrains AI", perplexity: "Perplexity", mistral: "Mistral",
  cloudflare_ai: "Cloudflare AI", ollama_cloud: "Ollama Cloud", xiaomi_mimo: "Xiaomi MIMO", open_code_go: "Opencode Go",
  qoder: "Qoder", gemini_ai: "Gemini CLI", grok: "Grok (xAI)", nvidia_nim: "NVIDIA NIM", poolside: "Poolside",
  kiro: "Kiro (AWS)", cline: "Cline", code_buddy: "CodeBuddy", zed: "Zed", alibaba_model_studio: "Alibaba Model Studio / DashScope",
  alibaba_coding_plan: "Alibaba Coding Plan", alibaba_token_plan: "Alibaba Token Plan (Qwen Cloud)",
  alibaba_cloud: "Alibaba Cloud — billing", antigravity: "Antigravity", kimchi: "Kimchi", eleven_labs: "ElevenLabs",
  nine_router: "9router", open_router: "OpenRouter", litellm: "LiteLLM proxy", extension: "Extension",
};
const demoLevelOf = (provider) => Object.keys(DEMO_LEVELS).find((level) => DEMO_LEVELS[level].includes(provider)) ?? "undocumented";
const demoLevel = (level) => dl(`access.level.${level}`, { official: "Official", undocumented: "Undocumented", derived: "Derived", unavailable: "Unavailable" }[level]);
