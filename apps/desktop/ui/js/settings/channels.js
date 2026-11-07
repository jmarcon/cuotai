// Settings › Alerts › Notification channels (PRD 0044 RF4/RF5): the list of destinations (DS-CMP-LIST), the editor
// with its consent step (DS-CMP-DLG), Send test and the delivery log. Every rule lives in the core: the shell
// validates the address, renders the example payload and stores the address in the system vault; the page never
// receives an address back, only the host, the filters in words and the last delivery.
import { currentLocale, t, te, tle } from "../i18n.js";
import { channelAdd, channelConsent, channelEdit, channelRemove, channelSetEnabled, channelTest, channelsView, esc, icon } from "../lib.js";
import { showDialog } from "../motion.js";
import { askDialog, l10nError } from "./common.js";
import { buttons, disc, row, rowError, subpage, subRow, sw, toast } from "./rows.js";

const KIND_NAMES = { webhook: "Webhook", ntfy: "ntfy", slack: "Slack", discord: "Discord" };
const KIND_ICONS = { webhook: "globe", ntfy: "bell", slack: "plug", discord: "plug" };
const SEVERITIES = ["info", "warning", "critical"];
const DEFAULT_KINDS = ["reset", "threshold", "exhausted", "recovered", "projected", "overage", "floor", "session_expiring", "limit_changed"];

/** The row that opens the sub-page (inside the page's "More" disclosure, PRD 0020 RF2.2's 8-row limit). */
export const channelsRowHtml = () => subRow({ key: "channels", label: te("settings.channels.title"), info: te("settings.channels.help") });

/** The sub-page; the list is drawn once the page loads (`bindChannels`). */
export const channelsHtml = () => subpage("channels", te("settings.channels.title"),
  `<section class="set-sec" data-channels><div data-channels-body><p class="hint">${te("settings.channels.loading")}</p></div></section>`);

/** "Thresholds, Exhaustion · of every account · names hidden": the core's words, joined. */
export const filterWords = (w) => [w.kinds.map((k) => tle(k)).join(", "), tle(w.accounts), w.severity ? tle(w.severity) : "", tle(w.names)].filter(Boolean).join(" · ");

const when = (at) => esc(new Date(at).toLocaleString(currentLocale()));

function lastHtml(c) {
  if (!c.last) return te("settings.channels.never");
  const status = c.last.status ?? "";
  return c.last.outcome === "delivered"
    ? te("settings.channels.lastOk", { when: when(c.last.at), status })
    : te("settings.channels.lastFailed", { when: when(c.last.at), status });
}

/** RF5.1: one row per channel: kind, label, host and filters, the switch, Send test, Edit and Remove. */
export function listHtml(view) {
  const failing = view.channels.filter((c) => c.failing_long);
  const banner = failing.length
    ? `<p class="hint warn-txt" role="alert" data-channels-failing>${icon("warn")} ${te("settings.channels.failingBanner", { labels: failing.map((c) => esc(c.label)).join(", ") })}</p>` : "";
  const rows = view.channels.length
    ? view.channels.map((c) => row({ key: `channel-${c.id}`, kw: "channels", label: `${icon(KIND_ICONS[c.kind])} ${esc(c.label)}`,
      desc: `${esc(KIND_NAMES[c.kind])} · ${esc(c.host)} · ${filterWords(c.filters_text)}`,
      control: (a) => buttons(
        `<button class="btn sm" type="button" data-channel-test="${esc(c.id)}">${te("settings.channels.test")}</button>`,
        `<button class="btn sm" type="button" data-channel-edit="${esc(c.id)}">${te("settings.channels.edit")}</button>`,
        `<button class="icon" type="button" data-channel-remove="${esc(c.id)}" title="${te("settings.channels.remove")}" aria-label="${te("settings.channels.remove")}">${icon("trash")}</button>`,
        sw(`channel-on-${c.id}`, c.enabled, a, `data-channel-on="${esc(c.id)}"`)),
      body: `<span class="hint${c.failing_status != null || (c.last && c.last.outcome !== "delivered") ? " warn-txt" : ""}">${lastHtml(c)}</span>` })).join("")
    : `<p class="hint" data-channels-empty>${te("settings.channels.empty")}</p>`;
  const log = view.log.length
    ? `<ul class="hook-runs" data-channel-log>${view.log.map((e) => {
      const label = view.channels.find((c) => c.id === e.channel)?.label ?? e.channel;
      return `<li class="${e.outcome === "failed" ? "warn-txt" : ""}"><time datetime="${esc(e.at)}">${when(e.at)}</time> ${esc(label)} · ${esc(e.kind)} · ${te(`settings.channels.outcome.${e.outcome}`)}${e.status ? ` ${e.status}` : ""}</li>`;
    }).join("")}</ul>`
    : `<p class="hint" data-channel-log>${te("settings.channels.logEmpty")}</p>`;
  return `${banner}<p class="hint">${te("settings.channels.intro")}</p>${rows}
    <div class="dlg-actions left"><button class="btn" type="button" data-channel-add ${view.channels.length >= view.max ? "disabled" : ""}>${icon("plus")} ${te("settings.channels.add")}</button></div>
    <p class="err" id="channels-msg" role="status"></p>
    ${disc("channel-log", te("settings.channels.log"), log)}`;
}

