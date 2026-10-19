# Idiomas do Cuotai

Um idioma novo = um único arquivo de catálogo. O core publica os textos
que ele mesmo emite em `crates/cuotai-core/locales/{en-US,es}.json` (a
resolução segue a ordem: idioma exato → mesma língua → `en-US` →
`fallback` em português; `pt-*` nunca consulta catálogo); a UI publica
apenas os textos próprios da casca (rótulos, diálogos, bandeja,
temas, lista de idiomas) em `apps/desktop/ui/locales/<código>.json`.

```json
{
  "_meta": { "name": "Español" },
  "common.add": "Añadir cuenta",
  "quota.count": { "one": "{count} cuota", "other": "{count} cuotas" },
  "metric.balance": "Saldo: {amount}"
}
```

* `_meta.name` é o nome exibido no seletor (obrigatório).
* Valores são texto simples; `{variável}` é substituída (não renomeie as variáveis).
* Plural: objeto com as categorias do `Intl.PluralRules` do idioma (`one`, `other`, `few`, `many`…), escolhido por `{count}`.
* Chaves ausentes caem para `en-US` e, se também faltarem, a própria chave aparece.
* O texto é sempre escapado antes de ir para a tela: não use HTML.
* Frontends **não fazem correspondência por texto**: o core entrega cada
  texto como `{ id, params, fallback }` (RF8) e o `tl()`/`t()` do
  `js/i18n.js` resolve `id` no catálogo (UI + core, com pacotes de
  idioma do usuário por cima).

## Embutidos

Para incluir um idioma novo no app:

* Acrescente um arquivo `crates/cuotai-core/locales/<código>.json` para
  os ids que o core emite (`field.*`, `native.*`, `login.*`,
  `credential_help.*`, `oauth.*`, `cost_advisory.*`, `edit_error.*`,
  `query.*`, `note.*`, `remove_error.*`, `remove.kept.*`, `status.*`,
  `provider.*`, `binding.*`, `metric_window.*`, `pace.*`). `pt-BR` é a fonte — sem arquivo, o `fallback`
  embutido em cada `Localized` aparece em português. Outros idiomas
  precisam de um arquivo com todas as chaves do core; sem ele, o
  idioma cai para `en-US`.
* Acrescente um arquivo `apps/desktop/ui/locales/<código>.json` para
  os textos da UI (`common.*`, `dialog.*`, `tray.*`, `settings.*`,
  `theme.*`, `account.*`, `card.*`, `delete.*`, `filter.*`, `lang.*`,
  `metric.*`, `quota.*`, `sort.*`, `time.*`, `userContent.*`).
* Acrescente `{ code, name }` em `LOCALES` (`js/i18n.js`).

`en-US` é o idioma de reserva e deve ter todas as chaves do core e da UI.

## Adicionados pelo usuário (sem recompilar)

Coloque um único arquivo `<pasta de configuração>/cuotai/locales/<código>.json`
e reinicie o app. Esse arquivo pode misturar chaves da UI e do core —
substitui qualquer outro catálogo do mesmo código. A pasta é
`~/Library/Application Support/cuotai` no macOS, `%APPDATA%\cuotai` no
Windows e `~/.config/cuotai` no Linux, ou o que estiver em
`CUOTAI_USER_DIR`. O botão de pasta na barra superior copia o caminho.
Arquivos inválidos são ignorados e o motivo aparece num aviso.
Um exemplo parcial está em `apps/desktop/examples/user/locales/fr.json`.
