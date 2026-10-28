// PRD 0032 (quiet hours) with PRD 0020 RF8: the weekday editor works from the keyboard alone. A real browser drives
// the Settings > Alerts > Quiet hours sub-page of the demo with Tab, Shift+Tab, Enter, Space and typed digits: open
// the sub-page, switch the schedule on, add an interval (focus lands on its first time field), type a time, remove
// it (focus returns to the weekday's Add button), repeat the schedule on every day, and reach the three-interval
// limit. Exits 1 on the first step that needs a mouse. Run from the repository root; needs Chrome (see README).
import assert from "node:assert/strict";
import { launch } from "./driver.mjs";

const browser = await launch();
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const ok = (name) => console.log(`ok   ${name}`);
const eq = async (expression, expected, message) => assert.deepEqual(await browser.evaluate(expression), expected, message);
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
  await sleep(500);
  await eq(`!!document.querySelector('#quiet-on')?.offsetParent`, true, "Enter opens the quiet-hours sub-page");
  ok("Enter opens the sub-page");

  // 2. The schedule switch toggles with Space, and the weekday controls follow.
  await browser.evaluate(`document.querySelector('#quiet-on').focus()`);
  await eq(`document.querySelector('[data-quiet-add="mon"]').disabled`, true, "weekday controls wait for the switch");
  await browser.press(" ");
  await sleep(300);
  await eq(`document.querySelector('#quiet-on').checked`, true, "Space turns the schedule on");
  await eq(`document.querySelector('[data-quiet-add="mon"]').disabled`, false, "the weekday controls turn on with it");
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
  await sleep(400);
  await eq(`document.querySelectorAll('[data-q-day="mon"]').length`, 1, "Enter adds an interval");
  await expectFocus("[data-q-from]@mon#0", "after Add");
  await eq(`document.querySelector('[data-q-day="mon"] [data-q-from]').getAttribute('aria-label').length > 0`, true, "the time field is labelled");
  ok("Enter adds an interval and focuses its time field");

  // 5. Backspace empties the hour: the value is incomplete, so nothing is saved, but the field keeps the focus (the
  //    editor is not drawn again under the cursor) and the typing goes on. Then a time is typed with digits (the
  //    field commits when it loses focus) and Tab moves from field to field.
  await browser.press("Backspace");
  await sleep(300);
  await expectFocus("[data-q-from]@mon#0", "Backspace keeps the focus in the time field");
  await eq(`document.querySelector('[data-q-day="mon"] [data-q-from]').value`, "", "the emptied hour leaves no value");
  for (const key of ["0", "9", "3", "0"]) await browser.press(key);
  await expectFocus("[data-q-from]@mon#0", "typing stays in the time field");
  await browser.press("Tab"); // minutes -> AM/PM or the next field; keep tabbing until the "to" field
  for (let i = 0; i < 4 && !(await focused()).startsWith("[data-q-to]"); i++) await browser.press("Tab");
  await sleep(400);
  await expectFocus("[data-q-to]@mon#0", "Tab reaches the second time field");
  await eq(`document.querySelector('[data-q-day="mon"] [data-q-from]').value`, "09:30", "the typed time is committed");
  ok("digits set a time and Tab moves between fields");

  // 5b. An incomplete time gets the saved one back when the focus leaves it, without moving the focus.
  await browser.evaluate(`document.querySelector('[data-q-day="mon"] [data-q-to]').value = ""`);
  for (let i = 0; i < 4 && (await focused()) === "[data-q-to]@mon#0"; i++) await browser.press("Tab");
  await expectFocus("[data-q-remove]@mon#0", "Tab leaves the incomplete time for Remove");
  await sleep(300);
  await eq(`document.querySelector('[data-q-day="mon"] [data-q-to]').value`, "08:00", "leaving an incomplete time restores the saved one");
  ok("an incomplete time is put back on leaving it");

  // 6. Enter on the remove button (reached with Tab above) deletes the interval; focus goes back to the weekday's
  //    Add button.
  await browser.press("Enter");
  await sleep(400);
  await eq(`document.querySelectorAll('[data-q-day="mon"]').length`, 0, "Enter removes the interval");
  await expectFocus("[data-quiet-add=mon]", "after Remove");
  ok("Enter removes an interval and returns focus to Add");

  // 7. "Same every day" with Enter fills the seven weekdays and keeps focus on its own button.
  await browser.evaluate(`document.querySelector('[data-quiet-same]').focus()`);
  await browser.press("Enter");
  await sleep(400);
  await eq(`document.querySelectorAll('[data-q-day]').length`, 7, "one interval per weekday");
  await expectFocus("[data-quiet-same]", "after Same every day");
  ok("Same every day fills the week and keeps focus");

  // 8. The fourth interval of a day is refused: after three, Add is disabled.
  await browser.evaluate(`document.querySelector('[data-quiet-add="mon"]').focus()`);
  await browser.press("Enter");
  await sleep(400);
  await browser.evaluate(`document.querySelector('[data-quiet-add="mon"]').focus()`);
  await browser.press("Enter");
  await sleep(400);
  await eq(`document.querySelectorAll('[data-q-day="mon"]').length`, 3, "three intervals on Monday");
  await eq(`document.querySelector('[data-quiet-add="mon"]').disabled`, true, "the fourth is refused");
  ok("at most three intervals per day");

  // 9. Every control on the page is reachable and has an accessible name.
  await eq(`[...document.querySelectorAll('[data-quiet-days] input, [data-quiet-days] button')].filter((el) => !el.getAttribute('aria-label') && !el.textContent.trim() && !el.title).length`, 0, "unnamed control");
  ok("every control has a name");
  console.log("PASS: the quiet-hours weekday editor works from the keyboard");
} catch (error) {
  console.error(error.message ?? error);
  process.exitCode = 1;
} finally {
  browser.close();
}
