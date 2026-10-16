# Idiomas do Cuotai

Um idioma = um arquivo `locales/<código>.json` (ex.: `pt-BR`, `en-US`, `es`).

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
* Textos que vêm do core (nomes de campos de cada provedor, ajuda, erros) ainda não são traduzíveis; ver PRD 0001, RF8.

## Embutidos

Para incluir um idioma no app: crie o arquivo aqui e acrescente `{ code, name }` em `LOCALES` (`js/i18n.js`).
`pt-BR` é a fonte; `en-US` é o idioma de reserva e deve ter todas as chaves.

## Adicionados pelo usuário (sem recompilar)

Coloque o arquivo em `<pasta de configuração>/cuotai/locales/<código>.json` e reinicie o app. A pasta é
`~/Library/Application Support/cuotai` no macOS, `%APPDATA%\cuotai` no Windows e `~/.config/cuotai` no Linux,
ou o que estiver em `CUOTAI_USER_DIR`. O botão de pasta na barra superior copia o caminho. Um idioma do usuário
com o mesmo código de um embutido o substitui. Arquivos inválidos são ignorados e o motivo aparece num aviso.
Um exemplo parcial está em `apps/desktop/examples/user/locales/fr.json`.
