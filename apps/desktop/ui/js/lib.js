import { DEFAULT_THEME, GROUP_ORDER, THEMES, effectiveMode, themeById } from "./themes.js";
import { LOCALES, currentLocale, setLocale, t, te, tl } from "./i18n.js";
import { esc } from "./util.js";
import { flagCountry, flagSvg } from "./flags.js";
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
export const formSchema = (provider) => call("form_schema", { provider });
export const visibleFields = (provider, choices, authMode, loginParams = {}) => call("visible_fields", { provider, choices, authMode, loginParams });
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
export const saveAccount = (input) => call("save_account", { input });
export const deleteAccount = (index, purge = false) => call("delete_account", { index, purge });
export const setRefreshSeconds = (seconds) => call("set_refresh_seconds", { seconds });
export const moveAccount = (from, to) => call("move_account", { from, to });
export const getSettings = () => call("get_settings");
export const setUserDir = (path) => call("set_user_dir", { path });
export const openUserDir = () => call("open_user_dir");
export const openSettingsWindow = () => isDesktop && T.core.invoke("open_settings");
/** Seletor nativo de pasta (plugin de diálogo do Tauri). */
export async function pickFolder(defaultPath) {
  if (isDesktop && T.dialog) return T.dialog.open({ directory: true, multiple: false, defaultPath: defaultPath || undefined });
  return "/Users/demo/CuotaiUser"; // navegador (demo)
}
export const setTrayMenu = (payload) => isDesktop && T.core.invoke("set_tray_menu", { payload });
export const openMainWindow = () => isDesktop && T.core.invoke("show_main_window");
export const quitApp = () => isDesktop && T.core.invoke("quit_app");

