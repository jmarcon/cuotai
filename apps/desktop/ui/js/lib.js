import { DEFAULT_THEME, GROUP_ORDER, THEMES, effectiveMode, themeById } from "./themes.js";
import { LOCALES, currentLocale, setLocale, t, te, tl } from "./i18n.js";
import { esc } from "./util.js";
import { flagCountry, flagSvg } from "./flags.js";
import { crossFade } from "./motion.js";
export { esc };
// Camada compartilhada (sem framework, sem bundler). Espelha cuotai_core::model.

// ---------- util ----------
// Ícones (paths do Lucide, ISC).
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
};
export const icon = (name, cls = "") => `<svg class="i ${cls}" viewBox="0 0 24 24" aria-hidden="true">${paths[name]}</svg>`;

// ---------- API (Tauri ou demo no navegador) ----------
const T = window.__TAURI__;
export const isDesktop = !!T;
const call = (cmd, args) => (isDesktop ? T.core.invoke(cmd, args) : demo[cmd](args ?? {}));
export const getView = () => call("get_view");
export const refreshAll = () => call("refresh_all");
export const refreshAccount = (index) => call("refresh_account", { index });
export const listProviders = () => call("list_providers");
export const listExtensions = () => call("list_extensions");
export const extensionDisclosure = (id) => call("extension_disclosure", { id });
export const enableExtension = (candidate) => call("enable_extension", { candidate });
export const disableExtension = (id) => call("disable_extension", { id });
export const testExtension = (id) => call("test_extension", { id });
export const formSchema = (provider) => call("form_schema", { provider });
export const visibleFields = (provider, choices, authMode, loginParams = {}) => call("visible_fields", { provider, choices, authMode, loginParams });
export const fieldChoices = (provider, choices, authMode) => call("field_choices", { provider, choices, authMode });
/** PRD 0005 RF3: how the draft's usage is read (level, notes, cost warning, save confirmation), from the core. */
export const draftAccess = (provider, authMode, fields, sessionSlots = []) => call("draft_access", { provider, authMode, fields, sessionSlots });
// RF2: login orquestrado pelo core; a UI só renderiza eventos e envia entradas.
export const loginStart = (request) => call("login_start", { request });
export const loginSubmitCode = (code) => call("login_submit_code", { code });
export const loginCancel = (attempt) => call("login_cancel", { attempt });
/** Assina os eventos tipados do login (sem segredos). Devolve função de cancelamento da assinatura. */
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
export const setUserDir = (path) => call("set_user_dir", { path });
// Animações (PRD 0004 RF1.2): `desktop.json` é a fonte; o localStorage só espelha para a leitura síncrona do <head>.
export const MOTIONS = ["system", "always", "reduced", "off"];
const applyMotion = (m) => { m = MOTIONS.includes(m) ? m : "system"; document.documentElement.dataset.motion = m; put("cuotai.motion", m); };
export const setMotion = async (motion) => { await call("set_motion", { motion }); applyMotion(motion); };
/** PRD 0020 RF7.2: `desktop.json` is the single source of theme, mode, language, animations, account order and
 *  grouping (and pace). `localStorage` only mirrors them so the <head> script can paint before the first IPC answer;
 *  no window listens to `storage` events for them any more. */
export const setPreference = async (key, value) => { await call("set_preference", { key, value }); mirrorPreference(key, value); };
const MIRROR_KEYS = { theme: "cuotai.theme", theme_mode: "cuotai.themeMode", sort: "cuotai.sort", group_recharging: "cuotai.group-recharging", motion: "cuotai.motion", locale: "cuotai.locale" };
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
    return await applyPreferences(s, { boot });
  } catch { return new Set(); }
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
// PRD 0003 RF3: reset notices (desktop.json; the backend schedules and sends them). `setNotifyEnabled` asks the
// system permission when turning on and resolves to `granted`, `denied`, `prompt` or `off`.
export const setNotifyEnabled = (on) => call("set_notify_enabled", { on });
export const setNotifyOptions = (leadMinutes, when, includeOther) => call("set_notify_options", { leadMinutes, when, includeOther });
export const setAccountNotify = (accountKey, on) => call("set_account_notify", { accountKey, on });
export const sendTestNotification = (title, body) => call("send_test_notification", { title, body });
// PRD 0008 RF3: usage alerts (desktop.json; the backend judges each reading and sends them). `setAlertsEnabled` asks
// the same system permission as reset notices and resolves to `granted`, `denied`, `prompt` or `off`.
export const setAlertsEnabled = (on) => call("set_alerts_enabled", { on });
export const setAlertOptions = (thresholds, exhaustion, recovery, overage) => call("set_alert_options", { thresholds, exhaustion, recovery, overage });
export const setMetricAlert = (accountKey, label, on) => call("set_metric_alert", { accountKey, label, on });
// PRD 0011 RF4.3: public provider status queries (desktop.json, off by default; the backend asks the core).
export const setStatusEnabled = (on) => call("set_status_enabled", { on });
// PRD 0012 RF5: the local server (desktop.json, off by default); the backend runs the core's server over this app's
// own refresh cycle. Each call answers the server's new state; the token never comes back except to be copied.
export const serveStatus = () => call("serve_status");
export const setServe = (enabled, port, allowBrowsers) => call("set_serve", { enabled, port, allowBrowsers });
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
/** "Sobre": identidade do projeto e lista de bibliotecas de código aberto (vem do core). */
export const aboutInfo = () => call("about_info");
/** PRD 0019 RF7.7: o histórico do changelog já analisado pelo core (sem rede). */
export const whatsNew = () => call("whats_new");
/** Abre no navegador um repositório da lista do "Sobre" (o shell recusa qualquer outro endereço). */
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
export const onSettingsChanged = (cb) => {
  if (isDesktop) return T.event.listen(SETTINGS_CHANGED, (e) => cb(e.payload?.key ?? ""));
  const ch = demoChannel();
  if (ch) ch.onmessage = (e) => cb(e.data?.key ?? "");
  // Node (the UI smokes) keeps a process alive while a channel listens; `unref` lets it exit. Browsers lack it.
  ch?.unref?.();
  return Promise.resolve(() => ch?.close());
};
/** Seletor nativo de pasta (plugin de diálogo do Tauri). */
export async function pickFolder(defaultPath) {
  if (isDesktop && T.dialog) return T.dialog.open({ directory: true, multiple: false, defaultPath: defaultPath || undefined });
  return "/Users/demo/CuotaiUser"; // navegador (demo)
}
/// RF1.2: aplica o `cuotai.toml` escolhido (`null` = padrão do SO); recarrega as contas e devolve a nova view.
export const setConfigFile = (path) => call("set_config_file", { path });
/** Seletor nativo de arquivo TOML. */
export async function pickTomlFile(defaultPath) {
  if (isDesktop && T.dialog) return T.dialog.open({ multiple: false, directory: false, defaultPath: defaultPath || undefined, filters: [{ name: "TOML", extensions: ["toml"] }] });
  return "/Users/demo/cuotai.toml"; // navegador (demo)
}
/** Caminho efetivo do `cuotai.toml` com a origem (variável, escolhido ou padrão do SO); texto não escapado. */
export const configLabel = (view) => view?.config_path ? `${view.config_path} · ${t(`settings.config.source.${view.config_source}`)}` : "";
/** RF1.2: aviso quando a TUI usou outro arquivo por último; texto não escapado. */
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
// PRD 0002 RF6.1: catálogo de modelos por conta. `generation` vem da `View` lida pela UI; se a configuração mudou
// desde então o backend recusa com "stale" e o resultado (da identidade antiga) é descartado.
export const STALE = "stale";
export const modelState = (index, generation) => call("model_state", { index, generation });
export const loadModels = (index, generation) => call("load_models", { index, generation });
export const syncModels = (index, generation) => call("sync_models", { index, generation });
/** Fonte do catálogo ou motivo de não listar, para conta ainda não salva (sem rede). */
export const modelSupport = (provider) => call("model_support", { provider });
// RF8: erros do backend chegam como JSON `{ id, params, fallback }` ou texto simples.
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

