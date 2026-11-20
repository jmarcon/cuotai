// PRD 0037 re-validation (V-09): the account form of every provider in the overflow check. The provider list is the
// keys of `apps/desktop/ui/js/demo-forms.json`, which the desktop test `demo_forms_fixture` writes from
// `Provider::ALL` and the backend's `form_schema`, so a new provider gets its page without an edit here. A provider
// the Add-account picker offers (`demo-catalog.json`) is opened from the picker; one without a usage query (not
// offered) from Edit on its card in the `?every-provider` demo set. Providers with a page of their own in `check.mjs`
// keep it (`OWN_FORM_PAGES`). `scripts/test-demo-fixture-ui.mjs` checks that every provider has one of the two.
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const UI = join(dirname(fileURLToPath(import.meta.url)), "../../ui");
const json = (path) => JSON.parse(readFileSync(join(UI, path), "utf8"));

/** Providers whose form already has its own page in `check.mjs` (with the state that page needs). */
export const OWN_FORM_PAGES = {
  groq: "dialog-add", aws_bedrock: "dialog-add-bedrock", nine_router: "dialog-add-9router", litellm: "dialog-add-litellm",
  eleven_labs: "dialog-add-eleven", xiaomi_mimo: "dialog-add-xiaomi", open_router: "dialog-add-openrouter", claude: "dialog-add-claude-feed",
};

/** The languages the per-provider form pages run in: the reference, the longest words (de), CJK (ja) and the
 *  pseudo-locale, whose padding exceeds every shipped language. The forms' own pages above run in all of them. */
export const FORM_LOCALES = ["en-US", "de", "ja", "en-XA"];

/** Every provider of the core, in `demo-forms.json` order (by slug). */
export const coreProviders = () => Object.keys(json("js/demo-forms.json").providers);

/** `check.mjs` page entries (`[name, page, width, height, storage, clicks]`) for every provider without its own page. */
export function providerFormPages() {
  const offered = new Set(json("js/demo-catalog.json").entries.map((e) => e.provider));
  return coreProviders().filter((p) => !OWN_FORM_PAGES[p]).map((p) => offered.has(p)
    ? [`dialog-form-${p}`, "index.html", 1200, 820, {}, ["[data-add]", `.pick[data-provider="${p}"]`]]
    : [`dialog-form-${p}`, "index.html?every-provider", 1200, 820, {}, [`.card[data-card^="${p}::"] [data-details]`, "[data-edit]"]]);
}
