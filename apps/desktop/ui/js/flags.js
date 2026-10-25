// Bandeiras em SVG embutido (emoji de bandeira não renderiza no Windows). Proporção 3:2.
// Para um país novo, acrescente uma entrada em FLAGS; sem bandeira, usa-se um globo.
const bands = (dir, colors) => {
  const n = colors.length;
  return colors.map((c, i) =>
    dir === "h"
      ? `<rect y="${(20 / n) * i}" width="30" height="${20 / n + 0.05}" fill="${c}"/>`
      : `<rect x="${(30 / n) * i}" width="${30 / n + 0.05}" height="20" fill="${c}"/>`).join("");
};
const stripes13 = Array.from({ length: 13 }, (_, i) =>
  `<rect y="${(20 / 13) * i}" width="30" height="${20 / 13 + 0.05}" fill="${i % 2 ? "#fff" : "#b22234"}"/>`).join("");
const stars = Array.from({ length: 12 }, (_, i) =>
  `<circle cx="${1.8 + (i % 4) * 2.7}" cy="${1.6 + Math.floor(i / 4) * 2.7}" r=".55" fill="#fff"/>`).join("");

const FLAGS = {
  br: `<rect width="30" height="20" fill="#009c3b"/><path d="M15 2.2 27.2 10 15 17.8 2.8 10Z" fill="#ffdf00"/><circle cx="15" cy="10" r="4.6" fill="#002776"/><path d="M10.5 9.1c3-1.1 6.3-.6 9 1.8" stroke="#fff" stroke-width="1" fill="none"/>`,
  us: `${stripes13}<rect width="12.6" height="10.77" fill="#3c3b6e"/>${stars}`,
  es: `<rect width="30" height="20" fill="#aa151b"/><rect y="5" width="30" height="10" fill="#f1bf00"/>`,
  pt: `<rect width="30" height="20" fill="#f00"/><rect width="12" height="20" fill="#060"/><circle cx="12" cy="10" r="3.4" fill="#ff0" stroke="#f00" stroke-width="1"/>`,
  fr: bands("v", ["#0055a4", "#fff", "#ef4135"]),
  it: bands("v", ["#009246", "#fff", "#ce2b37"]),
  de: bands("h", ["#000", "#dd0000", "#ffce00"]),
  nl: bands("h", ["#ae1c28", "#fff", "#21468b"]),
  ru: bands("h", ["#fff", "#0039a6", "#d52b1e"]),
  pl: bands("h", ["#fff", "#dc143c"]),
  ar: `${bands("h", ["#74acdf", "#fff", "#74acdf"])}<circle cx="15" cy="10" r="2.4" fill="#f6b40e"/>`,
  mx: `${bands("v", ["#006847", "#fff", "#ce1126"])}<circle cx="15" cy="10" r="2.6" fill="#8b5a2b"/>`,
  jp: `<rect width="30" height="20" fill="#fff"/><circle cx="15" cy="10" r="6" fill="#bc002d"/>`,
};
const GLOBE = `<rect width="30" height="20" fill="#4a6fa5"/><circle cx="15" cy="10" r="7" fill="none" stroke="#fff" stroke-width="1.2"/><path d="M8 10h14M15 3c-3 3-3 11 0 14m0-14c3 3 3 11 0 14" stroke="#fff" stroke-width="1" fill="none"/>`;
const LANG_FLAG = { en: "us", es: "es", pt: "pt", fr: "fr", de: "de", it: "it", nl: "nl", ru: "ru", pl: "pl", ja: "jp" };

/** País da bandeira de um idioma: `_meta.flag` explícito, senão a região do código (pt-BR → br), senão a língua. */
export function flagCountry(code, explicit) {
  if (/^[A-Za-z]{2}$/.test(explicit ?? "")) return explicit.toLowerCase();
  const [lang, region] = code.split("-");
  if (region && /^[A-Za-z]{2}$/.test(region)) return region.toLowerCase();
  return LANG_FLAG[lang.toLowerCase()] ?? "";
}

/** DS-CMP-SEL-3: flags whose field is white get a 1 px `--border` hairline (jp; kr and id join with PRD 0035). */
const WHITE_FIELD = new Set(["jp", "kr", "id"]);

export function flagSvg(country, width = 24) {
  const art = FLAGS[country] ?? GLOBE;
  return `<svg class="flag${WHITE_FIELD.has(country) ? " white-field" : ""}" viewBox="0 0 30 20" width="${width}" height="${Math.round((width * 2) / 3)}" aria-hidden="true">${art}</svg>`;
}
