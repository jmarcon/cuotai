# Support

Cuotai is a free, open-source project maintained in the open. Support goes
**through GitHub only**: there is no support e-mail address, chat or phone.

## Where to ask

| You want to | Go to |
|---|---|
| Report a bug | [Issues](https://github.com/jmarcon/cuotai/issues/new/choose), "Bug" |
| Report a provider that stopped working or shows wrong numbers | [Issues](https://github.com/jmarcon/cuotai/issues/new/choose), "Provider problem" |
| Ask for a new provider | [Issues](https://github.com/jmarcon/cuotai/issues/new/choose), "New provider" |
| Ask a question or share a setup | [Discussions](https://github.com/jmarcon/cuotai/discussions) |
| Report a security problem | [Private vulnerability reporting](https://github.com/jmarcon/cuotai/security/advisories/new), as described in [`SECURITY.md`](SECURITY.md). Never in a public issue. |
| Contribute a fix | [`CONTRIBUTING.md`](CONTRIBUTING.md) |

Read the [provider matrix](docs/providers.md) first: it says which
interfaces are documented, which are not, and how each provider is read.

## What to include

For a provider problem, use **Report a problem** instead of describing it by
hand: in the desktop app it is in the details of a failing account and in
Settings > About; in the terminal, `cuotai --report-problem ACCOUNT` prints the
same text. It lists the Cuotai version, your system, the install channel, the
provider, the failure category and the names of the fields that changed, and
nothing else: never your account name, e-mail, organization, configuration,
environment-variable names or any value. Nothing is sent by Cuotai; you read
the text and decide whether to post it.

For anything else: the Cuotai version, your operating system, how you
installed it (`cuotai --install-source`), what you expected and what
happened. **Never paste a real key, token, cookie or the contents of your
accounts file.**

## Response targets

These are goals of a volunteer project, not guarantees:

- a provider breakage is triaged within **3 days**;
- once a fix is merged, a patch release follows as soon as practical. It
  reaches the desktop app through the in-app updater and the other
  installs through their package managers (see
  [`docs/installing.md`](docs/installing.md));
- the interfaces Cuotai cannot check automatically are re-verified on a
  schedule: evidence older than **90 days** raises a warning and older than
  **180 days** fails a non-required check (see
  [`docs/providers.md`](docs/providers.md));
- release cadence and which versions get fixes follow
  [`docs/release-policy.md`](docs/release-policy.md): only the latest stable
  release is supported.

## What is not supported

- Paid or private support, service levels or custom development.
- Recovering an account, a password or a subscription with a provider:
  contact the provider.
- Interfaces a provider documents no way to read. Cuotai only reads usage; it
  never changes an account.
- Versions older than the latest stable release.
