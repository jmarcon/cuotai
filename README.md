<div align="center">
  <h1>
    <picture>
      <source media="(prefers-color-scheme: dark)" srcset="apps/desktop/ui/img/logotype-on-dark.svg" />
      <source media="(prefers-color-scheme: light)" srcset="apps/desktop/ui/img/logotype-on-light.svg" />
      <img src="apps/desktop/ui/img/logotype.svg" alt="Cuotai" height="64" />
    </picture>
  </h1>
  <p><strong>All your AI quotas, usage limits and costs in one place.</strong></p>
  <p>
    A free, open-source desktop app that shows how much of your 5-hour window, weekly quota or monthly budget is left, across every AI assistant and API you use, and warns you before you run out.
  </p>
  <p>
    <a href="#what-is-cuotai">What it does</a> •
    <a href="#install">Install</a> •
    <a href="#a-tour-of-the-desktop-app">Desktop tour</a> •
    <a href="#terminal-dashboard">Terminal</a> •
    <a href="#themes">Themes</a> •
    <a href="#help-cuotai-grow">Support the project</a> •
    <a href="#documentation">Documentation</a> •
    <a href="https://www.julianomarcon.com.br/cuotai/">Website</a>
  </p>
  <img src="docs/assets/screenshots/desktop-hero.png" alt="The Cuotai desktop app: the main window with one card per AI account and the tray popover with quota rings" width="100%" />
  <p><em>The main window and the tray popover, shown here with made-up demo accounts.</em></p>
</div>

---

## What is Cuotai?

If you work with several AI assistants and APIs, you probably hop between dashboards to find out how much is left of the 5-hour window, the weekly quota or the monthly budget. **Cuotai puts all of it in one place** and keeps it fresh in the background, so you know before a limit stops you.

**Why the name?** Cuotai comes from *cuota*, how many Portuguese speakers say "quota", plus *AI*, which is why the logo sets it apart: it keeps an eye on the quotas, usage limits and costs of your AI providers. Its four-color C is a gauge that is never quite closed, the same ring that shows how much quota each account has left.

It is made for developers, teams and heavy users of coding assistants and AI APIs who juggle more than one subscription or key.

**What you get**