// ---------- formatação ----------
/** Percentual restante (0–100) segundo a classificação do core; null = sem medida/limite. */
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
export const fmtNumber = (n, max = 2) => (n == null ? "—" : new Intl.NumberFormat(currentLocale(), { maximumFractionDigits: max }).format(n));
// Unidades que descrevem "contagem de requisições": ficam implícitas. Outras (USD, créditos, tokens) são exibidas.
const QUIET_UNITS = new Set(["", "%", "—", "solicitações", "requisições", "consultas", "unidades"]);
const unitSuffix = (m) => (QUIET_UNITS.has(m.unit) ? "" : ` ${m.unit}`);
export function fmtAmount(m) {
  if (m.limit == null) return m.used == null ? "—" : `${fmtNumber(m.used)}${unitSuffix(m)}`;
  return `${fmtNumber(m.used)} / ${fmtNumber(m.limit)}${unitSuffix(m)}`;
}
/** One recorded value of a metric with `unit` (history): "60%", "12.5 USD", or a bare count. */
export const fmtValue = (v, unit) => (unit === "%" ? `${fmtNumber(v)}%` : `${fmtNumber(v)}${unitSuffix({ unit })}`);
/** Métrica sem limite utilizável: texto no lugar da barra (nunca inventa "100% livre"). */
export function textualMetric(m) {
  if (m.state === "exhausted" && m.no_cap) return t("metric.exhausted");
  if (m.state === "available" && m.uncapped) {
    const amount = m.remaining ?? m.used;
    if (amount == null) return null;
    return t(m.remaining != null ? "metric.balance" : "metric.usage", { amount: `${fmtNumber(amount)}${unitSuffix(m)}` });
  }
  return null;
}
export function fmtCountdown(iso) {
  if (!iso) return null;
  let s = Math.floor((new Date(iso).getTime() - Date.now()) / 1000);
  if (Number.isNaN(s)) return null;
  if (s <= 0) return t("time.now");
  const d = Math.floor(s / 86400); s -= d * 86400;
  const h = Math.floor(s / 3600); s -= h * 3600;
  const m = Math.floor(s / 60);
  return d > 0 ? `${d}d ${h}h ${m}m` : h > 0 ? `${h}h ${m}m` : `${m}m`;
}
/** Contagem até a próxima consulta: "4m 20s", "1h 05m", "2d 3h". `ms` é um instante absoluto (Unix, ms). */
export function fmtNext(ms, now = Date.now()) {
  let s = Math.ceil((ms - now) / 1000);
  if (!Number.isFinite(s)) return "";
  if (s <= 0) return t("time.now");
  const d = Math.floor(s / 86400); s -= d * 86400;
  const h = Math.floor(s / 3600); s -= h * 3600;
  const m = Math.floor(s / 60); s -= m * 60;
  if (d > 0) return `${d}d ${h}h`;
  if (h > 0) return `${h}h ${String(m).padStart(2, "0")}m`;
  return m > 0 ? `${m}m ${String(s).padStart(2, "0")}s` : `${s}s`;
}
/** Date without a time, localized for release and other calendar-date labels (DS-MSG-8). */
export function fmtDate(iso) {
  if (!iso) return null;
  const d = new Date(iso);
  return Number.isNaN(d.getTime())
    ? null
    : d.toLocaleDateString(currentLocale(), { day: "numeric", month: "short", year: "numeric", timeZone: "UTC" });
}
export function fmtReset(iso) {
  if (!iso) return null;
  const d = new Date(iso);
  if (Number.isNaN(d.getTime())) return null;
  const time = d.toLocaleTimeString(currentLocale(), { hour: "2-digit", minute: "2-digit" });
  if (d.toDateString() === new Date().toDateString()) return t("time.today", { time });
  return `${d.toLocaleDateString(currentLocale(), { day: "2-digit", month: "short" })}, ${time}`;
}
/** Clock time of `iso` in the viewer's local zone, with the day when it is not today ("14:30", "24 Oct, 14:30"). */
export function fmtClock(iso) {
  if (!iso) return null;
  const d = new Date(iso);
  if (Number.isNaN(d.getTime())) return null;
  const time = d.toLocaleTimeString(currentLocale(), { hour: "2-digit", minute: "2-digit" });
  if (d.toDateString() === new Date().toDateString()) return time;
  return `${d.toLocaleDateString(currentLocale(), { day: "2-digit", month: "short" })}, ${time}`;
}
/** PRD 0033 RF2.1: a preserved or restored metric whose window renewed; the core sends it without values. */
export const isRenewedWaiting = (m) => m.expired === true;
/** "Renewed at 14:30; waiting for a reading", in the viewer's local time. */
export const renewedText = (m) => t("metric.renewedWaiting", { time: fmtClock(m.resets_at) ?? "—" });
export const statusLabel = (status) => t(`status.${status}`);
/** Quotas que definem o resumo e a ordenação da conta: sem a janela curta de rate limit (não é a cota do
 *  plano) e sem as que o usuário ocultou. */
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
function fmtExtra(value, unit, currency) {
  if (value == null) return "—";
  if (currency) {
    try { return new Intl.NumberFormat(currentLocale(), { style: "currency", currency }).format(value); } catch { /* unknown code */ }
  }
  if (unit === "unid. provedor") {
    const n = fmtNumber(value);
    return tl({ id: "overage.amount.provider_units", params: { value: n }, fallback: `${n} unid. provedor` });
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
  const rtf = new Intl.RelativeTimeFormat(currentLocale(), { style: "short" });
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
export const reportKey = (r) => `${r.provider}::${r.account}`;
/** Resolve the structured note sidecar by the same index as the legacy fallback. */
export const noteText = (report, index) =>
  report.notes_l10n?.[index] ? tl(report.notes_l10n[index]) : report.notes[index] ?? "";
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
  nine_router: B("9router", "#0ea5e9", "9r"),
  extension: B("Extension", "#8b5cf6", "Ex"),
};
export const brand = (p) => brands[p] ?? B(p, "#888888");
/** PRD 0029 RF3.4: the account's display name, its configured name or else its provider's. */
export const accountLabel = (r) => (r.account ?? "").trim() || brand(r.provider).label;
// Logos de provider: glifos SVG monocromáticos (lobehub/icons, MIT; zed: simple-icons, CC0; ver img/providers/NOTICE.md)
// pintados por máscara CSS sobre um ladrilho com a cor da marca — nítidos em qualquer tamanho e sem fundo branco.
// [arquivo, fundo do ladrilho, cor do glifo]; o fundo omitido usa a cor da marca e o glifo, branco.
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
  if (provider === "kimchi") // arte própria, já colorida e circular: sem ladrilho nem máscara
    return `<span class="logo art" style="${style}" aria-hidden="true"><img src="img/providers/kimchi.svg" alt="" draggable="false" /></span>`;
  const g = glyphs[provider];
  if (!g) return `<span class="logo fb" style="${style};--c:${b.color}" aria-hidden="true"><b>${esc(b.mark)}</b></span>`;
  const [file, bg = b.color, fg = "#fff"] = g;
  return `<span class="logo" style="${style};--c:${bg};--fg:${fg}" aria-hidden="true"><i class="glyph" style="-webkit-mask-image:url(img/providers/${file}.svg);mask-image:url(img/providers/${file}.svg)"></i></span>`;
};

// ---------- tema (compartilhado entre janelas via localStorage + evento storage) ----------
// Registro e regras de modo em js/themes.js; tokens em css/themes/<id>.css.
const get = (k, d) => { try { return localStorage.getItem(k) || d; } catch { return d; } };
const put = (k, v) => { try { localStorage.setItem(k, v); } catch {} };
const systemDark = () => matchMedia("(prefers-color-scheme: dark)").matches;
export const theme = {
  mode: get("cuotai.themeMode", "system"),
  id: get("cuotai.theme", get("cuotai.darkTheme", DEFAULT_THEME)), // bruto: temas do usuário só existem após o boot; darkTheme: chave antiga
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
/** Aparência rápida (auto/claro/escuro). `themeSelect` acrescenta a escolha do tema (só na tela de configurações). */
export function themeSwitchHtml({ themeSelect = false } = {}) {
  const th = themeById(theme.id);
  const modes = [["system", "monitor"], ["light", "sun"], ["dark", "moon"]];
  const groups = [...new Set(THEMES.map((x) => x.group))].sort((a, b) => {
    const ia = GROUP_ORDER.indexOf(a), ib = GROUP_ORDER.indexOf(b);
    return (ia < 0 ? 99 : ia) - (ib < 0 ? 99 : ib) || a.localeCompare(b);
  });
  const options = groups.map((g) =>
    `<optgroup label="${esc(groupLabel(g))}">${THEMES.filter((x) => x.group === g)
      .map((x) => `<option value="${esc(x.id)}" ${x.id === th.id ? "selected" : ""}>${esc(x.label)}</option>`).join("")}</optgroup>`).join("");
  return `<div class="theme-switch">
    <div class="segmented" role="radiogroup" aria-label="${te("theme.mode.label")}">${modes.map(([id, ic]) => {
      const off = id !== "system" && !th.modes.includes(id);
      const label = t(`theme.mode.${id}`) + (off ? ` (${t("theme.mode.unavailable")})` : "");
      return `<button role="radio" aria-checked="${theme.mode === id}" class="${theme.mode === id ? "on" : ""}" title="${esc(label)}" data-theme-mode="${id}" ${off ? "disabled" : ""}>${icon(ic)}</button>`;
    }).join("")}</div>
    ${themeSelect ? `<select aria-label="${te("theme.label")}" title="${te("theme.label")}" data-theme-id>${options}</select>` : ""}
  </div>`;
}
let langOpen = false;
const flagOf = (l) => flagSvg(flagCountry(l.code, l.flag));
/** Botão com a bandeira do idioma atual e menu (bandeira + nome). `inline` mostra a lista aberta (tela de configurações). */
export function langMenuHtml({ inline = false } = {}) {
  const cur = LOCALES.find((l) => l.code === currentLocale()) ?? LOCALES[0];
  const items = LOCALES.map((l) => `<button role="menuitemradio" aria-checked="${l.code === cur.code}" class="${l.code === cur.code ? "on" : ""}" data-locale="${esc(l.code)}">${flagOf(l)}<span>${esc(l.name)}</span></button>`).join("");
  if (inline) return `<div class="lang-list" role="radiogroup" aria-label="${te("lang.label")}">${items}</div>`;
  return `<div class="lang">
    <button class="icon lang-btn" data-lang-toggle title="${te("lang.label")}: ${esc(cur.name)}" aria-haspopup="menu" aria-expanded="${langOpen}">${flagOf(cur)}</button>
    <div class="lang-menu" role="menu" ${langOpen ? "" : "hidden"}>${items}</div>
  </div>`;
}

/** Delegação de eventos do seletor; `rerender` é chamado após a troca (tema e idioma com cross-fade, RF3.3). */
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
    if (e.target.closest("[data-lang-toggle]")) { langOpen = !langOpen; sortOpen = false; return rerender(); }
    const item = e.target.closest("[data-locale]");
    if (item) { langOpen = false; await setLocale(item.dataset.locale); return crossFade(rerender); } // carrega antes: a transição não congela a tela
    if ((langOpen || sortOpen) && !e.target.closest(".lang")) { langOpen = false; sortOpen = false; rerender(); }
  });
  addEventListener("keydown", (e) => { if (e.key === "Escape" && (langOpen || sortOpen)) { langOpen = false; sortOpen = false; rerender(); } });
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

