// PRD 0035 RF1.5: a static copy of the core's locale registry (`cuotai_core::text::LOCALES`), used only where the
// `locale_registry` command is unavailable (the browser demo). The block between the markers is generated: run
// `CUOTAI_REGENERATE_LOCALES=1 cargo test -p cuotai-core locale_registry_copies`; a plain run fails on drift.
// registry:begin
export const REGISTRY = [
  {"code":"pt-BR","endonym":"Português","flag":"br","script":"Latn","dir":"ltr","languages":["pt"],"excluded_scripts":[],"excluded_regions":[],"plural_rule":"zero_one","status":"shipped","critical_reviewed":false,"phase":0},
  {"code":"en-US","endonym":"English","flag":"us","script":"Latn","dir":"ltr","languages":["en"],"excluded_scripts":[],"excluded_regions":[],"plural_rule":"one","status":"source","critical_reviewed":false,"phase":0},
  {"code":"es","endonym":"Español","flag":"es","script":"Latn","dir":"ltr","languages":["es"],"excluded_scripts":[],"excluded_regions":[],"plural_rule":"one","status":"shipped","critical_reviewed":false,"phase":0},
  {"code":"it","endonym":"Italiano","flag":"it","script":"Latn","dir":"ltr","languages":["it"],"excluded_scripts":[],"excluded_regions":[],"plural_rule":"one","status":"shipped","critical_reviewed":false,"phase":0},
  {"code":"zh-CN","endonym":"中文（简体）","flag":"cn","script":"Hans","dir":"ltr","languages":["zh","cmn"],"excluded_scripts":["hant"],"excluded_regions":["tw","hk","mo"],"plural_rule":"other","status":"in_progress","critical_reviewed":false,"phase":1},
  {"code":"fr","endonym":"Français","flag":"fr","script":"Latn","dir":"ltr","languages":["fr"],"excluded_scripts":[],"excluded_regions":[],"plural_rule":"zero_one","status":"in_progress","critical_reviewed":false,"phase":1},
  {"code":"de","endonym":"Deutsch","flag":"de","script":"Latn","dir":"ltr","languages":["de"],"excluded_scripts":[],"excluded_regions":[],"plural_rule":"one","status":"shipped","critical_reviewed":false,"phase":1},
  {"code":"ja","endonym":"日本語","flag":"jp","script":"Jpan","dir":"ltr","languages":["ja"],"excluded_scripts":[],"excluded_regions":[],"plural_rule":"other","status":"in_progress","critical_reviewed":false,"phase":1},
  {"code":"ru","endonym":"Русский","flag":"ru","script":"Cyrl","dir":"ltr","languages":["ru"],"excluded_scripts":[],"excluded_regions":[],"plural_rule":"east_slavic","status":"shipped","critical_reviewed":false,"phase":1},
  {"code":"ko","endonym":"한국어","flag":"kr","script":"Kore","dir":"ltr","languages":["ko"],"excluded_scripts":[],"excluded_regions":[],"plural_rule":"other","status":"shipped","critical_reviewed":false,"phase":2},
  {"code":"hi-IN","endonym":"हिन्दी","flag":"in","script":"Deva","dir":"ltr","languages":["hi"],"excluded_scripts":[],"excluded_regions":[],"plural_rule":"hindi","status":"shipped","critical_reviewed":false,"phase":2},
  {"code":"id-ID","endonym":"Bahasa Indonesia","flag":"id","script":"Latn","dir":"ltr","languages":["id","in"],"excluded_scripts":[],"excluded_regions":[],"plural_rule":"other","status":"in_progress","critical_reviewed":false,"phase":2},
  {"code":"tr","endonym":"Türkçe","flag":"tr","script":"Latn","dir":"ltr","languages":["tr"],"excluded_scripts":[],"excluded_regions":[],"plural_rule":"one","status":"in_progress","critical_reviewed":false,"phase":2},
  {"code":"vi","endonym":"Tiếng Việt","flag":"vn","script":"Latn","dir":"ltr","languages":["vi"],"excluded_scripts":[],"excluded_regions":[],"plural_rule":"other","status":"in_progress","critical_reviewed":false,"phase":2},
];
// registry:end

/**
 * PRD 0035 RF1.3: the registry row of a raw language code (`zh_CN.UTF-8`, `in`, `fr-CA`), or null. The twin of
 * `cuotai_core::text::normalize` (and of the pre-paint copy in the HTML pages): strip an encoding and a modifier, accept
 * `_`, ignore case, match the parsed language subtag (never a prefix: `est` is not `es`), honour the row's excluded
 * scripts and regions (`zh-TW` and `zh-Hant` are never Simplified Chinese).
 */
export function normalizeLocale(raw, rows = REGISTRY) {
  if (typeof raw !== "string") return null;
  const tags = raw.trim().split(/[.@]/)[0].split(/[-_]/);
  const lang = (tags.shift() ?? "").toLowerCase();
  if (!/^[a-z]{2,3}$/.test(lang)) return null;
  let script = null;
  let region = null;
  for (const tag of tags) {
    if (/^[A-Za-z]{4}$/.test(tag) && !script && !region) script = tag.toLowerCase();
    else if (/^[A-Za-z]{2}$/.test(tag) && !region) region = tag.toLowerCase();
    else if (/^[0-9]{3}$/.test(tag) && !region) region = tag;
    else if (!/^[A-Za-z0-9]{1,8}$/.test(tag)) return null;
  }
  const row = rows.find((r) => r.languages.includes(lang));
  if (!row) return null;
  if (script ? row.excluded_scripts.includes(script) : region && row.excluded_regions.includes(region)) return null;
  return row;
}