/** The editor's first step: kind, name, address (masked) and filters. `channel`: the one edited, else `null`. */
export function formHtml(view, channel) {
  const f = channel?.filters ?? { kinds: DEFAULT_KINDS, accounts: [], min_severity: "info" };
  const kind = channel?.kind ?? "ntfy";
  const names = f.include_names == null ? "default" : f.include_names ? "on" : "off";
  return `<h2 id="channel-dlg-title">${te(channel ? "settings.channels.editTitle" : "settings.channels.addTitle")}</h2>
    <div class="f"><span id="channel-kind-label">${te("settings.channels.kind")}</span>
      <div class="seg" role="radiogroup" aria-labelledby="channel-kind-label" data-channel-kind>${view.kinds.map((k) =>
        `<button type="button" role="radio" data-value="${k}" aria-checked="${k === kind}" tabindex="${k === kind ? 0 : -1}" ${channel ? "disabled" : ""}>${esc(KIND_NAMES[k])}</button>`).join("")}</div></div>
    <label class="f"><span>${te("settings.channels.label")}</span><input data-channel-label maxlength="64" autocomplete="off" value="${esc(channel?.label ?? "")}" /></label>
    <label class="f"><span>${te("settings.channels.url")}</span><input data-channel-url type="password" autocomplete="off" spellcheck="false"
      placeholder="${te(channel ? "settings.channels.urlKeep" : `settings.channels.urlHint.${kind}`)}" />
      <small>${te("settings.channels.urlHelp")}</small></label>
    <fieldset class="f channel-kinds"><legend>${te("settings.channels.sends")}</legend>
      <div class="hook-events">${view.notice_kinds.map((k) => `<label class="check"><input type="checkbox" class="chk" data-channel-notice="${k.id}" ${f.kinds.includes(k.id) ? "checked" : ""} /><span>${tle(k.label)}</span></label>`).join("")}</div></fieldset>
    ${view.accounts.length ? `<fieldset class="f"><legend>${te("settings.channels.accounts")}</legend>
      <div class="hook-events">${view.accounts.map((a) => `<label class="check"><input type="checkbox" class="chk" data-channel-account="${esc(a.id)}" ${f.accounts.includes(a.id) ? "checked" : ""} /><span>${esc(a.name)}</span></label>`).join("")}</div>
      <small>${te("settings.channels.accountsHelp")}</small></fieldset>` : ""}
    <label class="f"><span>${te("settings.channels.severity")}</span><select data-channel-severity>${SEVERITIES.map((s) =>
      `<option value="${s}" ${f.min_severity === s ? "selected" : ""}>${te(`settings.channels.severity.${s}`)}</option>`).join("")}</select></label>
    <label class="f"><span>${te("settings.channels.names")}</span><select data-channel-names>${["default", "on", "off"].map((n) =>
      `<option value="${n}" ${names === n ? "selected" : ""}>${te(`settings.channels.names.${n}`)}</option>`).join("")}</select></label>
    <label class="check" data-channel-sign-row ${kind === "webhook" ? "" : "hidden"}><input type="checkbox" class="chk" data-channel-sign ${channel?.signed ? "checked" : ""} />
      <span>${te("settings.channels.sign")} <small>${te("settings.channels.signHelp")}</small></span></label>
    <p class="err" data-channel-err role="alert"></p>
    <div class="dlg-actions"><button class="btn" type="button" data-channel-cancel>${te("common.cancel")}</button>
      <button class="btn primary" type="button" data-channel-next>${te("settings.channels.continue")}</button></div>`;
}

