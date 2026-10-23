# Building Cuotai from source

Until the first packaged release is published (see [`installing.md`](installing.md#channel-status)), building from source is the way to run Cuotai. It is also the starting point for contributors; the rules for changing the code are in [`../CONTRIBUTING.md`](../CONTRIBUTING.md) and [`../AGENTS.md`](../AGENTS.md).

## Prerequisites

- **Rust / Cargo**, 2021 edition. The repository pins the toolchain in `rust-toolchain.toml` (currently 1.99.1) and rustup picks it up automatically. There is no JavaScript package manager: the desktop interface is plain HTML, CSS and JS in `apps/desktop/ui/`, and dependencies come only through Cargo.
- *(Optional)* **AWS CLI v2** on your `PATH` for `aws_bedrock` accounts.
- *(Optional)* **Bailian CLI (`bl`)** for the console login of `alibaba_token_plan`.
- **Linux, desktop app only:** the Tauri build libraries. The release workflow installs, on Ubuntu, `libwebkit2gtk-4.1-dev libappindicator3-dev librsvg2-dev patchelf` (plus `rpm` to build the `.rpm`); other distributions have equivalent packages.

## Desktop app (Tauri)

The desktop app is a separate Cargo workspace in `apps/desktop/src-tauri`:

```sh
cd apps/desktop/src-tauri
cargo run
```

To build an installable package for your operating system:

```sh
cargo install tauri-cli
cd apps/desktop/src-tauri && cargo tauri build
```

For development there is `apps/desktop/dev.sh`, which uses the repository's `cuotai.toml` and a SQLite file that stores **tokens in plain text**, for development only; release builds refuse that store. Never use it with credentials you cannot afford to lose.

More about the shell, its commands and its settings file is in [`../apps/desktop/README.md`](../apps/desktop/README.md).

## Terminal dashboard and command line

Run the interactive dashboard (it opens the account wizard automatically on the first run):

```sh
cargo run -p cuotai-tui

# Announce quota resets and usage alerts (75%, 90%, exhausted, available again)
# while the dashboard is open (header banner, bell, OSC 9)
cargo run -p cuotai-tui -- --notify

# Show the providers' public status (off by default; also CUOTAI_STATUS=1)
cargo run -p cuotai-tui -- --status
```

Install the `cuotai` binary locally, or straight from the repository:

```sh
cargo install --path crates/cuotai-tui --locked
cargo install --git https://github.com/jmarcon/cuotai cuotai-tui --locked
```

The command-line recipes (`--list-providers`, `--once`, `--json`, `--json-v1`, `--serve`, `--watch`, `--history`, the cost commands) are in [`advanced.md`](advanced.md) and [`tui.md`](tui.md).

## Checks before you propose a change

From the repository root:

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

For desktop changes, also run `cargo clippy --all-targets -- -D warnings` and `cargo test` in `apps/desktop/src-tauri`. CI runs the same checks on Linux, macOS and Windows; the full list, the dependency policy and the CodeQL script are in [`../CONTRIBUTING.md`](../CONTRIBUTING.md#checks-before-submitting) and [`architecture.md`](architecture.md#development-and-verification-commands).

For a debug-only run of the terminal dashboard with a plaintext SQLite credential store (never for real credentials), `sh run.sh` selects `--dev-credentials-sqlite ./cuotai-dev.sqlite3`; release builds reject the flag.
