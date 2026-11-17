// PRD 0032 (quiet hours) with PRD 0020 RF8: the weekday editor works from the keyboard alone. A real browser drives
// the Settings > Alerts > Quiet hours sub-page of the demo with Tab, Shift+Tab, Enter, Space and typed digits: open
// the sub-page, switch the schedule on, add an interval (focus lands on its first time field), type a time, remove
// it (focus returns to the weekday's Add button), repeat the schedule on every day, and reach the three-interval
// limit. It then opens Report a problem and What's new, whose content loads after they open, and checks where focus
// lands. Exits 1 on the first step that needs a mouse. Run from the repository root; needs Chrome (see README).
import assert from "node:assert/strict";
import { launch } from "./driver.mjs";

const browser = await launch();
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const ok = (name) => console.log(`ok   ${name}`);
const eq = async (expression, expected, message) => assert.deepEqual(await browser.evaluate(expression), expected, message);
/** Polls `expression` in the page until it is truthy (the UI answered), instead of sleeping a fixed time and reading
 *  once; fails with `message` after `timeout` ms. Returns the value. */
const until = async (expression, message, { timeout = 5000, every = 50 } = {}) => {
  const end = Date.now() + timeout;
  for (;;) {
    const value = await browser.evaluate(expression);
    if (value) return value;
    if (Date.now() >= end) throw new Error(`${message}: not true after ${timeout} ms: ${expression}`);
    await sleep(every);
  }
};
/** What has focus, as a short selector-like name. */
const focused = () => browser.evaluate(`(() => {
  const el = document.activeElement;
  if (!el || el === document.body) return "body";
  if (el.id) return "#" + el.id;
  const own = ["data-open-sub", "data-quiet-add", "data-quiet-same", "data-q-from", "data-q-to", "data-q-remove"].find((a) => el.hasAttribute(a));
  const day = el.closest("[data-q-day]");
  return (own ? "[" + own + (el.getAttribute(own) ? "=" + el.getAttribute(own) : "") + "]" : el.tagName.toLowerCase())
    + (day ? "@" + day.dataset.qDay + "#" + day.dataset.qIndex : "");
})()`);
/** Tab (or Shift+Tab) until `expected` has focus; the ⓘ buttons of the rows sit in between. */
const tabTo = async (expected, step, { shift = false, max = 6 } = {}) => {
  for (let i = 0; i < max && (await focused()) !== expected; i++) await browser.press("Tab", { shift });
  await expectFocus(expected, step);
};
const expectFocus = async (expected, step) => assert.equal(await focused(), expected, `${step}: focus is on ${await focused()}, expected ${expected}`);

