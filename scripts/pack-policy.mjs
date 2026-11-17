// PRD 0035 re-validation S-01: the JavaScript reading of `crates/cuotai-core/src/user_pack/policy.json`, for the UI
// tests only (the webview never needs it: it receives packs the core already filtered). Same rules as
// `cuotai_core::user_pack`: an exact id beats any `prefix*` rule, the longest prefix wins, no match is protected, and a
// `confirm` or `consent` dot-segment is always protected.
import { readFile } from "node:fs/promises";

export const POLICY_URL = new URL("../crates/cuotai-core/src/user_pack/policy.json", import.meta.url);
export const policy = JSON.parse(await readFile(POLICY_URL, "utf8"));

/** The rule of `layer` ("core" or "ui") deciding `id`: `{ rule, cls }`, or null. */
export function ruleFor(layer, id) {
  let best = null;
  for (const cls of ["protected", "overridable"]) {
    for (const rule of policy[layer][cls]) {
      let score;
      if (rule.endsWith("*")) {
        if (!id.startsWith(rule.slice(0, -1))) continue;
        score = [0, rule.length - 1];
      } else if (rule === id) score = [1, rule.length];
      else continue;
      if (!best || score[0] > best.score[0] || (score[0] === best.score[0] && score[1] > best.score[1])) best = { rule, cls, score };
    }
  }
  return best && { rule: best.rule, cls: best.cls };
}

const guarded = (id) => id.split(".").some((s) => s === "confirm" || s === "consent");

/** The class of `id` in one layer. */
export const classIn = (layer, id) => (guarded(id) ? "protected" : ruleFor(layer, id)?.cls ?? "protected");

/** Whether a pack may replace `id` (`user_pack::is_overridable`): every layer with a rule for it says overridable. */
export function isOverridable(id) {
  if (guarded(id)) return false;
  const classes = ["core", "ui"].map((layer) => ruleFor(layer, id)?.cls).filter(Boolean);
  return classes.length > 0 && classes.every((c) => c === "overridable");
}