// Quotas ocultas pelo usuário (olho na tela de detalhes): compartilhadas entre as janelas e fora do resumo/ordem.
const hiddenListeners = new Set();
export const hiddenMetrics = persistentSet("cuotai.hiddenMetrics", () => hiddenListeners.forEach((f) => f()));
export const onHiddenChange = (f) => hiddenListeners.add(f);
export const isMetricHidden = (r, m) => hiddenMetrics.has(metricKey(r, m));

// Contas desativadas no painel (interruptor do card): compartilhadas entre janelas e com o menu da bandeja.
const disabledListeners = new Set();
export const disabledAccounts = persistentSet("cuotai.disabledAccounts", () => disabledListeners.forEach((f) => f()));
export const onDisabledChange = (f) => disabledListeners.add(f);

// ---------- estado de dados ----------
// O backend é a única fonte: agenda por conta (RefreshCycle) e emite "snapshot".
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

// ---------- demo (só no navegador; mesma forma dos comandos Tauri) ----------
const h = (n) => new Date(Date.now() + n * 3600e3).toISOString();
const dm = (label, used, limit, resetH, kind, window, unit = "solicitações") => {
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
const demoReports = [
  dr("alex@example.com", "antigravity", [dm("Gemini (5h)", 15, 1000, 0.37, "quota", "window_5h"), dm("Claude & GPT (5h)", 0, 1000, 5, "quota", "window_5h"), dm("Gemini (Weekly)", 120, 1000, 151, "quota", "weekly"), dm("Claude & GPT (Weekly)", 0, 1000, 151.9, "quota", "weekly")]),
  // PRD 0023: the exhausted week is also paying for extra usage ("Limit hit · on extra $6.25").
  { ...dr("Account 2", "claude", [dm("session (5h)", 0, 100, 4.5, "quota", "window_5h", "%"), dm("weekly (7d)", 100, 100, 24.6, "quota", "weekly", "%"),
      { label: "Extra usage", used: 6.25, limit: 50, remaining: null, unit: "USD", kind: "spend", window: "monthly", period: null, resets_at: h(300), billing: "overage",
        state: "available", ratio: null, uncapped: true, no_cap: false }]),
    class: "exhausted", model_count: 1,
    overage: { state: "active", spent: 6.25, unit: "USD", currency: "USD", cap: 50, cycle_ends_at: h(300),
      text: { id: "overage.on_extra", params: { amount: "$6.25" }, fallback: "No extra · $6.25" },
      summary: { id: "overage.limit_hit_on_extra", params: { amount: "$6.25" }, fallback: "Limite atingido · no extra $6.25" }, caveat: null } },
  // PRD 0010 RF3 fixtures: an aging reading (10 min against 300 s) and a failure that kept its last valid reading.
  { ...dr("alex@example.com", "chat_gpt", [dm("5h", 6, 100, 67.6, "quota", "window_5h", "%")]), checked_at: ago(10), freshness: "aging" },
  { ...dr("jmarcon", "copilot", [dm("chat", 0, 0, 2.6, "quota", "other"), dm("premium_interactions", 7026, 7000, 2.6, "quota", "other")]), class: "exhausted" },
  { ...dr("Personal", "groq", [], ["Groq connected. No rate-limit data reported for this key yet."]), model_count: 4 },
  dr("Personal", "mini_max", [dm("M-series (5h)", 6, 100, 2.6, "quota", "window_5h"), dm("M-series (7d)", 60, 100, 98.6, "quota", "weekly"), dm("Video (5h)", 0, 100, 2.6, "quota", "window_5h")]),
  dr("Account 2", "code_buddy", [dm("Monthly", 0, 100, 757, "quota", "monthly"), dm("Bonus Pack 1", 0, 250, 62, "quota", null)]),
  dr("Account 1", "kimi", [dm("Weekly", 100, 100, 109, "quota", "weekly"), dm("Ratelimit", 0, 100, 4.8, "rate_limit", "other")]),
  dr("Personal", "ollama_cloud", [dm("Monthly", 0, 100, 248, "quota", "monthly")]),
  dr("Personal", "anthropic_api", [], ["Credencial ausente ou expirada."], "authentication_error"),
  dr("Pessoal", "mistral", [], ["HTTP 429: limite de requisições atingido."], "rate_limited"),
  { ...dr("Dev", "cursor", [], ["Serviço indisponível no momento."], "error"), checked_at: ago(1),
    last_good: { checked_at: ago(42), metrics: [dm("Fast requests", 380, 500, 120, "quota", "monthly")] } },
  dr("Work", "windsurf", [], ["Informe a variável de ambiente da credencial."], "needs_configuration"),
  { ...dr("Account 3", "kiro", [dm("credit", 596.15, 10000, 360, "quota", "other", "créditos")]),
    identity: { email: null, copy: { label: "ARN do perfil", value: "arn:aws:codewhisperer:us-east-1:123456789012:profile/EXAMPLEPROFILE" },
      badges: [{ tone: "accent", text: "IAM Identity Center" }, { tone: "info", text: "us-east-1" }, { tone: "good", text: "ativa" }] } },
  { ...dr("alex@example.com", "grok", [dm("On-demand", 1, 1, 100, "quota", "monthly")]), identity: { email: "alex@example.com", badges: [], copy: null } },
  dr("Personal", "deepseek_demo", [{ label: "Balance", used: null, limit: null, remaining: 0, unit: "USD", kind: "balance", window: null, period: null, resets_at: null, state: "available", ratio: null, uncapped: true, no_cap: false }]),
  // PRD 0005 RF4.2 fixtures: an Unavailable provider (no query) and an Undocumented one whose response broke the schema.
  dr("Work", "poolside", [], ["Poolside não publica endpoint de uso/saldo por chave."], "unsupported"),
  { ...dr("Personal", "zed", [], ["Resposta incompatível com a interface de consulta; nenhum valor foi estimado."], "error"),
    notes_l10n: [{ id: "query.schema", params: {}, fallback: "Resposta incompatível com a interface de consulta; nenhum valor foi estimado." }] },
  // PRD 0010 RF2.3 fixture: just opened, no answer yet; the reading recorded 3 h ago shows as stale.
  { ...dr("Restored", "chat_gpt", []), pending: true, class: "unmeasured", checked_at: ago(180),
    last_good: { checked_at: ago(180), metrics: [dm("5h", 30, 100, 2, "quota", "window_5h", "%")] } },
  // PRD 0016 RF1.2 fixture: a session still read from an environment variable, offered for the vault;
  // RF3.2: its pasted session expires in 2 days (the card warns).
  { ...dr("Personal", "xiaomi_mimo", [dm("Weekly", 6, 100, 96, "quota", "weekly", "%")]),
    session_expiry: { expires_at: new Date(Date.now() + 2.5 * 86400000).toISOString(), days: 2, soon: true,
      label: { id: "user_session.expiry.days", params: { days: 2 }, fallback: "Sessão expira em 2 dias." } } },
];
// PRD 0016 RF2.1 demo: the platform session slot the core declares for Xiaomi MiMo.
const DEMO_SESSION_SLOT = { slot: "platform_session_env",
  label: { id: "user_session.slot.xiaomi_mimo.platform_session_env", params: {}, fallback: "Sessão web de platform.xiaomimimo.com (saldo e Token Plan)" },
  instructions: { id: "user_session.instructions", params: { site: "platform.xiaomimimo.com" }, fallback: "Entre em platform.xiaomimimo.com no navegador. Nas ferramentas do desenvolvedor (F12), aba Rede, recarregue a página, escolha uma requisição a platform.xiaomimimo.com e copie o cabeçalho Cookie ou use \"Copiar como cURL\"; a tabela de cookies da aba Aplicativo também serve. Só os cookies desta sessão são guardados; o resto do texto colado é descartado." },
  hosts: ["platform.xiaomimimo.com"] };
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
const hhmm = (iso) => new Date(iso).toLocaleTimeString(currentLocale(), { hour: "2-digit", minute: "2-digit" });
const dp = (stage, tone, expected, used, resetH, runsOutH = null) => {
  const delta = (used - expected) * 100, points = String(Math.round(Math.abs(delta)));
  const label = { reserve: { id: "pace.reserve", params: { delta: points }, fallback: `${points} pontos de reserva` },
    on_pace: { id: "pace.on_pace", fallback: "No ritmo" }, ahead: { id: "pace.ahead", params: { delta: points }, fallback: `${points} pontos acima do ritmo` },
    exhausted: { id: "pace.exhausted", fallback: "Limite atingido até a renovação" } }[stage];
  const at = runsOutH == null ? null : h(runsOutH);
  const margin = runsOutH == null ? "" : fmtCountdown(h(resetH - runsOutH)).replace(/^(\d+d \d+h) \d+m$/, "$1");
  const outlook = stage === "exhausted" ? null : at
    ? { id: "pace.runs_out_at", params: { at: hhmm(at), margin }, fallback: `acaba às ${hhmm(at)}, ${margin} antes da renovação` }
    : { id: "pace.lasts_until_reset", fallback: "dura até a renovação" };
  return { elapsed_fraction: expected, expected_used_fraction: expected, used_fraction: used, delta_points: delta, stage, tone, source: "average",
    resets_at: h(resetH), projected_exhaustion_at: at, lasts_until_reset: stage !== "exhausted" && !at, label, outlook };
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
const demoView = () => {
  const now = new Date().toISOString();
  const reports = demoReports.map((r, index) => {
    const [paceIndex = null, paces = {}] = r.status === "ready" ? DEMO_PACES.get(r) ?? [] : [];
    const notify = !demoSettings.notify.off_accounts.includes(demoKey(r));
    const metrics = r.metrics.map((m, i) => ({ ...m, pace: paces[i] ?? null, alert: demoMetricAlert(r, m, notify) }));
    const incident = demoIncident(r);
    // RF3 mirror: a failed query during an incident gains the core's note, its status unchanged.
    const note = incident && r.status !== "ready" && ["degraded", "partial_outage", "major_outage"].includes(incident.state) && DEMO_INCIDENT_NOTE;
    return { ...r, index, binding: demoBinding(r), binding_rank: DEMO_BINDINGS.get(r)?.[1] ?? DEMO_BINDINGS.size + index,
      metrics, pace_index: paceIndex,
      next_refresh_at_ms: Date.now() + (40 + index * 37) * 1000, uses_oauth: r.provider === "kiro" || r.uses_oauth, access: demoAccess(r),
      account_key: demoKey(r), notify, usage_alerts: metrics.filter((m) => m.alert?.active).map((m) => m.label), incident,
      ...(note ? { notes: [...r.notes, note.fallback], notes_l10n: [...r.notes.map((n, i) => r.notes_l10n?.[i] ?? null), note] } : {}) };
  });
  return { config_path: demoSettings.config ?? "~/.config/cuotai/config.toml (demo)", config_source: demoSettings.config ? "chosen" : "system", refresh_seconds: demoSettings.refresh ?? 300, config_exists: true, account_count: demoReports.length, loaded: true,
    started_at: now, finished_at: now, reports, busy: [], error: null, generation: demoSettings.generation,
    // `?history-warning` shows the size-ceiling warning (PRD 0006 RF4.3) the core would send.
    history_warning: new URLSearchParams(location.search).has("history-warning")
      ? { id: "history.warning.ceiling", params: { max: "64", removed: "1200" }, fallback: "O histórico passou de 64 MiB: as 1200 medições mais antigas foram apagadas." }
      : null,
    history_epoch: demoSettings.historyEpoch,
    notify: { enabled: demoSettings.notify.enabled, next: demoNextNotice(reports), alerts: demoSettings.alerts.enabled },
    status: { enabled: demoSettings.statusQueries, domains: [...new Set(demoReports.map((r) => DEMO_STATUS_HOSTS[r.provider]).filter(Boolean))],
      outage: reports.some((r) => ["partial_outage", "major_outage"].includes(r.incident?.state)) },
    serve: demoServeView(), hooks: { state: demoHooks.enabled ? demoHooksView().state : "off" } };
};
// ponytail: demo mirror of the backend's local server state (PRD 0012 RF5); nothing listens in the browser.
// `?serve=taken` plays port 6738 held by a `cuotai --serve` instance (the core's probe says so in the app).
const DEMO_SERVE_TAKEN = new URLSearchParams(location.search).get("serve") === "taken";
function demoServeView() {
  const s = demoSettings.serve;
  const taken = s.enabled && DEMO_SERVE_TAKEN && s.port === 6738;
  const state = !s.enabled ? "off" : taken ? "error" : "on";
  return { state, enabled: s.enabled, port: s.port, allow_browsers: s.allow_browsers, token: s.secret != null,
    url: state === "on" ? `http://127.0.0.1:${s.port}` : null,
    error: taken ? { id: "serve.error.port_taken", params: { port: s.port }, fallback: `A porta ${s.port} está em uso; escolha outra porta para o servidor local.` } : null,
    cuotai_on_port: taken, default_port: 6738, port_min: 1024, port_max: 65535,
    browsers_warning: { id: "serve.allow_browsers.warning", params: {}, fallback: "Com navegadores permitidos, qualquer página aberta no navegador pode ler seu uso no servidor local (com token, qualquer página que o conheça). Ative só se uma página sua precisa desses dados." } };
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
  "account_unblocked", "refresh_failed", "refresh_recovered", "auth_required", "provider_incident", "usage_updated", "overage_started", "overage_ended", "session_expiring", "quota_reset_soon"];
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
      demoL10n("hooks.confirm.events", { events }, `eventos: ${events}`),
      demoL10n("hooks.confirm.timeout", { seconds: r.timeout_seconds ?? 10 }, `limite de tempo: ${r.timeout_seconds ?? 10} s`),
      demoL10n("hooks.confirm.command", { command }, `comando: ${command}`));
  });
  return lines;
}
/** The core's `hooks.error.*` for the cases the editor can produce. */
function demoHooksError(rules) {
  if (!rules.length) return demoL10n("hooks.error.empty", {}, "hooks.toml não tem nenhuma regra [[hooks]].");
  const seen = new Set();
  for (const [i, r] of rules.entries()) {
    const name = r.name.trim();
    if (!name || name.length > 64) return demoL10n("hooks.error.name", { index: i + 1, max: 64 }, `A regra ${i + 1} precisa de um name de 1 a 64 caracteres, sem caracteres de controle.`);
    if (!r.events.length) return demoL10n("hooks.error.no_events", { name }, `A regra "${name}" não lista nenhum evento.`);
    const program = r.command[0] ?? "";
    if (!program || !(program.startsWith("/") || !program.includes("/"))) return demoL10n("hooks.error.command", { name }, `O command da regra "${name}" deve ser uma lista de argumentos cujo primeiro item é um caminho absoluto ou um nome procurado no PATH, como ["/caminho/script", "--opcao"].`);
    if (seen.has(name)) return demoL10n("hooks.error.duplicate", { name }, `Há mais de uma regra chamada "${name}".`);
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
const DEMO_STATE_PT = { operational: "Operacional", degraded: "Desempenho degradado", partial_outage: "Interrupção parcial",
  major_outage: "Interrupção grave", maintenance: "Em manutenção", unknown: "Status desconhecido" };
const DEMO_INCIDENTS = {
  claude: ["partial_outage", "Elevated errors on claude.ai & Claude Code", "https://status.claude.com/incidents/demo1", 0.8],
  cursor: ["major_outage", "Cursor IDE requests failing", "https://status.cursor.com/incidents/demo2", 0.3],
  groq: ["degraded", "Increased latency on the API", "https://groqstatus.com/incident/demo3", 2],
  kimi: ["maintenance", "Scheduled database maintenance", "https://status.moonshot.cn/incidents/demo4", 0.5],
  grok: ["unknown"],
};
const DEMO_INCIDENT_NOTE = { id: "note.incident.ongoing", params: {}, fallback: "O provedor informa um incidente em andamento; a falha pode não ser da sua conta." };
function demoIncident(r) {
  const host = DEMO_STATUS_HOSTS[r.provider];
  if (!demoSettings.statusQueries || !host) return null;
  const offline = DEMO_STATUS === "offline";
  const [state, title, url, startedH] = offline ? ["unknown"] : DEMO_INCIDENTS[r.provider] ?? ["operational"];
  return { provider: r.provider, state, label: dl(`incident.state.${state}`, DEMO_STATE_PT[state]), page_url: `https://${host}/`,
    incident: title ? { title, started_at: h(-startedH), url } : null, read_at: offline ? null : ago(2), stale: offline };
}
// ponytail: demo mirror of `alerts::eligible` and of an active alert (a capped quota at or above the lowest
// threshold, bell and metric not muted); the app reads both from the backend's alert state.
function demoMetricAlert(r, m, notify) {
  const a = demoSettings.alerts;
  const used = m.used ?? (m.limit != null && m.remaining != null ? m.limit - m.remaining : null);
  if (!a.enabled || r.status !== "ready" || m.kind !== "quota" || !(m.limit > 0) || used == null) return null;
  const muted = (a.muted[demoKey(r)] ?? []).includes(m.label);
  const pct = (used / m.limit) * 100;
  return { muted, active: notify && !muted && (pct >= 100 || a.thresholds.some((t) => pct >= t)) };
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
    .sort((a, b) => a.at.localeCompare(b.at));
  return next[0] ?? null;
}
// ponytail: demo mirror of `access::describe` + `UsageAccess::explain` (default modes, generic pt summary and
// evidence); the app reads both from the core.
const DEMO_ROUTER = "https://github.com/decolua/9router/blob/f01fb909e37189008080632ddaf404f096345cde/open-sse/services/usage/";
function demoAccess(r) {
  const level = demoLevelOf(r.provider);
  const reason = level === "unavailable" ? dl(`access.reason.${r.provider}`, "Não há consulta pública de uso para esta credencial (demo).") : null;
  const url = level === "undocumented" ? `${DEMO_ROUTER}${r.provider}.js` : "https://example.com/docs (demo)";
  const changed = level === "undocumented" && r.status === "error" && r.notes_l10n?.some((n) => n.id === "query.schema")
    ? { id: "access.interface_changed", params: { evidence: url }, fallback: `A resposta não segue a interface interna conhecida: ela pode ter mudado sem aviso. Contrato de referência: ${url}` }
    : null;
  return { level, label: demoLevel(level), summary: dl(`access.summary.${r.provider}`, "Consulta de uso de demonstração."), reason,
    evidence: { kind: level === "undocumented" ? "reference" : "documentation", url },
    explanation: r.status === "ready" ? null : reason ?? changed, session: demoSessionNotice(demoSessionState(r)) };
}
// PRD 0016 RF4 demo: the MiMo account reads its platform session from a variable until it is migrated (or pasted), then
// from the vault until it is forgotten; the card mirrors `user_session::notice`.
const demoSessionState = (r) => r.provider !== "xiaomi_mimo" || demoSettings.sessionForgotten.has(r) ? null
  : demoSettings.sessionMigrated.has(r) ? "vault" : "env";
const demoSessionNotice = (state) => state && { level: dl("access.credential.user_session", "Sessão do usuário"),
  warning: state === "vault"
    ? dl("user_session.warning", "Esta é uma sessão de login completa. O Cuotai a guarda no cofre do sistema e a usa só para consultar este serviço.")
    : dl("user_session.warning_env", "Esta é uma sessão de login completa. O Cuotai a lê de uma variável de ambiente e a usa só para consultar este serviço; mova-a para o cofre do sistema no assistente da conta.") };
const demoSettings = { custom: null, config: null, generation: 1, historyRecording: true, historyEpoch: 0, historyErased: new Set(), sessionMigrated: new Set(), sessionForgotten: new Set(),
  notify: { enabled: false, lead_minutes: 0, when: "used", include_other: false, off_accounts: [] },
  alerts: { enabled: false, thresholds: [75, 90], exhaustion: true, recovery: true, overage: true, muted: {} }, statusQueries: DEMO_STATUS != null,
  serve: { enabled: DEMO_SERVE_TAKEN, port: 6738, allow_browsers: false, secret: null },
  widgets: { enabled: DEMO_WIDGETS, include_identity: false, max_accounts: 6, written: false } };
const demoPermission = (on) => (!on ? "off" : new URLSearchParams(location.search).has("notify-denied") ? "denied" : "granted");
// Modelos na demo: mesma validação (índice + geração) e o estado `unsupported` com motivo explícito de demonstração;
// nenhum catálogo inventado é exibido como real.
const demoModels = async ({ index, generation }) => {
  if (generation !== demoSettings.generation) throw STALE;
  if (!demoReports[index]) throw JSON.stringify({ id: "models.error.invalid_index", params: {}, fallback: "Conta inexistente nesta configuração." });
  return { state: "unsupported", reason: { id: "models.demo", params: {}, fallback: "Demonstração no navegador: o catálogo de modelos só é consultado no app desktop." } };
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
  if (!r) throw JSON.stringify({ id: "models.error.invalid_index", params: {}, fallback: "Conta inexistente nesta configuração." });
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
// ponytail: the browser demo has no logs; two fixed sources play the core's disclosure, the consent by digest, turning
// off (erasing what was derived), the chosen directories, the period, the retention, the background scan's last
// summary and the RF4 summary (synthetic days and models; one model without a price). `?cost-paused` plays a computer
// in energy-saving mode and `?cost-noaccess` a Claude Code directory the system refuses. Core texts keep their
// Portuguese fallback here (no core catalog in the demo).
const DEMO_COST = {
  claude_code: { label: "Claude Code", pattern: "**/*.jsonl", dirs: ["/Users/demo/.config/claude/projects", "/Users/demo/.claude/projects"], files: 12,
    providers: ["claude"], models: { "claude-sonnet-4-5-20250929": { entry: "claude-sonnet-4-5", input: 3, output: 15, cache_read: 0.3, cache_write: 3.75 }, "claude-haiku-5-5": null } },
  codex_cli: { label: "Codex CLI", pattern: "**/rollout-*.jsonl", dirs: ["/Users/demo/.codex/sessions", "/Users/demo/.codex/archived_sessions"], files: 3,
    providers: ["chat_gpt"], models: { "gpt-5-codex": { entry: "gpt-5-codex", input: 1.25, output: 10, cache_read: 0.125, cache_write: 1.25 } } },
};
const DEMO_PERIODS = { today: "Hoje", last_7_days: "Últimos 7 dias", last_30_days: "Últimos 30 dias", current_month: "Mês atual", all: "Tudo o que está disponível" };
const demoCost = { on: {}, paths: {}, period: "last_7_days", retention: 120, erased: new Set() };
const demoDirs = (id) => demoCost.paths[id]?.map((p) => p.replace(/^~(?=\/|$)/, "/Users/demo")) ?? DEMO_COST[id].dirs;
const demoDisclosure = (id) => {
  const { label, pattern } = DEMO_COST[id];
  const dirs = demoDirs(id);
  return { source: id, label, directories: dirs, custom: !!demoCost.paths[id], pattern, digest: `demo-${id}-${dirs.join("|")}`,
    purpose: demoL10n("cost.disclosure.purpose", { source: label }, `Para estimar, só neste computador, os tokens e o custo a preços de API do uso do ${label} por dia e por modelo.`),
    extracted: demoL10n("cost.disclosure.extracted", {}, "Só números são extraídos: o horário, o modelo, as contagens de tokens (entrada, saída, leitura e gravação de cache) e um resumo anônimo da sessão."),
    never: demoL10n("cost.disclosure.never", {}, "O conteúdo das conversas, prompts, respostas, nomes de arquivos e caminhos de projetos nunca é lido para o índice, gravado nem exibido. Nada sai do computador."),
    off: demoL10n("cost.disclosure.off", {}, "Desligar a fonte interrompe qualquer leitura e apaga tudo o que foi derivado dela.") };
};
const demoCostView = () => {
  const sources = Object.entries(DEMO_COST).map(([id, s]) => {
    const enabled = id in demoCost.on;
    const files = enabled && !demoCost.erased.has(id) ? s.files : 0;
    return { source: id, label: s.label, enabled, consented_at: demoCost.on[id] ?? null, paths: demoCost.paths[id] ?? [], disclosure: demoDisclosure(id),
      indexed: { files, invalid_lines: files ? 2 : 0 },
      summary: enabled ? demoL10n("cost.scan.ready", { source: s.label, files: String(s.files), requests: String(files ? 0 : 41), invalid: "2" },
        `${s.label}: ${s.files} log(s); ${files ? 0 : 41} requisição(ões) nova(s); 2 linha(s) inválida(s) ignorada(s).`) : null };
  });
  const on = sources.some((s) => s.enabled);
  return { sources, period: demoCost.period, periods: Object.entries(DEMO_PERIODS).map(([id, text]) => ({ id, label: demoL10n(`cost.period.${id}`, {}, text) })),
    retention_days: demoCost.retention, retention_min: 7, retention_max: 730,
    state: !on ? "off" : new URLSearchParams(location.search).has("cost-paused") ? "paused" : "waiting", error: null,
    size: on ? 86016 : 0 };
};
const DEMO_EFFECTIVE = "2026-10-21";
const demoNotice = () => demoL10n("cost.estimate.notice", {}, "Estimativa a preços de API, não é uma cobrança; valores em USD.");
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
    if (!(id in demoCost.on)) return { ...base, state: "off", note: demoL10n("cost.scan.off", { source: s.label }, `${s.label}: desligado; nada é lido.`), usage: null };
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
      ? demoL10n("cost.view.no_access", { source: s.label }, `${s.label}: sem acesso aos diretórios dos logs; nada foi contornado. Os números são os que o índice já tinha.`)
      : period.models.length ? null : demoL10n("cost.view.empty", { source: s.label, period: periodLabel }, `${s.label}: nenhum uso nos logs lidos (${periodLabel.fallback}).`);
    return { ...base, state, note, usage: { today: demoEstimate(rowsOf(0)), yesterday: demoEstimate(rowsOf(1)), period, days: period.models.length ? days : [] } };
  });
  const label = demoL10n(`cost.period.${demoCost.period}`, {}, DEMO_PERIODS[demoCost.period]);
  const all = Object.keys(DEMO_COST).filter((id) => id in demoCost.on && !demoCost.erased.has(id))
    .flatMap((id) => Array.from({ length: span + 1 }, (_, back) => demoDayRows(id, back)).flat());
  return { currency: "USD", notice: demoNotice(), span: { period: demoCost.period, days: null, label, first_day: demoCost.period === "all" ? null : at(span), last_day: at(0) }, sources,
    total: demoEstimate(all), pricing_error: null };
};
const demoPrices = () => {
  const models = Object.values(DEMO_COST).flatMap((s) => Object.values(s.models).filter(Boolean))
    .map((p) => ({ model: p.entry, origin: "embedded", rates: { input: p.input, output: p.output, cache_read: p.cache_read, cache_write: p.cache_write } }))
    .sort((a, b) => a.model.localeCompare(b.model));
  return { currency: "USD", notice: demoNotice(), tables: [{ origin: "embedded", effective: DEMO_EFFECTIVE, source: "https://claude.com/pricing#api; https://openai.com/api/pricing/" }],
    pricing_file: { path: "/Users/demo/Library/Application Support/cuotai/cost/pricing.json", used: false, error: null }, models };
};
// PRD 0018 RF2.6/RF3.1/RF2.5 browser demo: the same `UpdateView` fields.
// `?update=consent` shows the question without a request; `off`, `managed` and
// `error` exercise the other states.
const DEMO_UPDATE = new URLSearchParams(location.search).get("update");
const DEMO_NOTES = "Correções de medição e uma nova opção de atualização automática (PRD 0018).";
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
    channel: "stable", current_version: "0.71.1", last_check: demoUpdate.lastCheck,
    consent_pending: !demoUpdate.consentAnswered && !demoUpdate.disabledReason,
    phase: demoUpdate.phase, version: demoUpdate.phase === "idle" || demoUpdate.phase === "failed" ? null : demoUpdate.version,
    notes: demoUpdate.phase === "available" ? DEMO_NOTES : null, date: demoUpdate.phase === "available" ? demoUpdate.date : null,
    can_install: demoUpdate.canInstall, upgrade_command: demoUpdate.upgradeCommand, progress: demoUpdate.progress,
    dismissed: demoUpdate.dismissed, error: demoUpdate.error,
    manual_url: "https://github.com/jmarcon/cuotai/releases/latest",
  };
};
const demo = {
  get_view: async () => demoView(),
  refresh_all: async () => {},
  // Demo of PRD 0010 RF3.2: refreshing the failing account succeeds, so its preserved reading (and badge) goes away.
  refresh_account: async ({ index }) => {
    const r = demoReports[index];
    if (r?.last_good) demoReports[index] = { ...r, status: "ready", pending: false, checked_at: ago(0), metrics: r.last_good.metrics, notes: [], last_good: null };
  },
  list_providers: async () => Object.entries(brands).map(([id, b]) => ({ id, label: b.label, pickable: !DEMO_NO_QUERY.includes(id) })),
  list_extensions: async () => [],
  extension_disclosure: async () => ({ lines: [], candidate: { token: "demo" } }),
  enable_extension: async ({ candidate }) => ({ id: candidate?.id ?? "demo", message: dl("extensions.action.enabled", "Extensão ativada."), account: null }),
  disable_extension: async ({ id }) => ({ id, message: dl("extensions.action.disabled", `Extensão ${id} desativada.`), account: null }),
  test_extension: async ({ id }) => ({ source: `extension:${id}`, status: "ready", metrics: [], notes: [], violations: [] }),
  // ponytail: demo mirror of `access` for the browser (default modes only, generic pt summary); the app asks the core.
  draft_access: async ({ provider }) => {
    const level = demoLevelOf(provider);
    const reason = level === "unavailable" ? dl(`access.reason.${provider}`, "Não há consulta pública de uso para esta credencial (demo).") : null;
    return {
      level: demoLevel(level),
      summary: dl(`access.summary.${provider}`, "Consulta de uso de demonstração."),
      notes: [dl(`access.credential_hint.${level === "official" ? "admin_key" : "inference_key"}`, "Credencial da consulta (demo)."), ...(reason ? [reason] : [])],
      cost_advisory: provider === "aws_bedrock" ? dl("access.cost_advisory", "Provedor cobrado por consulta (demo).") : null,
      save_confirmation: reason && { id: "access.confirm_unavailable", params: { reason }, fallback: `${reason.fallback} A conta será gravada, mas mostrará este motivo em vez de números.` },
    };
  },
  form_schema: async ({ provider }) => provider === "kiro" ? demoKiroSchema() : ({ credential_kind: "env", default_env: "DEMO_API_KEY",
    credential_help: { id: "credential_help.demo", params: {}, fallback: "Nome da variável de ambiente com a credencial (demo)." },
    login: { modes: [{ id: "oauth", label: { id: "login.demo.oauth.label", params: {}, fallback: "Login OAuth (demo)" }, help: { id: "login.demo.oauth.help", params: {}, fallback: "No navegador de demonstração o login não executa." }, completion_notice: { id: "login.demo.oauth.completion", params: {}, fallback: "Login autorizado (demo)." }, credential_store: "cuotai_vault", account_binding: { type: "none" }, registration: [], options: [], unavailable_reason: { id: "login.demo.oauth.unavailable_reason", params: {}, fallback: "Login indisponível na demonstração." } }], unsupported_reason: null },
    sessions: provider === "xiaomi_mimo" ? [DEMO_SESSION_SLOT] : [],
    session_notice: provider === "xiaomi_mimo" ? demoSessionNotice("vault") : null,
    fields: [{ id: "field.demo.project_id", key: "project_id", label: { id: "field.demo.project_id.label", params: {}, fallback: "Project ID" }, help: { id: "field.demo.project_id.help", params: {}, fallback: "Opcional." }, required: false, options: null },
             { id: "field.demo.mode", key: "mode", label: { id: "field.demo.mode.label", params: {}, fallback: "Modo" }, help: { id: "field.demo.mode.help", params: {}, fallback: "" }, required: false, options: [{ id: "field.demo.mode.option.personal", index: 0, value: "personal", label: { id: "field.demo.mode.option.personal", params: {}, fallback: "Pessoal" } }, { id: "field.demo.mode.option.org", index: 1, value: "org", label: { id: "field.demo.mode.option.org", params: {}, fallback: "Organização" } }] }] }),
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
          ? dl("edit_notice.option_replaced_for_auth_mode", `${current.label.fallback} não está disponível neste modo; ${selected.label.fallback} foi selecionado.`)
          : null,
      };
    });
  },
  get_account: async ({ index }) => {
    const r = demoReports[index];
    const state = demoSessionState(r);
    const sessionOffer = state === "env";
    const label = { id: "user_session.slot.xiaomi_mimo.platform_session_env", params: {}, fallback: "Sessão web de platform.xiaomimimo.com (saldo e Token Plan)" };
    const hosts = ["platform.xiaomimimo.com"];
    return { name: r.account, provider: r.provider, credential: "DEMO_API_KEY",
      interval: "", uses_oauth: false, native_login: false, auth_mode: "credential", auth_options: {}, auth_registration: {}, fields: [{ key: "project_id", text: "", choice: null }, { key: "mode", text: null, choice: 0 }],
      session_slots: state === "vault" ? ["platform_session_env"] : [],
      session_migration: sessionOffer ? [{ slot: "platform_session_env", env: "MIMO_COOKIE", label, hosts,
        consent: { id: "user_session.migrate.consent", params: { label, env: "MIMO_COOKIE", hosts: hosts.join(", ") },
          fallback: `${label.fallback}: copiar o valor de MIMO_COOKIE para o cofre do sistema e tirar MIMO_COOKIE do TOML. A sessão só é enviada a ${hosts.join(", ")}; a variável continua no seu ambiente até você removê-la.` } }] : [] };
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
    if (kept.length < 2) throw JSON.stringify({ id: "user_session.paste.missing", params: { names: "api-platform_serviceToken, userId" }, fallback: "Faltam cookies desta sessão: api-platform_serviceToken, userId." });
    const discarded = names.length - kept.length;
    const expiry = { id: "user_session.expiry.unknown", params: {}, fallback: "Validade desconhecida." };
    return { kept, discarded, expiry: null, summary: { id: "user_session.paste.summary", params: { kept: kept.join(", "), discarded, expiry }, fallback: `Guardados: ${kept.join(", ")}. Descartados: ${discarded}. Validade desconhecida.` } };
  },
  test_session: async ({ input }) => {
    await demo.preview_session({ paste: input.sessions?.[0]?.paste ?? "" });
    return { ok: true, status: { id: "status.ready", params: {}, fallback: "OK" }, note: null, metrics: 2, expiry: null };
  },
  save_account: async ({ input }) => {
    window.__lastSaveRequest = input;
    // PRD 0029 RF3: the core rule — a name is needed only beside another account of the provider.
    const others = demoReports.filter((x, i) => i !== input.index && x.provider === input.provider);
    if (!input.name.trim() && others.length) throw JSON.stringify({ id: "edit_error.name_required_same_provider", params: { provider: { id: `provider.${input.provider}`, params: {}, fallback: input.provider } }, fallback: "Dê um nome a esta conta: você tem outra conta do provedor." });
    const peer = input.index == null ? others.find((x) => !x.account.trim()) : null;
    if (peer) {
      if (!input.peerName?.trim()) throw JSON.stringify({ id: "edit_error.name_required", field: "peer_name", params: {}, fallback: "Informe um nome para a conta." });
      peer.account = input.peerName.trim();
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
  // Login de demonstração: emite a sequência de eventos do core (prompt → progresso → concluído).
  login_start: async ({ request }) => {
    window.__lastLoginRequest = request;
    const attempt = 1;
    setTimeout(() => demoEmit({ attempt, type: "prompt", instructions: "Autorize no navegador do sistema.", url: "https://example.test/device", user_code: "ABCD-1234" }), 100);
    setTimeout(() => demoEmit({ attempt, type: "finished", params: {} }), 500);
    return attempt;
  },
  login_submit_code: async () => { throw "Login indisponível na demonstração do navegador."; },
  login_cancel: async () => {},
  delete_account: async ({ index, purge }) => {
    const oauth = demoReports[index]?.provider === "kiro" || demoReports[index]?.uses_oauth;
    demoReports.splice(index, 1);
    demoSettings.generation++;
    return { outcome: !oauth ? "removed" : purge ? "purged" : "kept", other_account: null, message: null };
  },
  set_refresh_seconds: async ({ seconds }) => {
    if (seconds < 30) throw JSON.stringify({ id: "edit_error.config_bounds", params: {}, fallback: "refresh_seconds deve ser >= 30." });
    demoSettings.refresh = seconds;
    demoSettings.generation++;
  },
  move_account: async ({ from, to }) => {
    if (from < 0 || to < 0 || from >= demoReports.length || to >= demoReports.length) throw "Conta inexistente.";
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
  model_support: async () => ({ supported: false, text: { id: "models.demo", params: {}, fallback: "Demonstração no navegador: o catálogo de modelos só é consultado no app desktop." } }),
  config_changed_on_disk: async () => false,
  get_settings: async () => ({ user_dir: demoSettings.custom ?? "~/.config/cuotai", default_dir: "~/.config/cuotai", custom_dir: demoSettings.custom, env_locked: false, config_file: demoSettings.config, config_env_locked: false, motion: get("cuotai.motion", "system"), show_pace: get(PACE_KEY, "on") !== "off",
    theme: get("cuotai.theme", "") || null, theme_mode: get("cuotai.themeMode", "system"), sort: get(SORT_KEY, "custom"), group_recharging: get(GROUP_KEY, "off") === "on", locale: get("cuotai.locale", "") || null, local_storage_migrated: true,
    history_days: demoSettings.historyDays ?? 90, history_days_min: 7, history_days_max: 365, history_recording: demoSettings.historyRecording, status_queries: demoSettings.statusQueries,
    notify: { ...demoSettings.notify }, notify_leads: [0, 5, 15, 60], notify_whens: ["used", "exhausted", "always"],
    alerts: structuredClone(demoSettings.alerts), alert_min: 10, alert_max: 99, version: "demo" }),
  set_user_dir: async ({ path }) => { demoSettings.custom = path ?? null; },
  set_motion: async ({ motion }) => { if (!MOTIONS.includes(motion)) throw "invalid_motion"; },
  set_show_pace: async () => {},
  set_preference: async ({ key, value }) => {
    const ok = { theme: typeof value === "string" && /^[\w.-]{1,64}$/.test(value), theme_mode: THEME_MODE_IDS.includes(value), sort: SORT_MODES.includes(value), group_recharging: typeof value === "boolean" };
    if (!ok[key]) throw "invalid_preference";
    mirrorPreference(key, value);
    await emitSettingsChanged(key);
  },
  migrate_local_storage: async () => demo.get_settings(),
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
    if (demoSettings.serve.secret == null) throw JSON.stringify({ id: "serve.error.token_missing", params: {}, fallback: "O token do servidor local não está mais no cofre do sistema; gere um novo ou revogue o token." });
    return demoSettings.serve.secret;
  },
  cost_status: async () => demoCostView(),
  cost_usage: async () => demoCostUsage(),
  cost_prices: async () => demoPrices(),
  cost_set_paths: async ({ source, paths }) => {
    const chosen = [...new Set(paths.map((p) => p.trim()).filter(Boolean))];
    if (chosen.some((p) => !(p.startsWith("/") || p === "~" || p.startsWith("~/")))) {
      throw JSON.stringify(demoL10n("cost.error.relative_path", {}, "Use um diretório absoluto ou que comece com ~/."));
    }
    if (JSON.stringify(chosen) === JSON.stringify(demoCost.paths[source] ?? [])) return demoCostView();
    if (source in demoCost.on) { delete demoCost.on[source]; demoCost.erased.add(source); }
    if (chosen.length) demoCost.paths[source] = chosen; else delete demoCost.paths[source];
    return demoCostView();
  },
  cost_enable: async ({ source, digest }) => {
    if (!DEMO_COST[source] || digest !== demoDisclosure(source).digest) {
      throw JSON.stringify(demoL10n("cost.error.consent_changed", {}, "Os diretórios desta fonte mudaram desde que foram exibidos; revise e confirme de novo."));
    }
    demoCost.on[source] = new Date().toISOString();
    demoCost.erased.delete(source);
    return demoCostView();
  },
  cost_disable: async ({ source }) => { delete demoCost.on[source]; demoCost.erased.add(source); return demoCostView(); },
  cost_set_options: async ({ period, retentionDays }) => {
    if (!(Number.isInteger(retentionDays) && retentionDays >= 7 && retentionDays <= 730)) {
      throw JSON.stringify(demoL10n("cost.error.retention_bounds", { min: "7", max: "730" }, "A retenção do índice de custo vai de 7 a 730 dias."));
    }
    Object.assign(demoCost, { period, retention: retentionDays });
    return demoCostView();
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
      demoHooks.runs.unshift({ ...run, summary: demoL10n("hooks.run.succeeded", { rule: name, event: run.event, ms: 42 }, `${name} (${run.event}): concluído em 42 ms`) });
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
  set_notify_options: async ({ leadMinutes, when, includeOther }) => {
    if (![0, 5, 15, 60].includes(leadMinutes) || !["used", "exhausted", "always"].includes(when)) throw "invalid_notify";
    Object.assign(demoSettings.notify, { lead_minutes: leadMinutes, when, include_other: includeOther });
  },
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
  set_config_file: async ({ path }) => { demoSettings.config = path ?? null; demoSettings.generation++; return demoView(); },
  open_user_dir: async () => {},
  // Demonstração: amostra do histórico (no app, o core entrega o changelog embutido).
  whats_new: async () => ({
    current: "demo",
    title: { id: "changelog.title", params: {}, fallback: "Novidades" },
    entries_language: { id: "changelog.entries_language", params: {}, fallback: "As entradas são mantidas em inglês, o idioma do changelog." },
    releases: [
      { version: "demo", date: "2026-10-25", current: true, paragraphs: [], categories: [
        { label: { id: "changelog.category.added", params: {}, fallback: "Adicionado" }, entries: ["Sample entry for the What's new screen in the browser demo."] },
      ] },
    ],
    history: null,
  }),
  // Demonstração: amostra da lista real (no app, a lista completa vem do core).
  // Demonstração: o texto do relato é do core; aqui, um exemplo sem dado algum.
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

// ---------- ordem das contas ----------
// O modo personalizado segue `report.index`, a ordem persistida no TOML pelo
// core e compartilhada com a TUI. Só o modo de ordenação é preferência local.
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
export const splitRecharging = (list) => (groupRecharging ? [list.filter((r) => !isRecharging(r)), list.filter(isRecharging)] : [list, []]);
// PRD 0007 RF3.4: consumption pace, on by default; `desktop.json` is the source, localStorage mirrors it for windows.
const PACE_KEY = "cuotai.pace";
let showPace = get(PACE_KEY, "on") !== "off";
const applyPace = (on) => { showPace = on !== false; put(PACE_KEY, showPace ? "on" : "off"); };
export const paceShown = () => showPace;
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
  const parts = short ? [tl(p.outlook ?? p.label)]
    : [tl(p.label), expected ? t("metric.pace.expected", { pct: Math.round(p.expected_used_fraction * 100) }) : "", p.outlook ? tl(p.outlook) : ""];
  const text = parts.filter(Boolean).join(" · ");
  return label ? `${label}: ${text}` : text;
}
const PACE_ICON = { good: "check", neutral: "clock", warn: "warn", bad: "xcircle" };
/** Pace line: tone icon plus text (never color alone); the tooltip says it is a linear estimate. */
export function paceHtml(p, opts) {
  const text = paceText(p, opts);
  return `<p class="pace-line" data-pace="${esc(p.stage)}" title="${esc(`${text}. ${t("card.pace.estimate")}`)}">${icon(PACE_ICON[p.tone] ?? "clock", p.tone)}<span>${esc(text)}</span></p>`;
}
export function initOrderSync(onChange) {
  onPreferencesChanged((changed) => {
    if (["sort", "group_recharging", "show_pace"].some((k) => changed.has(k))) onChange();
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
  applyMotion(s.motion);
  if (was.motion !== (s.motion || "system")) changed.add("motion");
  applyPace(s.show_pace);
  if (was.pace !== showPace) changed.add("show_pace");
  for (const [key, value] of [["theme", theme.id], ["theme_mode", theme.mode], ["sort", sortMode], ["group_recharging", groupRecharging]]) mirrorPreference(key, value);
  if (s.locale && boot) mirrorPreference("locale", s.locale); // the language loads right after: it picks the mirror up
  else if (s.locale && s.locale !== currentLocale()) { await setLocale(s.locale); changed.add("locale"); }
  return changed;
}
const THEME_MODE_IDS = ["system", "light", "dark"];
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

/** Resumo da conta: a quota mais apertada (menor % disponível; empate → reinício mais próximo), ou o texto de
 *  uma métrica sem limite, ou o motivo de não haver resumo. */
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
/** Janelas de 5h e semanal da conta (a classificação vem do core: `metric.window`). Em cada janela vale a quota mais
 *  apertada, entre as não ocultas; null quando a conta não tem a janela. */
export function windowsOf(r) {
  const when = (m) => (m.resets_at ? new Date(m.resets_at).getTime() : Infinity);
  const pick = (w) => planMetrics(r).filter((m) => m.window === w).map((m) => ({ m, pct: remainingPct(m) })).filter((x) => x.pct != null)
    .sort((a, b) => a.pct - b.pct || when(a.m) - when(b.m))[0] ?? null;
  return { h5: pick("window_5h"), week: pick("weekly") };
}
/** Pior % disponível entre as janelas de rate limit (não ocultas); null quando não há. */
export function rateLimitOf(r) {
  const v = r.metrics.filter((m) => m.kind === "rate_limit" && !isMetricHidden(r, m)).map(remainingPct).filter((x) => x != null);
  return v.length ? Math.min(...v) : null;
}

/** Instante (ms) do reinício mais próximo entre as métricas da conta; null quando nenhuma informa. */
function soonestReset(r) {
  const now = Date.now();
  const s = summaryOf(r);
  if (s.m?.resets_at) { const t0 = new Date(s.m.resets_at).getTime(); if (Number.isFinite(t0) && t0 > now) return t0; } // o mesmo reinício que o card mostra
  const times = planMetrics(r).map((m) => (m.resets_at ? new Date(m.resets_at).getTime() : NaN)).filter((x) => Number.isFinite(x) && x > now);
  return times.length ? Math.min(...times) : null;
}
/** Lista na ordem escolhida. A personalizada é a ordem do arquivo; fora dela, contas sem o dado vão para o fim. */
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
/** Botão + menu de ordenação (personalizada, reinício, % disponível). */
export function sortMenuHtml() {
  const label = (m) => (m === "custom" ? t("sort.custom") : t(`sort.${m}`));
  const items = SORT_MODES.map((m) => `<button role="menuitemradio" aria-checked="${m === sortMode}" class="${m === sortMode ? "on" : ""}" data-sort="${m}">${label(m)}</button>`).join("");
  return `<div class="sortmenu lang">
    <button class="icon" data-sort-toggle title="${te("sort.label")}: ${esc(label(sortMode))}" aria-label="${te("sort.label")}" aria-haspopup="menu" aria-expanded="${sortOpen}">${icon("sort")}</button>
    <div class="lang-menu" role="menu" ${sortOpen ? "" : "hidden"}>${items}</div>
  </div>`;
}

// ---------- categorias de aviso (cards e popover): ícone dedicado e cor própria, da mais grave à menos grave ----------
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

// Kiro de demonstração: um modo de login (oauth) e os campos de credencial, como no core.
const dl = (id, fallback) => ({ id, params: {}, fallback });
// Default-mode level per provider, as `access::describe` classifies them (demo only).
const DEMO_LEVELS = {
  derived: ["gemini_api", "vertex_ai", "groq"],
  unavailable: ["qwen", "perplexity", "cloudflare_ai", "gemini_ai", "nvidia_nim", "poolside", "alibaba_model_studio"],
  official: ["open_ai_api", "anthropic_api", "aws_bedrock", "azure_open_ai", "cursor", "copilot", "windsurf", "kimi", "mistral", "grok", "cline", "alibaba_cloud"],
};
// Providers without a usage query in any mode (`access::has_usage_query`, demo only): left out of the picker.
const DEMO_NO_QUERY = ["qwen", "cloudflare_ai", "nvidia_nim", "poolside", "alibaba_model_studio"];
const demoLevelOf = (provider) => Object.keys(DEMO_LEVELS).find((level) => DEMO_LEVELS[level].includes(provider)) ?? "undocumented";
const demoLevel = (level) => dl(`access.level.${level}`, { official: "Oficial", undocumented: "Não documentada", derived: "Derivada", unavailable: "Indisponível" }[level]);
function demoKiroSchema() {
  const opts = (key, items) => items.map(([value, label], index) => ({ id: `${key}.${value}`, index, value, label: dl(`${key}.${value}`, label) }));
  return {
    credential_kind: "env", default_env: "KIRO_ACCESS_TOKEN", credential_help: dl("credential_help.kiro", "Access token Kiro (demo)."),
    login: { unsupported_reason: null, modes: [{ id: "oauth", label: dl("login.oauth.label", "Login no navegador"), help: dl("login.oauth.help", "Login nativo no navegador do sistema."),
      completion_notice: dl("login.oauth.completion", "Login autorizado (demo)."),
      credential_store: "cuotai_vault", account_binding: { type: "none" },
      registration: [], unavailable_reason: null,
      options: [
        { key: "auth_method", label: dl("o.auth_method", "Método de autenticação"), help: dl("o.auth_method.h", "builder-id ou idc."), required: false, validation: "none", choices: opts("o.auth_method", [["builder-id", "AWS Builder ID"], ["idc", "IAM Identity Center"]]) },
        { key: "region", label: dl("o.region", "Região (opcional)"), help: dl("o.region.h", ""), required: false, validation: "no_whitespace", choices: null },
        { key: "start_url", label: dl("o.start_url", "URL de início IDC (opcional)"), help: dl("o.start_url.h", ""), required: false, validation: "no_whitespace", choices: null },
      ] }] },
    fields: [
      { key: "auth_method", label: dl("f.auth_method", "Método de autenticação"), help: dl("f.auth_method.h", "Origem do access token."), required: false,
        options: [
          ...opts("f.auth_method", [["builder-id", "AWS Builder ID (padrão)"], ["idc", "IAM Identity Center"]]).map((option) => ({ ...option, auth_modes: ["oauth"], unavailable_reason: dl(`${option.id}.unavailable`, "Disponível somente no login nativo.") })),
          ...opts("f.auth_method", [["google", "Google"], ["github", "GitHub"], ["import", "Importado"], ["api_key", "API key"], ["external_idp", "IdP externo"]]).map((option, offset) => ({ ...option, index: offset + 2, auth_modes: ["credential"], unavailable_reason: dl(`${option.id}.unavailable`, "Disponível somente com credencial de ambiente.") })),
        ] },
      { key: "profile_arn", label: dl("f.profile_arn", "ARN do perfil CodeWhisperer (opcional)"), help: dl("f.profile_arn.h", ""), required: false, options: null },
    ],
  };
}
