import { DEFAULT_THEME, GROUP_ORDER, THEMES, effectiveMode, themeById } from "./themes.js";
import { LOCALES, currentLocale, pendingLocales, setLocale, t, te, tl } from "./i18n.js";
import { esc, uiPath } from "./util.js";
import { desktopAction } from "./desktop-wording.js";
import { flagCountry, flagSvg } from "./flags.js";
import { crossFade } from "./motion.js";
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
};
export const icon = (name, cls = "") => `<svg class="i ${cls}" viewBox="0 0 24 24" aria-hidden="true">${paths[name]}</svg>`;

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
export const providerModes = (provider) => call("provider_modes", { provider });
export const detectSources = () => call("detect_sources");
export const setDetectEnabled = (on) => call("set_detect_enabled", { on });
export const dismissDetected = (kind) => call("dismiss_detected", { kind });
/** The new-provider issue template (allow-listed by the shell's `open_external`). */
export const NEW_PROVIDER_URL = "https://github.com/jmarcon/cuotai/issues/new?template=new-provider.yml";
export const listExtensions = () => call("list_extensions");
export const extensionDisclosure = (id) => call("extension_disclosure", { id });
export const enableExtension = (candidate) => call("enable_extension", { candidate });
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
/** PRD 0016 RF1.2: move the account's session variables into the system vault (after an explicit confirmation). */
export const migrateSession = (index) => call("migrate_session", { index });
/** PRD 0016 RF4.2: forget the account's web session, deleting its vault entry (after an explicit confirmation). */
export const forgetSession = (index) => call("forget_session", { index });
/** PRD 0029 RF6.2: the vault entries Cuotai created (orphans marked), and the removal of the orphans. */
export const vaultAudit = () => call("vault_audit");
export const vaultPrune = () => call("vault_prune");
/** PRD 0016 RF2.2: what a paste keeps (names, counts, expiry), never the value. */
export const previewSession = (provider, slot, paste) => call("preview_session", { provider, slot, paste });
/** PRD 0016 RF2.1: one read-only query of the draft with its pasted sessions; nothing is stored. */
export const testSession = (input) => call("test_session", { input });
/** PRD 0048 RF1: "Sign in and capture session" — the app's own sign-in window; resolves with its outcome. */
export const captureSession = (provider, slot) => call("capture_session", { provider, slot });
/** PRD 0048 RF6.2: closes a capture window still waiting for its sign-in. */
export const cancelSessionCapture = () => call("cancel_session_capture");
export const testAggregator = (input) => call("test_aggregator", { input });
export const openAggregatorDashboard = (index) => call("open_aggregator_dashboard", { index });
export const saveAccount = (input) => call("save_account", { input });
export const deleteAccount = (index, purge = false) => call("delete_account", { index, purge });
/// RF4: re-read the configuration from disk and rebuild the live
/// state (monitor/cycle/tokens/RF3 recovery). Returns the new
/// view. Used after an `edit_error.conflict` so the next save can
/// succeed against the fresh snapshot, and as the manual path for
/// external edits the UI detected via `configChangedOnDisk`.
export const reloadConfig = () => call("reload_config");
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
export const trayPreview = (style, accounts) => call("tray_preview", { style, accounts });
export const trayAccounts = () => call("tray_accounts");
/** Tauri maps command arguments from camelCase; `popover_view` (as `get_settings` names it) is accepted too. */
export const setTrayPrefs = ({ style, colored, accounts = null, popover_view = null, popoverView = popover_view }) =>
  call("set_tray_prefs", { style, colored, accounts, popoverView });
export const setAdvancedOpen = (open) => call("set_advanced_open", { open });
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
export const historyExport = (index, days, format, path, generation = null) => call("history_export", { index, generation, days, format, path });
export const historyErase = (index, generation = null) => call("history_erase", { index, generation });
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
// PRD 0008 RF3: usage alerts (desktop.json; the backend judges each reading and sends them). `setAlertsEnabled` asks
// the same system permission as reset notices and resolves to `granted`, `denied`, `prompt` or `off`.
export const setAlertsEnabled = (on) => call("set_alerts_enabled", { on });
/** PRD 0024 RF4.1: the projected-exhaustion warning, `minutes` (15, 30 or 60) before the stop; resolves to the
 *  permission like `setAlertsEnabled` (turning it on stays off unless `granted`). */
export const setProjectedAlert = (on, minutes) => call("set_projected_alert", { on, minutes });
export const setAlertOptions = (thresholds, exhaustion, recovery, overage) => call("set_alert_options", { thresholds, exhaustion, recovery, overage });
export const setMetricAlert = (accountKey, label, on) => call("set_metric_alert", { accountKey, label, on });
export const setPoolAlerts = (accountKey, each) => call("set_pool_alerts", { accountKey, each });
// PRD 0011 RF4.3: public provider status queries (desktop.json, off by default; the backend asks the core).
export const setStatusEnabled = (on) => call("set_status_enabled", { on });
// PRD 0012 RF5: the local server (desktop.json, off by default); the backend runs the core's server over this app's
// own refresh cycle. Each call answers the server's new state; the token never comes back except to be copied.
export const serveStatus = () => call("serve_status");
export const setServe = (enabled, port, allowBrowsers) => call("set_serve", { enabled, port, allowBrowsers });
// PRD 0031 RF11.2.5: "Share usage statistics" (`serve.stats`, /v1/stats), answered only with a token.
export const setServeStats = (stats) => call("set_serve_stats", { stats });
export const generateServeToken = () => call("generate_serve_token");
export const revokeServeToken = () => call("revoke_serve_token");
export const copyServeToken = () => call("copy_serve_token");
// PRD 0013 RF4: hooks (desktop.json, off by default); the backend runs the core's hooks over this app's own refresh
// cycle. Each call answers the hooks' new state; `onHookRun` fires after every run so the log is read again.
export const hooksStatus = () => call("hooks_status");
export const setHooksEnabled = (on) => call("set_hooks_enabled", { on });
export const confirmHooks = (digest) => call("confirm_hooks", { digest });
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
// background scan and summary). `costEnable` sends the digest of the disclosure the dialog showed; each settings call
// answers the new state. `costUsage` is the core's CostSummary the cards and details show.
export const costStatus = () => call("cost_status");
export const costUsage = () => call("cost_usage");
// PRD 0030 RF1/RF6: the plan prices of the configured accounts, the display currency and the plan value lines (the
// core's `PlansView`); typed by the person, kept on this computer, never exported. `price: null` clears a plan,
// `code: null` clears the display currency.
export const costPlans = () => call("cost_plans");
export const costSetPlan = (id, price, currency, cycleDay) => call("cost_set_plan", { id, price, currency, cycleDay });
export const costSetDisplay = (code, rate) => call("cost_set_display", { code, rate });
export const costEnable = (source, digest) => call("cost_enable", { source, digest });
export const costDisable = (source) => call("cost_disable", { source });
export const costSetPaths = (source, paths) => call("cost_set_paths", { source, paths });
export const costSetOptions = (period, retentionDays) => call("cost_set_options", { period, retentionDays });
export const costPrices = () => call("cost_prices");
export const costErase = () => call("cost_erase");
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
  const n = view?.notify?.next;
  if (!n) return null;
  const key = `card.binding.${n.window}`;
  const window = t(key) === key ? n.label : t(key);
  return t("notify.next", { name: `${brand(n.provider).label} ${window}`, when: fmtReset(n.at) });
}
/** RF3.2: the account's bell (card and details), only while reset notices or usage alerts (PRD 0008 RF3.2) are on;
 *  it mutes both. `aria-pressed` is its state. */
export function bellHtml(view, r) {
  if (!(view?.notify?.enabled || view?.notify?.alerts) || !r.account_key) return "";
  const label = t("notify.bell", { account: r.account });
  const state = t(r.notify ? "notify.bell.on" : "notify.bell.off");
  return `<button class="icon bell${r.notify ? " on" : ""}" aria-pressed="${r.notify}" aria-label="${esc(label)}" title="${esc(`${label}: ${state}`)}" data-bell="${esc(r.account_key)}" data-bell-on="${r.notify}">${icon(r.notify ? "bell" : "bellOff")}</button>`;
}
/** PRD 0008 RF3.2: one metric's usage alerts in the details (only while alerts are on and the metric can alert);
 *  pressed = alerts on, unpressed = muted. */
export function metricAlertHtml(r, m) {
  if (!m.alert || !r.account_key) return "";
  const on = !m.alert.muted;
  const label = t("alerts.metric", { metric: m.label });
  const state = t(on ? "notify.bell.on" : "notify.bell.off");
  return `<button class="icon bell${on ? " on" : ""}" aria-pressed="${on}" aria-label="${esc(label)}" title="${esc(`${label}: ${state}`)}" data-metric-alert="${esc(r.account_key)}" data-metric-label="${esc(m.label)}" data-metric-on="${on}">${icon(on ? "bell" : "bellOff")}</button>`;
}
/** Native "save as" for an export (`csv` or `json`); null when cancelled. */
export async function pickExportFile(format) {
  const defaultPath = `cuotai-history.${format}`;
  if (isDesktop && T.dialog) return T.dialog.save({ defaultPath, filters: [{ name: format.toUpperCase(), extensions: [format] }] });
  return `/Users/demo/${defaultPath}`; // navegador (demo)
}
/** "About": project identity and open-source library list (comes from the core). */
export const aboutInfo = () => call("about_info");
/** PRD 0019 RF7.7: the changelog history already parsed by the core (no network). */
export const whatsNew = () => call("whats_new");
/** Opens in the browser a repository from the "About" list (the shell refuses any other address). */
export const reportProblem = (index, generation) => call("report_problem", { index: index ?? null, generation });
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
  if (!f) { f = new Intl[kind](locale, options); formatters.set(key, f); }
  return f;
}
/** `date.toLocaleTimeString(locale, options)` / `toLocaleDateString` with the formatter cached (same output when
 *  `options` names the fields, which every caller here does). */
export const fmtLocal = (date, options, locale = currentLocale()) => intlFormatter("DateTimeFormat", locale, options).format(date);
const HHMM = { hour: "2-digit", minute: "2-digit" };
export const fmtNumber = (n, max = 2) => (n == null ? "—" : intlFormatter("NumberFormat", currentLocale(), { maximumFractionDigits: max }).format(n));
// Units that describe "request count": left implicit. Others (USD, credits, tokens) are shown.
const QUIET_UNITS = new Set(["", "%", "—", "solicitações", "requisições", "consultas", "unidades", "requests"]);
const unitSuffix = (m) => (QUIET_UNITS.has(m.unit) ? "" : ` ${m.unit}`);
export function fmtAmount(m) {
  if (m.limit == null) return m.used == null ? "—" : `${fmtNumber(m.used)}${unitSuffix(m)}`;
  return `${fmtNumber(m.used)} / ${fmtNumber(m.limit)}${unitSuffix(m)}`;
}
/** One recorded value of a metric with `unit` (history): "60%", "12.5 USD", or a bare count. */
export const fmtValue = (v, unit) => (unit === "%" ? `${fmtNumber(v)}%` : `${fmtNumber(v)}${unitSuffix({ unit })}`);
/** Metric without a usable limit: text in place of the bar (never invents "100% free"). */
export function textualMetric(m) {
  if (m.state === "exhausted" && m.no_cap) return t("metric.exhausted");
  if (m.state === "available" && m.uncapped) {
    const amount = m.remaining ?? m.used;
    if (amount == null) return null;
    return t(m.remaining != null ? "metric.balance" : "metric.usage", { amount: `${fmtNumber(amount)}${unitSuffix(m)}` });
  }
  return null;
}
/** PRD 0024 RF1.4/RF6.1: "Resets 19:30 · in 1h 35m", worded by the core (`resets_label`); only a reading without it
 *  (the browser demo, a window already past) falls back to the local countdown. Null without a reset time. */
export function resetsText(m) {
  if (m.resets_label) return tl(m.resets_label);
  const left = legacyCountdown(m.resets_at);
  return left ? t("quota.resetsIn", { time: left }) : null;
}
/** "1h 35m" until `iso` ("now" once past): the local countdown the reset timeline speaks in its accessible names. */
export const fmtCountdown = (iso) => legacyCountdown(iso);
// PRD 0035 RF10.3: durations are catalog patterns (the core's `text.duration.*`, with their English fallbacks), never
// concatenated letters, so each language words them (`{h}時間{m}分`, `{h} ч {m} мин`).
const DURATION_EN = {
  "text.duration.minutes": "{m} min",
  "text.duration.hours": "{h}h",
  "text.duration.hours_minutes": "{h}h {m}m",
  "text.duration.days": "{d}d",
  "text.duration.days_hours": "{d}d {h}h",
};
const durationPart = (id, params) =>
  tl({ id, params, fallback: DURATION_EN[id].replace(/\{(\w+)\}/g, (m, n) => (n in params ? String(params[n]) : m)) });
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
/** Clock time of `iso` in the viewer's local zone, with the day when it is not today ("14:30", "24 Oct, 14:30"). */
export function fmtClock(iso) {
  if (!iso) return null;
  const d = new Date(iso);
  if (Number.isNaN(d.getTime())) return null;
  const time = fmtLocal(d, HHMM);
  if (d.toDateString() === new Date().toDateString()) return time;
  return `${fmtLocal(d, { day: "2-digit", month: "short" })}, ${time}`;
}
/** PRD 0033 RF2.1: a preserved or restored metric whose window renewed; the core sends it without values. */
export const isRenewedWaiting = (m) => m.expired === true;
/** "Renewed at 14:30; waiting for a reading", in the viewer's local time. */
export const renewedText = (m) => t("metric.renewedWaiting", { time: fmtClock(m.resets_at) ?? "—" });
export const statusLabel = (status) => t(`status.${status}`);
/** Quotas that define the account's summary and ordering: without the short rate-limit window (it is not the
 *  plan quota) and without the ones the user hid. */
