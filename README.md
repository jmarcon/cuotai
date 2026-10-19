<div align="center">
  <img src="apps/desktop/ui/img/logo.svg" alt="Cuotai logo" width="84" height="84" />
  <h1>Cuotai</h1>
  <p><strong>All your AI quotas, usage limits and costs in one place.</strong></p>
  <p>
    A read-only, multi-account monitor written in Rust, with a <strong>desktop app (Tauri 2)</strong>, an <strong>interactive TUI (Ratatui)</strong> and a <strong>JSON CLI</strong>.
  </p>
  <p>
    <a href="#desktop-app">Desktop</a> •
    <a href="#terminal-tui--cli">TUI &amp; CLI</a> •
    <a href="#supported-providers">36 providers</a> •
    <a href="#getting-started">Getting started</a> •
    <a href="#documentation">Documentation</a> •
    <a href="#license">License</a>
  </p>
</div>

---

## Why Cuotai?

Anyone working with several AI assistants and APIs ends up switching between dozens of web dashboards to find out how much is left of the 5-hour window, the weekly quota or the monthly budget. **Cuotai** brings it all together:

- **100% read-only, zero tokens spent:** it only queries quota, billing and header endpoints. It **never** sends inference requests that would burn your credits to measure a limit.
- **Secure by default:** `cuotai.toml` holds references only. OAuth tokens live in the operating system's native vault (**macOS Keychain**, **Windows Credential Manager**, **Linux Secret Service**) with automatic renewal, or in environment variables.
- **Built-in native login:** sign in through the browser, Device Code or AWS SSO without copying tokens by hand (Claude, ChatGPT/Codex, GitHub Copilot, Gemini CLI, Kiro, Antigravity, Cursor, Kimi, Zed, CodeBuddy and more).
- **Three interfaces, one core (`cuotai-core`):**
  - **Desktop and system tray (Tauri 2):** a detailed main window, a quick menu-bar popover (donut or list view) and a native right-click menu.
  - **Terminal TUI (Ratatui):** a fast dashboard with keyboard and mouse support, instant filtering and a full account wizard.
  - **CLI (`--once` / `--json`):** built for scripts, status bars (Waybar, SketchyBar, tmux) and automation.
- **Themes and internationalization:** 14 built-in themes (Dracula, Catppuccin, Tokyo Night, One Dark, GitHub, macOS, Windows 11, Ubuntu, Synthwave, CRT, Win98/XP, Tron), custom themes and languages, and a UI in **Portuguese**, **English** and **Spanish**.

---

## Desktop app

The desktop app watches your accounts in the background with incremental per-account scheduling, drag-and-drop reordering (synced with the TUI) and immediate visual alerts when a quota needs attention or runs out.

<div align="center">
  <img src="docs/assets/desktop-main.png" alt="Cuotai desktop main window" width="100%" />
  <p><em>The Cuotai desktop main window showing 5-hour and weekly windows, rate limits and account status.</em></p>
</div>

<div align="center">
  <img src="docs/assets/desktop-tray.png" alt="Cuotai desktop tray popover" width="420" />
  <p><em>The tray popover in the compact donut view, with an alert summary in the footer.</em></p>
</div>

---

## Supported providers

Cuotai ships **36 integrations**. For parameters, default variables and limitations of each provider, see **[`docs/providers.md`](docs/providers.md)**.

| Category | Providers |
|---|---|
| **Assistants and subscriptions** | **Claude** (`claude`), **OpenAI Codex / ChatGPT** (`chat_gpt`), **GitHub Copilot** (`copilot`), **Cursor** (`cursor` — personal and Teams), **Windsurf** (`windsurf`), **Kiro / AWS** (`kiro`), **Antigravity** (`antigravity`), **Cline** (`cline`), **Zed** (`zed`), **CodeBuddy** (`code_buddy`), **Qoder** (`qoder`), **OpenCode Go** (`open_code_go`) |
| **CLIs and coding plans** | **Gemini CLI** (`gemini_ai`), **Grok CLI / xAI** (`grok`), **Kimi Coding & API** (`kimi`), **MiniMax Token/Coding Plan** (`mini_max`), **Alibaba Coding Plan** (`alibaba_coding_plan`), **Alibaba Token Plan / Qwen Cloud** (`alibaba_token_plan`), **Xiaomi MiMo** (`xiaomi_mimo`), **Ollama Cloud** (`ollama_cloud`), **Qwen Code** (`qwen`) |
| **Cloud billing and admin APIs** | **OpenAI Admin API** (`open_ai_api`), **Anthropic Admin API** (`anthropic_api`), **Google Gemini API Monitoring** (`gemini_api`), **Google Vertex AI** (`vertex_ai`), **AWS Bedrock Cost Explorer** (`aws_bedrock`), **Azure OpenAI Cost Management** (`azure_open_ai`), **Alibaba Cloud BSS** (`alibaba_cloud`), **Mistral Admin** (`mistral`), **Perplexity Computer Analytics** (`perplexity`), **Kimchi** (`kimchi`), **Groq** (`groq`) |
| **Additional catalog** | **Alibaba Model Studio** (`alibaba_model_studio`), **Cloudflare AI** (`cloudflare_ai`), **NVIDIA NIM** (`nvidia_nim`), **Poolside** (`poolside`) |