// ---------- formatação ----------
/** Percentual restante (0–100) segundo a classificação do core; null = sem medida/limite. */
export function remainingPct(m) {
  if (m.state === "exhausted") return 0;
  if (m.state === "available" && m.ratio != null) return m.ratio * 100;
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
export function fmtReset(iso) {
  if (!iso) return null;
  const d = new Date(iso);
  if (Number.isNaN(d.getTime())) return null;
  const time = d.toLocaleTimeString(currentLocale(), { hour: "2-digit", minute: "2-digit" });
  if (d.toDateString() === new Date().toDateString()) return t("time.today", { time });
  return `${d.toLocaleDateString(currentLocale(), { day: "2-digit", month: "short" })}, ${time}`;
}
export const statusLabel = (status) => t(`status.${status}`);
/** Quotas que definem o resumo e a ordenação da conta: sem a janela curta de rate limit (não é a cota do
 *  plano) e sem as que o usuário ocultou. */
export const planMetrics = (r) => r.metrics.filter((m) => m.kind !== "rate_limit" && !isMetricHidden(r, m));
export const worstRemaining = (r) => {
  const v = planMetrics(r).map(remainingPct).filter((x) => x != null);
  return v.length ? Math.min(...v) : null;
};
export const reportKey = (r) => `${r.provider}::${r.account}`;
/** Resolve the structured note sidecar by the same index as the legacy fallback. */
export const noteText = (report, index) =>
  report.notes_l10n?.[index] ? tl(report.notes_l10n[index]) : report.notes[index] ?? "";
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
};
export const brand = (p) => brands[p] ?? B(p, "#888888");
// Logos de provider (9router, MIT; ver img/providers/NOTICE.md). Sem arquivo/erro: sigla colorida.
const images = {
  open_ai_api: "openai.png", anthropic_api: "anthropic.png", chat_gpt: "codex.png", claude: "claude.png",
  gemini_api: "gemini.png", vertex_ai: "vertex.png", aws_bedrock: "bedrock.png", azure_open_ai: "azure.png",
  cursor: "cursor.png", copilot: "copilot.png", windsurf: "windsurf.png", mini_max: "minimax.png",
  kimi: "kimi.png", qwen: "qwen.png", groq: "groq.png", perplexity: "perplexity.png", mistral: "mistral.png",
  cloudflare_ai: "cloudflare-ai.png", ollama_cloud: "ollama.png", xiaomi_mimo: "xiaomi-mimo.png",
  open_code_go: "opencode-go.png", qoder: "qoder.png", gemini_ai: "gemini-cli.png", grok: "grok-cli.png",
  nvidia_nim: "nvidia.png", poolside: "poolside.png", kiro: "kiro.png", cline: "cline.png",
  code_buddy: "codebuddy-intl.png", zed: "zed.png", alibaba_model_studio: "alims-intl.png",
  alibaba_coding_plan: "alicode-intl.png", alibaba_token_plan: "alitp-intl.png", alibaba_cloud: "alims-intl.png",
  antigravity: "antigravity.png", kimchi: "kimchi.svg",
};
export const logo = (provider, size = 40) => {
  const b = brand(provider);
  const file = images[provider];
  const img = file
    ? `<img src="img/providers/${file}" alt="" draggable="false" onerror="this.parentElement.classList.add('fb');this.remove()" />`
    : "";
  return `<span class="logo ${file ? "" : "fb"}" style="width:${size}px;height:${size}px;font-size:${size * 0.4}px;--c:${b.color}" aria-hidden="true">${img}<b>${esc(b.mark)}</b></span>`;
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
}
applyTheme();
export function initTheme(onChange = () => {}) {
  matchMedia("(prefers-color-scheme: dark)").addEventListener("change", () => { applyTheme(); onChange(); });
  addEventListener("storage", (e) => {
    if (e.key === "cuotai.themeMode" && e.newValue) theme.mode = e.newValue;
    else if (e.key === "cuotai.theme" && e.newValue) theme.id = e.newValue;
    else if (e.key === "cuotai.locale" && e.newValue) { setLocale(e.newValue).then(onChange); return; }
    else return;
    applyTheme(); onChange();
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

/** Delegação de eventos do seletor; `rerender` é chamado após a troca. */
export function bindThemeSwitch(root, rerender) {
  root.addEventListener("click", (e) => {
    const b = e.target.closest("[data-theme-mode]");
    if (!b || b.disabled) return;
    theme.mode = b.dataset.themeMode; put("cuotai.themeMode", theme.mode); applyTheme(); rerender();
  });
  root.addEventListener("change", async (e) => {
    if (e.target.matches("[data-theme-id]")) {
      theme.id = themeById(e.target.value).id; put("cuotai.theme", theme.id); applyTheme(); rerender();
    }
  });
  root.addEventListener("click", async (e) => {
    if (e.target.closest("[data-sort-toggle]")) { sortOpen = !sortOpen; langOpen = false; return rerender(); }
    const sortItem = e.target.closest("[data-sort]");
    if (sortItem) { sortMode = sortItem.dataset.sort; put(SORT_KEY, sortMode); sortOpen = false; return rerender(); }
    if (e.target.closest("[data-lang-toggle]")) { langOpen = !langOpen; sortOpen = false; return rerender(); }
    const item = e.target.closest("[data-locale]");
    if (item) { langOpen = false; await setLocale(item.dataset.locale); return rerender(); }
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
  s.refreshOne = (i) => refreshAccount(i).catch((e) => { s.error = String(e); render(); });
  if (isDesktop) T.event.listen("snapshot", (e) => set(e.payload));
  queueMicrotask(() => getView().then(set).catch((e) => { s.error = String(e); render(); }));
  setInterval(render, 30000); // countdowns
  return s;
}

// ---------- demo (só no navegador; mesma forma dos comandos Tauri) ----------
const h = (n) => new Date(Date.now() + n * 3600e3).toISOString();
const dm = (label, used, limit, resetH, unit = "solicitações") => {
  const unmeasured = used === 0 && limit === 0;
  const exhausted = !unmeasured && limit > 0 && used >= limit;
  return { label, used, limit, remaining: limit - used, unit, period: null, resets_at: h(resetH),
    state: unmeasured ? "unmeasured" : exhausted ? "exhausted" : "available",
    ratio: unmeasured || exhausted ? (exhausted ? 0 : null) : (limit - used) / limit, uncapped: false, no_cap: false,
    kind: /rate ?limit/i.test(label) ? "rate_limit" : "quota",
    window: /rate ?limit/i.test(label) ? null : /5 ?h|5 hora|sess/i.test(label) ? "5h" : /7 ?d|seman|week/i.test(label) ? "week" : null };
};
const dr = (account, provider, metrics, notes = [], status = "ready") => ({
  account, provider, checked_at: new Date().toISOString(), status, metrics, notes, source: "demo",
  class: "available", refresh_seconds: 300, uses_oauth: false, identity: { email: null, badges: [], copy: null } });
const demoReports = [
  dr("alex@example.com", "antigravity", [dm("Gemini (5h)", 15, 1000, 0.37), dm("Claude & GPT (5h)", 0, 1000, 5), dm("Gemini (Weekly)", 26, 1000, 151), dm("Claude & GPT (Weekly)", 0, 1000, 151.9)]),
  dr("Account 2", "claude", [dm("session (5h)", 0, 100, 4.5, "%"), dm("weekly (7d)", 41, 100, 24.6, "%")]),
  dr("alex@example.com", "chat_gpt", [dm("5h", 6, 100, 67.6, "%")]),
  { ...dr("jmarcon", "copilot", [dm("chat", 0, 0, 2.6), dm("premium_interactions", 7026, 7000, 2.6)]), class: "exhausted" },
  dr("Personal", "groq", [], ["Groq connected. No rate-limit data reported for this key yet."]),
  dr("Personal", "mini_max", [dm("M-series (5h)", 6, 100, 2.6), dm("M-series (7d)", 1, 100, 98.6), dm("Video (5h)", 0, 100, 2.6)]),
  dr("Account 2", "code_buddy", [dm("Monthly", 0, 100, 757), dm("Bonus Pack 1", 0, 250, 62)]),
  dr("Account 1", "kimi", [dm("Weekly", 100, 100, 109), dm("Ratelimit", 0, 100, 4.8)]),
  dr("Personal", "ollama_cloud", [dm("Monthly", 0, 100, 248)]),
  dr("Personal", "anthropic_api", [], ["Credencial ausente ou expirada."], "authentication_error"),
  dr("Pessoal", "mistral", [], ["HTTP 429: limite de requisições atingido."], "rate_limited"),
  dr("Dev", "cursor", [], ["Serviço indisponível no momento."], "error"),
  dr("Work", "windsurf", [], ["Informe a variável de ambiente da credencial."], "needs_configuration"),
  { ...dr("Account 3", "kiro", [dm("credit", 596.15, 10000, 360, "créditos")]),
    identity: { email: null, copy: { label: "ARN do perfil", value: "arn:aws:codewhisperer:us-east-1:123456789012:profile/EXAMPLEPROFILE" },
      badges: [{ tone: "accent", text: "IAM Identity Center" }, { tone: "info", text: "us-east-1" }, { tone: "good", text: "ativa" }] } },
  { ...dr("alex@example.com", "grok", [dm("On-demand", 1, 1, 100)]), identity: { email: "alex@example.com", badges: [], copy: null } },
  dr("Personal", "deepseek_demo", [{ label: "Balance", used: null, limit: null, remaining: 0, unit: "USD", period: null, resets_at: null, state: "available", ratio: null, uncapped: true, no_cap: false, kind: "quota" }]),
];
const demoView = () => {
  const now = new Date().toISOString();
  return { config_path: "~/cuotai.toml (demo)", refresh_seconds: demoSettings.refresh ?? 300, config_exists: true, account_count: demoReports.length, loaded: true,
    started_at: now, finished_at: now, reports: demoReports.map((r, index) => ({ ...r, index, next_refresh_at_ms: Date.now() + (40 + index * 37) * 1000, uses_oauth: r.provider === "kiro" || r.uses_oauth })), busy: [], error: null };
};
const demoSettings = { custom: null };
const demo = {
  get_view: async () => demoView(),
  refresh_all: async () => {},
  refresh_account: async () => {},
  list_providers: async () => Object.entries(brands).map(([id, b]) => ({ id, label: b.label })),
  form_schema: async ({ provider }) => provider === "kiro" ? demoKiroSchema() : ({ credential_kind: "env", default_env: "DEMO_API_KEY",
    credential_help: { id: "credential_help.demo", params: {}, fallback: "Nome da variável de ambiente com a credencial (demo)." },
    login: { modes: [{ id: "oauth", label: { id: "login.demo.oauth.label", params: {}, fallback: "Login OAuth (demo)" }, help: { id: "login.demo.oauth.help", params: {}, fallback: "No navegador de demonstração o login não executa." }, completion_notice: { id: "login.demo.oauth.completion", params: {}, fallback: "Login autorizado (demo)." }, credential_store: "cuotai_vault", account_binding: { type: "none" }, registration: [], options: [], unavailable_reason: { id: "login.demo.oauth.unavailable_reason", params: {}, fallback: "Login indisponível na demonstração." } }], unsupported_reason: null },
    cost_advisory: null,
    fields: [{ id: "field.demo.project_id", key: "project_id", label: { id: "field.demo.project_id.label", params: {}, fallback: "Project ID" }, help: { id: "field.demo.project_id.help", params: {}, fallback: "Opcional." }, required: false, options: null },
             { id: "field.demo.mode", key: "mode", label: { id: "field.demo.mode.label", params: {}, fallback: "Modo" }, help: { id: "field.demo.mode.help", params: {}, fallback: "" }, required: false, options: [{ id: "field.demo.mode.option.personal", value: "personal", label: { id: "field.demo.mode.option.personal", params: {}, fallback: "Pessoal" } }, { id: "field.demo.mode.option.org", value: "org", label: { id: "field.demo.mode.option.org", params: {}, fallback: "Organização" } }] }] }),
  visible_fields: async ({ provider, authMode }) => provider === "kiro"
    ? ["auth_method", "profile_arn"].filter((k) => !(authMode === "oauth" && k === "auth_method"))
    : ["project_id", "mode"],
  get_account: async ({ index }) => ({ name: demoReports[index].account, provider: demoReports[index].provider, credential: "DEMO_API_KEY",
    interval: "", uses_oauth: false, native_login: false, auth_mode: "credential", auth_options: {}, auth_registration: {}, fields: [{ key: "project_id", text: "", choice: null }, { key: "mode", text: null, choice: 0 }] }),
  save_account: async ({ input }) => {
    window.__lastSaveRequest = input;
    if (!input.name.trim()) throw "Informe um nome para a conta.";
    const r = dr(input.name, input.provider, [dm("Exemplo", 10, 100, 5)]);
    input.index == null ? demoReports.push(r) : (demoReports[input.index] = r);
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
    return { outcome: !oauth ? "removed" : purge ? "purged" : "kept", other_account: null, message: null };
  },
  set_refresh_seconds: async ({ seconds }) => {
    if (seconds < 30) throw JSON.stringify({ id: "edit_error.config_bounds", params: {}, fallback: "refresh_seconds deve ser >= 30." });
    demoSettings.refresh = seconds;
  },
  move_account: async ({ from, to }) => {
    if (from < 0 || to < 0 || from >= demoReports.length || to >= demoReports.length) throw "Conta inexistente.";
    const [account] = demoReports.splice(from, 1);
    demoReports.splice(to, 0, account);
  },
  get_settings: async () => ({ user_dir: demoSettings.custom ?? "~/.config/cuotai", default_dir: "~/.config/cuotai", custom_dir: demoSettings.custom, env_locked: false, version: "demo" }),
  set_user_dir: async ({ path }) => { demoSettings.custom = path ?? null; },
  open_user_dir: async () => {},
};

// ---------- ordem das contas ----------
// O modo personalizado segue `report.index`, a ordem persistida no TOML pelo
// core e compartilhada com a TUI. Só o modo de ordenação é preferência local.
const SORT_KEY = "cuotai.sort";
export const SORT_MODES = ["custom", "reset-asc", "reset-desc", "pct-asc", "pct-desc"];
let sortMode = SORT_MODES.includes(get(SORT_KEY, "custom")) ? get(SORT_KEY, "custom") : "custom";
export const currentSort = () => sortMode;
export function initOrderSync(onChange) {
  addEventListener("storage", (e) => {
    if (e.key === SORT_KEY && SORT_MODES.includes(e.newValue)) sortMode = e.newValue;
    else return;
    onChange();
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
  const planAll = r.metrics.filter((m) => m.kind !== "rate_limit");
  return { kind: planAll.length && !quotas.length ? "hidden" : "none" };
}
/** Janelas de 5h e semanal da conta (a classificação vem do shell: `metric.window`). Em cada janela vale a quota mais
 *  apertada, entre as não ocultas; null quando a conta não tem a janela. */
export function windowsOf(r) {
  const when = (m) => (m.resets_at ? new Date(m.resets_at).getTime() : Infinity);
  const pick = (w) => planMetrics(r).filter((m) => m.window === w).map((m) => ({ m, pct: remainingPct(m) })).filter((x) => x.pct != null)
    .sort((a, b) => a.pct - b.pct || when(a.m) - when(b.m))[0] ?? null;
  return { h5: pick("5h"), week: pick("week") };
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
export const ALERTS = [
  { id: "error", icon: "xcircle", tone: "bad", label: () => t("status.error"), match: (r) => r.status === "error" },
  { id: "auth", icon: "key", tone: "accent", label: () => t("status.authentication_error"), match: (r) => r.status === "authentication_error" },
  { id: "exhausted", icon: "battery", tone: "bad", label: () => t("tray.cat.exhausted"), match: (r) => r.status === "ready" && r.class === "exhausted" },
  { id: "rate", icon: "gauge", tone: "warn", label: () => t("status.rate_limited"), match: (r) => r.status === "rate_limited" },
  { id: "config", icon: "wrench", tone: "info", label: () => t("status.needs_configuration"), match: (r) => r.status === "needs_configuration" },
  { id: "unsupported", icon: "ban", tone: "muted", label: () => t("status.unsupported"), match: (r) => r.status === "unsupported" },
];
export const alertOf = (r) => ALERTS.find((c) => c.match(r)) ?? null;

// Kiro de demonstração: um modo de login (oauth) e os campos de credencial, como no core.
const dl = (id, fallback) => ({ id, params: {}, fallback });
function demoKiroSchema() {
  const opts = (key, items) => items.map(([value, label]) => ({ id: `${key}.${value}`, value, label: dl(`${key}.${value}`, label) }));
  return {
    credential_kind: "env", default_env: "KIRO_ACCESS_TOKEN", credential_help: dl("credential_help.kiro", "Access token Kiro (demo)."), cost_advisory: null,
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
        options: opts("f.auth_method", [["builder-id", "AWS Builder ID (padrão)"], ["idc", "IAM Identity Center"], ["google", "Google"], ["github", "GitHub"]]) },
      { key: "profile_arn", label: dl("f.profile_arn", "ARN do perfil CodeWhisperer (opcional)"), help: dl("f.profile_arn.h", ""), required: false, options: null },
    ],
  };
}
