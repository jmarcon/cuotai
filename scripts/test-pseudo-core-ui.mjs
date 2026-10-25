#!/usr/bin/env node
// Browser-free UI test for PRD 0034 RF7: the en-XA pseudo-locale gets the core's texts again. The core has no en-US
// catalog (English is the source of its fallbacks), so `tl` derives the pseudo text from the English fallback.
import assert from "node:assert/strict";
import { installUiEnv } from "./ui-test-env.mjs";

installUiEnv({ locale: "en-US" });
globalThis.location = { search: "?pseudo" };

const i18n = await import("../apps/desktop/ui/js/i18n.js");
await i18n.initI18n();
assert.ok(i18n.hasLocale(i18n.PSEUDO), "the pseudo-locale is registered in the browser demo with ?pseudo");
assert.equal(i18n.currentLocale(), i18n.PSEUDO);

const core = i18n.tl({ id: "demo.core.only.fallback", params: { n: 3 }, fallback: "Service unavailable ({n})" });
assert.ok(core.startsWith("⟦") && core.endsWith("⟧"), `core text is pseudo-localized: ${core}`);
assert.ok(core.includes("(3)"), `its placeholder is still filled: ${core}`);
assert.ok(core.includes("Sërvïçë"), `its letters take accents: ${core}`);

// A UI key keeps working, and a plain string is returned unchanged.
assert.ok(i18n.t("tray.ok").startsWith("⟦"));
assert.equal(i18n.tl("plain"), "plain");

// Other languages are unchanged: the fallback is used as it is.
await i18n.setLocale("en-US");
assert.equal(i18n.tl({ id: "demo.core.only.fallback", params: { n: 3 }, fallback: "Service unavailable ({n})" }), "Service unavailable (3)");
console.log("ok pseudo-locale core texts");

// An id that only the English UI catalog and a core catalog know resolves to the core's translation, never to English.
await i18n.setLocale("es");
assert.equal(i18n.t("status.rate_limited"), "Limitado");
assert.equal(i18n.t("pace.chip.lasts"), "Llega al reinicio");
console.log("ok core translations beat the English UI catalog");
