// PRD 0025 RF9 / PRD 0022 RF8.2 (docs/design-system.md DS-CMP-DENS-1..5): what an account row shows. The presets
// Detailed, Standard (default), Minimal and Custom map to one element map; the row component reads it and hides an
// element by leaving it out of the layout (no gap). The figure and the meter are not in the map: they are never
// hideable. A hidden element only moves: it stays in the expanded row, the details, the tooltip and the row's
// accessible name. `desktop.json` stores `rows.tray`, `rows.window` and `rows.separate` (the shell validates them).
import { getSettings, onSettingsChanged } from "./lib.js";

/** DS-CMP-DENS-1 order: the switches of Custom, top to bottom. */
export const ELEMENTS = ["plan", "label", "monogram", "unit", "corridor", "status", "ring", "runs_out", "returns", "age", "incident", "extra", "sublimits"];
export const PRESETS = ["detailed", "standard", "minimal", "custom"];

const all = (on) => Object.fromEntries(ELEMENTS.map((k) => [k, on]));
/** DS-CMP-DENS-1 values. `age` on = always show the reading's age (off = only when aging or stale, DS-CMP-ROW-4);
 *  Minimal keeps the status items but shows only one of them (`one`, DS-CMP-DENS-3). */
export const PRESET_VALUES = {
  detailed: all(true),
  standard: { ...all(true), age: false, sublimits: false },
  minimal: { ...all(false), monogram: true, status: true, runs_out: true, returns: true, incident: true },
};

/** The element map of a stored layout (`{ preset, elements }`); unknown presets read as Standard and a Custom map
 *  fills its gaps from Standard. `one`: Minimal's single status. */
export function resolve(layout) {
  const preset = PRESETS.includes(layout?.preset) ? layout.preset : "standard";
  if (preset !== "custom") return { preset, one: preset === "minimal", elements: { ...PRESET_VALUES[preset] } };
  const elements = { ...PRESET_VALUES.standard };
  for (const k of ELEMENTS) if (typeof layout?.elements?.[k] === "boolean") elements[k] = layout.elements[k];
  return { preset, one: false, elements };
}

/** RF9.2: choosing a preset sets every switch to its values; changing a switch makes the layout Custom. */
export const choosePreset = (preset) => (preset === "custom" ? { preset, elements: { ...PRESET_VALUES.standard } } : { preset });
export function toggled(layout, key, on) {
  if (!ELEMENTS.includes(key)) return layout;
  const { elements } = resolve(layout);
  return { preset: "custom", elements: { ...elements, [key]: !!on } };
}

/** The layout a window uses (DS-CMP-DENS-4): the tray's, or the window's own when the separate switch is on. */
export const layoutFor = (rows, scope) => (scope === "window" && rows?.separate ? rows?.window : rows?.tray);

/** The elements this layout leaves out of the row (the CSS hook). `age` is not hidden by its switch (off means
 *  "only when aging or stale"); `label` hides only the secondary account label, never a row title. */
export const hiddenOf = ({ elements }) => ELEMENTS.filter((k) => k !== "age" && !elements[k]);

/**
 * DS-CMP-DENS-3: Minimal's one status, by priority: incident > stale > exhausted or blocked (with its return time) >
 * runs out before the reset > the pace stage. `facts`: which of them apply; returns the winning kind or null.
 */
export function oneStatus({ incident = false, stale = false, exhausted = false, runsOut = false, pace = false } = {}) {
  if (incident) return "incident";
  if (stale) return "stale";
  if (exhausted) return "exhausted";
  if (runsOut) return "runs_out";
  if (pace) return "pace";
  return null;
}

let current = resolve(null);
/** The element map this window renders with. */
export const rowLayout = () => current;

/** Applies `layout` to this window: the row component reads it and the root carries the CSS hook. */
export function applyRowLayout(layout) {
  current = resolve(layout);
  const root = globalThis.document?.documentElement;
  if (root) {
    root.dataset.rowsHide = hiddenOf(current).join(" ");
    root.dataset.rowsPreset = current.preset;
  }
  return current;
}

/** Reads this window's layout (`scope`: "tray" or "window") and follows every later change from Settings. */
export async function initRowLayout(scope, onChange) {
  const load = async () => {
    const s = await getSettings().catch(() => null);
    applyRowLayout(layoutFor(s?.rows, scope));
  };
  await load();
  onSettingsChanged((key) => { if (key === "rows") load().then(onChange); });
}
