//! Git-side release checks (PRD 0021 RF1.1): the tag must be annotated and
//! its commit must be contained in the main branch. All commands run without
//! a shell and the tag name is validated by [`crate::parse_tag`] before it
//! reaches this module.

use std::path::Path;
use std::process::Command;

use crate::{err, Error, Res};

/// Fails unless `tag` is an annotated tag whose commit is an ancestor of
/// `main_ref` (for example `origin/main`).
pub fn check_tag_on_main(repo: &Path, tag: &str, main_ref: &str) -> Res<()> {
    let tag_ref = format!("refs/tags/{tag}");
    let object_type = run(repo, &["cat-file", "-t", &tag_ref]).map_err(|e| {
        Error::Msg(format!(
            "tag '{tag}' is not available in this checkout: {e}"
        ))
    })?;
    if object_type.trim() != "tag" {
        return err(format!(
            "tag '{tag}' is not an annotated tag; release tags are created with `git tag -a`"
        ));
    }
    let commit = run(repo, &["rev-parse", &format!("{tag_ref}^{{commit}}")])?;
    let status = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(["merge-base", "--is-ancestor", commit.trim(), main_ref])
        .status()
        .map_err(|e| Error::Msg(format!("cannot run git merge-base: {e}")))?;
    match status.code() {
        Some(0) => Ok(()),
        Some(1) => err(format!(
            "tag '{tag}' points at commit {} which is not contained in '{main_ref}'; \
             release tags must point at a commit on the main branch",
            commit.trim()
        )),
        _ => err(format!(
            "git merge-base failed for tag '{tag}' against '{main_ref}' \
             (is '{main_ref}' fetched? checkout needs fetch-depth: 0)"
        )),
    }
}

/// Runs `git` in `repo` without a shell and returns its standard output.
pub fn run(repo: &Path, args: &[&str]) -> Res<String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(args)
        .output()
        .map_err(|e| Error::Msg(format!("cannot run git: {e}")))?;
    if !output.status.success() {
        return err(format!(
            "git {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}
