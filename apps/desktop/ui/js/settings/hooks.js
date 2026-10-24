// Settings › Advanced › Integrations › Hooks (PRD 0013 RF4), moved unchanged from the old dialog (PRD 0020 RF2.5).
import { currentLocale, t, te, tle } from "../i18n.js";
import { confirmHooks, esc, hooksStatus, icon, onHookRun, saveHooks, setHooksEnabled, testHook } from "../lib.js";
import { showDialog } from "../motion.js";
import { l10nError, serveErrorText } from "./common.js";

/** PRD 0013 RF4: hooks, off by default. The switch, the state (running, waiting for the confirmation, or why not), an
 *  editor of `hooks.toml`'s rules (name, events and the command as one field per argument; filters and time limits
 *  written by hand are kept and shown), a test per running rule that fires its sample event, and the latest runs.
 *  Turning hooks on, or saving rules while they are on, first shows the exact commands to confirm (RF4.2). */
export function hooksHtml(hv, drafts, dirty) {
  const review = `<button class="btn" data-hooks-review>${te("settings.hooks.review")}</button>`;
  const state = {
    on: `<p class="hint" data-hooks-state><strong>${te("settings.hooks.state.on", { count: hv.running.length })}</strong></p>`,
    off: `<p class="hint" data-hooks-state><strong>${te("settings.hooks.state.off")}</strong></p>`,
    unconfirmed: `<p class="hint warn-txt" data-hooks-state role="alert">${icon("warn")} <strong>${te("settings.hooks.state.unconfirmed")}</strong></p>
      <div class="dlg-actions left">${review}</div>`,
    invalid: `<p class="hint warn-txt" data-hooks-state role="alert">${icon("warn")} <strong>${te("settings.hooks.state.invalid")}</strong> ${tle(hv.error)}</p>`,
    missing: `<p class="hint" data-hooks-state>${te("settings.hooks.state.missing")}</p>`,
    no_config: `<p class="hint" data-hooks-state>${te("settings.hooks.state.noConfig")}</p>`,
  }[hv.state] ?? "";
  const editor = hv.state === "no_config" ? "" : hv.rules == null
    ? `<p class="hint warn-txt">${te("settings.hooks.unparsable")}</p>`
    : `${drafts.map((d, i) => ruleHtml(hv, d, i, dirty)).join("")}
      <div class="dlg-actions left"><button class="btn" data-hook-add>${icon("plus")} ${te("settings.hooks.add")}</button>
        <button class="btn primary" data-hooks-save ${dirty ? "" : "disabled"}>${te("settings.hooks.save")}</button>
        <button class="btn" data-hooks-discard ${dirty ? "" : "disabled"}>${te("settings.hooks.discard")}</button></div>`;
  const runs = hv.runs.length
    ? `<ul class="hook-runs" data-hook-runs>${hv.runs.map((r) => `<li class="${r.outcome === "exited" && r.exit_code === 0 ? "" : "warn-txt"}">
        <time datetime="${esc(r.at)}">${esc(new Date(r.at).toLocaleString(currentLocale()))}</time> ${tle(r.summary)}</li>`).join("")}</ul>`
    : `<p class="hint" data-hook-runs>${te("settings.hooks.runs.none")}</p>`;
  return `<h4>${te("settings.hooks.title")}</h4>
        <p class="hint">${te("settings.hooks.help")}</p>
        <label class="check"><input id="hooks-on" type="checkbox" ${hv.enabled ? "checked" : ""} ${hv.state === "no_config" ? "disabled" : ""} /> ${te("settings.hooks.on")}</label>
        ${state}
        ${hv.path ? `<div class="path-row"><code title="${esc(hv.path)}">${esc(hv.path)}</code></div>` : ""}
        ${hv.warnings.map((w) => `<p class="hint warn-txt">${tle(w)}</p>`).join("")}
        ${editor}
        <p class="err" id="hooks-msg" role="status"></p>
        <h5>${te("settings.hooks.runs.title")}</h5>
        ${runs}`;
}

