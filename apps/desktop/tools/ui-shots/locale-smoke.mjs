#!/usr/bin/env node
// PRD 0034 RF7: renders the tray, the main window, the settings pages and the dialogs of the browser demo in en-US, es
// and it (with the real core catalogs) and fails on any Portuguese word in the visible text, accessible names,
// tooltips and placeholders. pt-BR is not scanned: it is the one language that may contain Portuguese.
// Usage: node apps/desktop/tools/ui-shots/locale-smoke.mjs [locale ...]   Exit: 0 clean, 1 with Portuguese found.
import { launch } from "./driver.mjs";

const args = process.argv.slice(2).filter((a) => !a.startsWith("--"));
const LOCALES = args.length ? args : ["en-US", "es", "it", "de", "ru", "ko", "hi-IN", "id-ID", "tr", "vi", "zh-CN", "fr", "ja"];
// Languages whose own orthography uses the letters of CHARS (Vietnamese ã ê ô â, Turkish zekâ): the letter test is skipped there.
const CHARS_OK = new Set(["vi", "tr"]);
const PAGES = [
  ["tray", "tray.html", 380, 600, []],
  ["tray-account", "tray.html", 380, 600, ["[data-open]"]],
  ["tray-stats", "tray.html", 380, 600, ["[data-open-stats]"]],
  ["main", "index.html", 1200, 820, []],
  ["main-details", "index.html", 1200, 900, ["[data-details]"]],
  ["main-extensions", "index.html", 1200, 900, ["[data-extensions]"]],
  ["main-add", "index.html", 1200, 900, ["[data-add]"]],
  ["main-stats", "index.html", 1200, 820, ["[data-stats-toggle]"]],
  ["first-run", "index.html?first-run", 1200, 820, []],
  ...["general", "dashboard", "alerts", "menubar", "integrations", "cost", "about"].map((p) => [`settings-${p}`, `settings.html?page=${p}`, 860, 900, []]),
  ["settings-cost-plans", "settings.html?page=cost", 860, 900, ['[data-open-sub="cost-plans"]']],
  ["settings-whats-new", "settings.html?page=about", 860, 900, ["[data-whats-new]"]],
  ["settings-report", "settings.html?page=about", 860, 900, ["[data-report-problem]"]],
];
// Portuguese letters not used by English, Spanish or Italian (French uses ê, â and ô, so the letters do not count there)
// and words that are not valid in those languages.
const CHARS = /[ãõêôâ]/i;
const WORDS = new Set(["não", "você", "voce", "conta", "contas", "cota", "cotas", "sessão", "pessoal", "trabalho", "hoje", "ontem",
  "desligado", "precisa", "informe", "resposta", "serviço", "extensão", "atualização", "câmbio", "diretórios", "solicitações",
  "requisições", "requisição", "créditos", "demonstração", "navegador", "dias", "fonte", "nenhum", "nenhuma", "mês", "semana", "salvar",
  "ativada", "desativada", "indisponível", "disponível", "aprovação", "configuração", "carregada", "inválido", "inválida",
  "autorização", "pendente", "refaça", "sucesso", "cole", "este", "esta", "fluxo", "aceita", "aguarde", "encerrada", "ligado"]);
// Words that are also valid Spanish or Italian: they only count on the English pages.
const SHARED = new Set(["conta", "contas", "cota", "cotas", "pendente", "dias", "navegador", "cole", "este", "esta", "fonte", "mes",
  "semana", "salvar", "informe", "disponible", "aguarde", "fluxo", "ligado", "sucesso", "carregada"]);

const COLLECT = `(() => {
  const out = [];
  const add = (where, text) => { if (text && text.trim()) out.push([where, text.trim()]); };
  add("text", document.body.innerText);
  for (const d of document.querySelectorAll("dialog")) add("dialog", d.textContent);
  for (const e of document.querySelectorAll("[aria-label],[title],[placeholder],[alt]")) {
    for (const a of ["aria-label", "title", "placeholder", "alt"]) add(a, e.getAttribute(a));
  }
  // A catalog key shown instead of its text (a missing translation).
  for (const e of document.querySelectorAll("body *")) {
    if (e.children.length === 0 && /^[a-z][a-z0-9_]*(\\.[a-z_][a-z0-9_]*)+$/.test(e.textContent.trim())) add("catalog key", "MISSING-KEY " + e.textContent.trim());
  }
  return out;
})()`;

/** The Portuguese markers of `text`; a word that is valid in the page's language does not count. */
function markers(text, locale) {
  const found = new Set();
  const key = /^MISSING-KEY (.+)$/.exec(text);
  if (key) return [`missing catalog key ${key[1]}`];
  for (const m of text.matchAll(/\p{L}+/gu)) {
    const word = m[0].toLowerCase();
    if (CHARS.test(word) && !CHARS_OK.has(locale) && locale !== "fr") found.add(word);
    else if (WORDS.has(word) && !(SHARED.has(word) && locale !== "en-US")) found.add(word);
  }
  return [...found];
}

const ui = await launch();
let problems = 0;
try {
  for (const locale of LOCALES) {
    for (const [name, page, w, h, clicks] of PAGES) {
      await ui.open(page, { w, h, scale: 1, locale });
      if (clicks.length) await ui.click(...clicks);
      const texts = (await ui.evaluate(COLLECT)) ?? [];
      const bad = [];
      for (const [where, text] of texts) {
        const found = markers(text, locale);
        if (found.length) bad.push(`${where}: ${found.join(", ")}`);
      }
      problems += bad.length;
      console.log(`${bad.length ? "FAIL" : "ok  "} ${name} ${locale}`);
      for (const line of [...new Set(bad)]) console.log(`       ${line}`);
    }
  }
} finally {
  ui.close();
}
console.error(`locale smoke: ${problems} problem(s)`);
process.exit(problems ? 1 : 0);
