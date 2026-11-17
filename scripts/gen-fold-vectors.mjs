#!/usr/bin/env node
// PRD 0035 RF10.5: generates the search-folding table of `crates/cuotai-core/src/text/fold.rs` (between the
// `fold-table:begin`/`fold-table:end` markers) and the shared vector file `crates/cuotai-core/src/text/vectors/fold.json`
// from the desktop's `fold()` in `apps/desktop/ui/js/util.js`, which decomposes Latin letters with Node's NFD.
// Run `node scripts/gen-fold-vectors.mjs` after changing `fold()`; `cargo test -p cuotai-core text::fold` and
// `node scripts/test-i18n-infra-ui.mjs` then check that both twins give every vector the same result.
import { readFileSync, writeFileSync } from "node:fs";

const root = new URL("../", import.meta.url);
const { fold } = await import(new URL("apps/desktop/ui/js/util.js", root));
const BLOCKS = [[0x00c0, 0x024f], [0x1e00, 0x1eff]];
const hex = (cp) => cp.toString(16);
const rustChar = (c) => `'\\u{${hex(c.codePointAt(0))}}'`;

// The Rust table: every code point of the two blocks whose fold is not its plain lower-case form.
const entries = [];
for (const [from, to] of BLOCKS) {
  for (let cp = from; cp <= to; cp++) {
    const c = String.fromCodePoint(cp);
    const folded = fold(c);
    if (folded !== c.toLowerCase()) entries.push(`(${rustChar(c)}, ${JSON.stringify(folded)})`);
  }
}
const rows = [];
for (let i = 0; i < entries.length; i += 6) rows.push(`    ${entries.slice(i, i + 6).join(", ")},`);
const table = `#[rustfmt::skip]\nconst FOLDED: &[(char, &str)] = &[\n${rows.join("\n")}\n];`;
const rsPath = new URL("crates/cuotai-core/src/text/fold.rs", root);
const rs = readFileSync(rsPath, "utf8");
const begin = "// fold-table:begin\n";
const end = "// fold-table:end";
const start = rs.indexOf(begin) + begin.length;
const stop = rs.indexOf(end);
if (start < begin.length || stop < start) throw new Error("fold.rs: missing fold-table markers");
writeFileSync(rsPath, `${rs.slice(0, start)}${table}\n${rs.slice(stop)}`);

// The vectors: every code point of the two blocks, then words in the shipped languages and other scripts.
const vectors = {};
for (const [from, to] of BLOCKS) for (let cp = from; cp <= to; cp++) {
  const c = String.fromCodePoint(cp);
  vectors[c] = fold(c);
}
const words = [
  "Søren", "Ðặng", "ı İ ş ß đ ł", "ẞ Ø Ð Ħ Ŀ Ŧ ſ ŉ", "Đồng", "İstanbul", "ISTANBUL", "Straße", "Łódź", "Ağrı ŞİRKET",
  "Tiếng Việt", "Nguyễn Ưu đãi", "Configuração São", "Çalışma Ğ Ş", "Ærø Œuvre ×", "Vie\u0302\u0323t", "Ǿ Ǣ Ǯ ǰ",
  "日本語のアカウント", "ｱｶｳﾝﾄ", "Ａｃｃｏｕｎｔ", "中文（简体）", "한국어", "설정 필요", "हिन्दी", "सेटअप चाहिए", "\u0958",
  "Русский Ёлка й", "Ελληνικά ΣΟΦΙΑ", "", "claude-3.5 / API_KEY",
];
for (const w of words) vectors[w] = fold(w);
// Invisible characters and combining marks are written as `\uXXXX` escapes.
const escaped = (json) => json.replace(/[\u0080-\u00a0\u0300-\u036f\u2000-\u200f\u2028-\u202f\u205f-\u206f\u212a\u3000\ufeff]/g,
  (c) => `\\u${c.charCodeAt(0).toString(16).padStart(4, "0")}`);
writeFileSync(new URL("crates/cuotai-core/src/text/vectors/fold.json", root), escaped(`${JSON.stringify(vectors, null, 1)}\n`));
console.log(`fold table: ${entries.length} entries; vectors: ${Object.keys(vectors).length}`);