try {
  await browser.open("settings.html?page=alerts", { w: 900, h: 760, scale: 1 });
  // 1. The quiet-hours row opens its sub-page with Enter.
  await browser.evaluate(`document.querySelector('[data-open-sub="quiet"]').focus()`);
  await expectFocus("[data-open-sub=quiet]", "row button");
  await browser.press("Enter");
  await until(`!!document.querySelector('#quiet-on')?.offsetParent`, "Enter opens the quiet-hours sub-page");
  ok("Enter opens the sub-page");

  // 2. The schedule switch toggles with Space, and the weekday controls follow.
  await browser.evaluate(`document.querySelector('#quiet-on').focus()`);
  await eq(`document.querySelector('[data-quiet-add="mon"]').disabled`, true, "weekday controls wait for the switch");
  await browser.press(" ");
  await until(`document.querySelector('#quiet-on').checked`, "Space turns the schedule on");
  await until(`document.querySelector('[data-quiet-add="mon"]').disabled === false`, "the weekday controls turn on with it");
  ok("Space toggles the schedule switch");

  // 3. Tab reaches the digest switch, the "same every day" shortcut and each weekday's Add button, in reading order
  //    (the ⓘ buttons of the rows sit in between).
  await tabTo("#quiet-digest", "Tab to the digest switch");
  await tabTo("[data-quiet-same]", "Tab to Same every day");
  await tabTo("[data-quiet-add=mon]", "Tab to Monday");
  await tabTo("[data-quiet-add=tue]", "Tab to Tuesday");
  await tabTo("[data-quiet-add=mon]", "Shift+Tab back to Monday", { shift: true });
  ok("Tab and Shift+Tab walk the controls in reading order");

  // 4. Enter on Add creates the interval and moves focus to its "from" field.
  await browser.press("Enter");
  await until(`document.querySelectorAll('[data-q-day="mon"]').length === 1`, "Enter adds an interval");
  await expectFocus("[data-q-from]@mon#0", "after Add");
  await eq(`document.querySelector('[data-q-day="mon"] [data-q-from]').getAttribute('aria-label').length > 0`, true, "the time field is labelled");
  ok("Enter adds an interval and focuses its time field");

  // 5. Backspace empties the hour: the value is incomplete, so nothing is saved, but the field keeps the focus (the
  //    editor is not drawn again under the cursor) and the typing goes on. Then a time is typed with digits and Tab
  //    moves from field to field. The field follows the machine's clock: 24-hour (here "22:00") or 12-hour with a
  //    day-period segment ("10:00 PM", macOS CI runners in en-US). Backspace clears only the hour, so in a 12-hour
  //    field "0930" alone keeps PM and reads 21:30; the "a" that follows picks AM there and is ignored by a
  //    24-hour field (its last segment is the minutes, which take digits only), so both read 09:30.
  await browser.press("Backspace");
  await until(`document.querySelector('[data-q-day="mon"] [data-q-from]')?.value === ""`, "the emptied hour leaves no value");
  await expectFocus("[data-q-from]@mon#0", "Backspace keeps the focus in the time field");
  for (const key of ["0", "9", "3", "0", "a"]) await browser.press(key);
  await expectFocus("[data-q-from]@mon#0", "typing stays in the time field");
  await eq(`document.querySelector('[data-q-day="mon"] [data-q-from]').value`, "09:30", "the typed time reads 09:30 in either clock");
  // The last segment (minutes, or AM/PM) has focus: Tab leaves the field for the "to" field.
  for (let i = 0; i < 4 && !(await focused()).startsWith("[data-q-to]"); i++) await browser.press("Tab");
  await until(`import("./js/lib.js").then((m) => m.getSettings()).then((s) => s.quiet.days.mon[0]?.from === "09:30")`, "the typed time is saved");
  await expectFocus("[data-q-to]@mon#0", "Tab reaches the second time field");
  await eq(`document.querySelector('[data-q-day="mon"] [data-q-from]').value`, "09:30", "the typed time stays after leaving the field");
  ok("digits set a time and Tab moves between fields");

  // 5b. An incomplete time gets the saved one back when the focus leaves it, without moving the focus, and the
  //     note shown while it was incomplete goes away.
  const note = `document.querySelector('[data-q-day="mon"]').closest('.set-row').querySelector('.set-row-err').textContent`;
  await eq(`${note} === ""`, true, "a complete time shows no note");
  await browser.evaluate(`(() => { const f = document.querySelector('[data-q-day="mon"] [data-q-to]'); f.value = ""; f.dispatchEvent(new Event("change", { bubbles: true })); })()`);
  await eq(`${note} !== ""`, true, "an incomplete time shows why it is not saved");
  for (let i = 0; i < 4 && (await focused()) === "[data-q-to]@mon#0"; i++) await browser.press("Tab");
  await expectFocus("[data-q-remove]@mon#0", "Tab leaves the incomplete time for Remove");
  await until(`document.querySelector('[data-q-day="mon"] [data-q-to]').value === "08:00"`, "leaving an incomplete time restores the saved one");
  await until(`${note} === ""`, "the note goes away with the restored time");
  ok("an incomplete time is put back on leaving it");

  // 6. Enter on the remove button (reached with Tab above) deletes the interval; focus goes back to the weekday's
  //    Add button.
  await browser.press("Enter");
  await until(`document.querySelectorAll('[data-q-day="mon"]').length === 0`, "Enter removes the interval");
  await expectFocus("[data-quiet-add=mon]", "after Remove");
  ok("Enter removes an interval and returns focus to Add");

  // 7. "Same every day" with Enter fills the seven weekdays and keeps focus on its own button.
  await browser.evaluate(`document.querySelector('[data-quiet-same]').focus()`);
  await browser.press("Enter");
  await until(`document.querySelectorAll('[data-q-day]').length === 7`, "one interval per weekday");
  await expectFocus("[data-quiet-same]", "after Same every day");
  ok("Same every day fills the week and keeps focus");

  // 8. The fourth interval of a day is refused: after three, Add is disabled.
  for (const count of [2, 3]) {
    await browser.evaluate(`document.querySelector('[data-quiet-add="mon"]').focus()`);
    await browser.press("Enter");
    await until(`document.querySelectorAll('[data-q-day="mon"]').length === ${count}`, `${count} intervals on Monday`);
  }
  await eq(`document.querySelector('[data-quiet-add="mon"]').disabled`, true, "the fourth is refused");
  ok("at most three intervals per day");

  // 9. Every control on the page is reachable and has an accessible name.
  await eq(`[...document.querySelectorAll('[data-quiet-days] input, [data-quiet-days] button')].filter((el) => !el.getAttribute('aria-label') && !el.textContent.trim() && !el.title).length`, 0, "unnamed control");
  ok("every control has a name");

  // 10. Report a problem and What's new replace their content once it loads, which destroys the control `showModal`
  //     focused: focus must land on the first control of the new content, never on `body`, and Escape must give it back.
  await browser.open("settings.html?page=about", { w: 900, h: 760, scale: 1 });
  for (const [opener, dialog] of [["data-report-problem", ".report-dlg"], ["data-whats-new", ".whats-new"]]) {
    await browser.evaluate(`document.querySelector('[${opener}]').focus()`);
    await browser.press("Enter");
    await until(`!!document.querySelector('${dialog}')?.open`, `Enter opens ${dialog}`);
    await until(`!!document.querySelector('${dialog} .dlg-shell-body')?.textContent.trim()`, `${dialog} has its content`);
    await eq(`document.activeElement.matches('${dialog} [data-shell-close]')`, true, `${dialog}: focus is on ${await focused()}, expected the first control`);
    await browser.press("Tab");
    await eq(`document.querySelector('${dialog}').contains(document.activeElement)`, true, `Tab stays in ${dialog}`);
    await browser.press("Escape");
    await until(`document.activeElement.hasAttribute('${opener}')`, `Escape returns focus to ${opener}`);
    ok(`${dialog}: focus survives the content replacement`);
  }
  // 11. PRD 0020 RF4.3 (re-validation R-04): a refused value keeps focus in its field, shows the row's error and
  //     returns to the saved value.
  await browser.open("settings.html?page=integrations", { w: 900, h: 760, scale: 1 });
  await until(`!!document.querySelector('#serve-port')`, "the Integrations page is drawn");
  await browser.evaluate(`(() => { const d = document.querySelector('[data-disc="serve-connection"]'); d.open = true; const f = document.querySelector('#serve-port'); f.focus(); f.select(); })()`);
  for (const ch of "80") await browser.press(ch);
  await browser.press("Enter");
  await until(`document.querySelector('#serve-port-msg').textContent.trim().length > 0`, "the row shows why");
  await until(`document.querySelector('#serve-port').value === "6738"`, "a refused port returns to the saved value");
  await expectFocus("#serve-port", "refused port");
  ok("a refused value keeps its row's error and the focus");

  // 11b. PRD 0031 re-validation V-06: without a token, "Share usage statistics" stays in the tab order with
  //      `aria-disabled`, is described by a visible reason, and Space changes nothing. V-02: every server change
  //      repaints the group, and the focus stays on the control that made it.
  await tabTo("#serve-stats", "Tab reaches the blocked stats switch", { max: 10 });
  await eq(`(() => { const s = document.querySelector('#serve-stats'); const d = document.getElementById(s.getAttribute('aria-describedby') ?? '');
    return [s.disabled, s.getAttribute('aria-disabled'), !!d && d.offsetParent !== null && d.textContent.trim().length > 0]; })()`,
  [false, "true", true], "the blocked switch is focusable and described by a visible reason");
  await browser.press(" ");
  await until(`document.querySelector('#serve-stats-msg').textContent.trim().length > 0`, "the refusal says why in the row");
  await eq(`document.querySelector('#serve-stats').checked`, false, "Space leaves the blocked switch off");
  await eq(`import("./js/lib.js").then((m) => m.serveStatus()).then((s) => s.stats)`, false, "nothing is saved");
  await expectFocus("#serve-stats", "Space on the blocked switch");
  ok("a blocked switch stays focusable, explains itself and refuses Space");
  await browser.evaluate(`document.querySelector('#serve-on').focus()`);
  await browser.press(" ");
  await until(`document.querySelector('[data-serve-state] code')?.textContent.includes('127.0.0.1')`, "Space turns the server on");
  await expectFocus("#serve-on", "after the server switch repaints the group");
  await eq(`document.querySelector('#serve-stats').hasAttribute('aria-disabled')`, false, "the token unblocks the stats switch");
  await browser.evaluate(`document.querySelector('#serve-stats').focus()`);
  await browser.press(" ");
  await until(`document.querySelector('#serve-stats').checked`, "Space turns the statistics on");
  await expectFocus("#serve-stats", "after the stats switch repaints the group");
  await browser.evaluate(`document.querySelector('#serve-access-log [data-value="errors"]').focus()`);
  await browser.press(" ");
  await until(`import("./js/lib.js").then((m) => m.serveStatus()).then((s) => s.access_log === "errors")`, "Space picks the access-log mode");
  await until(`document.activeElement?.matches('#serve-access-log [data-value="errors"]')`, "focus stays on the chosen access-log mode");
  await browser.evaluate(`document.querySelector('[data-serve-token-new]').focus()`);
  await browser.press("Enter");
  await until(`document.activeElement?.hasAttribute('data-serve-token-new')`, "focus stays on Renew token after the repaint");
  ok("server changes keep the focus on their control");

  // 12. RF6.2 (re-validation R-04, V-05): a search result inside a closed disclosure opens it, highlights the row and
  //     moves focus into the row; a second Esc in the empty field moves focus to the page title.
  await browser.open("settings.html?page=general", { w: 900, h: 760, scale: 1 });
  await until(`!!document.querySelector('[data-search]') && !!document.querySelector('#set-page-title')`, "the General page is drawn");
  await browser.evaluate(`localStorage.removeItem("cuotai.settings.disc.serve-connection")`);
  await browser.press("/");
  await until(`document.activeElement?.matches('[data-search]')`, "/ focuses the search field");
  for (const ch of "port") await browser.press(ch);
  // The search indexes every page before it answers: wait for its first result, however long that takes here.
  await until(`!!document.querySelector('.set-result')?.dataset.rowKey`, "the search shows a result", { timeout: 10000 });
  await eq(`document.querySelector('.set-result')?.dataset.rowKey`, "port", "the exact match comes first");
  await browser.evaluate(`document.querySelector('.set-result').focus()`);
  await browser.press("Enter");
  await until(`!!document.querySelector('[data-row="port"]')?.closest('details')?.open`, "the disclosure holding the row opens");
  await until(`!!document.querySelector('[data-row="port"]')?.classList.contains('found')`, "the row is highlighted");
  await until(`document.activeElement?.id === "serve-port"`, "focus moves into the row");
  await expectFocus("#serve-port", "after choosing a result");
  await browser.evaluate(`document.querySelector('[data-search]').focus()`);
  for (let i = 0; i < 2 && (await browser.evaluate(`document.activeElement.matches('[data-search]')`)); i++) {
    // The first Esc empties a field with text; Esc in the empty field leaves it.
    const typed = await browser.evaluate(`document.querySelector('[data-search]').value !== ""`);
    await browser.press("Escape");
    if (typed) await until(`document.querySelector('[data-search]').value === ""`, "Esc empties the search");
    else await until(`!document.activeElement.matches('[data-search]')`, "Esc in the empty search leaves it");
  }
  await until(`document.activeElement?.id === "set-page-title"`, "the second Esc moves focus to the page title");
  await expectFocus("#set-page-title", "second Esc in the search");
  ok("a search result reveals its row and focus follows");

  // 13. PRD 0032 re-validation C-06: a typed duplicate interval is refused by the core, the field that holds it is
  //     marked (`aria-invalid`, described by the row's error line, which names the interval it repeats in the
  //     language's clock), nothing is saved, and leaving the interval puts the saved times back and clears the mark.
  //     "Add interval" then proposes the next free hour, and the fourth is unavailable.
  await browser.open("settings.html?page=alerts&sub=quiet&quiet", { w: 900, h: 760, scale: 1 });
  await until(`!!document.querySelector('[data-q-day="mon"][data-q-index="1"] [data-q-from]')?.offsetParent`, "the quiet-hours sub-page opens on the demo schedule");
  const second = `document.querySelector('[data-q-day="mon"][data-q-index="1"] [data-q-from]')`;
  const monMsg = `document.querySelector('#quiet-mon-msg').textContent`;
  await browser.evaluate(`(() => { const row = document.querySelector('[data-q-day="mon"][data-q-index="1"]'); row.querySelector('[data-q-from]').focus();
    row.querySelector('[data-q-from]').value = "22:00"; row.querySelector('[data-q-to]').value = "07:00";
    row.querySelector('[data-q-from]').dispatchEvent(new Event("change", { bubbles: true })); })()`);
  await until(`${second}.getAttribute('aria-invalid') === "true"`, "the duplicate's field is marked invalid");
  await eq(`${second}.getAttribute('aria-describedby').split(' ')`, ["quiet-mon-msg", "q-readout-mon-1"], "the field is described by the row's error, then its read-out");
  await eq(`${monMsg}.replace(/\\u00a0/g, " ")`, "Already set: 10:00 PM–7:00 AM (Monday).", "the message names the repeated interval in the language's clock");
  await eq(`document.querySelector('#quiet-mon-msg').getAttribute('role')`, "alert", "the error line is announced");
  await eq(`import("./js/lib.js").then((m) => m.getSettings()).then((s) => s.quiet.days.mon[1])`, { from: "12:00", to: "13:00" }, "nothing is saved");
  await expectFocus("[data-q-from]@mon#1", "a refused time keeps the focus");
  for (let i = 0; i < 8 && (await focused()) !== "[data-q-remove]@mon#1"; i++) await browser.press("Tab");
  await expectFocus("[data-q-remove]@mon#1", "Tab leaves the refused interval");
  await until(`${second}.value === "12:00" && ${second}.getAttribute('aria-invalid') === null && ${monMsg} === ""`, "leaving the interval restores it and clears the error");
  await eq(`${second}.getAttribute('aria-describedby')`, "q-readout-mon-1", "the read-out alone describes the field again");
  await browser.evaluate(`document.querySelector('[data-quiet-add="mon"]').focus()`);
  await browser.press("Enter");
  await until(`document.querySelectorAll('[data-q-day="mon"]').length === 3`, "Enter adds a third interval");
  await eq(`import("./js/lib.js").then((m) => m.getSettings()).then((s) => s.quiet.days.mon[2])`, { from: "13:00", to: "14:00" }, "the next free hour is proposed");
  await eq(`document.querySelector('[data-quiet-add="mon"]').disabled`, true, "no fourth interval");
  ok("a typed duplicate is refused, tied to its field, and put back on leaving it");
  console.log("PASS: the quiet-hours weekday editor, the loading dialogs, a refused value and a search result work from the keyboard");
} catch (error) {
  console.error(error.message ?? error);
  process.exitCode = 1;
} finally {
  browser.close();
}