- **Every limit in one window.** One card per account with the 5-hour, weekly and monthly windows, rate limits, balances and spend, each with its reset time. Supports **40 services**, including Claude, ChatGPT / Codex, GitHub Copilot, Cursor, Gemini, Kiro, Windsurf, OpenAI and Anthropic admin APIs, AWS Bedrock, Azure OpenAI and many more ([full list](#supported-services)).
- **Paid extra usage.** When an account keeps working past its plan and bills the excess (Claude extra usage, Cursor and Grok on-demand, Kiro overage), Cuotai shows what you have spent so far apart from the plan limits and notifies you once per billing cycle when it starts. It only reads; it never changes billing settings.
- **Fresh after a reset, careful with costs.** About 30 seconds after a quota resets Cuotai asks once more, so a card stops saying "blocked" as soon as the new window opens, and a reading kept from before a reset says "Renewed at 14:30; waiting for a reading" instead of pretending to know. Services that bill per query (AWS Bedrock through Cost Explorer) are read once an hour by default, never more than every 15 minutes, and never after a reset.
- **Pace and "runs out" warnings.** Cuotai compares what you used so far with a steady pace until the reset and tells you when you are on track ("8 points in reserve") or will run out early ("runs out at 16:40, 2h 10m before the reset").
- **Alerts that matter.** Optional system notifications when a quota resets, crosses a threshold you choose, runs out or has slack again, and an optional warning 15, 30 or 60 minutes before the recent pace would use the whole quota. That warning is a linear estimate from recent readings, never from an average or a single reading, and is off until you turn it on. Nothing is sent to anyone: the app just tells you. Set **quiet hours** per weekday and everything but an exhausted quota waits for one summary when they end; texts leave account names out by default, so a locked screen reveals nothing; and the app and `cuotai --notify` never announce the same event twice.
- **Which account has room right now.** Put accounts that can replace each other in a group (`group = "coding"`) and Cuotai marks the best one, as advice only: it never switches accounts for you.
- **History and local cost.** Charts of how each quota moved over the last 24 hours, 7 days or 30 days, and an optional estimate of what your Claude Code, Codex CLI and Gemini CLI token use would cost at API prices, read from your own local logs.
- **Private by design.** Cuotai is read-only and spends no tokens: it only asks your providers for quota and billing numbers, never for an AI answer. Sign-ins and tokens are kept in your operating system's vault (macOS Keychain, Windows Credential Manager, Linux Secret Service), your settings file never contains a secret, and there is no Cuotai account or cloud service in the middle.
- **Your own gateways too.** Point Cuotai at a self-hosted 9router or LiteLLM proxy and it shows every connection inside it as a card of its own, grouped by upstream provider, and warns when a whole pool runs out.
- **Runs where you work.** Windows, macOS and Linux, with a tray or menu-bar popover for a quick look (the menu-bar title can show your tightest limit). A terminal dashboard and a command line for scripts are included.
- **Scriptable.** `cuotai --json`, an opt-in read-only local HTTP server, events and hooks with `--watch`, status-bar recipes (Waybar, SketchyBar, tmux, i3bar), shell completions and executable extensions for services Cuotai does not know yet.
- **Yours to shape.** 15 built-in themes (dark and light), your own themes, and an interface in 14 languages: English, Portuguese, Spanish, Italian, German, French, Russian, Turkish, Japanese, Korean, Simplified Chinese, Hindi, Indonesian and Vietnamese. Ten of them are machine-drafted and waiting for native proofreading, which is marked in their language files.
- **Free and open source**, under the MIT or Apache-2.0 license.

## Install

> **Status.** Every release is published on [GitHub Releases](https://github.com/jmarcon/cuotai/releases), the Homebrew tap and the Scoop bucket. On Releases, the macOS `.dmg` is signed and notarized, the Windows installers are **not code-signed yet** (SmartScreen will warn), and the Linux packages and the terminal archives are covered by the signed `SHA256SUMS`. winget, the AUR and crates.io are marked "coming soon" below: they are **not live yet**, so use Releases or build from source ([build guide](docs/building.md)). [`docs/installing.md`](docs/installing.md) records the status of every channel.

| System | Package manager | Direct download | Works today |
|---|---|---|---|
| **macOS** (Apple silicon and Intel) | Homebrew: `brew install --cask jmarcon/cuotai/cuotai-app` | `.dmg` from [Releases](https://github.com/jmarcon/cuotai/releases) | Homebrew, [download from Releases](https://github.com/jmarcon/cuotai/releases) or [build from source](docs/building.md#desktop-app-tauri) |
| **Windows** (x64; ARM64 optional) | winget: `winget install Cuotai.Cuotai` (coming soon) | `.exe` or `.msi` installer from [Releases](https://github.com/jmarcon/cuotai/releases) | [Download from Releases](https://github.com/jmarcon/cuotai/releases) or [build from source](docs/building.md#desktop-app-tauri) |
| **Linux** (x64; ARM64 optional) | AUR: `yay -S cuotai-bin` (coming soon) | `.deb`, `.rpm` or `.AppImage` from [Releases](https://github.com/jmarcon/cuotai/releases) | [Download from Releases](https://github.com/jmarcon/cuotai/releases) or [build from source](docs/building.md#desktop-app-tauri) |
| **Any system, terminal only** | `cargo install cuotai-tui --locked` or `cargo binstall cuotai-tui` (coming soon, once the crates are published) | CLI archive from [Releases](https://github.com/jmarcon/cuotai/releases) | The archive, or `cargo install --git https://github.com/jmarcon/cuotai cuotai-tui --locked` |

For the terminal version: Homebrew (`brew install jmarcon/cuotai/cuotai`) and Scoop (`scoop bucket add cuotai https://github.com/jmarcon/scoop-cuotai` then `scoop install cuotai`). The full channel status, with how each one is checked, is in [`docs/installing.md`](docs/installing.md).

**Good to know**

- **macOS.** Builds that are not notarized by Apple are blocked the first time you open them: open **System Settings → Privacy & Security** and choose **Open Anyway** (older macOS versions: Control-click the app, choose **Open**, then confirm). Each release's notes say whether its macOS build is signed and notarized ([details](docs/gui-macos.md#first-launch-and-gatekeeper)).
- **Windows.** Installers that are not code-signed trigger a SmartScreen "unknown publisher" warning: choose **More info**, then **Run anyway**. Each release's notes say whether the build is signed ([details](docs/gui-windows.md#first-launch-and-smartscreen)).
- **Linux.** The `.deb` and `.rpm` declare the libraries they need, so your package manager installs them. On GNOME, the tray icon needs the AppIndicator extension ([details](docs/gui-linux.md#tray-icon)).
- **Verify what you downloaded.** Every release ships `SHA256SUMS` (signed) and build provenance: see [how to verify a download](docs/installing.md#verify-your-download).
- **Uninstalling** removes the app only; your accounts file, history and vault entries stay until you delete them ([how](docs/installing.md#uninstalling)).

## A tour of the desktop app

Open Cuotai, press **Add account**, pick a provider and either sign in with your browser or paste your API key; Cuotai checks it with the provider and keeps it in your system's credential vault, never in the accounts file. From then on it refreshes in the background. A name is optional until you add a second account of the same provider, and renaming an account later keeps its alerts, muted quotas and history.

<div align="center">
  <img src="docs/assets/screenshots/desktop-dashboard.png" alt="Cuotai desktop dashboard with quota cards" width="100%" />
</div>

| Feature | What it does |
|---|---|
| **Dashboard** | One card per account, ordered the way you like (drag to reorder, or "most restrictive first"), highlighting the limit that blocks you right now and how old each reading is. |
| **Tray or menu bar** | Left click opens a popover with quota rings or a list; right click opens a quick menu. Closing the window keeps Cuotai running in the tray. The menu-bar title can show your tightest limit, one figure per account, small meters or just the icon, for the accounts you choose. |
| **Account details** | Every window with its bar, reset time and pace mark; hide or mute single quotas; browse the provider's model catalog. |
| **Pace and warnings** | Whether you will make it to the reset, as an estimate, in points or in percent, with a forecast and an opt-in "warn before running out". |
| **Best account** | The account of a group with the most room right now, marked "Best now". |
| **Notifications and alerts** | Opt-in system notices for resets, thresholds (75% and 90% by default), exhaustion and recovery, with quiet hours and a summary, texts without account names and a test button. |
| **History** | Area charts of each metric over 24 hours, 7 days or 30 days, with export to CSV or JSON. |
| **Statistics and limits coming back** | A Stats view (limit hits, average peak, extra spent, peak per day, recent events) and a "limits come back" timeline, from the history stored on your computer; `S` and `--stats`/`--timeline`/`--events-log` in the terminal. |
| **Local cost** | Opt-in token and estimated API-price cost for Claude Code, Codex CLI and Gemini CLI, from local logs only. Optionally compares the estimate with the plan price you type and shows a second currency at a rate you type; both are estimates at API prices, never a charge, and never leave your computer. |
| **Provider status** | Opt-in badge when a provider reports an incident, so you know a failure may not be your account. |
| **Themes and languages** | 15 themes in dark and light, custom themes, 14 languages. |
| **Updates** | An optional daily check that sends only the version, system and architecture. |

<div align="center">
  <img src="docs/assets/screenshots/desktop-details.png" alt="Account details with pace marks" width="48%" />
  <img src="docs/assets/screenshots/desktop-tray-list.png" alt="Tray popover in list view" width="30%" />
</div>

**Guides for your system**, with screenshots, where things are stored, notification permissions and how to uninstall:

- [Cuotai on macOS](docs/gui-macos.md): menu bar, Keychain, Gatekeeper.
- [Cuotai on Windows](docs/gui-windows.md): notification area, Credential Manager, SmartScreen.
- [Cuotai on Linux](docs/gui-linux.md): AppIndicator, Secret Service, `.deb` / `.rpm` / AppImage.

The complete list of desktop features and every setting is in the [desktop feature reference](docs/gui-features.md).

## Terminal dashboard

Prefer to stay in the terminal? The same engine powers an interactive dashboard with keyboard and mouse support, instant filtering and a built-in account wizard. Run `cuotai`, press `a` to add an account, and use `--once` or `--json` for scripts and status bars.

<div align="center">
  <img src="docs/assets/screenshots/tui-dashboard.png" alt="The Cuotai terminal dashboard in English with six placeholder accounts" width="100%" />
  <p><em>The terminal dashboard in English, with placeholder accounts. Its captions follow your language.</em></p>
</div>

Keyboard shortcuts, the account wizard, filters, the command line and its exit codes are in the [terminal guide](docs/tui.md); scripting recipes (`--json`, `--serve`, `--watch`, hooks, history and cost commands) are in [advanced usage](docs/advanced.md).

## Themes

Fifteen built-in themes, every one with a dark and a light mode. The picker and the mode switch (system, light, dark) are in **Settings › General**.

<table>
  <tr>
    <td align="center"><img src="docs/assets/screenshots/theme-cuotai-dark.png" alt="Cuotai theme, dark" /><br /><sub>Cuotai, dark (the app's own theme, default for new installs)</sub></td>
    <td align="center"><img src="docs/assets/screenshots/theme-cuotai-light.png" alt="Cuotai theme, light" /><br /><sub>Cuotai, light</sub></td>
    <td align="center"><img src="docs/assets/screenshots/theme-dracula-dark.png" alt="Dracula theme, dark" /><br /><sub>Dracula</sub></td>
  </tr>
  <tr>
    <td align="center"><img src="docs/assets/screenshots/theme-github-light.png" alt="GitHub theme, light" /><br /><sub>GitHub, light</sub></td>
    <td align="center"><img src="docs/assets/screenshots/theme-synthwave-dark.png" alt="Synthwave theme, dark" /><br /><sub>Old Terminal (synthwave)</sub></td>
    <td align="center"><img src="docs/assets/screenshots/theme-windows11-light.png" alt="Windows 11 theme, light" /><br /><sub>Windows 11, light</sub></td>
  </tr>
</table>

See all of them, in both modes, and learn how to add your own in [`docs/themes.md`](docs/themes.md).

## Supported services

Cuotai ships **40 integrations**. For what each one reads, the credential it needs and its limitations, see [`docs/providers.md`](docs/providers.md).

| Category | Services |
|---|---|
| **Assistants and subscriptions** | Claude, OpenAI Codex / ChatGPT, GitHub Copilot, Cursor (personal and Teams), Windsurf, Kiro / AWS, Antigravity, Cline, Zed, CodeBuddy, Qoder, OpenCode Go |
| **CLIs and coding plans** | Gemini CLI, Grok CLI / xAI, Kimi Coding and API, MiniMax Token / Coding Plan, Alibaba Coding Plan, Alibaba Token Plan / Qwen Cloud, Xiaomi MiMo, Ollama Cloud, Qwen Code |
| **Cloud billing and admin APIs** | OpenAI Admin API, Anthropic Admin API, Google Gemini API Monitoring, Google Vertex AI, AWS Bedrock Cost Explorer, Azure OpenAI Cost Management, Alibaba Cloud BSS, Mistral Admin, Perplexity Computer Analytics, Kimchi, Groq, ElevenLabs, OpenRouter |
| **Aggregators** | 9router and LiteLLM proxy (the quotas or budgets of the connections inside your own instance) |
| **Additional catalog** | Alibaba Model Studio, Cloudflare AI, NVIDIA NIM, Poolside |

Some providers expose usage only through interfaces the vendor does not document, and a few cost money per query (AWS Cost Explorer). [`docs/providers.md`](docs/providers.md) records how each integration is read, how far to trust it and its limitations; the app itself keeps these levels out of the cards and details.

## Help Cuotai grow

Cuotai is a one-person, open-source project and every bit of help counts. Support happens on GitHub:

- **Star the repository** so more people find it.
- **Get help.** [`SUPPORT.md`](SUPPORT.md) says where to ask and what response to expect.
- **Report a bug or ask for something** in [Issues](https://github.com/jmarcon/cuotai/issues). There are templates for bugs, provider problems and **new provider requests** (tell us which service you need and what its usage page or API looks like). Use [private reporting](SECURITY.md) for security issues, and never paste a real key or token.
- **Translate or improve a language.** The interface is available in 14 languages, and you can add another one as a single JSON file ([how](apps/desktop/ui/locales/README.md)). English is the source language: every text is written in English first and translated from it. Ten languages are machine drafts, so a native speaker's proofreading is the most useful help you can give.
- **Contribute a theme, a fix or a provider.** Read [`CONTRIBUTING.md`](CONTRIBUTING.md) first.
- **Tell others** if it saved you from hitting a limit mid-task.

There is no sponsorship page yet; if one appears it will be linked here.

## Documentation

All technical documentation lives in **[`docs/`](docs/)**.

**Using the app**

- [Cuotai on macOS](docs/gui-macos.md) · [on Windows](docs/gui-windows.md) · [on Linux](docs/gui-linux.md): install, first run, system specifics, uninstall.
- [Desktop feature reference](docs/gui-features.md): every desktop feature and setting.
- [Themes](docs/themes.md): the gallery and how to add your own.
- [Installing and verifying](docs/installing.md): channels and their status, verifying a download, uninstalling.
- [Terminal dashboard, command line and account wizard](docs/tui.md): shortcuts, wizard, configuration precedence, exit codes.
- [Providers, credentials and limitations](docs/providers.md) and [native sign-in and the vault](docs/oauth.md).
- [Platform support](docs/platforms.md): what works on macOS, Windows and Linux.

**Automation and integration**

- [Advanced usage](docs/advanced.md): the configuration file, JSON output, local cost and history from the command line.
- [Local HTTP server](docs/local-server.md), [events and `--watch` hooks](docs/events.md), [export contract v1](docs/export-v1.md), [executable extensions](docs/extensions.md) and [system widgets](docs/widgets.md).
- [Configuration example](cuotai.example.toml): a commented reference of every account and TOML parameter.

**For contributors and maintainers**

- [Building from source](docs/building.md) and [architecture, Rust library and verification](docs/architecture.md).
- [Desktop shell notes](apps/desktop/README.md).
- [Roadmap and product plans](docs/plans/0000_control.md), [changelog](CHANGELOG.md).
- [Releasing](docs/releasing.md), [release policy](docs/release-policy.md), [updater security](docs/updater-security.md) and [package-manager templates](packaging/README.md).

## License

Cuotai is distributed under the terms of either the [MIT license](LICENSE-MIT) or the [Apache License, Version 2.0](LICENSE-APACHE), at your option. Third-party notices are in [`THIRD_PARTY_NOTICES`](THIRD_PARTY_NOTICES).

Unless you explicitly state otherwise, any contribution intentionally submitted for inclusion in Cuotai, as defined in the Apache-2.0 license, is dual licensed as above, without any additional terms or conditions.
