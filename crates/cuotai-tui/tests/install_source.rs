mod common;

use std::{
    collections::BTreeMap,
    fs,
    path::Path,
    process::{Command, Output},
};

fn run(program: &Path, args: &[&str]) -> Output {
    Command::new(program)
        .args(args)
        .env_remove("APPIMAGE")
        .output()
        .expect("run cuotai")
}

#[test]
fn install_source_reports_the_running_binary_offline() {
    let directory =
        std::env::temp_dir().join(format!("cuotai-install-source-{}", std::process::id()));
    let _ = fs::remove_dir_all(&directory);
    fs::create_dir(&directory).expect("create smoke directory");
    let executable = directory.join(format!("cuotai{}", std::env::consts::EXE_SUFFIX));
    common::copy_executable(Path::new(env!("CARGO_BIN_EXE_cuotai")), &executable)
        .expect("copy smoke binary");
    let output = run(&executable, &["--install-source"]);
    fs::remove_dir_all(directory).expect("remove smoke directory");
    assert!(output.status.success(), "{output:?}");
    assert!(output.stderr.is_empty(), "{output:?}");

    let stdout = String::from_utf8(output.stdout).expect("UTF-8 output");
    let fields: BTreeMap<_, _> = stdout
        .lines()
        .filter_map(|line| line.split_once('='))
        .collect();
    assert_eq!(fields.get("source"), Some(&"direct"));
    assert_eq!(fields.get("updater_disabled_reason"), Some(&"none"));
    assert_eq!(fields.get("upgrade_command"), Some(&"none"));
}

#[test]
fn a_cargo_copy_never_gets_a_crates_io_command_offline() {
    let root = std::env::temp_dir().join(format!("cuotai-install-cargo-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(root.join("bin")).expect("create cargo home");
    #[cfg(unix)]
    let root = root.canonicalize().expect("canonical cargo home");
    let executable = root
        .join("bin")
        .join(format!("cuotai{}", std::env::consts::EXE_SUFFIX));
    common::copy_executable(Path::new(env!("CARGO_BIN_EXE_cuotai")), &executable)
        .expect("copy smoke binary");
    let output = Command::new(&executable)
        .arg("--install-source")
        .env_remove("APPIMAGE")
        .env("CARGO_HOME", &root)
        .output()
        .expect("run cuotai");
    fs::remove_dir_all(&root).expect("remove smoke directory");
    assert!(output.status.success(), "{output:?}");

    let stdout = String::from_utf8(output.stdout).expect("UTF-8 output");
    let fields: BTreeMap<_, _> = stdout
        .lines()
        .filter_map(|line| line.split_once('='))
        .collect();
    assert_eq!(fields.get("source"), Some(&"cargo"), "{stdout}");
    assert_eq!(fields.get("updater_disabled_reason"), Some(&"cargo"));
    assert_eq!(fields.get("upgrade_command"), Some(&"none"));
}

#[test]
fn install_source_refuses_query_modes_as_a_local_argument_error() {
    let output = run(
        Path::new(env!("CARGO_BIN_EXE_cuotai")),
        &["--install-source", "--once"],
    );
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert!(output.stdout.is_empty(), "{output:?}");
}
