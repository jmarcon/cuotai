//! PRD 0052 — offline diagnostics of this installation: the redacted app
//! log ([`logging`]) and its redactor ([`redact`]). Nothing here makes a
//! network request or reads a secret value.

pub mod logging;
pub mod redact;
