#!/usr/bin/env node
// Browser-free UI smoke for PRD 0018 RF3 and PRD 0022 RF17.2/RF17.4:
// render the production Settings About page and updater banner with every built-in catalog.
import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";

// A UTC-midnight release must keep its published day even west of UTC.
process.env.TZ = "America/Los_Angeles";

const root = new URL("../", import.meta.url);
const consentMode = process.argv[2] === "--consent" ? process.argv[3] : null;
const storage = new Map();
globalThis.window = {};
globalThis.document = {
  hidden: false,
  body: { append: () => {} },
  activeElement: null,
  documentElement: {
    lang: "",
    dataset: {},
    toggleAttribute: () => {},
  },
  addEventListener: () => {},
};
document.activeElement = document.body;
globalThis.MutationObserver = class {
  observe() {}
  disconnect() {}
};
globalThis.matchMedia = () => ({
  matches: false,
  addEventListener: () => {},
});
globalThis.addEventListener = () => {};
Object.defineProperty(globalThis, "navigator", {
  value: { languages: ["en-US"], language: "en-US" },
  configurable: true,
});
globalThis.location = { search: consentMode === "managed" ? "?update=managed" : consentMode === "yes" || consentMode === "switch" ? "?update=consent&update-consent-race" : consentMode ? "?update=consent" : "" };
globalThis.localStorage = {
  getItem: (key) => storage.get(key) ?? null,
  setItem: (key, value) => storage.set(key, value),
};
globalThis.fetch = async (relative) => {
  try {
    const path = new URL(`apps/desktop/ui/${relative}`, root);
    const data = JSON.parse(await readFile(fileURLToPath(path), "utf8"));
    return { ok: true, json: async () => data };
  } catch {
    return { ok: false, json: async () => ({}) };
  }
};

const i18n = await import("../apps/desktop/ui/js/i18n.js");
const updater = await import("../apps/desktop/ui/js/updater.js");
const lib = await import("../apps/desktop/ui/js/lib.js");
const aboutPage = await import("../apps/desktop/ui/js/settings/about.js");
await i18n.initI18n();
await updater.initUpdates();
await new Promise((resolve) => setTimeout(resolve, 0));

/** A minimal element for `bindUpdates`: listeners, `textContent`, `checked` and `disabled`. */
const fakeElement = (extra = {}) => ({
  textContent: "", checked: false, disabled: false, listeners: {},
  addEventListener(type, cb) { this.listeners[type] = cb; },
  ...extra,
});

if (consentMode === "switch") {
  // V-0018-07: while the question is unanswered, the Settings switch answers it; V-0018-05: the phase line follows
  // a download's progress without a page redraw.
  await i18n.setLocale("en-US");
  const pending = updater.updatesSectionHtml(updater.updateState());
  const pendingSwitch = pending.match(/<input id="update-automatic"[^>]*>/)?.[0] ?? "";
  assert.ok(!pendingSwitch.includes("disabled"), "the unanswered switch must answer the question");
  const elements = {
    "#update-automatic": fakeElement(),
    "#update-msg": fakeElement(),
    "#update-last-check": fakeElement(),
    "[data-update-phase]": fakeElement(),
    "[data-update-check]": fakeElement(),
  };
  const unbind = updater.bindUpdates({ querySelector: (selector) => elements[selector] ?? null });
  const automatic = elements["#update-automatic"];
  automatic.checked = true;
  await automatic.listeners.change({ target: automatic });
  await new Promise((resolve) => setTimeout(resolve, 0));
  assert.equal(updater.updateState().consent_pending, false, "the switch did not answer the question");
  assert.equal(updater.updateState().automatic, true, "the switch's answer was not yes");
  assert.equal(elements["#update-msg"].textContent, "", "answering through the switch showed an error");
  const phase = elements["[data-update-phase]"];
  const seen = new Set();
  const install = lib.updaterInstall();
  for (let i = 0; i < 40 && updater.updateState().phase !== "ready"; i++) {
    await new Promise((resolve) => setTimeout(resolve, 50));
    seen.add(phase.textContent);
  }
  await install;
  assert.ok([...seen].some((text) => /18%/.test(text)) && [...seen].some((text) => /78%/.test(text)), `the progress line did not move: ${[...seen].join(" | ")}`);
  assert.ok([...seen].every((text) => !text.includes("{")), `a placeholder leaked: ${[...seen].join(" | ")}`);
  unbind();
  console.log("PASS: the Settings switch answers the update question and the progress line moves");
  process.exit(0);
}

