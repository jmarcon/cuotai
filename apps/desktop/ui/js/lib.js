// Camada compartilhada (sem framework, sem bundler). Espelha cuotai_core::model.

// ---------- util ----------
export const esc = (s) =>
  String(s ?? "").replace(/[&<>"']/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" })[c]);

// Ícones (paths do Lucide, ISC).
const paths = {
  refresh: '<path d="M3 12a9 9 0 0 1 9-9 9.75 9.75 0 0 1 6.74 2.74L21 8"/><path d="M21 3v5h-5"/><path d="M21 12a9 9 0 0 1-9 9 9.75 9.75 0 0 1-6.74-2.74L3 16"/><path d="M8 16H3v5"/>',
  edit: '<path d="M21.17 6.81a1 1 0 0 0-3.99-3.99L3.84 16.17a2 2 0 0 0-.5.83l-1.32 4.35a.5.5 0 0 0 .62.62l4.35-1.32a2 2 0 0 0 .83-.5z"/>',
  trash: '<path d="M3 6h18"/><path d="M19 6v14a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V6"/><path d="M8 6V4a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v2"/>',
  eye: '<path d="M2.06 12.35a1 1 0 0 1 0-.7 10.75 10.75 0 0 1 19.88 0 1 1 0 0 1 0 .7 10.75 10.75 0 0 1-19.88 0"/><circle cx="12" cy="12" r="3"/>',
  eyeOff: '<path d="M10.73 5.08A10.43 10.43 0 0 1 12 5c7 0 10 7 10 7a13.16 13.16 0 0 1-1.67 2.68"/><path d="M6.61 6.61A13.53 13.53 0 0 0 2 12s3 7 10 7a9.74 9.74 0 0 0 5.39-1.61"/><line x1="2" x2="22" y1="2" y2="22"/>',
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
const invoke = (cmd, args) => T.core.invoke(cmd, args);
export const fetchSnapshot = () => (isDesktop ? invoke("get_snapshot") : demoSnapshot());
export const fetchAccount = async (index) => (isDesktop ? invoke("refresh_account", { index }) : (await demoSnapshot()).reports[index]);
export const openMainWindow = () => isDesktop && invoke("show_main_window");
export const quitApp = () => isDesktop && invoke("quit_app");

// ---------- formatação ----------
export function remainingPct(m) {
  if (m.limit != null && m.limit > 0) {
    const left = m.remaining ?? (m.used != null ? m.limit - m.used : null);
    if (left == null) return null;
    return Math.max(0, Math.min(100, (left / m.limit) * 100));
  }
  return null;
}
export const usedPct = (m) => {
  const r = remainingPct(m);
  return r == null ? null : 100 - r;
};
export const level = (p) => (p == null ? "unknown" : p >= 70 ? "good" : p >= 30 ? "warn" : "bad");
export const fmtNumber = (n, max = 2) => (n == null ? "—" : new Intl.NumberFormat("en-US", { maximumFractionDigits: max }).format(n));
export function fmtAmount(m) {
  if (m.limit == null) return m.used == null ? "—" : `${fmtNumber(m.used)} ${m.unit}`.trim();
  return `${fmtNumber(m.used)} / ${fmtNumber(m.limit)}`;
}
export function fmtCountdown(iso) {
  if (!iso) return null;
  let s = Math.floor((new Date(iso).getTime() - Date.now()) / 1000);
  if (Number.isNaN(s)) return null;
  if (s <= 0) return "agora";
  const d = Math.floor(s / 86400); s -= d * 86400;
  const h = Math.floor(s / 3600); s -= h * 3600;
  const m = Math.floor(s / 60);
  return d > 0 ? `${d}d ${h}h ${m}m` : h > 0 ? `${h}h ${m}m` : `${m}m`;
}
export function fmtReset(iso) {
  if (!iso) return null;
  const d = new Date(iso);
  if (Number.isNaN(d.getTime())) return null;
  const time = d.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });
  if (d.toDateString() === new Date().toDateString()) return `Hoje ${time}`;
  return `${d.toLocaleDateString([], { day: "2-digit", month: "short" })}, ${time}`;
}
export const statusLabel = {
  ready: "Atualizado", needs_configuration: "Configurar", unsupported: "Indisponível",
  authentication_error: "Autenticação", rate_limited: "Rate limit", error: "Erro",
};
export const worstRemaining = (r) => {
  const v = r.metrics.map(remainingPct).filter((x) => x != null);
  return v.length ? Math.min(...v) : null;
};
export const reportKey = (r) => `${r.provider}::${r.account}`;

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
export const logo = (provider, size = 40) => {
  const b = brand(provider);
  return `<span class="logo" style="width:${size}px;height:${size}px;background:${b.color};font-size:${size * 0.4}px" aria-hidden="true">${esc(b.mark)}</span>`;
};

