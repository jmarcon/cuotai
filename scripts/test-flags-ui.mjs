#!/usr/bin/env node
// PRD 0022 RF17.9 and DS-CMP-SEL-3: flag resolution (explicit `_meta.flag`, then the region, then the language),
// `pt` alone is Portugal and `pt-BR` is Brazil, unknown countries fall back to the globe, and every flag with a light
// fill (white or a light band, relative luminance 0.8 or more) gets the hairline class. No browser involved.
import assert from "node:assert/strict";

const { FLAGS, LIGHT_BAND_MIN, flagCountry, flagSvg, luminance } = await import("../apps/desktop/ui/js/flags.js");
assert.equal(flagCountry("pt"), "pt");
assert.equal(flagCountry("pt-BR"), "br");
assert.equal(flagCountry("pt", "BR"), "br", "an explicit _meta.flag wins");
assert.equal(flagCountry("es-MX"), "mx");
assert.equal(flagCountry("ko"), "kr");
assert.equal(flagCountry("zh-CN"), "cn");
assert.equal(flagCountry("xx"), "");
assert.match(flagSvg("xx"), /fill="#4a6fa5"/, "an unknown country: the globe");
assert.match(flagSvg("xx"), /class="flag"/, "the globe has no hairline");
// PRD 0035 re-validation V35-10 (DS-CMP-SEL-3): the hairline class follows the rule for every flag, read from the
// rendered artwork with an independent parse, and the flags with a white band at an edge or in the middle have it.
assert.equal(luminance("#fff"), 1);
assert.equal(luminance("#000"), 0);
assert.ok(luminance("#ffdf00") < LIGHT_BAND_MIN && luminance("#ffff00") >= LIGHT_BAND_MIN, "the threshold sits between Brazil's gold and pure yellow");
for (const country of Object.keys(FLAGS)) {
  const svg = flagSvg(country);
  const fills = [...svg.matchAll(/fill="(#[0-9a-fA-F]{3,6})"/g)].map((m) => m[1]);
  const light = fills.some((f) => luminance(f) >= LIGHT_BAND_MIN);
  assert.equal(/class="flag light-band"/.test(svg), light, `${country}: the hairline class follows the light-fill rule`);
}
for (const country of ["jp", "kr", "id", "ru", "fr", "it", "in"]) assert.match(flagSvg(country), /class="flag light-band"/, `${country}: light band, hairline`);
for (const country of ["de", "br"]) assert.match(flagSvg(country), /class="flag"/, `${country}: no light fill, no hairline`);
// PRD 0035 RF7.2: every registry language resolves to drawn artwork, never the globe.
const { REGISTRY } = await import("../apps/desktop/ui/js/locale-registry.js");
assert.equal(REGISTRY.length, 14);
for (const row of REGISTRY) {
  const country = flagCountry(row.code);
  assert.equal(country, row.flag, `${row.code}: the registry flag and flagCountry agree`);
  assert.doesNotMatch(flagSvg(country), /fill="#4a6fa5"/, `${row.code}: drawn, not the globe`);
}
assert.match(flagSvg("br"), /aria-hidden="true"/, "flags are decorative");
console.log("PASS: every registry language has a drawn flag; flags resolve by explicit flag, region, then language; every flag with a light band gets the hairline class");
