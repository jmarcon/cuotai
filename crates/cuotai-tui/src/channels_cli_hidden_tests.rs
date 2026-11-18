//! PRD 0044 RF5.2: the address prompt of `--channel-add` shows nothing.

use super::{collect_hidden, Cancelled};
use crossterm::event::{Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

fn key(code: KeyCode) -> Event {
    Event::Key(KeyEvent::new(code, KeyModifiers::NONE))
}

fn control(c: char) -> Event {
    Event::Key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL))
}

fn typed(text: &str) -> Vec<Event> {
    text.chars().map(|c| key(KeyCode::Char(c))).collect()
}

fn collect(events: Vec<Event>) -> Result<String, Cancelled> {
    collect_hidden(events).map(|url| url.to_string())
}

#[test]
fn a_paste_then_enter_is_the_address() {
    let url = "https://ntfy.sh/topic";
    assert_eq!(
        collect(vec![Event::Paste(url.into()), key(KeyCode::Enter)]),
        Ok(url.into())
    );
}

#[test]
fn a_pasted_line_break_is_dropped_and_enter_still_confirms() {
    let events = vec![
        Event::Paste("https://ntfy.sh/topic\r\n".into()),
        key(KeyCode::Enter),
        key(KeyCode::Char('y')),
    ];
    assert_eq!(collect(events), Ok("https://ntfy.sh/topic".into()));
}

#[test]
fn backspace_removes_one_grapheme() {
    let mut events = typed("https://x/e\u{301}");
    events.push(key(KeyCode::Backspace));
    events.extend(typed("ab"));
    events.push(key(KeyCode::Backspace));
    events.push(key(KeyCode::Enter));
    assert_eq!(collect(events), Ok("https://x/a".into()));
    assert_eq!(
        collect(vec![key(KeyCode::Backspace), key(KeyCode::Enter)]),
        Ok(String::new())
    );
}

#[test]
fn ctrl_u_clears_and_other_controls_type_nothing() {
    let mut events = typed("wrong");
    events.push(control('u'));
    events.push(control('a'));
    events.extend(typed("https://ntfy.sh/t"));
    events.push(Event::Key(KeyEvent::new_with_kind(
        KeyCode::Char('z'),
        KeyModifiers::NONE,
        KeyEventKind::Release,
    )));
    events.push(key(KeyCode::Enter));
    assert_eq!(collect(events), Ok("https://ntfy.sh/t".into()));
}

#[test]
fn esc_ctrl_c_and_the_end_of_input_cancel() {
    let mut esc = typed("https://ntfy.sh/t");
    esc.push(key(KeyCode::Esc));
    assert_eq!(collect(esc), Err(Cancelled));
    assert_eq!(collect(vec![control('c')]), Err(Cancelled));
    assert_eq!(collect(typed("https://ntfy.sh/t")), Err(Cancelled));
}
