//! PRD 0051 RF2.6: `cargo xtask check-example-config`.
//!
//! `cuotai.example.toml` documents optional syntax in comments. A block that
//! should stay valid is marked:
//!
//! ```toml
//! # example: groups
//! # [[groups]]
//! # id = "coding"
//! # example-end
//! ```
//!
//! Every line between the markers is `# ` followed by TOML (or a bare `#`).
//! [`materialize`] returns the file as it is (the base) plus one copy per
//! block with only that block uncommented in place, and [`check`] runs the
//! real strict reader on each, `cuotai --check-config --config FILE`, which
//! needs no credential and makes no network request.

use std::path::{Path, PathBuf};
use std::process::Command;

use crate::{err, Res};

const BEGIN: &str = "# example:";
const END: &str = "# example-end";

/// One document to check: `name` is `base` or the block's name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Example {
    pub name: String,
    pub text: String,
}

/// The base document and one document per marked block, in file order.
pub fn materialize(text: &str) -> Res<Vec<Example>> {
    let lines: Vec<&str> = text.lines().collect();
    let mut out = vec![Example {
        name: "base".to_owned(),
        text: text.to_owned(),
    }];
    let mut names = std::collections::BTreeSet::new();
    let mut index = 0;
    while index < lines.len() {
        let line = lines[index].trim_end();
        if line.trim_start() == END {
            return err(format!(
                "cuotai.example.toml:{}: '{END}' without '{BEGIN}'",
                index + 1
            ));
        }
        let Some(name) = line.strip_prefix(BEGIN).map(str::trim) else {
            index += 1;
            continue;
        };
        if name.is_empty() || !names.insert(name.to_owned()) {
            return err(format!(
                "cuotai.example.toml:{}: an example needs a unique name",
                index + 1
            ));
        }
        let start = index + 1;
        let end = (start..lines.len())
            .find(|&at| lines[at].trim() == END)
            .ok_or_else(|| {
                crate::Error::Msg(format!(
                    "cuotai.example.toml:{}: example '{name}' has no '{END}'",
                    index + 1
                ))
            })?;
        let mut copy: Vec<String> = lines.iter().map(|line| (*line).to_owned()).collect();
        for (at, line) in lines.iter().enumerate().take(end).skip(start) {
            if line.trim_start().starts_with(BEGIN) {
                return err(format!(
                    "cuotai.example.toml:{}: examples cannot nest",
                    at + 1
                ));
            }
            copy[at] = match line.strip_prefix("# ") {
                Some(toml) => toml.to_owned(),
                None if line.trim() == "#" => String::new(),
                None => {
                    return err(format!(
                        "cuotai.example.toml:{}: a line of example '{name}' must start with '# '",
                        at + 1
                    ))
                }
            };
        }
        if end == start {
            return err(format!(
                "cuotai.example.toml:{}: example '{name}' is empty",
                index + 1
            ));
        }
        out.push(Example {
            name: name.to_owned(),
            text: copy.join("\n") + "\n",
        });
        index = end + 1;
    }
    Ok(out)
}

/// Run `cuotai --check-config --config FILE` (`bin`) on every example of
/// `example_file`; the files go to `scratch`. Returns the names checked.
pub fn check(example_file: &Path, bin: &Path, scratch: &Path) -> Res<Vec<String>> {
    let text = std::fs::read_to_string(example_file)?;
    let examples = materialize(&text)?;
    if examples.len() < 2 {
        return err("cuotai.example.toml has no marked example block");
    }
    let mut failures = Vec::new();
    let mut names = Vec::new();
    for example in &examples {
        let file: PathBuf = scratch.join(format!("example-{}.toml", example.name));
        std::fs::write(&file, &example.text)?;
        let output = Command::new(bin)
            .args(["--check-config", "--config"])
            .arg(&file)
            .env_remove("CUOTAI_CONFIG")
            .output()?;
        if !output.status.success() {
            failures.push(format!(
                "{}: {}",
                example.name,
                String::from_utf8_lossy(&output.stdout).trim()
            ));
        }
        names.push(example.name.clone());
    }
    if failures.is_empty() {
        Ok(names)
    } else {
        err(format!(
            "example blocks rejected by the strict reader:\n{}",
            failures.join("\n")
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_block_is_uncommented_alone_in_place() {
        let text = "a = 1\n# example: one\n# b = 2\n#\n# c = 3\n# example-end\n# prose\n# example: two\n# [t]\n# example-end\n";
        let examples = materialize(text).unwrap();
        let names: Vec<_> = examples.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, ["base", "one", "two"]);
        assert_eq!(examples[0].text, text);
        assert_eq!(
            examples[1].text,
            "a = 1\n# example: one\nb = 2\n\nc = 3\n# example-end\n# prose\n# example: two\n# [t]\n# example-end\n"
        );
        assert!(examples[2].text.contains("\n[t]\n"));
        assert!(examples[2].text.contains("# b = 2"));
    }

    #[test]
    fn broken_markers_are_errors() {
        for text in [
            "# example: x\n# a = 1\n",
            "# example-end\n",
            "# example: x\n# example: y\n# example-end\n",
            "# example: x\nprose without a hash\n# example-end\n",
            "# example: x\n# example-end\n",
            "# example:\n# a = 1\n# example-end\n",
            "# example: x\n# a = 1\n# example-end\n# example: x\n# b = 1\n# example-end\n",
        ] {
            assert!(materialize(text).is_err(), "{text:?}");
        }
    }

    /// The real example file keeps balanced markers and several blocks.
    #[test]
    fn the_repository_example_has_marked_blocks() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../cuotai.example.toml");
        let text = std::fs::read_to_string(path).unwrap();
        let examples = materialize(&text).unwrap();
        assert!(examples.len() > 5, "{} blocks", examples.len() - 1);
    }
}