if (consentMode) {
  assert.ok(["close", "no", "yes", "managed"].includes(consentMode), `unknown consent mode: ${consentMode}`);
  document.hidden = true;
  const controls = new Map(
    ["[data-update-consent-close]", "[data-update-consent-no]", "[data-update-consent-yes]", "[data-update-consent-error]"]
      .map((selector) => [selector, {
        disabled: false,
        focused: false,
        onclick: null,
        textContent: "",
        focus() { this.focused = true; document.activeElement = this; },
      }]),
  );
  let dialogHtml = "";
  let removed = false;
  const closeListeners = [];
  const dialog = {
    open: false,
    isConnected: true,
    className: "",
    setAttribute: () => {},
    set innerHTML(html) { dialogHtml = html; },
    querySelector: (selector) => controls.get(selector) ?? null,
    addEventListener(type, callback) { if (type === "close") closeListeners.push(callback); },
    showModal() { this.open = true; },
    close() {
      this.open = false;
      for (const callback of closeListeners.splice(0)) callback();
    },
    remove() { removed = true; this.isConnected = false; },
    contains: () => false,
  };
  document.body = { append: (node) => assert.equal(node, dialog) };
  document.createElement = (tag) => {
    assert.equal(tag, "dialog");
    return dialog;
  };

  if (consentMode === "managed") {
    assert.equal(updater.updateState().consent_pending, false, "managed installs must not be asked");
    assert.equal(updater.updateState().enabled, false, "managed installs must keep internal checks off");
    assert.equal(await updater.promptForUpdateConsent(), false, "managed installs opened the consent dialog");
    assert.equal(dialog.open, false, "managed consent dialog unexpectedly opened");
    await assert.rejects(lib.updaterCheckNow(), (error) => error === "updater_off");
    console.log("PASS: updater consent stays hidden and requests stay off for managed installs");
    process.exit(0);
  }

  assert.equal(updater.updateState().consent_pending, true, "fresh profile must await an answer");
  assert.equal(updater.updateState().enabled, false, "fresh profile must keep checks disabled");
  await assert.rejects(lib.updaterCheckNow(), (error) => error === "updater_consent_required");
  assert.equal(await updater.promptForUpdateConsent(), true, "pending answer must open the dialog");
  assert.match(dialogHtml, /data-update-consent-yes/, "Yes action missing");
  assert.match(dialogHtml, /data-update-consent-no/, "No action missing");
  assert.ok(controls.get("[data-update-consent-no]").focused, "No must receive cautious initial focus");

  if (consentMode === "close") {
    controls.get("[data-update-consent-close]").onclick();
    await new Promise((resolve) => setTimeout(resolve, 0));
    assert.equal(updater.updateState().consent_pending, true, "closing must not answer");
    assert.equal(updater.updateState().enabled, false, "closing must keep every check off");
    await assert.rejects(lib.updaterCheckNow(), (error) => error === "updater_consent_required");
    assert.equal(await updater.promptForUpdateConsent(), false, "closing must not reopen in the same launch");
  } else {
    controls.get(`[data-update-consent-${consentMode}]`).onclick();
    await new Promise((resolve) => setTimeout(resolve, 0));
    const state = updater.updateState();
    assert.equal(state.consent_pending, false, `${consentMode}: answer was not recorded`);
    assert.equal(state.automatic, consentMode === "yes", `${consentMode}: automatic choice differs`);
    assert.equal(state.enabled, consentMode === "yes", `${consentMode}: request gate differs`);
    if (consentMode === "yes") {
      assert.equal(state.phase, "available", "newer updater event was overwritten by consent response");
      assert.match(updater.updateBannerHtml(), /data-update="available"/, "available update disappeared after consent");
    }
    if (consentMode === "no") {
      await assert.rejects(lib.updaterCheckNow(), (error) => error === "updater_off");
    }
  }
  assert.equal(dialog.open, false, `${consentMode}: dialog did not close`);
  assert.equal(removed, true, `${consentMode}: closed dialog was not removed`);
  console.log(`PASS: updater consent ${consentMode} path uses the production dialog and request gate`);
  process.exit(0);
}

