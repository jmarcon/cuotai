# Security policy

Cuotai reads quotas and costs from AI providers using the user's credentials. For that reason it treats credentials, tokens and sessions as the most sensitive asset of the project.

## Supported versions

Only the **latest stable version** receives security fixes. If you run an older version, please update before reporting when possible.

## Reporting a vulnerability

**Do not open a public issue.** Use GitHub's private vulnerability reporting:

<https://github.com/jmarcon/cuotai/security/advisories/new>

Include, without any real credentials:
- the Cuotai version and your operating system;
- what happens and what should happen;
- minimal steps to reproduce, using fictitious data;
- the impact you see (for example, token leakage, command execution, file access);
- when useful, the output of `cuotai --doctor --json` (or **Copy diagnostics** in the desktop app), the relevant lines of the redacted app log ([`docs/advanced.md`](docs/advanced.md#app-log)) and `cuotai --list-providers --json`, after you have reviewed them.

[`docs/privacy.md`](docs/privacy.md) lists every network flow and local file Cuotai has; a flow or file missing from it is worth reporting.

**Never** send tokens, cookies, API keys, the contents of a `cuotai.toml` that holds secrets, credential SQLite databases, or screenshots showing account data. If a real secret leaked while you were testing, revoke it with the provider before reporting.

## What to expect

Best-effort targets, with no contractual guarantee:
- acknowledgement within **7 days**;
- initial assessment within **14 days**;
- coordinated fix and disclosure with you, with credit if you wish.

## Scope

In scope, for example:
- credentials, tokens, cookies or sessions written to TOML, snapshots, logs, events or error messages;
- flaws in the system vault, OAuth token storage or token renewal;
- command execution, or file reads and writes beyond what the app declares;
- missing validation of input coming from providers or from user files.

Script extensions (PRD 0017) run as your own user, after you review an exact disclosure and approve them: **Cuotai restricts neither their network nor their files**, so what an approved extension does with them is the extension author's responsibility. What stays in scope is Cuotai's side of that contract: an extension running without approval or after its manifest or program changed, credentials or other accounts' data reaching an extension, or its output escaping validation.

Notification channels (PRD 0044) are in scope too. A channel's address and signing secret must live only in the system vault: never in `desktop.json`, the TOML, a log, the delivery log or a view. A channel must post only to the address the person added, with HTTPS (plain HTTP only for an ntfy server on loopback or a private network), no redirect followed, and only the notification texts described in [`docs/notifications.md`](docs/notifications.md).

Backups and the self-check are in scope too: a `cuotai --backup` archive must never hold a secret (vault entries, tokens, the local server token or anything shaped like a key; the command refuses to write when a file looks like one), a restore must never write outside its allow-listed files or to the vault, and `cuotai --doctor` and `--check-config` must never print a value, only names, positions and paths. `cuotai --doctor --probe` must send no credential, cookie or body (one `HEAD` to a fixed host per provider), and the doctor lines a problem report carries are ids only.

Out of scope:
- changes to or outages of provider interfaces (open a regular issue);
- the **plain-text** SQLite store of development mode (`--dev-credentials-sqlite`), which is intentional, exists only in debug builds and is refused in release builds;
- problems that require physical or administrative access to an already compromised machine;
- vulnerabilities in dependencies with no exploitation path in Cuotai (report them to the library; if there is a path, report it here).

## Dependencies

Dependency licenses and security advisories are checked with `cargo deny` (configuration in [`deny.toml`](deny.toml)). Relevant advisories ship in patch releases.
