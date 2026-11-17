# Cuotai languages

Each built-in language has a core catalog, a desktop UI catalog and a TUI
catalog. The core catalogs in `crates/cuotai-core/locales/<code>.json` contain
the texts emitted by the backend; English is the text written in the core's
code (the `fallback`), so en-US has no core catalog. The TUI catalogs in
`crates/cuotai-tui/locales/<code>.json` hold the terminal captions. The desktop UI catalogs in
`apps/desktop/ui/locales/<code>.json` contain shell labels, dialogs, tray
messages, settings and themes.

```json
{
  "_meta": { "name": "Italiano" },
  "common.add": "Aggiungi account",
  "quota.count": { "one": "{count} quota", "other": "{count} quote" },
  "metric.balance": "Saldo: {amount}"
}
```

- `_meta.name` is required and is shown in the language selector.
- Values are plain text. `{placeholder}` names are substituted and must not
  be changed.
- Plural objects use the language's `Intl.PluralRules` categories and select
  a form with `{count}`.
- Missing keys fall back to `en-US`, then to the key itself. This is for
  user language packs: a built-in catalog may not miss a key (the parity
  tests fail).
- Text is escaped before it is rendered; do not add HTML.
- **Untrusted text (PRD 0034 re-validation S-06).** A `{placeholder}` is
  filled with plain text that can come from outside: an account name, a
  provider's message, a model id, a path. Substitution is one pass: a value
  that itself contains `{name}` is shown as written, never expanded again.
  Each front end escapes the result for where it goes (HTML in the desktop,
  terminal text in the TUI, notices) and strips direction and control
  characters from it. So a catalog value holds no markup (no tag such as
  `<b>`, no character reference such as `&amp;`, and no `<`, `>` or `&`
  beyond what the source text has), no link or address a UI could act on
  (a URL in a help text is shown as text only), and no direction or
  zero-width characters (U+200B to U+200F, U+202A to U+202E, U+2060 to
  U+2064, U+2066 to U+2069, U+061C, U+FEFF), which can reorder or hide what
  is shown.
- A user language pack is named by a language tag the browser's `Intl`
  reads (`fr.json`, `sr-Latn-RS.json`); a name such as `en-US-US.json` is
  refused and listed as an invalid file.
- Frontends resolve stable core message IDs (`{ id, params, fallback }`) rather
  than matching translated text. `js/i18n.js` resolves IDs against the merged
  UI and core catalogs, with user language packs taking precedence.

## Adding a built-in language

Every language is one row of the core registry, `cuotai_core::text::LOCALES`
(`crates/cuotai-core/src/text/registry.rs`, PRD 0035 RF1): code, endonym,
flag, script, language subtags and aliases, plural rule and status. The core,
the TUI and the desktop all read it; no test loop, prefix chain or
hard-coded list needs editing. The fourteen rows (pt-BR, en-US, es, it,
zh-CN, fr, de, ja, ru, ko, hi-IN, id-ID, tr and vi) are shipped. A new row
starts with the status `in_progress`: it resolves (a Japanese system is
recognized as Japanese) but reads the English source, and the language list
shows it as "Translation in progress", not selectable.

To ship a registered language:

1. Add `crates/cuotai-core/locales/<code>.json` with every core id (the key
   set of `pt-BR.json`), and its line in the `embedded_catalogs!` table of
   `crates/cuotai-core/src/text.rs`.
2. Add `crates/cuotai-tui/locales/<code>.json` with every TUI id, and its
   line in the `tui_catalogs!` table of `crates/cuotai-tui/src/l10n.rs`.
3. Add `apps/desktop/ui/locales/<code>.json` with every id of `en-US.json`
   (the UI source).
4. Set the row's `status` to `shipped`, then regenerate the desktop's two
   generated copies of the registry, the static copy in
   `js/locale-registry.js` and the pre-paint list in `js/first-paint.js`
   (the external script the three HTML pages load):
   `CUOTAI_REGENERATE_LOCALES=1 cargo test -p cuotai-core locale_registry_copies`.

