// Menu do clique direito no ícone da bandeja: texto simples, uma linha por conta ("nome: % disponível").
// As linhas seguem a mesma ordem e os mesmos filtros do popover; o shell só monta o menu nativo.
import { t } from "./i18n.js";
import { ageText, bindingPct, blockedText, brand, disabledAccounts, nextNoticeText, reportKey, setTrayMenu, shownAt, sortReports, statusLabel } from "./lib.js";

let last = "";

const shownReports = (reports) => sortReports(reports).filter((r) => !disabledAccounts.has(reportKey(r)));

export function trayMenuLines(reports) {
  const shown = shownReports(reports);
  const repeated = new Set();
  const seen = new Set();
  for (const r of shown) (seen.has(r.provider) ? repeated : seen).add(r.provider);
  return shown.map((r) => {
    // PRD 0008 RF3.4: "▲" marks an account with an active usage alert.
    const label = repeated.has(r.provider) ? `${brand(r.provider).label} · ${r.account}` : brand(r.provider).label;
    const name = r.usage_alerts?.length ? `▲ ${label}` : label;
    const pct = bindingPct(r);
    const value = r.status !== "ready" ? statusLabel(r.status) : blockedText(r) ?? (pct != null ? `${Math.round(pct)}%` : "—");
    // Keep the required value and age when an arbitrary account name would exceed the native 80-character limit.
    const suffix = `: ${value} · ${ageText(r)}`;
    const chars = [...name], max = 80 - [...suffix].length;
    return `${chars.length <= max ? name : `${chars.slice(0, Math.max(0, max - 1)).join("")}…`}${suffix}`;
  });
}

/** PRD 0010 RF3.3: icon tooltip with the age of the oldest reading on screen ("Cuotai" alone without accounts). */
export function trayTooltip(reports) {
  const oldest = shownReports(reports).reduce((a, r) => (!a || new Date(shownAt(r)) < new Date(shownAt(a)) ? r : a), null);
  if (!oldest) return "Cuotai";
  const age = ageText(oldest), account = `${brand(oldest.provider).label} (${oldest.account})`;
  const max = 80 - [...t("tray.tooltip", { account: "", age })].length, chars = [...account];
  const bounded = chars.length <= max ? account : `${chars.slice(0, Math.max(0, max - 1)).join("")}…`;
  return t("tray.tooltip", { account: bounded, age });
}

/** Envia o menu ao shell quando algo mudou (dados, ordem, idioma, contas desativadas ou quotas ocultas). */
export function syncTrayMenu(view) {
  if (!view) return;
  const payload = {
    lines: trayMenuLines(view.reports ?? []),
    empty: t("account.empty"),
    open: t("common.open"),
    settings: t("settings.title"),
    quit: t("common.quit"),
    tooltip: trayTooltip(view.reports ?? []),
    // PRD 0003 RF3.4: a line above the accounts and a second tooltip line.
    notice: nextNoticeText(view),
  };
  const key = JSON.stringify(payload);
  if (key === last) return;
  last = key;
  setTrayMenu(payload);
}