// ---------- tema (compartilhado entre janelas via localStorage + evento storage) ----------
// Novos temas escuros: adicione aqui e em css/themes.css ([data-dark="<id>"]).
export const DARK_THEMES = [{ id: "dracula", label: "Dracula" }];
const get = (k, d) => { try { return localStorage.getItem(k) || d; } catch { return d; } };
const put = (k, v) => { try { localStorage.setItem(k, v); } catch {} };
export const theme = { mode: get("cuotai.themeMode", "system"), dark: get("cuotai.darkTheme", "dracula") };
export function applyTheme() {
  const dark = theme.mode === "dark" || (theme.mode === "system" && matchMedia("(prefers-color-scheme: dark)").matches);
  document.documentElement.dataset.theme = dark ? "dark" : "light";
  document.documentElement.dataset.dark = theme.dark;
}
applyTheme();
export function initTheme(onChange = () => {}) {
  matchMedia("(prefers-color-scheme: dark)").addEventListener("change", applyTheme);
  addEventListener("storage", (e) => {
    if (e.key === "cuotai.themeMode" && e.newValue) theme.mode = e.newValue;
    else if (e.key === "cuotai.darkTheme" && e.newValue) theme.dark = e.newValue;
    else return;
    applyTheme(); onChange();
  });
}
export function themeSwitchHtml() {
  const modes = [["system", "monitor", "Sistema"], ["light", "sun", "Claro"], ["dark", "moon", "Escuro"]];
  return `<div class="theme-switch">
    <div class="segmented" role="radiogroup" aria-label="Modo do tema">${modes.map(([id, ic, label]) =>
      `<button role="radio" aria-checked="${theme.mode === id}" class="${theme.mode === id ? "on" : ""}" title="${label}" data-theme-mode="${id}">${icon(ic)}</button>`).join("")}</div>
    <select aria-label="Tema escuro" data-dark-theme>${DARK_THEMES.map((t) => `<option value="${t.id}" ${theme.dark === t.id ? "selected" : ""}>${t.label}</option>`).join("")}</select>
  </div>`;
}
/** Delegação de eventos do seletor; `rerender` é chamado após a troca. */
export function bindThemeSwitch(root, rerender) {
  root.addEventListener("click", (e) => {
    const b = e.target.closest("[data-theme-mode]");
    if (!b) return;
    theme.mode = b.dataset.themeMode; put("cuotai.themeMode", theme.mode); applyTheme(); rerender();
  });
  root.addEventListener("change", (e) => {
    if (!e.target.matches("[data-dark-theme]")) return;
    theme.dark = e.target.value; put("cuotai.darkTheme", theme.dark); applyTheme(); rerender();
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

// ---------- estado de dados ----------
export function createStore(render, pollMs = 60000) {
  const s = { snapshot: null, error: null, loading: false, busy: new Set() };
  s.refresh = async () => {
    s.loading = true; render();
    try { s.snapshot = await fetchSnapshot(); s.error = null; }
    catch (e) { s.error = typeof e === "string" ? e : e?.message ?? "Falha ao atualizar"; }
    s.loading = false; render();
  };
  s.refreshOne = async (i) => {
    s.busy.add(i); render();
    try { const r = await fetchAccount(i); s.snapshot.reports[i] = r; } catch {}
    s.busy.delete(i); render();
  };
  queueMicrotask(s.refresh); // adia: render() pode referenciar o store ainda não atribuído
  setInterval(s.refresh, pollMs);
  setInterval(render, 30000); // countdowns
  return s;
}

// ---------- demo (só no navegador) ----------
function demoSnapshot() {
  const h = (n) => new Date(Date.now() + n * 3600e3).toISOString();
  const m = (label, used, limit, resetH, unit = "req") => ({ label, used, limit, remaining: limit - used, unit, period: null, resets_at: h(resetH) });
  const now = new Date().toISOString();
  const r = (account, provider, metrics, notes = [], status = "ready") => ({ account, provider, checked_at: now, status, metrics, notes, source: "demo" });
  return Promise.resolve({ started_at: now, finished_at: now, reports: [
    r("juliano.marcon@gmail.com", "antigravity", [m("Gemini (5h)", 15, 1000, 0.37), m("Claude & GPT (5h)", 0, 1000, 5), m("Gemini (Weekly)", 26, 1000, 151), m("Claude & GPT (Weekly)", 0, 1000, 151.9)]),
    r("Account 2", "claude", [m("session (5h)", 0, 100, 4.5, "%"), m("weekly (7d)", 41, 100, 24.6, "%")]),
    r("juliano.marcon@gmail.com", "chat_gpt", [m("5h", 6, 100, 67.6, "%")]),
    r("jmarcon", "copilot", [m("chat", 0, 0, 2.6), m("premium_interactions", 7026, 7000, 2.6)]),
    r("Personal", "groq", [], ["Groq connected. No rate-limit data reported for this key yet."]),
    r("Personal", "mini_max", [m("M-series (5h)", 6, 100, 2.6), m("M-series (7d)", 1, 100, 98.6), m("Video (5h)", 0, 100, 2.6)]),
    r("Account 2", "code_buddy", [m("Monthly", 0, 100, 757), m("Bonus Pack 1", 0, 250, 62)]),
    r("Account 1", "kimi", [m("Weekly", 100, 100, 109), m("Ratelimit", 0, 100, 4.8)]),
    r("Personal", "ollama_cloud", [m("Monthly", 0, 100, 248)]),
    r("Personal", "anthropic_api", [], ["Credencial ausente ou expirada."], "authentication_error"),
  ] });
}