/** RF4.1: the consent step: kind, host, the filters in words, the example payload and the sentence; Save consents. */
export function consentHtml(consent) {
  return `<h2 id="channel-dlg-title">${te("settings.channels.consentTitle", { kind: esc(KIND_NAMES[consent.kind]) })}</h2>
    <dl class="channel-consent">
      <dt>${te("settings.channels.host")}</dt><dd><code>${esc(consent.masked)}</code></dd>
      <dt>${te("settings.channels.sends")}</dt><dd>${filterWords(consent.filters)}</dd></dl>
    ${consent.warning ? `<p class="hint warn-txt" role="alert">${icon("warn")} ${tle(consent.warning)}</p>` : ""}
    <div class="f"><span>${te("settings.channels.example")}</span><pre class="channel-example" tabindex="0">${esc(consent.example)}</pre></div>
    <p><strong>${tle(consent.sentence)}</strong></p>
    <p class="err" data-channel-err role="alert"></p>
    <div class="dlg-actions"><button class="btn" type="button" data-channel-back>${te("common.back")}</button>
      <button class="btn primary" type="button" data-channel-save>${te("settings.channels.save")}</button></div>`;
}

/** The signing secret, shown once after it was generated. */
const secretHtml = (secret) => `<h2 id="channel-dlg-title">${te("settings.channels.secretTitle")}</h2>
  <p>${te("settings.channels.secretHelp")}</p><pre class="channel-example" tabindex="0">${esc(secret)}</pre>
  <div class="dlg-actions"><button class="btn" type="button" data-channel-copy>${icon("copy")} ${te("settings.channels.copy")}</button>
    <button class="btn primary" type="button" data-channel-done>${te("settings.channels.done")}</button></div>`;

/** The editor dialog (add, or edit `channel`); resolves `true` when something was saved. Steps replace each other
 *  in one dialog (DS-CMP-DLG-2): the form, the consent (only when an address was pasted), then the secret if any. */
function openEditor(view, channel) {
  return new Promise((resolve) => {
    const d = document.createElement("dialog");
    d.className = "dlg channel-dlg";
    d.setAttribute("aria-labelledby", "channel-dlg-title");
    const body = document.createElement("div");
    body.className = "dlg-body";
    d.append(body);
    document.body.append(d);
    let saved = false;
    let draft = null;
    let url = "";
    d.addEventListener("close", () => { d.remove(); resolve(saved); }, { once: true });
    const error = (x) => { body.querySelector("[data-channel-err]").textContent = l10nError(x); };
    const read = () => {
      const kind = body.querySelector("[data-channel-kind] [aria-checked=true]")?.dataset.value ?? "ntfy";
      const names = body.querySelector("[data-channel-names]").value;
      return {
        kind,
        label: body.querySelector("[data-channel-label]").value,
        sign: kind === "webhook" && body.querySelector("[data-channel-sign]").checked,
        filters: {
          kinds: [...body.querySelectorAll("[data-channel-notice]:checked")].map((b) => b.dataset.channelNotice),
          accounts: [...body.querySelectorAll("[data-channel-account]:checked")].map((b) => b.dataset.channelAccount),
          min_severity: body.querySelector("[data-channel-severity]").value,
          include_names: names === "default" ? null : names === "on",
        },
      };
    };
    const finish = (result) => {
      saved = true;
      if (result.signing_secret) { secret(result.signing_secret); return; }
      d.close();
    };
    const save = async () => {
      try { finish(channel ? await channelEdit(channel.id, draft, url || null) : await channelAdd(draft, url)); } catch (x) { error(x); }
    };
    const form = () => {
      body.innerHTML = formHtml(view, channel);
      if (draft) restore();
      const kinds = body.querySelector("[data-channel-kind]");
      kinds.addEventListener("click", (e) => {
        const b = e.target.closest("[role=radio]");
        if (!b || b.disabled) return;
        for (const x of kinds.querySelectorAll("[role=radio]")) { x.setAttribute("aria-checked", String(x === b)); x.tabIndex = x === b ? 0 : -1; }
        body.querySelector("[data-channel-sign-row]").hidden = b.dataset.value !== "webhook";
        if (!channel) body.querySelector("[data-channel-url]").placeholder = t(`settings.channels.urlHint.${b.dataset.value}`);
      });
      body.querySelector("[data-channel-cancel]").onclick = () => d.close();
      body.querySelector("[data-channel-next]").onclick = async () => {
        draft = read();
        url = body.querySelector("[data-channel-url]").value.trim();
        if (!draft.filters.kinds.length) { error(t("settings.channels.noKinds")); return; }
        // RF4.3: an unchanged address saves without a new consent; a new one asks again.
        if (channel && !url) { await save(); return; }
        try { consent(await channelConsent(draft, url)); } catch (x) { error(x); }
      };
      body.querySelector("[data-channel-label]").focus();
    };
    const restore = () => {
      body.querySelector("[data-channel-label]").value = draft.label;
      body.querySelector("[data-channel-url]").value = url;
      for (const b of body.querySelectorAll("[data-channel-notice]")) b.checked = draft.filters.kinds.includes(b.dataset.channelNotice);
      for (const b of body.querySelectorAll("[data-channel-account]")) b.checked = draft.filters.accounts.includes(b.dataset.channelAccount);
      body.querySelector("[data-channel-severity]").value = draft.filters.min_severity;
      const names = draft.filters.include_names;
      body.querySelector("[data-channel-names]").value = names == null ? "default" : names ? "on" : "off";
      for (const x of body.querySelectorAll("[data-channel-kind] [role=radio]")) x.setAttribute("aria-checked", String(x.dataset.value === draft.kind));
      body.querySelector("[data-channel-sign-row]").hidden = draft.kind !== "webhook";
      body.querySelector("[data-channel-sign]").checked = draft.sign;
    };
    const consent = (c) => {
      body.innerHTML = consentHtml(c);
      body.querySelector("[data-channel-back]").onclick = form;
      body.querySelector("[data-channel-save]").onclick = save;
      body.querySelector("[data-channel-save]").focus();
    };
    const secret = (value) => {
      body.innerHTML = secretHtml(value);
      body.querySelector("[data-channel-copy]").onclick = () => navigator.clipboard?.writeText(value).then(() => toast(t("settings.channels.copied")));
      body.querySelector("[data-channel-done]").onclick = () => d.close();
    };
    form();
    showDialog(d);
  });
}