/** One rule of the editor: name, events (payload names with a label) and the command, one field per argument. */
function ruleHtml(hv, d, i, dirty) {
  const kept = [d.provider != null && `provider = ${JSON.stringify(d.provider)}`, d.account != null && `account = ${JSON.stringify(d.account)}`,
    d.min_threshold != null && `min_threshold = ${d.min_threshold}`, d.timeout_seconds != null && `timeout_seconds = ${d.timeout_seconds}`].filter(Boolean);
  const testable = !dirty && hv.state === "on" && hv.running.includes(d.name);
  return `<fieldset class="hook-rule" data-rule="${i}">
    <legend>${te("settings.hooks.rule", { index: i + 1 })}</legend>
    <label class="f"><span>${te("settings.hooks.name")}</span><input data-name value="${esc(d.name)}" maxlength="64" autocomplete="off" spellcheck="false" /></label>
    <div class="f"><span>${te("settings.hooks.events")}</span>
      <div class="hook-events">${hv.events.map((name) => `<label class="check"><input type="checkbox" data-event="${name}" ${d.events.includes(name) ? "checked" : ""} />
        <span><code>${name}</code> <small>${te(`settings.hooks.event.${name}`)}</small></span></label>`).join("")}</div></div>
    <div class="f"><span>${te("settings.hooks.command")}</span>
      <ol class="hook-args">${d.command.map((arg, j) => `<li><input data-arg="${j}" value="${esc(arg)}" autocomplete="off" spellcheck="false"
          aria-label="${te(j === 0 ? "settings.hooks.program" : "settings.hooks.argument", { index: j })}" placeholder="${te(j === 0 ? "settings.hooks.programHint" : "settings.hooks.argumentHint")}" />
        <button class="icon" data-arg-remove="${j}" title="${te("settings.hooks.argumentRemove")}" aria-label="${te("settings.hooks.argumentRemove")}">${icon("trash")}</button></li>`).join("")}</ol>
      <small>${te("settings.hooks.commandHelp")}</small></div>
    <div class="dlg-actions left"><button class="btn" data-arg-add>${icon("plus")} ${te("settings.hooks.argumentAdd")}</button></div>
    ${kept.length ? `<p class="hint">${te("settings.hooks.kept", { filters: kept.join(", ") })}</p>` : ""}
    <div class="dlg-actions left"><button class="btn" data-hook-test ${testable ? "" : "disabled"} title="${te(testable ? "settings.hooks.testHelp" : "settings.hooks.testOff")}">${te("settings.hooks.test")}</button>
      <button class="btn danger-btn" data-hook-remove>${icon("trash")} ${te("settings.hooks.remove")}</button></div>
  </fieldset>`;
}

/** Paints the hooks and binds them. Edits stay in the page until saved (the core validates and writes the file over
 *  the revision it was read at); a failure keeps them and shows why. The log repaints after every run. */
