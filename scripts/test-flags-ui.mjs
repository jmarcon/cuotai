#!/usr/bin/env node
// PRD 0022 RF17.9 and DS-CMP-SEL-3: flag resolution (explicit `_meta.flag`, then the region, then the language),
// `pt` alone is Portugal and `pt-BR` is Brazil, unknown countries fall back to the globe, and only white-field flags
// get the hairline. No browser involved.
import assert from "node:assert/strict";

const { flagCountry, flagSvg } = await import("../apps/desktop/ui/js/flags.js");
assert.equal(flagCountry("pt"), "pt");
assert.equal(flagCountry("pt-BR"), "br");
assert.equal(flagCountry("pt", "BR"), "br", "an explicit _meta.flag wins");
assert.equal(flagCountry("es-MX"), "mx");
assert.equal(flagCountry("ko"), "kr");
assert.equal(flagCountry("zh-CN"), "cn");
assert.equal(flagCountry("xx"), "");
assert.match(flagSvg("jp"), /class="flag white-field"/);
assert.match(flagSvg("br"), /class="flag"/);
assert.match(flagSvg("xx"), /fill="#4a6fa5"/, "an unknown country: the globe");
assert.match(flagSvg("kr"), /class="flag white-field"/);
assert.match(flagSvg("id"), /class="flag white-field"/);
// PRD 0035 RF7.2: every registry language resolves to drawn artwork, never the globe.
const { REGISTRY } = await import("../apps/desktop/ui/js/locale-registry.js");
assert.equal(REGISTRY.length, 14);
for (const row of REGISTRY) {
  const country = flagCountry(row.code);
  assert.equal(country, row.flag, `${row.code}: the registry flag and flagCountry agree`);
  assert.doesNotMatch(flagSvg(country), /fill="#4a6fa5"/, `${row.code}: drawn, not the globe`);
}
assert.match(flagSvg("br"), /aria-hidden="true"/, "flags are decorative");
console.log("PASS: every registry language has a drawn flag; flags resolve by explicit flag, region, then language; white fields get a hairline");