/** Loads the list into the sub-page and binds it; every action draws it again from the shell's answer. */
export async function bindChannels(root, ctx) {
  const box = root.querySelector("[data-channels-body]");
  if (!box) return;
  let view;
  const paint = async () => {
    try { view = await channelsView(); } catch (x) { box.innerHTML = `<p class="hint">${esc(l10nError(x))}</p>`; return; }
    box.innerHTML = listHtml(view);
    const desc = root.querySelector("#channels-desc");
    if (desc) desc.textContent = view.channels.length ? t("settings.channels.count", { count: view.channels.filter((c) => c.enabled).length }) : t("settings.channels.none");
  };
  const message = (text) => { const el = box.querySelector("#channels-msg"); if (el) el.textContent = text; };
  box.addEventListener("click", async (e) => {
    const target = e.target.closest("button");
    if (!target) return;
    const find = (id) => view.channels.find((c) => c.id === id);
    if (target.matches("[data-channel-add]")) {
      if (await openEditor(view, null)) { toast(); ctx.changed?.("channels"); await paint(); }
    } else if (target.dataset.channelEdit) {
      if (await openEditor(view, find(target.dataset.channelEdit))) { toast(); await paint(); }
    } else if (target.dataset.channelRemove) {
      const c = find(target.dataset.channelRemove);
      const ok = await askDialog("channel-remove", te("settings.channels.removeTitle", { label: esc(c.label) }),
        `<p>${te("settings.channels.removeBody")}</p>`, te("settings.channels.remove"));
      if (!ok) return;
      try { await channelRemove(c.id); toast(); await paint(); } catch (x) { message(l10nError(x)); }
    } else if (target.dataset.channelTest) {
      const c = find(target.dataset.channelTest);
      target.disabled = true;
      try {
        const outcome = await channelTest(c.id);
        message(outcome.outcome === "delivered"
          ? t("settings.channels.testOk", { host: c.host, status: outcome.status })
          : t("settings.channels.testFailed", { host: c.host, reason: outcome.status ?? outcome.error ?? "" }));
        await paint();
      } catch (x) { message(l10nError(x)); target.disabled = false; }
    }
  });
  box.addEventListener("change", async (e) => {
    const box_ = e.target.closest("[data-channel-on]");
    if (!box_) return;
    const want = box_.checked;
    try { await channelSetEnabled(box_.dataset.channelOn, want); rowError(box_, ""); toast(); } catch (x) { box_.checked = !want; rowError(box_, l10nError(x)); }
  });
  await paint();
}