// PRD 0020 RF2.5: Updates lives on the Settings window's About page; render that production page module.
const settingsPageHtml = async () => aboutPage.html({ settings: await lib.getSettings(), update: await lib.updaterStatus() });

const base = {
  enabled: true,
  automatic: true,
  disabled_reason: null,
  channel: "stable",
  current_version: "1.2.3",
  last_check: null,
  consent_pending: false,
  phase: "available",
  version: "1.2.4",
  notes: "Fixes",
  date: "2026-10-22T00:00:00Z",
  can_install: true,
  upgrade_command: null,
  progress: null,
  dismissed: false,
  error: null,
  manual_url: "https://github.com/jmarcon/cuotai/releases/latest",
};

const expectedReleaseDates = {
  "pt-BR": "publicada em 22 de out. de 2026",
  "en-US": "released on Oct 22, 2026",
  es: "publicada el 22 oct 2026",
  it: "pubblicato il 22 ott 2026",
  de: "veröffentlicht am 22. Okt. 2026",
  ru: "выпущено 22 окт. 2026 г.",
  ko: "2026년 10월 22일 출시",
  "hi-IN": "22 अक्टू॰ 2026 को जारी",
  "id-ID": "dirilis pada 22 Okt 2026",
  tr: "22 Eki 2026 tarihinde yayımlandı",
  vi: "phát hành ngày 22 thg 10, 2026",
  "zh-CN": "发布于 2026年10月22日",
  fr: "publiée le 22 oct. 2026",
  ja: "2026年10月22日リリース",
};