`_meta.name` of each catalog is the registry endonym. Values are strings or,
for an id that embeds `{count}`, a plural object with exactly the categories
of the language's rule (`other` for ja, zh-CN, ko, id-ID and vi; `one` and
`other` for en-US, de, es, it, tr, pt-BR, fr and hi-IN; `one`, `few`, `many`
and `other` for ru). The parity tests (`core_catalogs_are_in_parity`,
`desktop_ui_catalogs_are_in_parity` and the TUI's
`the_registry_matches_the_tui_catalogs`) fail on a missing or extra id, a
renamed placeholder, markup (an HTML tag, comment or character reference
in any value, or a `<`, `>` or `&` the reference value lacks), a wrong
plural shape, an empty value, a control, direction or zero-width character,
a decomposed (non-NFC) Latin or Cyrillic text, a `_meta.name`
that is not the endonym, and a file the registry does not list (or the
reverse). A row whose script is not Latin or Cyrillic also needs its
`--font-script` stack in `css/themes.css`, and every row needs drawn flag
artwork in `js/flags.js`; tests check both. A right-to-left row fails a test
until a PRD covers mirroring.

## Glossary

PRD 0035 RF11.4. One term per concept across the core, TUI and desktop
catalogs of a language. The table records the term each catalog uses today
(read from the catalogs; en-US is the source). A draft or a pull request
that touches a catalog uses these terms; a reviewer who wants a different
term changes it here and in all three catalogs of that language in the same
pull request.

Never translate: Cuotai, provider and product names, plan names as the
provider spells them, model ids, `CUOTAI_*` and other environment-variable
names, CLI flags, file names, TOML keys, and the words `OAuth`, `API`,
`token`, `URL`, `JSON`, `TOML`, `CLI` and `TUI` where the language uses
them in English. Placeholders such as `{account}` are never translated,
including the literal ones inside the Azure ARM path
(`.../accounts/{account}` and `.../accounts/{name}`).

| Language | account | provider | quota | reset | pace | fresh / aging / stale | credential vault | usage alerts | refresh | sign in |
|---|---|---|---|---|---|---|---|---|---|---|
| en-US | account | provider | quota | reset | pace (On pace) | Fresh / Aging / Stale | credential vault | Usage alerts | Refresh | Sign in |
| pt-BR | conta | provedor | quota (cota in some texts) | reiniciar / renovar | ritmo (No ritmo) | Recente / Envelhecendo / Desatualizada | cofre de credenciais | Alertas de uso | Atualizar | Entrar |
| es | cuenta | proveedor | cuota | reiniciar | ritmo (Al ritmo) | Reciente / Envejeciendo / Desactualizada | almacén de credenciales | Alertas de uso | Actualizar | Iniciar sesión |
| it | account | provider | quota | azzerarsi | ritmo (In linea) | Aggiornato / In invecchiamento / Obsoleto | vault delle credenziali | Avvisi di utilizzo | Aggiorna | Accedi |
| zh-CN | 账户 | 提供商 | 额度 | 重置 | 节奏 (符合节奏) | 最新 / 较旧 / 过期 | 凭据保险库 (系统保险库) | 用量提醒 | 刷新 | 登录 |
| fr | compte | fournisseur | quota | réinitialisation (se réinitialise) | rythme (Au rythme) | Récent / Vieillissant / Obsolète | coffre d’identifiants (coffre du système) | Alertes d’utilisation | Actualiser | Se connecter |
| de | Konto | Anbieter | Kontingent | Zurücksetzung | Tempo (Im Tempo) | Aktuell / Älter werdend / Veraltet | Tresor für Zugangsdaten | Nutzungswarnungen | Aktualisieren | Anmelden |
| ja | アカウント | プロバイダー | クォータ | リセット | ペース (ペースどおり) | 最新 / やや古い / 古い | 認証情報の保管庫 (システムの保管庫) | 使用量アラート | 更新 | サインイン |
| ru | учётная запись | провайдер | квота | сброс | темп (В темпе) | Актуально / Стареет / Устарело | хранилище учётных данных | Оповещения об использовании | Обновить | Войти |
| ko | 계정 | 제공업체 | 할당량 | 초기화 | 페이스 (페이스 유지) | 최신 / 조금 오래됨 / 오래됨 | 자격 증명 보관소 (시스템 보관소) | 사용량 알림 | 새로 고침 | 로그인 |
| hi-IN | खाता | प्रदाता | कोटा | रीसेट | गति (गति पर) | ताज़ा / पुराना होता / पुराना | क्रेडेंशियल वॉल्ट (सिस्टम वॉल्ट) | उपयोग अलर्ट | रीफ़्रेश | साइन इन |
| id-ID | akun | penyedia | kuota | reset | laju (Sesuai laju) | Baru / Mulai usang / Usang | brankas kredensial | Peringatan penggunaan | Segarkan | Masuk |
| tr | hesap | sağlayıcı | kota | sıfırlanma | hız (Hızında) | Güncel / Eskiyor / Eski | kimlik bilgisi kasası | Kullanım uyarıları | Yenile | Oturum aç |
| vi | tài khoản | nhà cung cấp | hạn mức | đặt lại | nhịp độ (Đúng nhịp độ) | Mới / Cũ dần / Cũ | kho thông tin xác thực | Cảnh báo sử dụng | Làm mới | Đăng nhập |

