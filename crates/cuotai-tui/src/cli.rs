//! PRD 0052 RF1.4 — the explicit stderr presenter. Human lifecycle text
//! that is part of the CLI contract still goes to stderr (never stdout);
//! each message also records one app-log event with a stable id and no
//! text (the redacted log never copies what a person reads).

use std::fmt::Display;

use cuotai_core::diagnostics::logging;
pub(crate) use cuotai_core::diagnostics::logging::{Field, Level};

/// Print `text` on stderr and record `event` at `level`.
pub(crate) fn message(level: Level, event: &'static str, text: impl Display) {
    message_with(level, event, &[], text);
}

/// [`message`] with typed, redacted log fields.
pub(crate) fn message_with(
    level: Level,
    event: &'static str,
    fields: &[(&'static str, Field<'_>)],
    text: impl Display,
) {
    eprintln!("{text}");
    logging::event(level, event, fields);
}