export async function bindHooks(box, ctx) {
  let hv;
  try { hv = await hooksStatus(); } catch (e) { box.querySelector(".hint").textContent = l10nError(e); return; }
  let drafts = structuredClone(hv.rules ?? []);
  let dirty = false;
  const paint = (next, message = "") => {
    if (next) { hv = next; if (!dirty) drafts = structuredClone(hv.rules ?? []); }
    box.innerHTML = hooksHtml(hv, drafts, dirty);
    box.querySelector("#hooks-msg").textContent = message;
    bind();
  };
  // The state is read again (a refused confirmation or save may mean the file changed); unsaved edits stay.
  const fail = (e) => {
    const message = serveErrorText(e);
    hooksStatus().then((next) => paint(next, message), () => paint(null, message));
  };
  const edited = () => {
    if (dirty) return;
    dirty = true;
    box.querySelector("[data-hooks-save]").disabled = false;
    box.querySelector("[data-hooks-discard]").disabled = false;
    for (const b of box.querySelectorAll("[data-hook-test]")) { b.disabled = true; b.title = t("settings.hooks.testOff"); }
  };
  /** RF4.2: confirm the rules listed in `view`, then (when asked) turn hooks on. */
  const confirmThen = async (view, enable) => {
    if (view.digest && !(await confirmCommands(view.listing))) return paint(null);
    if (view.digest) view = await confirmHooks(view.digest);
    paint(enable ? await setHooksEnabled(true) : view);
    ctx.changed("hooks");
  };
  const bind = () => {
    box.querySelector("#hooks-on").onchange = (e) => {
      const on = e.target.checked;
      (on ? confirmThen(hv, true) : setHooksEnabled(false).then((next) => { paint(next); ctx.changed("hooks"); })).catch(fail);
    };
    const review = box.querySelector("[data-hooks-review]");
    if (review) review.onclick = () => confirmThen(hv, false).catch(fail);
    for (const fs of box.querySelectorAll("[data-rule]")) {
      const d = drafts[Number(fs.dataset.rule)];
      fs.querySelector("[data-name]").oninput = (e) => { d.name = e.target.value; edited(); };
      for (const c of fs.querySelectorAll("[data-event]")) c.onchange = () => {
        d.events = c.checked ? [...d.events.filter((x) => x !== c.dataset.event), c.dataset.event] : d.events.filter((x) => x !== c.dataset.event);
        edited();
      };
      for (const input of fs.querySelectorAll("[data-arg]")) input.oninput = () => { d.command[Number(input.dataset.arg)] = input.value; edited(); };
      const restructure = (change, focus) => () => {
        change();
        dirty = true;
        paint(null);
        box.querySelector(focus())?.focus();
      };
      for (const b of fs.querySelectorAll("[data-arg-remove]")) {
        b.onclick = restructure(() => d.command.splice(Number(b.dataset.argRemove), 1), () => `[data-rule="${fs.dataset.rule}"] [data-arg-add]`);
      }
      fs.querySelector("[data-arg-add]").onclick = restructure(() => d.command.push(""), () => `[data-rule="${fs.dataset.rule}"] [data-arg="${d.command.length - 1}"]`);
      fs.querySelector("[data-hook-remove]").onclick = restructure(() => drafts.splice(Number(fs.dataset.rule), 1), () => "[data-hook-add]");
      fs.querySelector("[data-hook-test]").onclick = () => testHook(d.name).then((next) => paint(next, t("settings.hooks.testSent", { name: d.name })), fail);
    }
    const add = box.querySelector("[data-hook-add]");
    if (!add) return;
    add.onclick = () => {
      drafts.push({ name: "", events: [], command: [""] });
      dirty = true;
      paint(null);
      box.querySelector(`[data-rule="${drafts.length - 1}"] [data-name]`)?.focus();
    };
    box.querySelector("[data-hooks-discard]").onclick = () => { dirty = false; hooksStatus().then((next) => paint(next), fail); };
    box.querySelector("[data-hooks-save]").onclick = async () => {
      try {
        const next = await saveHooks(hv.revision, drafts);
        dirty = false;
        paint(next, t("settings.hooks.saved"));
        ctx.changed("hooks");
        // Saved while on: the new commands run only once confirmed.
        if (next.enabled && next.digest) await confirmThen(next, false);
      } catch (e) { fail(e); }
    };
  };
  paint(hv);
  // The log repaints after every run while this page is shown; leaving the page stops listening.
  const unlisten = onHookRun(() => {
    if (!box.isConnected) { unlisten.then((stop) => stop()); return; }
    hooksStatus().then((next) => paint(next), () => {});
  });
  ctx.onLeave?.(() => unlisten.then((stop) => stop()));
}

/** RF4.2: the confirmation as a dialog with the core's exact listing (the file, then each rule's name, events,
 *  filters, time limit and command as a JSON array, so quotes and separators show). Resolves to the user's answer;
 *  closing it any other way declines. */
function confirmCommands(listing) {
  return new Promise((resolve) => {
    const d = document.createElement("dialog");
    d.className = "dlg hooks-confirm";
    d.setAttribute("role", "alertdialog");
    d.setAttribute("aria-labelledby", "hooks-confirm-title");
    const line = (l) => (l.id === "hooks.confirm.header" ? `<p>${tle(l)}</p>`
      : l.id === "hooks.confirm.rule" ? `<h3>${tle(l)}</h3>`
        : l.id === "hooks.confirm.command" ? `<p><code class="hook-command">${tle(l)}</code></p>`
          : `<p class="hint">${tle(l)}</p>`);
    d.innerHTML = `<div class="dlg-body"><h2 id="hooks-confirm-title">${te("settings.hooks.confirm.title")}</h2>
      <p class="hint warn-txt">${te("settings.hooks.confirm.help")}</p>
      <div class="hook-listing" data-hooks-listing>${listing.map(line).join("")}</div>
      <div class="dlg-actions"><button class="btn" data-hooks-decline>${te("common.cancel")}</button>
        <button class="btn danger-btn" data-hooks-accept>${te("settings.hooks.confirm.yes")}</button></div></div>`;
    document.body.append(d);
    let answer = false;
    d.addEventListener("close", () => resolve(answer), { once: true });
    d.querySelector("[data-hooks-accept]").onclick = () => { answer = true; d.close(); };
    d.querySelector("[data-hooks-decline]").onclick = () => d.close();
    showDialog(d);
    d.querySelector("[data-hooks-decline]").focus();
  });
}
