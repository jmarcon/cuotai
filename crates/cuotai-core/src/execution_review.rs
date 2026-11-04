//! PRD 0050 RF2: what a front end asks, in a native operating-system dialog
//! it opens itself, before an action saves, approves, enables or tests a
//! local command (hooks and extensions). The texts are core-owned and typed,
//! so a webview never supplies the words of the question nor its answer.
//!
//! The body of each dialog is [`Review::intro`] followed by the core's own
//! listing of the commands ([`crate::hooks::Hooks::review`],
//! [`crate::extensions::management::Manager::review`]).

use crate::text::Localized;

/// The actions that need a native confirmation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Review {
    /// Write new hook rules to `hooks.toml` (saving never approves them).
    SaveHooks,
    /// Approve the rules in `hooks.toml` to run.
    ApproveHooks,
    /// Turn on the hooks already approved.
    EnableHooks,
    /// Run one approved hook now with a sample event.
    TestHook,
    /// Approve an extension and use it.
    EnableExtension,
    /// Run an extension once now.
    TestExtension,
}

impl Review {
    pub fn title(self) -> Localized {
        match self {
            Self::SaveHooks => {
                Localized::fixed("execution.review.save_hooks.title", "Save commands")
            }
            Self::ApproveHooks => {
                Localized::fixed("execution.review.approve_hooks.title", "Approve commands")
            }
            Self::EnableHooks => {
                Localized::fixed("execution.review.enable_hooks.title", "Turn on hooks")
            }
            Self::TestHook => {
                Localized::fixed("execution.review.test_hook.title", "Run a hook now")
            }
            Self::EnableExtension => Localized::fixed(
                "execution.review.enable_extension.title",
                "Approve extension",
            ),
            Self::TestExtension => Localized::fixed(
                "execution.review.test_extension.title",
                "Run an extension now",
            ),
        }
    }

    /// The first line of the body: what pressing the button does.
    pub fn intro(self) -> Localized {
        match self {
            Self::SaveHooks => Localized::fixed(
                "execution.review.save_hooks.intro",
                "Cuotai will save these commands to hooks.toml. Saving does not approve them: they run only after you approve them.",
            ),
            Self::ApproveHooks => Localized::fixed(
                "execution.review.approve_hooks.intro",
                "Cuotai will run these commands on your computer, with your user's powers, whenever the listed events happen.",
            ),
            Self::EnableHooks => Localized::fixed(
                "execution.review.enable_hooks.intro",
                "Turning hooks on runs these approved commands on your computer whenever the listed events happen.",
            ),
            Self::TestHook => Localized::fixed(
                "execution.review.test_hook.intro",
                "Cuotai will run this command on your computer now, with a sample event.",
            ),
            Self::EnableExtension => Localized::fixed(
                "execution.review.enable_extension.intro",
                "Cuotai will run this extension's program on your computer, with your user's powers, every time it refreshes the account.",
            ),
            Self::TestExtension => Localized::fixed(
                "execution.review.test_extension.intro",
                "Cuotai will run this extension's program on your computer once now.",
            ),
        }
    }

    /// The confirming button.
    pub fn accept(self) -> Localized {
        match self {
            Self::SaveHooks => Localized::fixed("execution.review.save_hooks.accept", "Save"),
            Self::ApproveHooks => {
                Localized::fixed("execution.review.approve_hooks.accept", "Approve")
            }
            Self::EnableHooks => {
                Localized::fixed("execution.review.enable_hooks.accept", "Turn on")
            }
            Self::TestHook | Self::TestExtension => {
                Localized::fixed("execution.review.run.accept", "Run now")
            }
            Self::EnableExtension => {
                Localized::fixed("execution.review.enable_extension.accept", "Approve")
            }
        }
    }
}

/// The declining button.
pub fn cancel() -> Localized {
    Localized::fixed("execution.review.cancel", "Cancel")
}

/// The fixed error of a declined (or failed) native confirmation: nothing
/// was written, approved, enabled or run.
pub fn declined() -> Localized {
    Localized::fixed(
        "execution.declined",
        "Not confirmed in the system dialog; nothing was saved, approved or run.",
    )
}

/// Another native confirmation is already open; this request did nothing.
pub fn pending() -> Localized {
    Localized::fixed(
        "execution.pending",
        "A confirmation is already open; answer it first.",
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_review_has_its_own_title_intro_and_button() {
        let all = [
            Review::SaveHooks,
            Review::ApproveHooks,
            Review::EnableHooks,
            Review::TestHook,
            Review::EnableExtension,
            Review::TestExtension,
        ];
        let titles: std::collections::BTreeSet<_> = all.iter().map(|r| r.title().id).collect();
        assert_eq!(titles.len(), all.len());
        let intros: std::collections::BTreeSet<_> = all.iter().map(|r| r.intro().id).collect();
        assert_eq!(intros.len(), all.len());
        for review in all {
            assert!(review.title().id.starts_with("execution.review."));
            assert!(!review.accept().fallback.is_empty());
        }
        assert_eq!(declined().id, "execution.declined");
    }
}