The zh-CN, fr, ja, ko and hi-IN rows were machine-reviewed on 2026-10-25
(the catalogs record it in `_meta.machine_reviewed`); the other rows are
read as the catalogs stand and await their own review. Every catalog of the
ten new languages keeps `_meta.status` "machine-translated, needs native
review" until a native reviewer signs the critical strings (RF11.2).

### Register

| Language | Register | Notes |
|---|---|---|
| en-US | neutral, second person "you" | source |
| pt-BR | informal "você" | |
| es | informal "tú" | precedent for tone |
| it | informal "tu" | precedent for tone |
| zh-CN | no pronoun by default; 您 only in error, consent, privacy and warning texts | help and labels drop 您 (自定价格表, not 您的价格表) |
| fr | "vous", imperative in the vous form | no-break space U+00A0 before `:`, narrow no-break space U+202F before `;` `?` `!`, U+00A0 inside « » (enforced by `style_tests.rs`; code such as `://`, `::1`, `Authorization:`, `#!` keeps its ASCII form) |
| de | "Sie" | |
| ja | です/ます | no あなた; use 現在のユーザー for "your user"; full-width colon `：` (no space after it) after Japanese text |
| ru | "вы" | |
| ko | 합쇼체 (-ㅂ니다/-습니다), polite imperative -세요 | the catalogs use 합쇼체 consistently; no 해요체 sentences; confirmation titles end in …하시겠습니까? (to confirm with a native reader: many Korean apps use 해요체 …할까요? in titles) |
| hi-IN | आप, imperative in the -एँ/-ें form | never तुम or तू |
| id-ID | "Anda" | |
| tr | "siz" | |
| vi | "bạn" | |

`crates/cuotai-core/src/text/style_tests.rs` enforces the register table (de, zh-CN, ja, ko), the glossary terms (ru, ko, zh-CN, de, hi-IN, vi, as ratchets of reviewed exceptions) and the fr/ja punctuation on the three catalog layers.

### Review checklist

Used by reviewers and by every pull request that touches a catalog (RF11.7):

- placeholders intact and in a natural order; the typed consent answers
  (`*.confirm.answer`, `*.consent.answer`) are easy to type;
- no brand, provider or product name translated;
- the glossary terms above and the register of the language;
- critical texts (RF11.3) say what happened and what to do, without blame,
  and mean what the English source says: compare with the English fallback
  in the code or with `en-US.json`, never with another translation;
- units and numbers outside the sentence; no markup and no exclamation
  marks; sentence case where the language has case;