let expectedUpdateKeys;
const renderedHeadings = [];
const renderedDates = [];
for (const { code } of i18n.LOCALES) {
  const catalogPath = new URL(`apps/desktop/ui/locales/${code}.json`, root);
  const catalog = JSON.parse(await readFile(fileURLToPath(catalogPath), "utf8"));
  const updateKeys = Object.keys(catalog).filter((key) => key.startsWith("update.")).sort();
  expectedUpdateKeys ??= updateKeys;
  assert.deepEqual(updateKeys, expectedUpdateKeys, `${code}: updater catalog keys drifted`);

  await i18n.setLocale(code);
  assert.equal(lib.fmtDate(null), null, `${code}: absent release date must stay absent`);
  assert.equal(lib.fmtDate("not-a-date"), null, `${code}: invalid release date must stay absent`);
  const section = updater.updatesSectionHtml(base);
  const page = await settingsPageHtml();
  const headings = Array.from(page.matchAll(/<h2>([^<]*)<\/h2>/g), (match) => match[1])
    .filter((heading) => heading === catalog["update.title"]);
  assert.match(section, /^<section class="set-sec" data-updates>/, `${code}: updater section wrapper missing`);
  assert.deepEqual(headings, [catalog["update.title"]], `${code}: the About page must render exactly one updater heading`);
  renderedHeadings.push(`${code}: ${headings[0]}`);
  assert.match(section, /id="update-automatic"/, `${code}: settings switch missing`);
  assert.ok(!section.includes("update."), `${code}: untranslated updater key rendered`);
  const consent = updater.updateConsentDialogHtml();
  assert.ok(consent.includes(catalog["update.consent.title"]), `${code}: consent question missing`);
  assert.ok(consent.includes(lib.esc(catalog["update.consent.body"])), `${code}: consent disclosure missing`);
  assert.ok(consent.includes(lib.esc(catalog["update.consent.no"])), `${code}: No action missing`);
  assert.ok(consent.includes(lib.esc(catalog["update.consent.yes"])), `${code}: Yes action missing`);
  assert.match(consent, /data-update-consent-close[^>]*aria-label=/, `${code}: close action lacks an accessible name`);
  assert.ok(
    consent.indexOf("data-update-consent-no") < consent.indexOf("data-update-consent-yes"),
    `${code}: primary Yes action must be last`,
  );

  const banner = updater.updateBannerHtml();
  assert.match(banner, /data-update="available"/, `${code}: available banner missing`);
  assert.ok(banner.includes(catalog["update.now"]), `${code}: update action did not render`);

  // PRD 0019 RF7.7 (re-validation V-01): the notes are drawn from the structured view, never raw Markdown or refs.
  const notesBody = banner.match(/<div class="upd-notes-body" data-update-notes>[\s\S]*<\/div>/)?.[0] ?? "";
  assert.ok(notesBody, `${code}: structured notes missing`);
  assert.match(notesBody, /<h4>0\.72\.0 <small>2026-10-22<\/small><\/h4>/, `${code}: newest version heading missing`);
  assert.match(notesBody, /<h4>0\.71\.1 /, `${code}: second section missing`);
  assert.equal((notesBody.match(/<li>/g) ?? []).length, 4, `${code}: entries not listed`);
  assert.doesNotMatch(notesBody, /##|PRD \d|\{count\}|changelog\.category/, `${code}: raw Markdown, refs or keys leaked`);
  assert.ok(notesBody.includes(lib.esc(catalog["update.notesFull"])), `${code}: release-page link missing`);
  assert.match(notesBody, /data-update-manual/, `${code}: the link must open the release page`);
  assert.match(notesBody, /<p class="hint">[^<]*3[^<]*<\/p>/, `${code}: "and N more" missing`);
  {
    const st = updater.updateState();
    const saved = st.notes;
    st.notes = null;
    const bare = updater.updateBannerHtml();
    assert.doesNotMatch(bare, /<li>|<h4>/, `${code}: unreadable notes must show no body`);
    assert.ok(bare.includes(lib.esc(catalog["update.notesFull"])), `${code}: unreadable notes keep the link`);
    st.notes = saved;
  }
  // PRD 0019 RF1.1 (re-validation V-02): Settings › About shows the dual licence from the shell.
  assert.ok(page.includes("MIT OR Apache-2.0"), `${code}: About must show the dual licence`);

  const renderedDate = banner.match(/<span class="hint">([^<]+)<\/span>/)?.[1] ?? "";
  assert.equal(renderedDate, expectedReleaseDates[code], `${code}: release date is not localized`);
  assert.doesNotMatch(banner, /\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}(?:\.\d+)?Z/, `${code}: raw ISO release timestamp rendered`);
  const updateState = updater.updateState();
  const releaseDate = updateState.date;
  for (const date of [null, "not-a-date"]) {
    updateState.date = date;
    assert.doesNotMatch(
      updater.updateBannerHtml(),
      /<span class="hint">/,
      `${code}: ${date == null ? "absent" : "invalid"} release date rendered a label`,
    );
  }
  updateState.date = releaseDate;
  renderedDates.push(`${code}: ${renderedDate}`);
  // V-0018-02 / R-0018-02: a failed check stays off the dashboard; a failed download or install is a dismissable
  // status (not an alert), and a download that kept its update offers a retry.
  {
    const saved = { ...updateState };
    Object.assign(updateState, { phase: "failed", error: "timeout", failed_stage: "check", can_retry: false, dismissed: false });
    assert.equal(updater.updateBannerHtml(), "", `${code}: a failed check must not raise a dashboard notice`);
    Object.assign(updateState, { error: "network", failed_stage: "download", can_retry: true });
    const failed = updater.updateBannerHtml();
    assert.match(failed, /role="status"/, `${code}: a failed download is a status, not an alert`);
    assert.doesNotMatch(failed, /role="alert"/, `${code}: a failed download must not be an alert`);
    assert.ok(failed.includes(lib.esc(catalog["update.error.network"])), `${code}: download failure text missing`);
    assert.ok(failed.includes(lib.esc(catalog["update.retry"])), `${code}: retry action missing`);
    assert.match(failed, /data-update-dismiss[^>]*aria-label=/, `${code}: dismiss lacks an accessible name`);
    Object.assign(updateState, { error: "io", failed_stage: "install", can_retry: false });
    assert.match(updater.updateBannerHtml(), /data-update-manual/, `${code}: a failed install offers the manual link`);
    updateState.dismissed = true;
    assert.equal(updater.updateBannerHtml(), "", `${code}: a dismissed failure must stay closed`);
    for (const key of Object.keys(updateState)) delete updateState[key];
    Object.assign(updateState, saved);
  }
  const preferenceOff = updater.updatesSectionHtml({
    ...base,
    enabled: false,
    automatic: false,
    disabled_reason: "preference",
  });
  const preferenceSwitch = preferenceOff.match(/<input id="update-automatic"[^>]*>/)?.[0] ?? "";
  assert.ok(!preferenceSwitch.includes("disabled"), `${code}: preference-off switch cannot be re-enabled`);
  const pending = updater.updatesSectionHtml({
    ...base,
    enabled: false,
    automatic: false,
    consent_pending: true,
  });
  const pendingSwitch = pending.match(/<input id="update-automatic"[^>]*>/)?.[0] ?? "";
  assert.ok(!pendingSwitch.includes("disabled"), `${code}: unanswered switch must answer the question (V-0018-07)`);
  assert.equal(pending.split(lib.esc(catalog["update.consent.pending"])).length - 1, 1, `${code}: the pending answer must show once`);
  assert.equal(preferenceOff.split(lib.esc(catalog["update.off.preference"])).length - 1, 1, `${code}: the off reason must show once`);

  const policyOff = updater.updatesSectionHtml({
    ...base,
    enabled: false,
    disabled_reason: "env",
  });
  const policySwitch = policyOff.match(/<input id="update-automatic"[^>]*>/)?.[0] ?? "";
  assert.ok(policySwitch.includes("disabled"), `${code}: external policy switch must stay locked`);
  assert.ok(!/\bchecked\b/.test(policySwitch), `${code}: a policy that keeps checks off must not show the switch on (V-0018-04)`);

  // V-0018-01 / R-0018-01: every phase and every refusal renders as a translated sentence, with no placeholder or
  // raw code left.
  const raw = /\{[a-z]+\}|\b[a-z]+_[a-z_]+\b/;
  const phases = [
    { phase: "idle" }, { phase: "checking" }, { phase: "uptodate" }, { phase: "available" },
    { phase: "downloading", progress: 42 }, { phase: "downloading", progress: null }, { phase: "installing" }, { phase: "ready" },
    { phase: "failed", error: "timeout", failed_stage: "check" }, { phase: "failed", error: "network", failed_stage: "download", can_retry: true },
    { phase: "failed", error: "io", failed_stage: "install" },
  ];
  for (const extra of phases) {
    const view = { ...base, ...extra };
    const line = updater.updatesSectionHtml(view).match(/<p class="hint" data-update-phase>([^<]*)<\/p>/)?.[1] ?? "";
    assert.ok(line && !raw.test(line), `${code}: Settings phase line for ${JSON.stringify(extra)}: ${line}`);
    const about = updater.updateStateText(view);
    assert.ok(about && !raw.test(about), `${code}: About state for ${JSON.stringify(extra)}: ${about}`);
    if (extra.phase === "installing" || extra.phase === "ready") assert.ok(about.includes("1.2.4"), `${code}: ${extra.phase} lacks the version`);
  }
  assert.ok(updater.updateStateText({ ...base, phase: "failed", failed_stage: "download" }) !== updater.updateStateText({ ...base, phase: "failed", failed_stage: "check" }),
    `${code}: a failed download must not read as a failed check`);
  for (const refusal of ["login_in_progress", "no_update", "stale", "unavailable", "already_running", "updater_off",
    "updater_consent_required", "unwritable", "network", "timeout", "signature", "io", "insecure_endpoint", "other"]) {
    const text = lib.updateErrText(refusal);
    assert.equal(text, catalog[`update.error.${refusal}`], `${code}: ${refusal} is not translated`);
    assert.ok(!raw.test(text), `${code}: ${refusal} renders a raw code: ${text}`);
  }
  assert.ok(policyOff.includes("CUOTAI_UPDATER=off"), `${code}: environment override is not visible`);
}

const consentSmokes = [];
for (const mode of ["close", "no", "yes", "managed", "switch"]) {
  const child = spawnSync(process.execPath, [fileURLToPath(import.meta.url), "--consent", mode], {
    encoding: "utf8",
  });
  assert.equal(child.status, 0, `${mode} consent smoke failed:\n${child.stdout}\n${child.stderr}`);
  consentSmokes.push(child.stdout.trim());
}
console.log(consentSmokes.join("\n"));

console.log(`Rendered updater headings: ${renderedHeadings.join(" | ")}`);
console.log(`Rendered release dates: ${renderedDates.join(" | ")}`);
console.log(`PASS: updater Settings renders one heading, a localized release date and translated states in all ${i18n.LOCALES.length} languages`);