---

## Getting started

### Prerequisites

- **Rust / Cargo** (2021 edition)
- *(Optional)* **AWS CLI v2** on your `PATH` for `aws_bedrock` accounts
- *(Optional)* **Bailian CLI (`bl`)** for the console login of `alibaba_token_plan`

### 1. Desktop app (Tauri)

```sh
cd apps/desktop/src-tauri
cargo run
```

To build an installable package for your operating system:

```sh
cargo install tauri-cli
cd apps/desktop/src-tauri && cargo tauri build
```

### 2. Terminal TUI and CLI

Run the interactive dashboard in your terminal (it opens the wizard automatically on first run):

```sh
cargo run -p cuotai-tui
```

Or install the `cuotai` binary locally:

```sh
cargo install --path crates/cuotai-tui --locked
```

Quick command-line queries (without opening the interactive interface):

```sh
# List the 36 available providers
cargo run -p cuotai-tui -- --list-providers

# Human-readable summary in the terminal (single run)
cargo run -p cuotai-tui -- --config cuotai.toml --once

# Structured JSON snapshot for scripts and status bars
cargo run -p cuotai-tui -- --config cuotai.toml --json
```

In the JSON output, each metric carries a `kind` (`quota`, `rate_limit`, `balance`, `credits`, `spend` or `usage`) and a `window` (`window_5h`, `daily`, `weekly`, `monthly`, `other` or `null`), classified by the provider adapter. Consumers do not need to parse `label` or `period` to tell quotas, rate limits and windows apart.

---

## Documentation

The detailed technical documentation lives in **[`docs/`](docs/)**. Some documents are still being translated from Portuguese to English.

- **[TUI, CLI and account wizard guide (`docs/tui.md`)](docs/tui.md)** — Keyboard and mouse shortcuts (including the Models section, `m`, with local search and `s` to sync models), the step-by-step wizard flow, configuration file precedence and CLI exit codes.
- **[OAuth login, sessions and vault (`docs/oauth.md`)](docs/oauth.md)** — How native browser and Device Code logins work, the assisted Cursor import, OAuth preparation for Google/Azure/AWS SSO and secure storage in the Keychain.
- **[Provider catalog and limitations (`docs/providers.md`)](docs/providers.md)** — The full table of the 36 providers, default environment variables, `9router` compatibility, metric semantics and per-query cost warnings (such as AWS Cost Explorer).
- **[Architecture, Rust library and verification (`docs/architecture.md`)](docs/architecture.md)** — The split between `cuotai-core`, `cuotai-tui` and `apps/desktop`, using `cuotai-core` as a Rust library, the internationalization contract and the test suite.
- **[Desktop app guide (`apps/desktop/README.md`)](apps/desktop/README.md)** — Details specific to the Tauri 2 shell, themes and the user customization folder.
- **[Configuration example (`cuotai.example.toml`)](cuotai.example.toml)** — A commented reference of every `[[accounts]]` entry and TOML parameter.
- **[Roadmap (`docs/plans/`)](docs/plans/0000_control.md)** — Product requirement documents (PRDs) with their implementation status and dependencies.

---

## Contributing

Contributions are welcome. Read [`CONTRIBUTING.md`](CONTRIBUTING.md) first, and report security issues privately as described in [`SECURITY.md`](SECURITY.md).

---

## License

Cuotai is distributed under the terms of either the [MIT license](LICENSE-MIT) or the [Apache License, Version 2.0](LICENSE-APACHE), at your option. Third-party notices are in [`THIRD_PARTY_NOTICES`](THIRD_PARTY_NOTICES).

Unless you explicitly state otherwise, any contribution intentionally submitted for inclusion in Cuotai, as defined in the Apache-2.0 license, is dual licensed as above, without any additional terms or conditions.