- lengths within the budgets of RF9.3;
- for ja, zh-CN and ko, no machine-translation artifacts (wrong Han word,
  mixed-width punctuation);
- the native reviewer signs in the pull request.

## Length

Shorter is better for labels. A label more than 30 % longer than en-US, or
over its class budget in [`docs/i18n-length-budgets.md`](../../../../docs/i18n-length-budgets.md),
is reworded during review, never truncated by the UI; the core test
`length_budget` fails on a new one. Descriptions and callouts may wrap.

## Provenance and status

`_meta.status` says how a catalog was made (for example `machine-translated,
needs native review`) and `_meta.reviewed` records the reviews (`false`, or
the reviewers' GitHub handles with the date and the scope, `critical` or
`full`). A reviewer who finishes a scope adds their entry; never claim a
review that did not happen. The parity tests check the shape of `_meta`:

- `status`: a non-empty text;
- `reviewed`: `false`, or a list of `{ "handle": "github-handle", "date":
  "YYYY-MM-DD", "scope": "critical" | "full" }`;
- `machine_reviewed`: the `YYYY-MM-DD` date of the last machine pass;
- `source`: optional, the Cuotai version of the English source the catalog
  was synchronized with;
- `pending_review`: optional, the ids of this catalog whose text is a
  machine draft added after the last review (RF11.6). A pull request that
  adds or changes an English text and drafts it for a language nobody on
  the pull request can write appends the id here; the next reviewer starts
  with this list and removes the ids they checked. Every id must exist in
  the catalog, once.

Any other `_meta` key fails the tests, so a typo is not ignored.

### Translation status

The table below is generated from the registry and the `_meta` of each
language's catalogs; edit the catalogs, never the table. "Provenance" is
the desktop catalog's `status` ("maintainer" when it has none), the review
columns list the `reviewed` entries of the desktop catalog, and "Pending
review" counts the `pending_review` ids of the three catalogs.

<!-- status-table:begin (generated by `CUOTAI_REGENERATE_DOCS=1 cargo test -p cuotai-core locales_status_table`) -->
| Locale | Language | Flag | Core catalog | UI catalog | TUI catalog | Provenance | Critical review | Full review | Pending review |
|---|---|---|---|---|---|---|---|---|---:|
| `pt-BR` | Português | br | complete | complete | complete | maintainer | pending | pending | 9 |
| `en-US` | English | us | source (code) | source | source (code) | English source | source | source | 0 |
| `es` | Español | es | complete | complete | complete | maintainer | pending | pending | 9 |
| `it` | Italiano | it | complete | complete | complete | maintainer | pending | pending | 8 |
| `zh-CN` | 中文（简体） | cn | complete | complete | complete | machine-translated, needs native review | pending | pending | 125 |
| `fr` | Français | fr | complete | complete | complete | machine-translated, needs native review | pending | pending | 1485 |
| `de` | Deutsch | de | complete | complete | complete | machine-translated, needs native review | pending | pending | 95 |
| `ja` | 日本語 | jp | complete | complete | complete | machine-translated, needs native review | pending | pending | 154 |
| `ru` | Русский | ru | complete | complete | complete | machine-translated, needs native review | pending | pending | 137 |
| `ko` | 한국어 | kr | complete | complete | complete | machine-translated, needs native review | pending | pending | 107 |
| `hi-IN` | हिन्दी | in | complete | complete | complete | machine-translated, needs native review | pending | pending | 47 |
| `id-ID` | Bahasa Indonesia | id | complete | complete | complete | machine-translated, needs native review | pending | pending | 29 |
| `tr` | Türkçe | tr | complete | complete | complete | machine-translated, needs native review | pending | pending | 29 |
| `vi` | Tiếng Việt | vn | complete | complete | complete | machine-translated, needs native review | pending | pending | 64 |
<!-- status-table:end -->