export const planMetrics = (r) => r.metrics.filter((m) => m.kind !== "rate_limit" && !isOverage(m) && !isMetricHidden(r, m));

// ---------- PRD 0023: paid extra usage (overage) ----------
/** Billed on top of the plan: never a plan meter, never the summary, always a textual figure. */
export const isOverage = (m) => m.billing === "overage";
/** The core's overage reading of a current report; null for a failed, pending or preserved one. */
export const overageOf = (r) => (r.status === "ready" && !r.pending ? r.overage ?? null : null);
/** The overage while money is being billed (`active`) or the extra budget is used up (`cap_reached`). */
export const billingOverage = (r) => {
  const o = overageOf(r);
  return o && (o.state === "active" || o.state === "cap_reached") ? o : null;
};
/** One amount: money with the locale's currency format, provider units as the core words them, anything else with
 *  its unit. A missing amount is "—", never zero (DS-MSG-5). */
export function fmtExtra(value, unit, currency) {
  if (value == null) return "—";
  if (currency) {
    try { return new Intl.NumberFormat(currentLocale(), { style: "currency", currency }).format(value); } catch { /* unknown code */ }
  }
  if (unit === "unid. provedor") {
    const n = fmtNumber(value);
    return tl({ id: "overage.amount.provider_units", params: { value: n }, fallback: `${n} provider units` });
  }
  return `${fmtNumber(value)}${unit ? ` ${unit}` : ""}`;
}
/** Figure first (DS-MSG-4): "$6.25 extra of $50" for one overage metric. */
export function fmtMetricExtra(m) {
  if (m.used == null) return "—";
  const amount = fmtExtra(m.used, m.unit, m.kind === "spend" ? m.unit : null);
  return m.limit > 0 ? t("metric.overageCap", { amount, cap: fmtExtra(m.limit, m.unit, m.kind === "spend" ? m.unit : null) })
    : t("metric.overage", { amount });
}
/** The account-level line: the figure while billing, the core's words for the other states. */
export function overageLineText(o) {
  if (o.state === "disabled") return "";
  if (o.state === "active" || o.state === "cap_reached") {
    if (o.spent == null) return tl(o.text);
    const amount = fmtExtra(o.spent, o.unit, o.currency);
    return o.cap > 0 ? t("metric.overageCap", { amount, cap: fmtExtra(o.cap, o.unit, o.currency) }) : t("metric.overage", { amount });
  }
  return tl(o.text);
}
/** Card chip: the core's summary ("Limit hit · on extra $6.25"), icon plus text, warning role (money is charged). */
export function overageChipHtml(r) {
  const o = billingOverage(r);
  if (!o) return "";
  const text = tl(o.summary);
  return `<span class="mini warn" data-overage-chip title="${esc(text)}" aria-label="${esc(`${t("card.extra.title")}: ${text}`)}">${icon("coins")}<span class="mini-t">${esc(text)}</span></span>`;
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
  const cap = o.cap > 0 ? fmtExtra(o.cap, o.unit, o.currency) : null;
  return `<div class="metric extra-detail" data-overage-detail>
    <div class="metric-top"><div><h3>${te("card.extra.title")}</h3><span class="sub">${esc(tl(o.text))}</span></div></div>
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
/** Short name of the binding window ("Week"); windows the UI does not name use the metric label. */
export const bindingLabel = (r) => {
  const w = r.binding?.window;
  return w && w !== "other" ? t(`card.binding.${w}`) : r.binding?.label ?? "";
};
export const isBlocked = (r) => r.status === "ready" && r.binding?.reason === "exhausted";
/** "Blocked until Today 14:30" (unknown return stays unknown, never "now"); null when not blocked. */
export const blockedText = (r) => {
  if (!isBlocked(r)) return null;
  const at = fmtReset(r.binding.blocked_until);
  return at ? t("card.blockedUntil", { time: at }) : t("card.blocked");
};
/** Chip text "Week · 100%" (consumed share of the binding window). */
export const bindingChipText = (r) => `${bindingLabel(r)} · ${Math.round(r.binding.used_ratio * 100)}%`;
export const isBindingMetric = (r, m) => r.binding != null && r.metrics[r.binding.index] === m;
/** PRD 0010 RF4.1: the current reading is exhausted or blocked; a failure (even with a preserved reading) is not. */
export const isRecharging = (r) => r.status === "ready" && (r.class === "exhausted" || isBlocked(r));
/** "Returns Today 14:30", or "Return unknown" when the core knows no `blocked_until`. */
export const returnText = (r) => {
  const at = isBlocked(r) ? fmtReset(r.binding.blocked_until) : null;
  return at ? t("group.returnsAt", { time: at }) : t("group.returnUnknown");
};
// ---------- PRD 0010 RF3: measurement age and the preserved reading ----------
/** The current query failed, or none answered yet (`r.pending`, a reading restored from the history on opening),
 *  and the core kept the last valid reading (`r.last_good`): always shown as stale. */
export const isStaleReading = (r) => (r.pending || r.status !== "ready") && r.last_good != null;
/** Instant of the reading on screen: the preserved one while the current query failed. */
export const shownAt = (r) => (isStaleReading(r) ? r.last_good.checked_at : r.checked_at);
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
/** Plain text: "Stale · 42 min ago", "Fresh · 3 min ago", or just the age. */
export const ageText = (r) => {
  const f = freshnessOf(r);
  const age = f ? `${t(`freshness.${f}`)} · ${fmtAge(shownAt(r))}` : fmtAge(shownAt(r));
  // PRD 0033 RF3.2: the provider states its data is older than the query ("data as of 06:00").
  const asOf = !isStaleReading(r) && r.data_as_of ? fmtClock(r.data_as_of) : null;
  return asOf ? `${age} · ${t("freshness.data_as_of", { time: asOf })}` : age;
};
const FRESHNESS_ICON = { fresh: "check", aging: "clock", stale: "warn" };
/** Age chip with the freshness icon and text (never color alone); the absolute time is in the tooltip. */
export function ageHtml(r) {
  const f = freshnessOf(r);
  const title = [t("age.measuredAt", { time: fmtReset(shownAt(r)) ?? "—" }), r.clock_adjusted && !isStaleReading(r) ? t("age.clockAdjusted") : ""]
    .filter(Boolean).join(" · ");
  return `<span class="age ${f ?? ""}" data-age data-freshness="${f ?? ""}" title="${esc(title)}">${icon(FRESHNESS_ICON[f] ?? "clock")}<span>${esc(ageText(r))}</span></span>`;
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
export const causeText = (r) => (r.pending ? t("card.waiting") : r.access?.explanation ? tl(r.access.explanation) : r.notes.length ? noteText(r, 0) : statusLabel(r.status));
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
  const checked = s.read_at ? t("incident.checked", { age: fmtAge(s.read_at) }) : "";
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
export function sessionOpenLinkHtml(slot) {
  const url = slot?.open_url;
  if (typeof url !== "string" || !url.startsWith("https://")) return "";
  let host;
  try { host = new URL(url).host; } catch { return ""; }
  return `<button type="button" class="link session-open" data-session-url="${esc(url)}">${icon("window")}<span>${te("dialog.session.open", { site: host })}</span></button>`;
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
    s.read_at ? t("incident.checked", { age: fmtAge(s.read_at) }) : t("incident.notRead"), s.stale ? t("incident.stale") : ""].filter(Boolean);
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
const B = (label, color, mark) => ({ label, color, mark: mark ?? label[0].toUpperCase() });
const brands = {
  open_ai_api: B("OpenAI API", "#10a37f", "O"), anthropic_api: B("Anthropic API", "#d97757", "A"),
  chat_gpt: B("OpenAI Codex", "#10a37f", "C"), claude: B("Claude Code", "#d97757", "✻"),
  gemini_api: B("Gemini (Cloud Monitoring)", "#4285f4", "G"), vertex_ai: B("Vertex AI", "#4285f4", "V"),
  aws_bedrock: B("AWS Bedrock", "#ff9900", "B"), azure_open_ai: B("Azure OpenAI", "#0078d4", "Az"),
  cursor: B("Cursor", "#8b8b8b", "Cu"), copilot: B("GitHub Copilot", "#6e40c9", "Co"),
  windsurf: B("Windsurf", "#09b6a2", "W"), mini_max: B("Minimax Coding", "#e73b77", "M"),
  kimi: B("Kimi", "#1783ff", "K"), qwen: B("Qwen", "#615ced", "Q"), groq: B("Groq", "#f55036", "g"),
  perplexity: B("Perplexity", "#20b8cd", "P"), mistral: B("Mistral", "#fa520f", "Mi"),
  cloudflare_ai: B("Cloudflare AI", "#f6821f", "CF"), ollama_cloud: B("Ollama Cloud", "#9a9a9a", "Ol"),
  xiaomi_mimo: B("Xiaomi MIMO", "#ff6900", "Mi"), open_code_go: B("Opencode Go", "#3b82f6", "OC"),
  qoder: B("Qoder", "#22c55e", "Qo"), gemini_ai: B("Gemini AI", "#4285f4", "G"), grok: B("Grok (xAI)", "#9a9a9a", "X"),
  nvidia_nim: B("NVIDIA NIM", "#76b900", "N"), poolside: B("Poolside", "#a855f7", "Po"), kiro: B("Kiro AI", "#8a5cf6", "Ki"),
  cline: B("Cline", "#22d3ee", "Cl"), code_buddy: B("CodeBuddy", "#6d5cf6", "CB"), zed: B("Zed", "#3b82f6", "Z"),
  alibaba_model_studio: B("Alibaba Model Studio", "#ff6a00", "Al"), alibaba_coding_plan: B("Alibaba Coding Plan", "#ff6a00", "Al"),
  alibaba_token_plan: B("Alibaba Token Plan", "#ff6a00", "Al"), alibaba_cloud: B("Alibaba Cloud", "#ff6a00", "Al"),
  antigravity: B("Antigravity", "#7c6cf0", "Ag"), kimchi: B("Kimchi", "#ef4444", "Km"),
  open_router: B("OpenRouter", "#6467f2", "OR"),
  nine_router: B("9router", "#0369a1", "9r"), litellm: B("LiteLLM proxy", "#14b8a6", "LL"),
  extension: B("Extension", "#8b5cf6", "Ex"),
};
export const brand = (p) => brands[p] ?? B(p, "#888888");
/** PRD 0029 RF3.4: the account's display name, its configured name or else its provider's. */
export const accountLabel = (r) => (r.account ?? "").trim() || brand(r.provider).label;
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
      <div class="lang-menu theme-menu" role="menu" aria-label="${te("theme.label")}" ${themeOpen ? "" : "hidden"}>${groups.map((g) => `<div class="theme-menu-group" role="presentation">${esc(groupLabel(g))}</div>${THEMES.filter((x) => x.group === g)
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
/** Button with the current language's flag and a menu (flag + name). `inline` shows the open list (settings screen). */
export function langMenuHtml({ inline = false, field = false } = {}) {
  const cur = LOCALES.find((l) => l.code === currentLocale()) ?? LOCALES[0];
  // PRD 0022 RF9.6 (DS-CMP-SEL-3): flag (decorative) + the endonym in its own language; the selected one is marked
  // by a check and weight, not by color alone.
  const role = inline ? "radio" : "menuitemradio";
  const items = LOCALES.map((l) => `<button role="${role}" aria-checked="${l.code === cur.code}" class="${l.code === cur.code ? "on" : ""}" data-locale="${esc(l.code)}"${inline ? ` tabindex="${l.code === cur.code ? 0 : -1}"` : ""}>${flagOf(l)}<span lang="${esc(l.code)}">${esc(l.name)}</span>${l.code === cur.code ? icon("tick", "sel-tick") : ""}</button>`).join("")
    // PRD 0035 RF1/RF3.3: registry languages without catalogs yet stay visible, in the fixed order, but cannot be chosen.
    + pendingLocales().map((l) => `<button type="button" role="${role}" aria-checked="false" aria-disabled="true" class="pending" data-locale-pending="${esc(l.code)}" tabindex="-1">${flagOf(l)}<span lang="${esc(l.code)}">${esc(l.name)}</span><small>${te("lang.inProgress")}</small></button>`).join("");
  if (inline) return `<div class="lang-list" role="radiogroup" aria-label="${te("lang.label")}">${items}</div>`;
  // DS-CMP-SEL-3 (mockup 50-settings-general-language): one field with the flag and the endonym, opening the list of
  // every language; it scales to the 14 languages of PRD 0035.
  if (field) return `<div class="lang lang-field">
    <button type="button" class="lang-field-btn" data-lang-toggle aria-haspopup="menu" aria-expanded="${langOpen}" aria-label="${te("lang.label")}: ${esc(cur.name)}">${flagOf(cur)}<span lang="${esc(cur.code)}">${esc(cur.name)}</span>${icon("chevDown")}</button>
    <div class="lang-menu" role="menu" ${langOpen ? "" : "hidden"}>${items}</div>
  </div>`;
  return `<div class="lang">
    <button class="icon lang-btn" data-lang-toggle title="${te("lang.label")}: ${esc(cur.name)}" aria-label="${te("lang.label")}: ${esc(cur.name)}" aria-haspopup="menu" aria-expanded="${langOpen}">${flagOf(cur)}</button>
    <div class="lang-menu" role="menu" ${langOpen ? "" : "hidden"}>${items}</div>
  </div>`;
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
    if (e.target.closest("[data-sort-toggle]")) { sortOpen = !sortOpen; langOpen = false; return rerender(); }
    const sortItem = e.target.closest("[data-sort]");
    if (sortItem) { setSort(sortItem.dataset.sort); sortOpen = false; return rerender(); }
    if (e.target.closest("[data-lang-toggle]")) { langOpen = !langOpen; sortOpen = false; themeOpen = false; return rerender(); }
    if (e.target.closest("[data-theme-toggle]")) { themeOpen = !themeOpen; langOpen = false; sortOpen = false; return rerender(); }
    const pick = e.target.closest("[data-theme-pick]");
    if (pick) {
      themeOpen = false; theme.id = themeById(pick.dataset.themePick).id; setPreference("theme", theme.id).catch(() => {});
      return crossFade(() => { applyTheme(); return rerender(); });
    }
    const item = e.target.closest("[data-locale]");
    if (item) { langOpen = false; await setLocale(item.dataset.locale); return crossFade(rerender); } // loads first: the transition does not freeze the screen
    if ((langOpen || sortOpen || themeOpen) && !e.target.closest(".lang, .theme-field")) { langOpen = false; sortOpen = false; themeOpen = false; rerender(); }
  });
  addEventListener("keydown", (e) => { if (e.key === "Escape" && (langOpen || sortOpen || themeOpen)) { langOpen = false; sortOpen = false; themeOpen = false; rerender(); } });
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

// Accounts turned off in the panel (card switch): shared between windows and with the tray menu.
const disabledListeners = new Set();
export const disabledAccounts = persistentSet("cuotai.disabledAccounts", () => disabledListeners.forEach((f) => f()));
export const onDisabledChange = (f) => disabledListeners.add(f);

// ---------- data state ----------
// The backend is the only source: per-account schedule (RefreshCycle) and emits "snapshot".
export function createStore(render) {
  const s = { view: null, error: null };
  const set = (v) => { s.view = v; s.error = v?.error ?? null; render(); };
  s.refresh = () => refreshAll().catch((e) => { s.error = String(e); render(); });
  // The browser demo has no "snapshot" event: re-read the view after a refresh so its recovery shows.
  s.refreshOne = (i) => refreshAccount(i).then(() => isDesktop || getView().then(set)).catch((e) => { s.error = String(e); render(); });
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
const dm = (label, used, limit, resetH, kind, window, unit = "requests") => {
  const unmeasured = used === 0 && limit === 0;
  const exhausted = !unmeasured && limit > 0 && used >= limit;
  return { label, used, limit, remaining: limit - used, unit, kind, window, period: null, resets_at: h(resetH),
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
const demoReports = [
  dr("alex@example.com", "antigravity", [dm("Gemini (5h)", 15, 1000, 0.37, "quota", "window_5h"), dm("Claude & GPT (5h)", 0, 1000, 5, "quota", "window_5h"), dm("Gemini (Weekly)", 120, 1000, 151, "quota", "weekly"), dm("Claude & GPT (Weekly)", 0, 1000, 151.9, "quota", "weekly")]),
  // PRD 0023: the exhausted week is also paying for extra usage ("Limit hit · on extra $6.25").
  { ...dr("Account 2", "claude", [dm("session (5h)", 0, 100, 4.5, "quota", "window_5h", "%"), dm("weekly (7d)", 100, 100, 24.6, "quota", "weekly", "%"),
      { label: "Extra usage", used: 6.25, limit: 50, remaining: null, unit: "USD", kind: "spend", window: "monthly", period: null, resets_at: h(300), billing: "overage",
        state: "available", ratio: null, uncapped: true, no_cap: false }]),
    class: "exhausted", model_count: 1, plan: "Max",
    overage: { state: "active", spent: 6.25, unit: "USD", currency: "USD", cap: 50, cycle_ends_at: h(300),
      text: { id: "overage.on_extra", params: { amount: "$6.25" }, fallback: "On extra · $6.25" },
      summary: { id: "overage.limit_hit_on_extra", params: { amount: "$6.25" }, fallback: "Limit hit · on extra $6.25" }, caveat: null } },
  // PRD 0010 RF3 fixtures: an aging reading (10 min against 300 s) and a failure that kept its last valid reading.
  { ...dr("alex@example.com", "chat_gpt", [dm("5h", 6, 100, 67.6, "quota", "window_5h", "%")]), checked_at: ago(10), freshness: "aging", plan: "Plus" },
  { ...dr("octo-dev", "copilot", [dm("chat", 0, 0, 2.6, "quota", "other"), dm("Premium requests", 7026, 7000, 2.6, "quota", "other")]), class: "exhausted" },
  { ...dr("Personal", "groq", [], ["Groq connected. No rate-limit data reported for this key yet."]), model_count: 4 },
  // PRD 0023 RF10.3 demo: the video model is at its limit (the core's `scope_blocks`); the account keeps its quota.
  { ...dr("Personal", "mini_max", [dm("M-series (5h)", 6, 100, 2.6, "quota", "window_5h"), dm("M-series (7d)", 60, 100, 98.6, "quota", "weekly"), dm("Video (5h)", 0, 100, 2.6, "quota", "window_5h")]),
    scope_blocks: [{ index: 2, label: "Video", resets_at: h(2.6), text: { id: "metrics.scope_blocked", params: { model: "Video", at: new Date(h(2.6)).toTimeString().slice(0, 5) }, fallback: `Video at its limit until ${new Date(h(2.6)).toTimeString().slice(0, 5)}` } }] },
  dr("Account 2", "code_buddy", [dm("Monthly", 0, 100, 757, "quota", "monthly"), dm("Bonus Pack 1", 0, 250, 62, "quota", null)]),
  dr("Account 1", "kimi", [dm("Weekly", 100, 100, 109, "quota", "weekly"), dm("Ratelimit", 0, 100, 4.8, "rate_limit", "other")]),
  dr("Personal", "ollama_cloud", [dm("Monthly", 0, 100, 248, "quota", "monthly")]),
  drn("Personal", "anthropic_api", [], demoNote("auth", "Credential missing or expired."), "authentication_error"),
  drn("Personal", "mistral", [], demoNote("rate", "HTTP 429: rate limit reached."), "rate_limited"),
  { ...drn("Dev", "cursor", [], demoNote("down", "Service unavailable right now."), "error"), checked_at: ago(1),
    last_good: { checked_at: ago(42), metrics: [dm("Fast requests", 380, 500, 120, "quota", "monthly")] } },
  drn("Work", "windsurf", [], demoNote("config", "Add the API key to this account to finish setting it up."), "needs_configuration"),
  { ...dr("Account 3", "kiro", [dm("credit", 596.15, 10000, 360, "quota", "other", "credits")]),
    identity: { email: null, copy: { label: { id: "identity.profile_arn", params: {}, fallback: "Profile ARN" }, value: "arn:aws:codewhisperer:us-east-1:123456789012:profile/EXAMPLEPROFILE" },
      badges: [{ tone: "accent", text: "IAM Identity Center" }, { tone: "info", text: "us-east-1" }, { tone: "good", text: { id: "demo.kiro.badge.active", params: {}, fallback: "active" } }] } },
  { ...dr("alex@example.com", "grok", [dm("On-demand", 1, 1, 100, "quota", "monthly")]), identity: { email: "alex@example.com", badges: [], copy: null } },
  dr("Personal", "deepseek_demo", [{ label: "Balance", used: null, limit: null, remaining: 0, unit: "USD", kind: "balance", window: null, period: null, resets_at: null, state: "available", ratio: null, uncapped: true, no_cap: false }]),
  // PRD 0005 RF4.2 fixtures: an Unavailable provider (no query) and an Undocumented one whose response broke the schema.
  drn("Work", "poolside", [], demoNote("poolside", "Poolside publishes no usage or balance endpoint per key."), "unsupported"),
  { ...dr("Personal", "zed", [], ["Response is incompatible with the query interface; no value was estimated."], "error"),
    notes_l10n: [{ id: "query.schema", params: {}, fallback: "Response is incompatible with the query interface; no value was estimated." }] },
  // PRD 0010 RF2.3 fixture: just opened, no answer yet; the reading recorded 3 h ago shows as stale.
  { ...dr("Restored", "chat_gpt", []), pending: true, class: "unmeasured", checked_at: ago(180),
    last_good: { checked_at: ago(180), metrics: [dm("5h", 30, 100, 2, "quota", "window_5h", "%")] } },
  // PRD 0016 RF1.2 / PRD 0047 G5 fixture: a session set up in the terminal, offered for the vault;
  // RF3.2: its pasted session expires in 2 days (the card warns).
  { ...dr("Personal", "xiaomi_mimo", [dm("Weekly", 6, 100, 96, "quota", "weekly", "%")]),
    session_expiry: { expires_at: new Date(Date.now() + 2.5 * 86400000).toISOString(), days: 2, soon: true,
      label: { id: "user_session.expiry.days", params: { days: 2 }, fallback: "Session expires in 2 days." } } },
];
// PRD 0036 RF8 demo: a local 9router instance with four connections (two Claude, one of them exhausted; a Codex one
// off in 9router; an unmapped GLM one whose query failed). `?aggregator=off` leaves it out.
// PRD 0036 RF8.5 demo: the `hidden` and `show_both` lists of the 9router account (sub ids).
const demoConnFlags = { hidden: new Set(), show_both: new Set() };
const demoConnection = (id, label, upstream, mapped, metrics, extra = {}) => ({
  ...dr(`home/${id}`, mapped ?? "nine_router", metrics), account: label, demo_id: id,
  connection: { label, upstream, mapped, enabled_upstream: true, duplicate_of: null, plan: null, via: `open-sse/services/usage/${upstream}.js`, sub: id, show_both: false }, ...extra,
});
if (new URLSearchParams(location.search).get("aggregator") !== "off") {
  const conns = [
    demoConnection("a1b2c3", "work", "claude", "claude", [dm("session (5h)", 22, 100, 3, "quota", "window_5h", "%"), dm("weekly (7d)", 40, 100, 80, "quota", "weekly", "%")],
      { plan: "Max", connection: { label: "work", upstream: "claude", mapped: "claude", enabled_upstream: true, duplicate_of: new URLSearchParams(location.search).has("dup") ? "claude:Account 2" : null, plan: "Max", via: "open-sse/services/usage/claude.js", sub: "a1b2c3", show_both: false } }),
    { ...demoConnection("d4e5f6", "personal", "claude", "claude", [dm("session (5h)", 100, 100, 2, "quota", "window_5h", "%")]), class: "exhausted" },
    demoConnection("0718aa", "team", "codex", "chat_gpt", [], { status: "ready", notes: ["Off in 9router."],
      connection: { label: "team", upstream: "codex", mapped: "chat_gpt", enabled_upstream: false, duplicate_of: null, plan: null, via: null, sub: "0718aa", show_both: false } }),
    { ...demoConnection("9bc0de", "zhipu", "glm", null, [], { status: "error", notes: ["Reconnect this account in 9router."], class: "unmeasured" }) },
  ];
  demoReports.push({ ...dr("home", "nine_router", [], ["2 of 4 9router connections with usage.", "9router 0.5.109."]),
    class: "available", connections: conns,
    // PRD 0036 RF2.3/RF2.4: `?aggwarn` shows both standing warnings (the demo instance itself is loopback).
    ...(new URLSearchParams(location.search).has("aggwarn") ? { connection_warnings: { exposed: true, private_http: true } } : {}),
    aggregator: { kind: "9router", version: "0.5.109", host: "127.0.0.1:20128", available: 1, total: 3, connections: conns.length,
      pools: [
        { upstream: "claude", provider: "claude", available: 1, exhausted: 1, total: 2, returns_at: null,
          summary: { id: "pool.available", params: { upstream: "Claude", available: "1", total: "2" }, fallback: "Claude · 1 of 2 available" } },
        { upstream: "glm", provider: null, available: 0, exhausted: 0, total: 1, returns_at: null,
          summary: { id: "pool.unmeasured", params: { upstream: "glm", total: "1" }, fallback: "glm · 1 connections with unknown usage" } },
      ],
      alert_each: false, dashboard: true } });
}
// PRD 0016 RF2.1 demo: the platform session slot the core declares for Xiaomi MiMo.
// PRD 0027 RF6.4: the native-login detections of the core table (Kimi CLI, Grok CLI, Zed) for the demo picker.
export const DEMO_NATIVE_DETECT = [["claude", "claude_cli", "Claude Code", "Claude Code"], ["chat_gpt", "codex_cli", "Codex CLI", "OpenAI Codex"], ["kimi", "kimi_cli", "Kimi Code CLI", "Kimi"], ["grok", "grok_cli", "Grok CLI", "Grok (xAI)"], ["zed", "zed", "Zed", "Zed"]];
/** The demo's stable account id (PRD 0029) of the account at `index`. */
const demoAccountId = (index) => `acc_demo${String(index).padStart(8, "0")}`;
const DEMO_KEY_INPUT = { slot: "credential_env", label: { id: "secret_input.credential.label", params: {}, fallback: "API key or access token" },
  help: { id: "secret_input.help", params: {}, fallback: "Paste the value. Cuotai keeps it in the system vault and sends it only to this provider; the configuration file keeps just a reference." },
  required: true, cookie: false, field: null };
const DEMO_SESSION_SLOT = { slot: "platform_session_env", open_url: "https://platform.xiaomimimo.com/console/plan-manage",
  label: { id: "user_session.slot.xiaomi_mimo.platform_session_env", params: {}, fallback: "platform.xiaomimimo.com web session (balance and Token Plan)" },
  instructions: { id: "user_session.instructions", params: { site: "platform.xiaomimimo.com" }, fallback: "Sign in to platform.xiaomimimo.com in your browser. In the developer tools (F12), Network tab, reload the page, pick a request to platform.xiaomimimo.com and copy its Cookie header or use \"Copy as cURL\"; the cookie table of the Application tab works too. Only this session's cookies are kept; the rest of the pasted text is discarded." },
  hosts: ["platform.xiaomimimo.com"], capture: true };
// ponytail: fixed demo fixtures name their core-selected metric and rank explicitly; add a mocked backend when
// the browser demo needs dynamic reports. This keeps binding choice rules out of the interface.
const DEMO_BINDINGS = new Map([
  [0, [2, 5]], [1, [1, 0]], [2, [0, 6]], [3, [1, 1]], [4, [null, 10]], [5, [1, 4]], [6, [0, 8]], [7, [0, 2]],
  [8, [0, 9]], [9, [null, 11]], [10, [null, 12]], [11, [null, 13]], [12, [null, 14]], [13, [0, 7]], [14, [0, 3]], [15, [null, 15]],
].map(([index, binding]) => [demoReports[index], binding]));
const demoBinding = (r) => {
  const [metricIndex] = DEMO_BINDINGS.get(r) ?? [];
  if (metricIndex == null) return null;
  const m = r.metrics[metricIndex];
  const exhausted = m.state === "exhausted";
  const usedRatio = exhausted ? 1 : 1 - m.ratio;
  return { index: metricIndex, label: m.label, window: m.window, used_ratio: usedRatio, remaining: m.limit * (1 - usedRatio),
    resets_at: m.resets_at, reason: exhausted ? "exhausted" : "tightest", blocked_until: exhausted ? m.resets_at : null };
};
// ponytail: fixed core pace output per demo metric (one per stage), same shape as `MetricView.pace`; the browser
// demo never estimates pace itself. Add a mocked backend when the demo needs paces that follow the clock.
const hhmm = (iso) => fmtLocal(new Date(iso), HHMM);
const dp = (stage, tone, expected, used, resetH, runsOutH = null) => {
  const delta = (used - expected) * 100, points = String(Math.round(Math.abs(delta)));
  const label = { reserve: { id: "pace.reserve", params: { delta: points }, fallback: `${points} points in reserve` },
    on_pace: { id: "pace.on_pace", fallback: "On pace" }, ahead: { id: "pace.ahead", params: { delta: points }, fallback: `${points} points above the pace` },
    exhausted: { id: "pace.exhausted", fallback: "Limit reached until the reset" } }[stage];
  const at = runsOutH == null ? null : h(runsOutH);
  const margin = runsOutH == null ? "" : legacyCountdown(h(resetH - runsOutH)).replace(/^(\d+d \d+h) \d+m$/, "$1");
  const outlook = stage === "exhausted" ? null : at
    ? { id: "pace.runs_out_at", params: { at: hhmm(at), margin }, fallback: `runs out at ${hhmm(at)}, ${margin} before the reset` }
    : { id: "pace.lasts_until_reset", fallback: "lasts until the reset" };
  // PRD 0024 RF1: the core's stage chip in both units, as `PaceView.chips` carries it (demo values).
  const pct = String(Math.min(999, Math.round((Math.abs(used - expected) / Math.max(expected, 0.01)) * 100)));
  const chip = (glyph, id, params, compact) => ({ glyph, text: { id, params, fallback: id }, compact, hint: null });
  const chips = {
    exhausted: { points: chip("✕", "pace.chip.exhausted", {}, null), percent: chip("✕", "pace.chip.exhausted", {}, null) },
    on_pace: { points: chip("→", "pace.chip.on_pace", {}, null), percent: chip("→", "pace.chip.on_pace", {}, null) },
    ahead: { points: chip("↑", "pace.chip.ahead", { d: points }, `${points} pts`), percent: chip("↑", "pace.chip.ahead_pct", { p: pct }, `${pct}%`) },
    reserve: { points: chip("↓", "pace.chip.reserve", { d: points }, `${points} pts`), percent: chip("↓", "pace.chip.reserve_pct", { p: pct }, `${pct}%`) },
  }[stage];
  return { elapsed_fraction: expected, expected_used_fraction: expected, used_fraction: used, delta_points: delta, stage, tone, source: "average",
    resets_at: h(resetH), projected_exhaustion_at: at, lasts_until_reset: stage !== "exhausted" && !at, label, outlook, chips };
};
/** The local time the shell sends with a projection ("18:53", or the weekday and time when not today). */
const demoAt = (iso) => {
  const d = new Date(iso);
  const time = fmtLocal(d, HHMM);
  return d.toDateString() === new Date().toDateString() ? time : `${fmtLocal(d, { weekday: "short" })} ${time}`;
};
/** PRD 0024 RF4.3: the forecast the shell adds to a metric whose pace runs out before the reset (demo values). */
const demoForecast = (m, pace) => {
  if (!pace?.projected_exhaustion_at) return null;
  const at = demoAt(pace.projected_exhaustion_at);
  const margin = legacyCountdown(new Date(new Date(m.resets_at).getTime() - new Date(pace.projected_exhaustion_at).getTime() + Date.now()).toISOString())?.replace(/^(\d+d \d+h) \d+m$/, "$1") ?? "";
  return { at,
    callout: { id: "alerts.body.projected", params: { metric: m.label, at, margin }, fallback: `At this rate ${m.label} runs out ≈${at}, ${margin} before the reset` },
    chip: { glyph: "↑", text: { id: "pace.chip.runs_out", params: { at }, fallback: `Runs out ≈${at}` }, compact: `≈${at}`, hint: null } };
};
// report index → [pace_index, { metric index: pace }]; windows that just opened (or not yet) have none, as in the core.
const DEMO_PACES = new Map([
  [0, [2, { 0: dp("reserve", "good", 0.926, 0.015, 0.37), 2: dp("on_pace", "neutral", 0.101, 0.12, 151, 124.7), 3: dp("reserve", "good", 0.096, 0, 151.9) }]],
  [1, [1, { 0: dp("reserve", "good", 0.1, 0, 4.5), 1: dp("exhausted", "bad", 0.854, 1, 24.6) }]],
  [5, [1, { 0: dp("reserve", "good", 0.48, 0.06, 2.6), 1: dp("ahead", "warn", 0.413, 0.6, 98.6, 46.3), 2: dp("reserve", "good", 0.48, 0, 2.6) }]],
  [7, [0, { 0: dp("exhausted", "bad", 0.351, 1, 109) }]],
  [8, [0, { 0: dp("reserve", "good", 0.656, 0, 248) }]],
  [14, [0, { 0: dp("exhausted", "bad", 0.866, 1, 100) }]],
].map(([index, pace]) => [demoReports[index], pace]));
// PRD 0022 RF18.2: `?fixture=six` swaps the demo accounts for the mockups' set (the six accounts plus a second Claude
// account, example.com identities only), so the captures compare with `assets/0022/after/` (01, 03, 10, 11). With
// `&bedrock` it adds an AWS Bedrock account (PRD 0027 RF3.1 "Billed per query" chip). `?mono=XY` (any demo) gives the
// first account a monogram override (DS-CMP-MONO-4), as `desktop.json` would.
const DEMO_QUERY = new URLSearchParams(globalThis.location?.search ?? "");
if (DEMO_QUERY.get("fixture") === "six") {
  const overage = (amount) => ({ state: "active", spent: amount, unit: "USD", currency: "USD", cap: 50, cycle_ends_at: h(300),
    text: { id: "overage.on_extra", params: { amount: `$${amount.toFixed(2)}` }, fallback: `On extra · $${amount.toFixed(2)}` },
    summary: { id: "overage.limit_hit_on_extra", params: { amount: `$${amount.toFixed(2)}` }, fallback: `Limit hit · on extra $${amount.toFixed(2)}` }, caveat: null });
  const six = [
    [{ ...dr("work@example.com", "claude", [dm("session (5h)", 78, 100, 1.2, "quota", "window_5h", "%"), dm("weekly (7d)", 41, 100, 70, "quota", "weekly", "%")]), plan: "Max", model_count: 3 },
      [0, 0], [0, { 0: dp("ahead", "warn", 0.62, 0.78, 1.2, 0.4) }]],
    [{ ...dr("side@example.com", "claude", [dm("session (5h)", 29, 100, 3.1, "quota", "window_5h", "%"), dm("weekly (7d)", 12, 100, 120, "quota", "weekly", "%")]), plan: "Pro" },
      [0, 3], [0, { 0: dp("on_pace", "good", 0.3, 0.29, 3.1) }]],
    [{ ...dr("me@example.com", "chat_gpt", [dm("5h", 37, 100, 2.4, "quota", "window_5h", "%"), dm("weekly", 20, 100, 90, "quota", "weekly", "%")]), plan: "Plus" }, [0, 2], [0, { 0: dp("reserve", "good", 0.5, 0.37, 2.4) }]],
    [{ ...dr("dev@example.com", "cursor", [dm("Fast requests", 500, 500, 200, "quota", "monthly"),
      { label: "Extra usage", used: 6.25, limit: 50, remaining: null, unit: "USD", kind: "spend", window: "monthly", period: null, resets_at: h(200), billing: "overage",
        state: "available", ratio: null, uncapped: true, no_cap: false }]), class: "exhausted", plan: "Pro", overage: overage(6.25) }, [0, 1], null],
    [dr("ops@example.com", "kimi", [dm("Weekly", 41, 100, 50, "quota", "weekly"), dm("Ratelimit", 0, 100, 4.8, "rate_limit", "other")]), [0, 4], [0, { 0: dp("on_pace", "neutral", 0.42, 0.41, 50) }]],
    [{ ...drn("team@example.com", "groq", [], demoNote("down", "Service unavailable right now."), "error"), checked_at: ago(2),
      last_good: { checked_at: ago(38), metrics: [dm("Requests (day)", 26, 100, 9, "quota", "daily")] } }, [null, 5], null],
    [drn("api@example.com", "anthropic_api", [], demoNote("auth", "Credential missing or expired."), "authentication_error"), [null, 6], null],
    ...(DEMO_QUERY.has("bedrock") ? [[dr("billing@example.com", "aws_bedrock", [], ["Cost Explorer: no spend recorded this month (demo)."]), [null, 7], null]] : []),
  ];
  demoReports.splice(0, demoReports.length, ...six.map(([r]) => r));
  for (const [r, binding, pace] of six) {
    DEMO_BINDINGS.set(r, binding);
    if (pace) DEMO_PACES.set(r, pace);
  }
}
if (DEMO_QUERY.get("mono") && demoReports[0]) demoReports[0].monogram = DEMO_QUERY.get("mono");
// The core's binding of a demo connection: its most consumed quota (demo only; the app gets it from the core).
const demoConnectionBinding = (c) => {
  const ratio = (m) => (m.limit ? m.used / m.limit : 0);
  const index = c.metrics.reduce((best, m, i) => (best < 0 || ratio(m) > ratio(c.metrics[best]) ? i : best), -1);
  if (index < 0) return null;
  const m = c.metrics[index], used = Math.min(1, ratio(m));
  return { index, label: m.label, window: m.window, used_ratio: used, remaining: m.limit * (1 - used), resets_at: m.resets_at,
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
const demoView = () => {
  const now = new Date().toISOString();
  const reports = demoReports.map((r, index) => {
    const [paceIndex = null, paces = {}] = r.status === "ready" ? DEMO_PACES.get(r) ?? [] : [];
    const notify = !demoSettings.notify.off_accounts.includes(demoKey(r));
    const metrics = r.metrics.map((m, i) => ({ ...m, pace: paces[i] ?? null, forecast: demoForecast(m, paces[i]), resets_label: m.resets_at && legacyCountdown(m.resets_at) ? { id: "resets.pair", params: { at: demoAt(m.resets_at), in: t("quota.resetsIn", { time: legacyCountdown(m.resets_at) }) }, fallback: `Resets ${demoAt(m.resets_at)}` } : null, alert: demoMetricAlert(r, m, notify) }));
    const incident = demoIncident(r);
    // RF3 mirror: a failed query during an incident gains the core's note, its status unchanged.
    const note = incident && r.status !== "ready" && ["degraded", "partial_outage", "major_outage"].includes(incident.state) && DEMO_INCIDENT_NOTE;
    // PRD 0036 RF8: a connection keeps its aggregator's index (actions act on it) and its own core key.
    const connections = (r.connections ?? []).filter((c) => !demoConnFlags.hidden.has(c.demo_id))
      .map((c) => ({ ...c, connection: { ...c.connection, show_both: demoConnFlags.show_both.has(c.demo_id),
        duplicate_of: demoConnFlags.show_both.has(c.demo_id) ? null : c.connection.duplicate_of } }))
      .map((c) => ({ ...c, index, binding: demoConnectionBinding(c), binding_rank: 0, pace_index: null,
      metrics: c.metrics.map((m) => ({ ...m, pace: null, alert: null })), next_refresh_at_ms: Date.now() + 90e3, access: demoAccess(r),
      account_key: `nine_router:${r.account}/${c.demo_id}`, notify: true, usage_alerts: [], incident: null }));
    return { ...r, account: demoName(r.account), connections, index, binding: demoBinding(r), binding_rank: DEMO_BINDINGS.get(r)?.[1] ?? DEMO_BINDINGS.size + index,
      metrics, pace_index: paceIndex,
      next_refresh_at_ms: Date.now() + (40 + index * 37) * 1000, uses_oauth: r.provider === "kiro" || r.uses_oauth, access: demoAccess(r),
      account_key: demoKey(r), notify, usage_alerts: metrics.filter((m) => m.alert?.active).map((m) => m.label), incident,
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
    history_epoch: demoSettings.historyEpoch,
    notify: { enabled: demoSettings.notify.enabled, next: demoNextNotice(reports), alerts: demoSettings.alerts.enabled },
    status: { enabled: demoSettings.statusQueries, domains: [...new Set(demoReports.map((r) => DEMO_STATUS_HOSTS[r.provider]).filter(Boolean))],
      outage: reports.some((r) => ["partial_outage", "major_outage"].includes(r.incident?.state)) },
    serve: demoServeView(), hooks: { state: demoHooks.enabled ? demoHooksView().state : "off" },
    tray: { style: demoSettings.tray.style, selected: demoTraySelection().map((a) => a.key), popover_view: demoSettings.tray.popover_view ?? "list" } };
};
// ponytail: a rough demo of the core's `tray::summary` (PRD 0025 RF1); the app always asks the core.
const demoTrayAccounts = () => demoReports.map((r) => {
  const b = demoBinding(r);
  return { key: demoKey(r), provider: r.provider, provider_label: r.provider, name: demoName(r.account),
    remaining: b ? Math.max(0, 1 - b.used_ratio) : null, exhausted: b?.reason === "exhausted" };
});
function demoTraySelection(style = demoSettings.tray.style, accounts = demoSettings.tray.accounts) {
  const all = demoTrayAccounts();
  if (accounts) return accounts.map((k) => all.find((a) => a.key === k)).filter(Boolean);
  return all.slice(0, style === "meters" ? 4 : style === "logos_percent" ? 6 : all.length);
}
function demoTrayPreview({ style, accounts }) {
  const picked = demoTraySelection(style, accounts);
  const pct = (a) => (a.remaining == null ? "—" : `${Math.round(a.remaining * 100)}%`);
  const items = picked.map((a) => ({ ...a, level: a.remaining == null ? null : level(a.remaining * 100), figure: pct(a) }));
  const tight = [...items].filter((a) => a.remaining != null).sort((a, b) => a.remaining - b.remaining)[0];
  const title = style === "tightest_percent" ? (tight ? `${tight.figure} · ${tight.name}`.slice(0, 24) : items.length ? "—" : null)
    : style === "logos_percent" ? items.slice(0, 3).map((a) => `${a.provider_label[0].toUpperCase()} ${a.figure}`).join(" · ") : null;
  const label = items.map((a) => `${a.name} ${a.figure}`).join("; ");
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
    url: state === "on" ? `http://127.0.0.1:${s.port}` : null,
    error: taken ? { id: "serve.error.port_taken", params: { port: s.port }, fallback: `Port ${s.port} is in use; choose another port for the local server.` } : null,
    cuotai_on_port: taken, default_port: 6738, port_min: 1024, port_max: 65535,
    browsers_warning: { id: "serve.allow_browsers.warning", params: {}, fallback: "With browsers allowed, any page open in your browser can read your usage from the local server (with a token, any page that knows it). Turn it on only if a page of yours needs this data." } };
}
// ponytail: demo mirror of the widget snapshot (PRD 0014 RF4.1); nothing is written in the browser. `?widgets`
// starts with publishing on, so the file path and the macOS prerequisites are visible as in the app.
const DEMO_WIDGETS = new URLSearchParams(location.search).has("widgets");
function demoWidgetsView() {
  const w = demoSettings.widgets;
  return { prefs: { enabled: w.enabled, include_identity: w.include_identity, max_accounts: w.max_accounts },
    path: "~/.local/share/cuotai/widget.json (demo)", app_group: DEMO_WIDGETS, macos: true, accounts_min: 1, accounts_max: 24 };
}
// ponytail: demo mirror of the backend's hooks (PRD 0013 RF4) with the core's validation, listing and digest reduced
// to what the screen needs; nothing runs in the browser: a test logs a made-up successful run after a moment.
// `?hooks=changed` starts with the hooks on and a rule changed since its confirmation (the main-window banner).
const DEMO_HOOK_EVENTS = ["quota_threshold_crossed", "quota_exhausted", "quota_recovered", "quota_reset", "account_blocked",
  "account_unblocked", "refresh_failed", "refresh_recovered", "auth_required", "provider_incident", "usage_updated", "overage_started", "overage_ended", "session_expiring", "quota_reset_soon",
  "pool_exhausted", "pool_recovered"];
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
  cursor: "status.cursor.com", windsurf: "status.windsurf.com", grok: "status.x.ai" };
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
  const [state, title, url, startedH] = offline ? ["unknown"] : DEMO_INCIDENTS[r.provider] ?? ["operational"];
  return { provider: r.provider, state, label: dl(`incident.state.${state}`, DEMO_STATE_EN[state]), page_url: `https://${host}/`,
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
      .map((m) => ({ index: r.index, provider: r.provider, account: r.account, window: m.window, label: m.label, resets_at: m.resets_at, at: new Date(new Date(m.resets_at).getTime() - lead).toISOString() })))
    .filter((e) => new Date(e.at).getTime() > Date.now())
    .sort((a, b) => a.at.localeCompare(b.at, "en")); // ISO instants: a fixed, language-free order
  return next[0] ?? null;
}
// ponytail: demo mirror of `access::describe` + `UsageAccess::explain` (default modes, generic pt summary and
// evidence); the app reads both from the core.
const DEMO_ROUTER = "https://github.com/decolua/9router/blob/f01fb909e37189008080632ddaf404f096345cde/open-sse/services/usage/";
function demoAccess(r) {
  const level = demoLevelOf(r.provider);
  const reason = level === "unavailable" ? dlx(`access.reason.${r.provider}`, "demo.access.reason", "No public usage query for this credential (demo).") : null;
  const url = level === "undocumented" ? `${DEMO_ROUTER}${r.provider}.js` : "https://example.com/docs (demo)";
  const changed = level === "undocumented" && r.status === "error" && r.notes_l10n?.some((n) => n.id === "query.schema")
    ? { id: "access.interface_changed", params: { evidence: url }, fallback: `The response does not follow the known internal interface: it may have changed without notice. Reference contract: ${url}` }
    : null;
  return { level, label: demoLevel(level), summary: dlx(`access.summary.${r.provider}`, "demo.access.summary", "Demo usage query."), reason, metered: r.provider === "aws_bedrock",
    evidence: { kind: level === "undocumented" ? "reference" : "documentation", url },
    explanation: r.status === "ready" ? null : reason ?? changed, session: demoSessionNotice(demoSessionState(r)) };
}
// PRD 0016 RF4 demo: the MiMo account reads its platform session from a variable until it is migrated (or pasted), then
// from the vault until it is forgotten; the card mirrors `user_session::notice`.
const demoSessionState = (r) => r.provider !== "xiaomi_mimo" || demoSettings.sessionForgotten.has(r) ? null
  : demoSettings.sessionMigrated.has(r) ? "vault" : "env";
const demoSessionNotice = (state) => state && { level: dl("access.credential.user_session", "User session"),
  warning: state === "vault"
    ? dl("user_session.warning", "This is a full login session. Cuotai stores it in the system vault and uses it only to query this service.")
    : dl("user_session.warning_env", "This is a full login session. Cuotai reads it from an environment variable and uses it only to query this service; move it to the system vault in the account wizard.") };
const demoSettings = { monograms: {}, profileExisted: new URLSearchParams(location.search).has("profile-existed"), whatsNewDue: new URLSearchParams(location.search).has("whats-new"), tray: { style: "tightest_percent", colored: false, accounts: null, popover_view: null }, advancedOpen: false,
  // `?first-run` shows the first-run step (PRD 0025 RF6) in the demo.
  onboarding: new URLSearchParams(location.search).has("first-run") ? 0 : 1,
  custom: null, config: null, generation: 1, historyRecording: true, historyEpoch: 0, historyErased: new Set(), sessionMigrated: new Set(), sessionForgotten: new Set(),
  notify: { enabled: false, lead_minutes: 0, when: "used", include_other: false, off_accounts: [], private: true },
  quiet: { enabled: false, days: {}, digest: true }, missedSeen: new Set(),
  alerts: { enabled: false, thresholds: [75, 90], exhaustion: true, recovery: true, overage: true, muted: {} }, statusQueries: DEMO_STATUS != null, detect: { enabled: new URLSearchParams(location.search).get("detect") === "ask" ? null : true, dismissed: [] },
  serve: { enabled: DEMO_SERVE_TAKEN, port: 6738, allow_browsers: false, secret: null },
  widgets: { enabled: DEMO_WIDGETS, include_identity: false, max_accounts: 6, written: false } };
const demoPermission = (on) => (!on ? "off" : new URLSearchParams(location.search).has("notify-denied") ? "denied" : "granted");
// Models in the demo: same validation (index + generation) and the `unsupported` state with an explicit demonstration reason;
// no invented catalog is shown as real.
const demoModels = async ({ index, generation }) => {
  if (generation !== demoSettings.generation) throw STALE;
  if (!demoReports[index]) throw JSON.stringify({ id: "models.error.invalid_index", params: {}, fallback: "Account does not exist in this configuration." });
  return { state: "unsupported", reason: { id: "models.demo", params: {}, fallback: "Browser demo: the model catalog is only queried in the desktop app." } };
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
    return { key: `${r.provider}-${i}`, label: m.label, unit: m.unit, kind: m.kind, window: m.window, limit, samples: points.length,
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
    return { day, share: gap ? null : Math.min(1, Math.max(0.05, demoWave(seed + back) * 1.2)), is_today: back === 0 };
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
    { occurred_at: at(4), kind: "quota_threshold_crossed", account: "work@example.com", parent: demoReports[0].account, is_connection: true, summary: { id: "events.summary.quota_threshold_crossed", params: { metric: "weekly (7d)", threshold: 80 }, fallback: "weekly (7d): 80% used" } },
    { occurred_at: at(7), kind: "quota_reset", account: demoReports[1].account, summary: { id: "events.summary.quota_reset", params: { metric: "session (5h)" }, fallback: "session (5h): window reset" } },
    { occurred_at: at(30), kind: "quota_exhausted", account: demoReports[1].account, summary: { id: "events.summary.quota_exhausted", params: { metric: "weekly (7d)" }, fallback: "weekly (7d): quota exhausted" } },
    { occurred_at: at(55), kind: "refresh_failed", account: null, summary: { id: "events.summary.refresh_failed", params: { category: { id: "events.category.provider_error", params: {}, fallback: "provider error" } }, fallback: "Refresh failed: provider error" } },
  ].filter((e) => account == null || e.account === demoReports[account].account).map((e) => ({ ...e, account: demoName(e.account), parent: e.parent ? demoName(e.parent) : e.parent }));
  return { summary: { range, hits: accounts.reduce((a, x) => a + x.hits, 0), average_peak: averages.length ? averages.reduce((a, x) => a + x, 0) / averages.length : null,
      accounts, extra_spent: account == null && !erased ? [{ unit: "USD", amount: 6.25 }] : null, daily: dailyAll, unknown_limits: 0,
      retention_days: 90, first_sample_at: at(24 * 10), coverage: withData < 2 ? { kind: "not_enough" } : withData === days ? { kind: "full" } : { kind: "partial", days_with_data: withData } },
    indexes, history_kept: { id: "stats.history_kept", params: { days: 90 }, fallback: "History kept 90 days on this device" },
    not_enough: { id: "stats.not_enough", params: {}, fallback: "Not enough history" }, removed_account: demoRemoved, events, recording: demoSettings.historyRecording };
};
const demoTimeline = async ({ range, generation }) => {
  if (generation !== demoSettings.generation) throw STALE;
  const now = Date.now(), span = range === "days7" ? 7 * DAY_MS : DAY_MS;
  const all = demoView().reports.filter((r) => r.binding && r.status === "ready").map((r) => {
    const at = r.binding.blocked_until ?? r.binding.resets_at;
    const share = Math.min(1, Math.max(0, 1 - r.binding.used_ratio));
    return { account_index: r.index, name: r.account, provider: r.provider, label: r.binding.label ?? "", returns_at: at ?? null, remaining_share: share,
      level: share >= 0.7 ? "good" : share >= 0.3 ? "warn" : "bad", exhausted: r.binding.reason === "exhausted", stale: r.freshness === "stale" };
  });
  const by = (a, b) => new Date(a.returns_at) - new Date(b.returns_at);
  const known = all.filter((l) => l.returns_at);
  return { range, now: new Date(now).toISOString(), ends_at: new Date(now + span).toISOString(),
    lanes: known.filter((l) => new Date(l.returns_at) - now <= span).sort(by), later: known.filter((l) => new Date(l.returns_at) - now > span).sort(by), unknown: all.filter((l) => !l.returns_at) };
};
// ponytail: the browser demo has no logs; two fixed sources play the core's disclosure, the consent by digest, turning
// off (erasing what was derived), the chosen directories, the period, the retention, the background scan's last
// summary and the RF4 summary (synthetic days and models; one model without a price). `?cost-paused` plays a computer
// in energy-saving mode and `?cost-noaccess` a Claude Code directory the system refuses. Core texts keep their
// English fallback here (the core's source language).
const DEMO_COST = {
  claude_code: { label: "Claude Code", pattern: "**/*.jsonl", dirs: ["/Users/demo/.config/claude/projects", "/Users/demo/.claude/projects"], files: 12,
    providers: ["claude"], models: { "claude-sonnet-4-5-20250929": { entry: "claude-sonnet-4-5", input: 3, output: 15, cache_read: 0.3, cache_write: 3.75 }, "claude-haiku-5-5": null } },
  codex_cli: { label: "Codex CLI", pattern: "**/rollout-*.jsonl", dirs: ["/Users/demo/.codex/sessions", "/Users/demo/.codex/archived_sessions"], files: 3,
    providers: ["chat_gpt"], models: { "gpt-5-codex": { entry: "gpt-5-codex", input: 1.25, output: 10, cache_read: 0.125, cache_write: 1.25 } } },
};
const DEMO_PERIODS = { today: "Today", last_7_days: "Last 7 days", last_30_days: "Last 30 days", current_month: "Current month", all: "Everything available" };
const demoCost = { on: {}, paths: {}, period: "last_7_days", retention: 120, erased: new Set() };
// `?cost` starts the demo with every local-cost source on, so the cards and the tray show their cost rows (fa gap 3).
if (new URLSearchParams(location.search).has("cost")) for (const id of Object.keys(DEMO_COST)) demoCost.on[id] = "2026-10-01T09:00:00Z";
const demoDirs = (id) => demoCost.paths[id]?.map((p) => p.replace(/^~(?=\/|$)/, "/Users/demo")) ?? DEMO_COST[id].dirs;
const demoDisclosure = (id) => {
  const { label, pattern } = DEMO_COST[id];
  const dirs = demoDirs(id);
  return { source: id, label, directories: dirs, custom: !!demoCost.paths[id], pattern, digest: `demo-${id}-${dirs.join("|")}`,
    purpose: demoL10n("cost.disclosure.purpose", { source: label }, `To estimate, on this computer only, the tokens and the cost at API prices of your ${label} usage per day and per model.`),
    extracted: demoL10n("cost.disclosure.extracted", {}, "Only numbers are extracted: the time, the model, the token counts (input, output, cache read and cache write) and an anonymous digest of the session."),
    never: demoL10n("cost.disclosure.never", {}, "Conversation content, prompts, answers, file names and project paths are never read into the index, stored or shown. Nothing leaves this computer."),
    off: demoL10n("cost.disclosure.off", {}, "Desligar a fonte interrompe qualquer leitura e apaga tudo o que foi derivado dela.") };
};
const demoCostView = () => {
  const sources = Object.entries(DEMO_COST).map(([id, s]) => {
    const enabled = id in demoCost.on;
    const files = enabled && !demoCost.erased.has(id) ? s.files : 0;
    return { source: id, label: s.label, enabled, consented_at: demoCost.on[id] ?? null, paths: demoCost.paths[id] ?? [], disclosure: demoDisclosure(id),
      indexed: { files, invalid_lines: files ? 2 : 0 },
      summary: enabled ? demoL10n("cost.scan.ready", { source: s.label, files: String(s.files), requests: String(files ? 0 : 41), invalid: "2" },
        `${s.label}: ${s.files} log(s); ${files ? 0 : 41} new request(s); 2 invalid line(s) ignored.`) : null };
  });
  const on = sources.some((s) => s.enabled);
  return { sources, period: demoCost.period, periods: Object.entries(DEMO_PERIODS).map(([id, text]) => ({ id, label: demoL10n(`cost.period.${id}`, {}, text) })),
    retention_days: demoCost.retention, retention_min: 7, retention_max: 730,
    state: !on ? "off" : new URLSearchParams(location.search).has("cost-paused") ? "paused" : "waiting", error: null,
    size: on ? 86016 : 0 };
};
const DEMO_EFFECTIVE = "2026-10-21";
const demoNotice = () => demoL10n("cost.estimate.notice", {}, "Estimate at API prices, not a charge; amounts in USD.");
const tokenSum = (t) => t.input + t.output + t.cache_read + t.cache_write + (t.cache_write_1h ?? 0);
const addTokens = (a, b) => ({ input: a.input + b.input, output: a.output + b.output, cache_read: a.cache_read + b.cache_read, cache_write: a.cache_write + b.cache_write, cache_write_1h: (a.cache_write_1h ?? 0) + (b.cache_write_1h ?? 0) });
const ZERO_TOKENS = { input: 0, output: 0, cache_read: 0, cache_write: 0, cache_write_1h: 0 };
/** The demo price of `model` (model names are unique across the demo sources); `null`: cost unknown. */
const demoPrice = (model) => Object.values(DEMO_COST).map((s) => s.models[model]).find((p) => p !== undefined) ?? null;
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
    return { model, tokens: mt, requests: mr, cost, price: p ? { entry: p.entry, origin: "embedded", rates: { input: p.input, output: p.output, cache_read: p.cache_read, cache_write: p.cache_write } } : null };
  });
  return { currency: "USD", total: priced || !models.length ? total : null, tokens, requests, unknown_tokens: unknown,
    unknown_share: tokenSum(tokens) ? tokenSum(unknown) / tokenSum(tokens) : 0, models,
    tables: priced ? [{ origin: "embedded", effective: DEMO_EFFECTIVE, source: "https://claude.com/pricing#api" }] : [], notice: demoNotice() };
};
const dayKey = (d) => `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, "0")}-${String(d.getDate()).padStart(2, "0")}`;
/** Synthetic usage of `id` on the day `back` days ago (some days without any). */
const demoDayRows = (id, back) => Object.keys(DEMO_COST[id].models).flatMap((model, i) => {
  const k = (back * 7 + i * 5 + id.length) % 9;
  if (k === 0 || (i > 0 && back % 3)) return [];
  return [{ model, requests: 4 * k, tokens: { input: 3_000 * k, output: 1_500 * k, cache_read: 45_000 * k, cache_write: 2_500 * k, cache_write_1h: 500 * k } }];
});
/** The core's `CostSummary` of the demo sources over the saved period. */
const demoCostUsage = () => {
  const now = new Date();
  const span = { today: 0, last_7_days: 6, last_30_days: 29, current_month: now.getDate() - 1, all: 44 }[demoCost.period];
  const at = (back) => { const d = new Date(now); d.setDate(d.getDate() - back); return dayKey(d); };
  const noAccess = new URLSearchParams(location.search).has("cost-noaccess");
  const sources = Object.entries(DEMO_COST).map(([id, s]) => {
    const base = { source: id, label: s.label, providers: s.providers };
    if (!(id in demoCost.on)) return { ...base, state: "off", note: demoL10n("cost.scan.off", { source: s.label }, `${s.label}: off; nothing is read.`), usage: null };
    const kept = !demoCost.erased.has(id);
    const rowsOf = (back) => (kept ? demoDayRows(id, back) : []);
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
    return { ...base, state, note, usage: { today: demoEstimate(rowsOf(0)), yesterday: demoEstimate(rowsOf(1)), period, days: period.models.length ? days : [] } };
  });
  const label = demoL10n(`cost.period.${demoCost.period}`, {}, DEMO_PERIODS[demoCost.period]);
  const all = Object.keys(DEMO_COST).filter((id) => id in demoCost.on && !demoCost.erased.has(id))
    .flatMap((id) => Array.from({ length: span + 1 }, (_, back) => demoDayRows(id, back)).flat());
  return { currency: "USD", notice: demoNotice(), span: { period: demoCost.period, days: null, label, first_day: demoCost.period === "all" ? null : at(span), last_day: at(0) }, sources,
    total: demoEstimate(all), pricing_error: null };
};
// PRD 0030 RF1/RF6 demo: plan prices by account key and one typed rate, kept for the page's life like the core's
// settings. The value line mirrors `cost::plan_values` for a plan in one currency (a ratio when it is USD or the
// display currency); the amounts are the demo period totals.
const demoPlan = { plans: {}, display: null };
const demoPlansView = () => {
  const today = new Date();
  const accounts = demoReports.flatMap((r) => {
    const [source, s] = Object.entries(DEMO_COST).find(([, c]) => c.providers.includes(r.provider)) ?? [];
    return source ? [{ id: demoKey(r), name: r.account, provider: r.provider, source, label: s.label, plan: demoPlan.plans[demoKey(r)] ?? null }] : [];
  });
  const usage = demoCostUsage();
  const values = Object.keys(DEMO_COST).flatMap((source) => {
    const planned = accounts.filter((a) => a.source === source && a.plan);
    const total = usage.sources.find((s) => s.source === source)?.usage?.period.total;
    if (!planned.length || total === undefined) return [];
    const label = DEMO_COST[source].label;
    const currencies = [...new Set(planned.map((a) => a.plan.currency))];
    const price = planned.reduce((sum, a) => sum + a.plan.price, 0);
    const plans = currencies.length === 1 ? `${currencies[0]} ${price.toFixed(2)}` : planned.map((a) => `${a.plan.currency} ${a.plan.price.toFixed(2)}`).join(" · ");
    const rate = currencies.length === 1 && currencies[0] === "USD" ? 1 : currencies.length === 1 && demoPlan.display?.code === currencies[0] ? demoPlan.display.rate : null;
    const params = { source: planned.length > 1 ? demoL10n("cost.plan.combined", { source: label }, `Contas do ${label}`) : label, estimate: total == null ? "—" : fmtMoney(total), plans };
    const text = total == null ? demoL10n("cost.plan.unknown", params, `${label}: cost at API prices unknown (models without a price) · plan ${plans}`)
      : rate ? demoL10n(rate === 1 ? "cost.plan.ratio" : "cost.plan.ratio_rate", { ...params, ratio: ((total * rate) / price).toFixed(1) }, `${label}: ≈ ${params.estimate} at API prices (estimate) · plan ${plans}`)
        : demoL10n("cost.plan.side", params, `${label}: ≈ ${params.estimate} at API prices (estimate) · plan ${plans}`);
    return [{ source, label, accounts: planned.map((a) => a.name), cycle_start: dayKey(new Date(today.getFullYear(), today.getMonth(), 1)), estimate: total, partial: false,
      plans: currencies.map((currency) => ({ currency, price: planned.filter((a) => a.plan.currency === currency).reduce((n, a) => n + a.plan.price, 0) })), ratio: rate && total != null ? (total * rate) / price : null, at_your_rate: rate != null && rate !== 1, text }];
  });
  const d = demoPlan.display;
  return { accounts, values, display: d && { code: d.code, rate: d.rate, set_on: d.set_on, age_days: Math.floor((today - new Date(d.set_on)) / 864e5), stale: Math.floor((today - new Date(d.set_on)) / 864e5) >= 30 } };
};
const fmtMoney = (amount) => `$${amount.toFixed(2)}`;
const demoPrices = () => {
  const models = Object.values(DEMO_COST).flatMap((s) => Object.values(s.models).filter(Boolean))
    .map((p) => ({ model: p.entry, origin: "embedded", rates: { input: p.input, output: p.output, cache_read: p.cache_read, cache_write: p.cache_write } }))
    .sort((a, b) => a.model.localeCompare(b.model, "en")); // model ids are identifiers, never translated
  return { currency: "USD", notice: demoNotice(), tables: [{ origin: "embedded", effective: DEMO_EFFECTIVE, source: "https://claude.com/pricing#api; https://openai.com/api/pricing/" }],
    pricing_file: { path: "/Users/demo/Library/Application Support/cuotai/cost/pricing.json", used: false, error: null }, models };
};
// PRD 0018 RF2.6/RF3.1/RF2.5 browser demo: the same `UpdateView` fields.
// `?update=consent` shows the question without a request; `off`, `managed` and
// `error` exercise the other states.
const DEMO_UPDATE = new URLSearchParams(location.search).get("update");
const DEMO_NOTES = "Measurement fixes and a new automatic update option (PRD 0018).";
const demoUpdate = {
  automatic: !["off", "consent"].includes(DEMO_UPDATE),
  consentAnswered: DEMO_UPDATE !== "consent",
  lastCheck: null,
  phase: DEMO_UPDATE === "error" ? "failed" : DEMO_UPDATE === "fresh" || DEMO_UPDATE === "off" || DEMO_UPDATE === "consent" ? "uptodate" : "available",
  version: "0.72.0", notes: DEMO_NOTES, date: "2026-10-22T00:00:00Z",
  canInstall: DEMO_UPDATE !== "managed", upgradeCommand: DEMO_UPDATE === "managed" ? "brew upgrade --cask cuotai" : null,
  progress: null, dismissed: false, error: DEMO_UPDATE === "error" ? "network" : null,
  disabledReason: DEMO_UPDATE === "managed" ? "homebrew" : DEMO_UPDATE === "off" ? "preference" : null,
};
const demoWait = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
const demoUpdateView = () => {
  const enabled = demoUpdate.consentAnswered && !demoUpdate.disabledReason;
  return {
    enabled, automatic: demoUpdate.automatic, disabled_reason: demoUpdate.disabledReason,
    channel: "stable", current_version: "0.71.1", build: null, last_check: demoUpdate.lastCheck,
    consent_pending: !demoUpdate.consentAnswered && !demoUpdate.disabledReason,
    phase: demoUpdate.phase, version: demoUpdate.phase === "idle" || demoUpdate.phase === "failed" ? null : demoUpdate.version,
    notes: demoUpdate.phase === "available" ? DEMO_NOTES : null, date: demoUpdate.phase === "available" ? demoUpdate.date : null,
    can_install: demoUpdate.canInstall, upgrade_command: demoUpdate.upgradeCommand, progress: demoUpdate.progress,
    dismissed: demoUpdate.dismissed, error: demoUpdate.error,
    manual_url: "https://github.com/jmarcon/cuotai/releases/latest",
  };
};
// Mockup 40: one approved and one not yet approved extension (`cuotai.demoExtras=1` adds the
// secrets and override controls); example.com identities only.
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
    if (r?.last_good) demoReports[index] = { ...r, status: "ready", pending: false, checked_at: ago(0), metrics: r.last_good.metrics, notes: [], last_good: null };
  },
  // Same key as the core's `picker_providers` (lowercased label), pickable first.
  list_providers: async () => Object.entries(brands).map(([id, b]) => ({ id, label: b.label, pickable: !DEMO_NO_QUERY.includes(id) }))
    .sort((a, b) => (b.pickable - a.pickable) || (a.label.toLowerCase() < b.label.toLowerCase() ? -1 : a.label.toLowerCase() > b.label.toLowerCase() ? 1 : 0)),
  // ponytail: the browser demo has no core; a name/slug filter with one sign-in method stands in for the search.
  search_providers: async ({ query }) => {
    const fold = (v) => String(v ?? "").normalize("NFD").replace(/[\u0300-\u036f]/g, "").toLowerCase();
    const q = fold(query).trim();
    return Object.entries(brands).filter(([id]) => !DEMO_NO_QUERY.includes(id) && id !== "extension")
      .filter(([id, b]) => !q || fold(b.label).includes(q) || id.includes(q))
      .map(([id, b]) => demoPickerEntry(id, b))
      .sort((x, y) => DEMO_METHOD_ORDER.indexOf(x.method) - DEMO_METHOD_ORDER.indexOf(y.method));
  },
  provider_modes: async ({ provider }) => [demoPickerEntry(provider, brand(provider))],
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
  extension_disclosure: async () => ({ lines: [], candidate: { token: "demo" } }),
  enable_extension: async ({ candidate }) => ({ id: candidate?.id ?? "demo", message: dl("extensions.action.enabled", "Extension enabled."), account: null }),
  disable_extension: async ({ id }) => ({ id, message: dl("extensions.action.disabled", `Extension ${id} disabled.`), account: null }),
  test_extension: async ({ id }) => ({ source: `extension:${id}`, status: "ready", metrics: [], notes: [], violations: [] }),
  // ponytail: demo mirror of `access` for the browser (default modes only, generic pt summary); the app asks the core.
  draft_access: async ({ provider }) => {
    const level = demoLevelOf(provider);
    const reason = level === "unavailable" ? dlx(`access.reason.${provider}`, "demo.access.reason", "No public usage query for this credential (demo).") : null;
    return {
      level: demoLevel(level),
      summary: dlx(`access.summary.${provider}`, "demo.access.summary", "Demo usage query."),
      notes: [dlx(`access.credential_hint.${level === "official" ? "admin_key" : "inference_key"}`, "demo.access.credential_hint", "Credencial da consulta (demo)."), ...(reason ? [reason] : [])],
      cost_advisory: provider === "aws_bedrock" ? dlx("access.cost_advisory", "demo.access.cost_advisory", "Provedor cobrado por consulta (demo).") : null,
      save_confirmation: reason && { id: "access.confirm_unavailable", params: { reason }, fallback: `${reason.fallback} The account will be saved, but it will show this reason instead of numbers.` },
    };
  },
  // PRD 0047: the desktop types the key (`secret_inputs`); the variable-name fields are terminal-only.
  form_schema: async ({ provider }) => provider === "kiro" ? demoKiroSchema() : ({ credential_kind: "env",
    secret_inputs: [DEMO_KEY_INPUT, ...(provider === "xiaomi_mimo" ? [{ slot: DEMO_SESSION_SLOT.slot, label: DEMO_SESSION_SLOT.label,
      help: DEMO_SESSION_SLOT.instructions, required: false, cookie: true, field: null }] : [])],
    terminal_only_fields: [],
    login: { modes: [{ id: "oauth", label: { id: "demo.login.oauth.label", params: {}, fallback: "OAuth sign-in (demo)" }, help: { id: "demo.login.oauth.help", params: {}, fallback: "The browser demo does not run the sign-in." }, completion_notice: { id: "demo.login.oauth.completion", params: {}, fallback: "Sign-in authorized (demo)." }, credential_store: "cuotai_vault", account_binding: { type: "none" }, registration: [], options: [], unavailable_reason: { id: "demo.login.oauth.unavailable", params: {}, fallback: "Sign-in is not available in the demo." } }], unsupported_reason: null },
    sessions: provider === "xiaomi_mimo" ? [DEMO_SESSION_SLOT] : [],
    session_notice: provider === "xiaomi_mimo" ? demoSessionNotice("vault") : null,
    fields: [{ id: "demo.field.project_id", key: "project_id", label: { id: "demo.field.project_id.label", params: {}, fallback: "Project ID" }, help: { id: "demo.field.project_id.help", params: {}, fallback: "Opcional." }, required: false, options: null },
             { id: "demo.field.mode", key: "mode", label: { id: "demo.field.mode.label", params: {}, fallback: "Mode" }, help: { id: "demo.field.mode.help", params: {}, fallback: "" }, required: false, options: [{ id: "demo.field.mode.option.personal", index: 0, value: "personal", label: { id: "demo.field.mode.option.personal", params: {}, fallback: "Personal" } }, { id: "demo.field.mode.option.org", index: 1, value: "org", label: { id: "demo.field.mode.option.org", params: {}, fallback: "Organization" } }] }] }),
  visible_fields: async ({ provider, authMode }) => provider === "kiro"
    ? ["auth_method", "profile_arn"].filter((k) => !(authMode === "oauth" && k === "auth_method"))
    : ["project_id", "mode"],
  field_choices: async ({ provider, choices, authMode }) => {
    const schema = provider === "kiro" ? demoKiroSchema() : await demo.form_schema({ provider });
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
      interval: "", group: "", account_id: demoAccountId(index), monogram: demoSettings.monograms[demoAccountId(index)] ?? "", uses_oauth: false, native_login: false, auth_mode: "credential", auth_options: {}, auth_registration: {}, fields: [{ key: "project_id", text: "", choice: null }, { key: "mode", text: null, choice: 0 }],
      session_slots: state === "vault" ? ["platform_session_env"] : [],
      credential_stored: true, secret_states: states,
      vault_migration: sessionOffer ? [{ slot: "platform_session_env", label,
        consent: { id: "secret_input.migrate.consent", params: { label },
          fallback: `${label.fallback}: Cuotai copies the value it reads today from the terminal setup into the system vault. From then on this account works without that setup; the configuration file keeps just a reference.` } }] : [] };
  },
  migrate_session: async ({ index }) => {
    demoSettings.sessionMigrated.add(demoReports[index]);
    return demo.get_account({ index });
  },
  vault_audit: async () => [
    { key: "00000000-0000-4000-8000-000000000001", created_at: "2026-10-01T12:00:00Z", kind: "oauth", provider: "claude", account_id: "acc_demo00000001", account: "Claude", orphan: false, present: true },
    ...(demoSettings.vaultPruned ? [] : [{ key: "00000000-0000-4000-8000-000000000002", created_at: "2026-09-01T12:00:00Z", kind: null, provider: null, account_id: null, account: null, orphan: true, present: true }]),
  ],
  vault_prune: async () => { const n = demoSettings.vaultPruned ? 0 : 1; demoSettings.vaultPruned = true; return n; },
  forget_session: async ({ index }) => {
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
  open_aggregator_dashboard: async () => {},
  // PRD 0036 RF2.2: the demo instance answers like a local 9router with three connections.
  test_aggregator: async ({ input }) => {
    const base = (input.fields ?? []).find((f) => f.key === "base_url")?.text ?? "";
    if (base && !/^https?:\/\//.test(base)) {
      return { ok: false, message: { id: "test.nine_router.unreachable", params: {}, fallback: "The address could not be reached." } };
    }
    return { ok: true, message: { id: "test.nine_router.connected", params: { connections: 3, version: "0.5.109" }, fallback: "9router 0.5.109: 3 connections with usage found." } };
  },
  test_session: async ({ input }) => {
    await demo.preview_session({ paste: input.sessions?.[0]?.paste ?? "" });
    return { ok: true, status: { id: "status.ready", params: {}, fallback: "OK" }, note: null, metrics: 2, expiry: null };
  },
  save_account: async ({ input }) => {
    window.__lastSaveRequest = input;
    // PRD 0029 RF3: the core rule — a name is needed only beside another account of the provider.
    const others = demoReports.filter((x, i) => i !== input.index && x.provider === input.provider);
    if (!input.name.trim() && others.length) throw JSON.stringify({ id: "edit_error.name_required_same_provider", params: { provider: { id: `provider.${input.provider}`, params: {}, fallback: input.provider } }, fallback: "Name this account: you have another account of the provider." });
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
  model_state: demoModels,
  load_models: demoModels,
  sync_models: demoModels,
  model_support: async () => ({ supported: false, text: { id: "models.demo", params: {}, fallback: "Browser demo: the model catalog is only queried in the desktop app." } }),
  config_changed_on_disk: async () => false,
  get_settings: async () => ({ user_dir: demoSettings.custom ?? "~/.config/cuotai", default_dir: "~/.config/cuotai", custom_dir: demoSettings.custom, env_locked: false, config_file: demoSettings.config, config_env_locked: false, motion: get("cuotai.motion", "system"), show_pace: get(PACE_KEY, "on") !== "off",
    theme: get("cuotai.theme", "") || null, theme_mode: get("cuotai.themeMode", "system"), sort: get(SORT_KEY, "custom"), aggregators_in_all: get("cuotai.aggregators-in-all", "grouped"), group_recharging: get(GROUP_KEY, "off") === "on", show_in_dock: demoSettings.showInDock !== false, locale: get("cuotai.locale", "") || null, local_storage_migrated: true,
    history_days: demoSettings.historyDays ?? 90, history_days_min: 7, history_days_max: 365, history_recording: demoSettings.historyRecording, status_queries: demoSettings.statusQueries,
    notify: { ...demoSettings.notify }, notify_leads: [0, 5, 15, 60], notify_whens: ["used", "exhausted", "always"],
    alerts: structuredClone(demoSettings.alerts), quiet: structuredClone(demoSettings.quiet), alert_min: 10, alert_max: 99, detect: structuredClone(demoSettings.detect), 
    tray: { ...demoSettings.tray }, advanced_open: !!demoSettings.advancedOpen,
    profile_existed: !!demoSettings.profileExisted, whats_new_auto: demoSettings.whatsNewAuto !== false, whats_new_due: demoSettings.whatsNewDue === true, monograms: { ...demoSettings.monograms }, rows: structuredClone(demoSettings.rows ?? {}), onboarding: demoSettings.onboarding, onboarding_version: 1, version: "demo" }),
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
  set_whats_new_auto: async ({ on }) => { demoSettings.whatsNewAuto = !!on; },
  whats_new_seen: async () => { demoSettings.whatsNewDue = false; },
  set_account_monogram: async ({ accountId, monogram }) => {
    if (!validMonogram(monogram)) throw "invalid_monogram";
    const text = String(monogram).trim();
    if (text) demoSettings.monograms[accountId] = text; else delete demoSettings.monograms[accountId];
    await emitSettingsChanged("monograms");
  },
  set_rows: async ({ rows }) => { demoSettings.rows = structuredClone(rows); },
  autostart_status: async () => !!demoSettings.autostart,
  autostart_set: async ({ enabled }) => { demoSettings.autostart = !!enabled; return demoSettings.autostart; },
  first_run_turn_on: async () => { demoSettings.alerts.enabled = true; demoSettings.alerts.overage = true; demoSettings.alerts.projected_lead_minutes = 60; demoSettings.onboarding = 1; return "granted"; },
  first_run_record: async ({ again }) => { demoSettings.onboarding = again ? 0 : 1; },
  set_advice_enabled: async () => {},
  set_pace_unit: async ({ unit }) => { if (!["points", "percent"].includes(unit)) throw "invalid_pace_unit"; },
  set_history_days: async ({ days }) => { if (!(Number.isInteger(days) && days >= 7 && days <= 365)) throw "invalid_history_days"; demoSettings.historyDays = days; },
  // ponytail: the browser demo has no history file; fixed counts let the export and erase flows be driven, and an
  // erased account (or everything) leaves the synthetic history of `account_history`.
  history_export: async ({ index, generation }) => { if (index != null && generation !== demoSettings.generation) throw STALE; return index == null ? 42 : 7; },
  history_erase: async ({ index, generation }) => { if (index != null && generation !== demoSettings.generation) throw STALE; demoSettings.historyErased.add(index ?? "all"); demoSettings.historyEpoch++; return index == null ? 42 : 7; },
  set_history_recording: async ({ on }) => { demoSettings.historyRecording = on; demoSettings.historyEpoch++; },
  set_status_enabled: async ({ on }) => { demoSettings.statusQueries = on; },
  serve_status: async () => demoServeView(),
  set_serve: async ({ enabled, port, allowBrowsers }) => {
    if (!Number.isInteger(port) || port < 1024 || port > 65535) throw "invalid_port";
    Object.assign(demoSettings.serve, { enabled, port, allow_browsers: allowBrowsers });
    return demoServeView();
  },
  set_serve_stats: async ({ stats }) => { demoSettings.serve.stats = stats; return demoServeView(); },
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
  revoke_serve_token: async () => { demoSettings.serve.secret = null; return demoServeView(); },
  copy_serve_token: async () => {
    if (demoSettings.serve.secret == null) throw JSON.stringify({ id: "serve.error.token_missing", params: {}, fallback: "The local server token is no longer in the system vault; generate a new one or revoke the token." });
    return demoSettings.serve.secret;
  },
  cost_status: async () => demoCostView(),
  cost_usage: async () => demoCostUsage(),
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
  cost_enable: async ({ source, digest }) => {
    if (!DEMO_COST[source] || digest !== demoDisclosure(source).digest) {
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
    demoPlan.display = { code: code.toUpperCase(), rate, set_on: dayKey(new Date()) };
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
    if (view.phase !== "available") throw "no_update";
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
  set_hooks_enabled: async ({ on }) => { demoHooks.enabled = on; return demoHooksView(); },
  confirm_hooks: async ({ digest }) => {
    if (!demoHooks.rules || demoDigest(demoHooks.rules) !== digest) throw JSON.stringify(demoL10n("hooks.error.changed", {}, "O hooks.toml mudou desde que foi exibido; revise as regras de novo."));
    demoHooks.confirmed = digest;
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
  set_notify_options: async ({ leadMinutes, when, includeOther }) => {
    if (![0, 5, 15, 60].includes(leadMinutes) || !["used", "exhausted", "always"].includes(when)) throw "invalid_notify";
    Object.assign(demoSettings.notify, { lead_minutes: leadMinutes, when, include_other: includeOther });
  },
  set_notify_private: async ({ on }) => { demoSettings.notify.private = on; },
  set_quiet_hours: async ({ quiet }) => {
    const days = quiet?.days ?? {};
    const time = /^([01]\d|2[0-3]):[0-5]\d$/;
    const valid = typeof quiet?.enabled === "boolean" && typeof quiet?.digest === "boolean"
      && Object.entries(days).every(([k, list]) => ["mon", "tue", "wed", "thu", "fri", "sat", "sun"].includes(k) && Array.isArray(list) && list.length <= 3
        && list.every((i) => time.test(i?.from) && time.test(i?.to)));
    if (!valid) throw "invalid_quiet";
    demoSettings.quiet = { enabled: quiet.enabled, digest: quiet.digest, days: structuredClone(days) };
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
  history_size: async () => (demoSettings.historyErased.has("all") ? 4096 : 1887437),
  account_history: demoHistory,
  stats: demoStats,
  reset_timeline: demoTimeline,
  set_config_file: async ({ path }) => { demoSettings.config = path ?? null; demoSettings.generation++; return demoView(); },
  open_user_dir: async () => {},
  // Demo: history sample (in the app, the core delivers the embedded changelog).
  whats_new: async () => ({
    current: "demo",
    title: { id: "changelog.title", params: {}, fallback: "What's new" },
    entries_language: { id: "changelog.entries_language", params: {}, fallback: "The entries stay in English, the language of the changelog." },
    releases: [
      { version: "demo", date: "2026-10-25", current: true, paragraphs: [], categories: [
        { label: { id: "changelog.category.added", params: {}, fallback: "Added" }, entries: ["Sample entry for the What's new screen in the browser demo."] },
      ] },
    ],
    history: null,
  }),
  // Demo: sample of the real list (in the app, the full list comes from the core).
  // Demo: the report text is the core's; here, an example with no data at all.
  report_problem: async ({ index }) => ({
    title: "[Provider] claude: schema_changed",
    body: `## Provider problem report\n\n- Cuotai version: demo\n- Provider: \`claude\`${index == null ? "" : "\n- Failure category: \`schema_changed\`"}\n`,
    url: "https://github.com/jmarcon/cuotai/issues/new?template=provider-problem.yml",
    copy_only: false,
  }),
  about_info: async () => ({
    name: "Cuotai", version: "demo", license: "MIT", repository: "https://github.com/jmarcon/cuotai",
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
  const show = action ? `<button type="button" class="btn sm ghost" data-show-account="${r.advice.best_index}">${te("advice.show", { name: esc(r.advice.best_name) })}</button>` : "";
  return `<p class="call" role="note">${icon("info")}<span>${esc(tl(r.advice.text))}</span>${show}</p>`;
}
export const setPaceUnit = async (unit) => { await call("set_pace_unit", { unit }); applyPaceUnit(unit); await emitSettingsChanged("pace_unit"); };
export const setShowPace = async (on) => { await call("set_show_pace", { show: on }); applyPace(on); };
/** The core's pace of `m` (never estimated here); null when the core has none or the user hid pace. */
export const paceOf = (m) => (showPace ? m.pace ?? null : null);
/** PRD 0033 RF4.1: the name of `m`'s window, for the pace line ("Week", "5h"); windows the UI does not name use the metric label. */
export const paceLabelOf = (m) => (m.window && m.window !== "other" ? t(`card.binding.${m.window}`) : m.label);
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
  const stage = p.chips ? tl(p.chips[paceUnitNow].text) : tl(p.label);
  const parts = short ? [p.outlook ? tl(p.outlook) : stage]
    : [stage, expected ? t("metric.pace.expected", { pct: Math.round(p.expected_used_fraction * 100) }) : "", p.outlook ? tl(p.outlook) : ""];
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
export const planBadgeHtml = (r) => (r?.plan ? `<span class="plan-badge" data-plan title="${esc(t("agg.plan"))}: ${esc(r.plan)}">${esc(r.plan)}</span>` : "");
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
    if (["sort", "group_recharging", "show_pace", "pace_unit", "advice_enabled"].some((k) => changed.has(k))) onChange();
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
/** The account's 5h and weekly windows (classification comes from the core: `metric.window`). In each window the tightest
 *  quota among the non-hidden ones wins; null when the account has no such window. */
export function windowsOf(r) {
  const when = (m) => (m.resets_at ? new Date(m.resets_at).getTime() : Infinity);
  const pick = (w) => planMetrics(r).filter((m) => m.window === w).map((m) => ({ m, pct: remainingPct(m) })).filter((x) => x.pct != null)
    .sort((a, b) => a.pct - b.pct || when(a.m) - when(b.m))[0] ?? null;
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
  const items = SORT_MODES.map((m) => `<button role="menuitemradio" aria-checked="${m === sortMode}" class="${m === sortMode ? "on" : ""}" data-sort="${m}"><span>${label(m)}</span>${m === sortMode ? icon("tick", "sel-tick") : ""}</button>`).join("");
  return `<div class="sortmenu lang">
    <button class="btn outline sort-btn" data-sort-toggle title="${te("sort.label")}: ${esc(label(sortMode))}" aria-label="${te("sort.label")}: ${esc(label(sortMode))}" aria-haspopup="menu" aria-expanded="${sortOpen}">${icon("sort")}${icon("chevDown")}</button>
    <div class="lang-menu" role="menu" ${sortOpen ? "" : "hidden"}>${items}</div>
  </div>`;
}

// ---------- notice categories (cards and popover): dedicated icon and own color, from most to least severe ----------
// `label(r)`: with the account, the category's text for it; without, the category's title.
export const ALERTS = [
  { id: "error", icon: "xcircle", tone: "bad", label: () => t("status.error"), match: (r) => r.status === "error" },
  { id: "auth", icon: "key", tone: "accent", label: () => t("status.authentication_error"), match: (r) => r.status === "authentication_error" },
  { id: "exhausted", icon: "battery", tone: "bad", label: () => t("tray.cat.exhausted"), match: (r) => r.status === "ready" && r.class === "exhausted" },
  // PRD 0008 RF3.4: an active usage alert (the exhausted state above keeps precedence).
  { id: "usage", icon: "bell", tone: "warn", label: (r) => (r ? t("alerts.active", { metrics: r.usage_alerts.join(", ") }) : t("alerts.activeTitle")),
    match: (r) => r.status === "ready" && r.class !== "exhausted" && r.usage_alerts?.length > 0 },
  { id: "rate", icon: "gauge", tone: "warn", label: () => t("status.rate_limited"), match: (r) => r.status === "rate_limited" },
  { id: "config", icon: "wrench", tone: "info", label: () => t("status.needs_configuration"), match: (r) => r.status === "needs_configuration" },
  { id: "unsupported", icon: "ban", tone: "muted", label: () => t("status.unsupported"), match: (r) => r.status === "unsupported" },
];
export const alertOf = (r) => ALERTS.find((c) => c.match(r)) ?? null;

// Demo Kiro: one login mode (oauth) and the credential fields, as in the core.
const dl = (id, fallback) => ({ id, params: {}, fallback });
// The core's own id when the catalogs know it, else a demo id (so no language shows the Portuguese fallback).
const dlx = (id, demoId, fallback) => dl(t(id) !== id ? id : demoId, fallback);
// Default-mode level per provider, as `access::describe` classifies them (demo only).
const DEMO_LEVELS = {
  derived: ["gemini_api", "vertex_ai", "groq"],
  unavailable: ["qwen", "perplexity", "cloudflare_ai", "gemini_ai", "nvidia_nim", "poolside", "alibaba_model_studio"],
  official: ["open_ai_api", "anthropic_api", "aws_bedrock", "azure_open_ai", "cursor", "copilot", "windsurf", "kimi", "mistral", "grok", "cline", "alibaba_cloud", "open_router"],
};
// Providers without a usage query in any mode (`access::has_usage_query`, demo only): left out of the picker.
// The core groups aggregators under "Self-hosted gateway" (`catalog::SignInMethod::SelfHostedGateway`).
const DEMO_GATEWAYS = ["nine_router", "litellm"];
// Providers the core classes as browser sign-in (a native login exists), shown first in the demo picker.
const DEMO_BROWSER = ["claude", "chat_gpt", "cursor", "copilot", "antigravity", "windsurf", "kimi", "grok", "kiro", "zed"];
const demoPickerEntry = (id, b) => ({ provider: id, label: b.label, mode: null, ...(DEMO_GATEWAYS.includes(id)
    ? { method: "self_hosted_gateway", method_label: { id: "catalog.method.self_hosted_gateway", params: {}, fallback: "Self-hosted gateway" } }
    : DEMO_BROWSER.includes(id)
      ? { method: "browser_sign_in", method_label: { id: "catalog.method.browser_sign_in", params: {}, fallback: "Sign in with the browser" } }
      : { method: "api_key", method_label: { id: "catalog.method.api_key", params: {}, fallback: "API key" } }),
  hint: DEMO_BROWSER.includes(id) ? { id: "catalog.hint.browser_sign_in", params: {}, fallback: "Sign in with the browser" }
    : { id: "catalog.hint.api_key", params: {}, fallback: "API key" }, metered: id === "aws_bedrock", recommended: true });
const DEMO_METHOD_ORDER = ["browser_sign_in", "api_key", "self_hosted_gateway"];
const DEMO_NO_QUERY = ["qwen", "cloudflare_ai", "nvidia_nim", "poolside", "alibaba_model_studio"];
const demoLevelOf = (provider) => Object.keys(DEMO_LEVELS).find((level) => DEMO_LEVELS[level].includes(provider)) ?? "undocumented";
const demoLevel = (level) => dl(`access.level.${level}`, { official: "Official", undocumented: "Undocumented", derived: "Derived", unavailable: "Unavailable" }[level]);
function demoKiroSchema() {
  const opts = (key, items) => items.map(([value, label], index) => ({ id: `${key}.${value}`, index, value, label: dl(`${key}.${value}`, label) }));
  return {
    credential_kind: "env", secret_inputs: [DEMO_KEY_INPUT], terminal_only_fields: [],
    login: { unsupported_reason: null, modes: [{ id: "oauth", label: dl("demo.kiro.login.oauth.label", "Login no navegador"), help: dl("demo.kiro.login.oauth.help", "Login nativo no navegador do sistema."),
      completion_notice: dl("demo.kiro.login.oauth.completion", "Sign-in authorized (demo)."),
      credential_store: "cuotai_vault", account_binding: { type: "none" },
      registration: [], unavailable_reason: null,
      options: [
        { key: "auth_method", label: dl("demo.kiro.o.auth_method", "Authentication method"), help: dl("demo.kiro.o.auth_method.h", "builder-id or idc."), required: false, validation: "none", choices: opts("demo.kiro.o.auth_method", [["builder-id", "AWS Builder ID"], ["idc", "IAM Identity Center"]]) },
        { key: "region", label: dl("demo.kiro.o.region", "Region (optional)"), help: dl("demo.kiro.o.region.h", ""), required: false, validation: "no_whitespace", choices: null },
        { key: "start_url", label: dl("demo.kiro.o.start_url", "IDC start URL (optional)"), help: dl("demo.kiro.o.start_url.h", ""), required: false, validation: "no_whitespace", choices: null },
      ] }] },
    fields: [
      { key: "auth_method", label: dl("demo.kiro.f.auth_method", "Authentication method"), help: dl("demo.kiro.f.auth_method.h", "Source of the access token."), required: false,
        options: [
          ...opts("demo.kiro.f.auth_method", [["builder-id", "AWS Builder ID (default)"], ["idc", "IAM Identity Center"]]).map((option) => ({ ...option, auth_modes: ["oauth"], unavailable_reason: dl("demo.kiro.f.auth_method.login_only", "Available only with the native sign-in.") })),
          ...opts("demo.kiro.f.auth_method", [["google", "Google"], ["github", "GitHub"], ["import", "Imported"], ["api_key", "API key"], ["external_idp", "External IdP"]]).map((option, offset) => ({ ...option, index: offset + 2, auth_modes: ["credential"], unavailable_reason: dl("demo.kiro.f.auth_method.env_only", "Available only with a typed access token.") })),
        ] },
      { key: "profile_arn", label: dl("demo.kiro.f.profile_arn", "ARN do perfil CodeWhisperer (opcional)"), help: dl("demo.kiro.f.profile_arn.h", ""), required: false, options: null },
    ],
  };
}
