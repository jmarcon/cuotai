# Temas do Cuotai

Um tema = um arquivo `css/themes/<id>.css` + uma entrada em `js/themes.js` + um `@import` em `css/themes/index.css`.

## Seletores

`<html data-theme="<id>" data-mode="dark|light">`. Cada arquivo define **apenas** blocos com os seletores

```css
:root[data-theme="<id>"][data-mode="dark"]  { … }
:root[data-theme="<id>"][data-mode="light"] { … }
```

**Todo tema deve oferecer os dois modos (`dark` e `light`)**, com a mesma identidade visual adaptada a cada um (ex.: um tema "terminal" claro vira papel/âmbar sobre creme; um tema de sistema operacional claro/escuro segue a variante oficial do sistema). Liste ambos em `modes` no registro. Todo seletor extra
(efeitos, ajustes finos) também deve começar por `:root[data-theme="<id>"]` (e, se for o caso, `[data-mode=…]`),
para nunca vazar para outro tema. Não use `!important`, não altere `html`/`body` fora dos tokens
e não redefina regras de layout (largura, grid, padding, tamanhos de fonte): só aparência.

## Tokens (todos obrigatórios por modo, exceto os marcados como opcionais)

Cores — **sempre `#rrggbb`** (o verificador de contraste lê esses valores):

| Token | Uso |
|---|---|
| `--bg` | fundo da janela / popover |
| `--surface` | cards, diálogos, controles |
| `--surface-2` | hover, trilhos secundários, grupos |
| `--border` | divisores e contornos |
| `--fg` / `--fg-muted` | texto principal / secundário |
| `--accent` / `--accent-fg` | destaque (switch, botão primário) e texto sobre ele |
| `--good` / `--warn` / `--bad` / `--info` | estados (≥ 70 %, 30–70 %, < 30 %, informativo) |
| `--track` | trilho vazio das barras |
| `color-scheme` | `dark` ou `light` |

Forma e tipografia (opcionais; têm padrão em `themes.css`):

`--font-ui`, `--font-mono`, `--radius-lg` (cards, diálogos, popover), `--radius-md` (linhas, métricas, caixas),
`--radius-sm` (botões, campos), `--radius-pill` (selos, switch, barras), `--logo-radius`, `--border-w`.

Superfícies e efeitos (opcionais):

`--shadow`, `--shadow-dialog`, `--card-bg` (aceita gradiente), `--head-bg` (cabeçalho do card, aceita gradiente),
`--topbar-bg`, `--btn-bg`, `--btn-fg`, `--input-bg`, `--switch-knob`, `--body-bg` (aceita gradiente/imagem).

Movimento (opcionais; padrão em `themes.css`, temas do usuário herdam):

| Token | Padrão | Uso |
|---|---|---|
| `--motion-fast` | `150ms` | hover, foco, mudanças de cor |
| `--motion-base` | `250ms` | interruptor, entradas, diálogos |
| `--motion-slow` | `400ms` | barras de quota e anel da bandeja |
| `--motion-spin` | `900ms` | uma volta do ícone de atualização em andamento |
| `--ease-out` | `cubic-bezier(0.22, 1, 0.36, 1)` | curva padrão de saída |
| `--ease-spring` | `linear(…)` (fallback `cubic-bezier(0.34, 1.56, 0.64, 1)`) | mola curta (interruptor) |

Temas retrô podem sobrescrever (o Win98 usa `steps()`; o CRT usa `0ms`). Use sempre os tokens, nunca durações fixas.
Os efeitos do painel (`js/motion.js`) derivam desses tokens: entrada, FLIP, saída e pulso do chip usam `--motion-base`;
contagem das porcentagens, "shake" ao esgotar e destaque de nível (1,5 × `--motion-slow`) usam `--motion-slow`; o giro
do botão e o shimmer de "Aguardando" (2 × `--motion-spin`) usam `--motion-spin`. Token em `0ms` = efeito desligado.
Com `data-motion="reduced"`/`"off"` no `<html>` (Configurações → Aparência → Animações) ou com
`prefers-reduced-motion: reduce` (exceto `data-motion="always"`), `themes.css` força as durações a `0ms`
com `!important`, vencendo o tema.

## Regras de qualidade

* Rode `python3 tools/contrast.py ui/css/themes/<id>.css` (contraste WCAG) e `python3 tools/lint_theme.py ui/css/themes/<id>.css` (escopo/regras) — ambos devem passar. São ferramentas opcionais de desenvolvimento; o app não depende de Python.
* `--good`, `--warn` e `--bad` devem ser distinguíveis entre si e do `--fg`.
* Nada de imagens externas, fontes remotas ou `url(http…)`: o app funciona offline. Fontes: use pilhas de
  fontes do sistema em `--font-ui`/`--font-mono`.
* Efeitos (brilho, scanlines) devem ser leves e não reduzir a legibilidade.

## Temas do usuário (sem recompilar)

Coloque `<id>.css` em `<pasta de configuração>/cuotai/themes/` (macOS `~/Library/Application Support/cuotai`,
Windows `%APPDATA%\cuotai`, Linux `~/.config/cuotai`, ou `CUOTAI_USER_DIR`) e reinicie o app. Regras:

* `id` = nome do arquivo (`[a-z0-9-]`, até 32 caracteres) e não pode repetir o de um tema embutido.
* Cabeçalho opcional na primeira linha: `/*! cuotai-theme {"label":"Nome","group":"custom","modes":["dark","light"]} */`.
  Sem ele, o nome é o id, o grupo é "Meus temas" e os modos são deduzidos dos seletores.
* Mesmo contrato dos embutidos, com validação mais estrita na carga: nada de `@import`, `@font-face`, URLs externas
  ou qualquer @-regra, e todo seletor deve começar por `:root[data-theme="<id>"]`. Temas fora disso são ignorados e o
  motivo aparece num aviso na janela principal.
* Exemplo: `apps/desktop/examples/user/themes/ocean.css`.