Native reviewers volunteer through a [Question issue](https://github.com/jmarcon/cuotai/issues/new?template=question.yml) ("Looking for
reviewers", one per language) and are credited by their GitHub handle in the
`reviewed` entries, and so in this table.

## User-provided languages

Place a single `<config directory>/cuotai/locales/<code>.json` file and restart
the app. It can combine UI and core keys. A pack whose code equals a
built-in language (for example `fr.json`) overrides that built-in catalog
key by key (PRD 0035 RF12.3); a pack for a language that is not built in
registers as a new language. The directory is `~/Library/Application Support/cuotai`
on macOS, `%APPDATA%\cuotai` on Windows and `~/.config/cuotai` on Linux, or
the value of `CUOTAI_USER_DIR`. The folder button in the top bar copies its
path. Invalid files are ignored and a warning explains why. A partial example
is available at `apps/desktop/examples/user/locales/nl.json` (Dutch, a language
that is not built in, so it registers as a new one). The RF12.3 override is
per key (PRD 0035 re-validation R-01): a pack whose code is or normalizes to a
built-in language (`fr.json`, `fr-CA.json`, `pt-PT.json`) is layered over that
language's shipped catalogs, which the app always loads, so every text the pack
does not carry keeps the built-in translation, and the shipped catalog's
`_meta` still says whether it was machine-translated. The pack never replaces
the catalog. Settings › General names the active pack's file.

**A pack cannot pose as a shipped language (PRD 0035 re-validation S-03).**
The language selector never shows a name the pack chose for a shipped
language: a pack whose code is, or normalizes to, a built-in language shows
that language's own name (a `fr.json` named "Deutsch" reads "Français"); any
other pack shows its `_meta.name`, or its code when that name matches a
built-in language's name (an `xx.json` named "Français" reads "xx"). Its item
and the language field always add "user pack · file". `en-US.json` is refused:
English (United States) is the fallback every language reads, so a pack cannot
replace it; `en-GB.json` is allowed and reads the shipped English for every key
it leaves out.

**Protected texts (PRD 0050 RF3.3, PRD 0035 re-validation S-01).** A pack
cannot change the words of a confirmation, a consent or disclosure, a billed-query
or cost warning, a security or privacy warning, the sign-in, session, secret and
credential dialogs, the update, first-run, backup and restore dialogs, the
local-server, hooks, extensions, channels, vault, erase and privacy settings, or
the notices about user content. The core parses each pack once and drops those
keys before the app sees it (a file it cannot read safely is refused whole); the
app shows the shipped translation of the pack's language for them, else English,
and a notice with how many keys it dropped, once per content of the file (Settings ›
Files keeps it).

Which keys are protected is decided by one file,
`crates/cuotai-core/src/user_pack/policy.json`, with a `core` and a `ui` section.
A rule is an exact key or a prefix ending in `*`; the longest match wins and an
exact key beats a prefix. **A key that matches no rule is protected**, and so is
any key with a `confirm` or `consent` dot-segment. When you add a text, classify
it there: it is protected when it appears in, or describes, one of the surfaces
above, including every title, button, checkbox and error inside them (those
dialogs use the protected `confirm.cancel`, `confirm.remove`, `confirm.close`,
`confirm.back` and `confirm.dismiss`, not `common.*`). A UI key that
`js/desktop-wording.js` maps a protected core text onto is protected too and is
listed by name. Tests fail on a shipped key no rule covers, on a rule in both
lists, on a desktop wording that weakens a core text, and on any changeable key
rendered on a protected surface (`scripts/test-pack-protection-ui.mjs`, which
also needs a new dialog to carry a `data-protected-texts` mark).

**The trade-off.** About a third of the texts (about 1 400 of 4 100) are
protected. A pack for a language Cuotai ships loses nothing visible: those
texts keep the shipped translation. A pack for a language Cuotai does not ship
(the Dutch example) shows them in English, so about a third of the interface
stays English with such a pack, on purpose: a pack can never make a
destructive or billed action read as something else.

**Folder limits (PRD 0050 RF3.4).** Each of `themes/` and `locales/` reads at
most 64 files with its extension, in name order; the rest are reported by name.
Symbolic links are refused, and a file over 256 KiB is skipped.
