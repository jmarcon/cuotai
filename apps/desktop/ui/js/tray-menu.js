// Menu do clique direito no ícone da bandeja: texto simples, uma linha por conta ("nome: % disponível").
// As linhas seguem a mesma ordem e os mesmos filtros do popover; o shell só monta o menu nativo.
import { t } from "./i18n.js";
import { bindingPct, blockedText, brand, disabledAccounts, reportKey, setTrayMenu, sortReports, statusLabel } from "./lib.js";

let last = "";

export function trayMenuLines(reports) {
  const shown = sortReports(reports).filter((r) => !disabledAccounts.has(reportKey(r)));
  const repeated = new Set();
  const seen = new Set();
  for (const r of shown) (seen.has(r.provider) ? repeated : seen).add(r.provider);
  return shown.map((r) => {
    const name = repeated.has(r.provider) ? `${brand(r.provider).label} · ${r.account}` : brand(r.provider).label;
    const pct = bindingPct(r);
    const value = r.status !== "ready" ? statusLabel(r.status) : blockedText(r) ?? (pct != null ? `${Math.round(pct)}%` : "—");
    return `${name}: ${value}`;
  });
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
  };
  const key = JSON.stringify(payload);
  if (key === last) return;
  last = key;
  setTrayMenu(payload);
}
