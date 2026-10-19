# PRD 0004 — Animações e efeitos visuais

| Campo | Valor |
|---|---|
| ID | 0004 |
| Status | Implementado (0.17.0 a 0.20.0) |
| Responsável | Desktop (principal) e TUI |
| Consumidores | `apps/desktop/ui`, `crates/cuotai-tui` |
| Data | 2026-10-19 |

## Estado da implementação (2026-10-19)

Cada RF é entregue em uma versão própria, com `cargo fmt`, `clippy -D warnings`, testes do workspace e do desktop e smoke da interface antes do commit.

| RF | Versão | Entrega |
|---|---|---|
| RF1 | 0.17.0 (`6725548`) | Tokens `--motion-*`/`--ease-*` no contrato de temas (win98 em `steps()`, CRT sem interpolação), `prefers-reduced-motion` e a opção **Aparência → Animações** em `desktop.json`, aplicada como `data-motion` antes do JS; o painel reconcilia cartões por `data-card` (`patch()` em `util.js`), preservando foco, rolagem e as barras. |
| RF2 | 0.18.0 | `js/motion.js`: contagem das porcentagens, destaque único na mudança de nível, tremida só ao esgotar, entrada em cascata, FLIP na reordenação, saída encolhendo, skeleton com shimmer (parado com a janela escondida), spinner só na conta em refresh, elevação no hover/foco, pulso do chip ao zerar e giro único do refresh. Painel parado sem animações. |
| RF3 | 0.19.0 | `showDialog()` em `motion.js` para todos os diálogos: fade + escala 0,96 → 1, backdrop com desfoque, fechamento animado depois do `close()` nativo (foco volta na hora ao botão de origem, inclusive no WKWebView) e detalhes crescendo a partir do cartão. A bandeja entra com fade + 6 px e o anel anima do valor anterior. Troca de tema e idioma com `startViewTransition` (250 ms) quando há suporte e movimento ligado. Smoke no app Tauri no macOS para diálogos, foco e tema; a entrada da bandeja foi provada só no navegador (a automação não abre a janela da bandeja). |
| RF4 | 0.20.0 | `crates/cuotai-tui/src/motion.rs`: com animação ativa o loop desenha a ~30 fps (poll de 33 ms), parado mantém o desenho só quando `dirty`. Gauge e barras do detalhe interpolam em 300 ms com resolução de 1/8 de célula, gradiente por nível com truecolor, rótulo da classe pisca duas vezes na mudança, spinner braille no título da conta em refresh e wipe de 120 ms em detalhes, Sobre e Modelos. `--no-animations`, `CUOTAI_NO_ANIMATIONS`, desligado em `TERM=dumb` e por padrão sob SSH (`CUOTAI_ANIMATIONS=1` reativa). Com animações desligadas a saída é idêntica à anterior; parada, a TUI não redesenha além do tique de 1 s já existente. |

## 1. Contexto

O desktop quase não tem movimento: um `@keyframes spin` (`themes.css`, `detail.css`), `transition: width 0.4s` nas barras (`themes.css`), o anel da bandeja (`tray.css`: `stroke-dasharray`) e duas transições de 0,2 s em `main.css`. Nenhum CSS trata `prefers-reduced-motion`. Além disso, `main.js` redesenha o painel com `root.innerHTML = …`, e isso recria os elementos a cada atualização. Na prática, a transição das barras quase nunca roda: o valor novo aparece de uma vez. Os 14 temas seguem o contrato de tokens de `css/themes/README.md`.

A TUI redesenha só quando `dirty`, com `event::poll` de 100 ms (`ui.rs`). Os cartões usam `Gauge` do Ratatui (`dashboard.rs`), e o detalhe usa barras de texto `█`/`░`. Não há nenhum efeito temporal.

## 2. Problema

Atualizações de quota, mudança de estado (disponível → esgotado), abertura de diálogos e reordenação de cartões acontecem sem continuidade visual: é difícil perceber o que mudou depois de um refresh. Por outro lado, animação em excesso:
- custa CPU e bateria num app que fica aberto na bandeja;
- atrapalha quem tem sensibilidade a movimento;
- num terminal, pode gerar flicker e tráfego em SSH/tmux.

## 3. Objetivos

- Deixar perceptível **o que mudou** (valor, estado, ordem) com movimento curto, de 150 a 400 ms.
- Dar ao desktop uma sensação moderna e coesa (entradas, diálogos, hover, carregamento) sem quebrar nenhum dos 14 temas.
- Na TUI, ter efeitos discretos e opcionais que não aumentem o custo quando o terminal está parado.
- Respeitar `prefers-reduced-motion` e oferecer um interruptor próprio nos dois frontends.

## 4. Não-objetivos

- Bibliotecas de animação (GSAP, Framer, Lottie) ou frameworks de UI. Só CSS, a Web Animations API e o que o Ratatui já oferece.
- Efeitos contínuos e decorativos com o painel parado (partículas, fundos animados, brilho pulsando sem motivo).
- Mudar dados, contratos do core ou a saída `--once`/`--json`.

## 5. Requisitos funcionais

### RF1 — Base de movimento no desktop (prioridade alta; pré-requisito)

- **RF1.1** Tokens de movimento no contrato de temas (`themes.css` e `css/themes/README.md`):
  - durações `--motion-fast` (150 ms), `--motion-base` (250 ms) e `--motion-slow` (400 ms);
  - curvas `--ease-out` e `--ease-spring`, esta via `linear()` com fallback.
  
  Temas retrô podem sobrescrever (o Win98 usa `steps()`, o CRT pode usar 0 ms). Temas de usuário herdam os padrões.
- **RF1.2** Movimento reduzido:
  - `@media (prefers-reduced-motion: reduce)` zera durações e remove deslocamentos, mantendo só mudanças de opacidade de até 150 ms;
  - a opção **Configurações → Aparência → Animações: Sistema / Sempre / Reduzidas / Desligadas** fica em `desktop.json` e é aplicada como `data-motion` no `<html>`, sem esperar o JS principal (como hoje no `index.html` para o tema).
- **RF1.3** Atualização incremental do painel:
  - trocar o `innerHTML` total por reconciliação por cartão, com chave `data-card` (já existe): criar, remover e atualizar só o que mudou;
  - preservar foco, posição de rolagem e elementos de barra, para que as transições rodem de verdade;
  - sem framework novo: uma função pequena que compara por chave e substitui os nós internos que mudaram.
- **Critério de aceite:**
  - refresh com valor alterado anima a largura da barra (verificado com `getAnimations()` ou captura intermediária);
  - foco no botão de um cartão sobrevive ao refresh;
  - com `prefers-reduced-motion` emulado e com "Desligadas", nenhuma transição passa de 150 ms;
  - os 14 temas continuam passando no smoke visual.

### RF2 — Efeitos do painel desktop (prioridade alta)

- **RF2.1** Dados:
  - barras e porcentagens interpolam entre o valor antigo e o novo (contagem numérica com `requestAnimationFrame`, só durante a transição);
  - mudança de nível (`good` → `warn` → `bad`) faz a cor transitar e dá um destaque único de 600 ms no cartão;
  - esgotar a quota aplica um "shake" curto de 1 ciclo só quando o estado muda, nunca a cada refresh.
- **RF2.2** Entrada e ordem:
  - cartões entram com fade + deslocamento de 8 px, em cascata de 30 ms (com teto de 300 ms no total);
  - reordenar (arrastar, setas ou ordenação) usa FLIP (medir → inverter → animar `transform`), de modo que os cartões deslizam até a nova posição;
  - remover a conta encolhe e some antes de sair do DOM.
- **RF2.3** Carregamento: skeleton com "shimmer" nos cartões em "Aguardando", e o spinner atual só no botão da conta em refresh. O shimmer para quando os dados chegam e não roda com a janela escondida (`document.hidden`).
- **RF2.4** Micro-interações:
  - hover e foco com elevação sutil (sombra/escala de 1,01);
  - o interruptor desliza com mola;
  - o chip da próxima consulta pulsa uma vez quando zera;
  - o ícone de refresh gira uma vez ao clicar.
  
  O foco visível (`:focus-visible`) nunca é removido nem animado de forma a sumir.
- **Critério de aceite:** gravação curta ou sequência de capturas de cada efeito em pelo menos Dracula, GitHub claro, Win98 e CRT; painel parado com 15 cartões sem animação ativa (`document.getAnimations().length === 0`); sem queda de quadros perceptível no perfil do WebKit ao atualizar 15 cartões.

### RF3 — Diálogos, bandeja e transições de tela (prioridade média)

- **RF3.1** Diálogos (`<dialog>`: conta, detalhes, sobre, configurações):
  - abrem com fade + escala de 0,96 → 1 e backdrop com desfoque leve onde houver suporte;
  - fecham com animação inversa antes de `close()`, sem atrasar o retorno do foco;
  - o popover de detalhes cresce a partir do cartão de origem.
- **RF3.2** A janela da bandeja (`tray.html`) aparece com fade + deslize de 6 px a partir do ícone. O anel de uso anima do valor anterior, e não do zero, a cada abertura.
- **RF3.3** Usar a View Transitions API, onde o WebKit do sistema a suportar, para trocar tema e idioma (cross-fade de 250 ms); sem suporte, troca imediata.
- **Critério de aceite:** smoke no app Tauri (não só no navegador) no macOS: abrir e fechar cada diálogo pelo teclado (`Esc`, `Tab`) mantém foco e leitura por leitor de tela; trocar de tema sem flash branco.

### RF4 — Efeitos na TUI (prioridade média)

- **RF4.1** Motor mínimo de animação em `ui.rs`:
  - com uma animação ativa, o loop passa a desenhar a cerca de 30 fps (`poll` de 33 ms);
  - sem animação, volta ao comportamento atual (desenho só quando `dirty`);
  - o estado da animação (valor de origem, de destino e início) fica por cartão, sem threads novas.
- **RF4.2** Efeitos:
  - `Gauge` e barras do detalhe interpolam do valor antigo ao novo em cerca de 300 ms após o refresh;
  - mudança de estado pisca o rótulo da classe (Disponível/Esgotado) duas vezes;
  - spinner braille (`⠋⠙⠹…`) no título do cartão em refresh, no lugar do texto estático;
  - transição de abertura e fechamento de detalhes, Sobre e Modelos com "wipe" vertical de 120 ms;
  - barras com meio-bloco (`▏▎▍▌▋▊▉█`) para resolução de 1/8 de célula, e gradiente de cor por nível em terminais com truecolor (com fallback para 16 cores quando `COLORTERM` não indicar truecolor).
- **RF4.3** Controle: `--no-animations` e a variável `CUOTAI_NO_ANIMATIONS`. As animações também ficam desligadas automaticamente em `TERM=dumb` e, por padrão, sob SSH (`SSH_CONNECTION`), onde cada quadro custa banda. `--once`/`--json` não mudam.
- **Critério de aceite:**
  - smoke em tmux: captura durante a animação mostra valor intermediário, e depois o final;
  - com animações desligadas, a saída é idêntica à de hoje;
  - parada, a TUI não redesenha (medido contando chamadas a `draw`);
  - sem flicker visível no Terminal.app, iTerm2 e tmux.

## 6. Requisitos não funcionais

- **Acessibilidade:**
  - nenhum efeito carrega informação sozinho: cor, texto e `aria-live` continuam sendo a fonte;
  - nada pisca mais de 3 vezes por segundo (WCAG 2.3.1);
  - movimento reduzido é respeitado por padrão.
  
  Validação completa exige teste manual com tecnologias assistivas.
- **Desempenho:**
  - animar apenas `transform`, `opacity` e cor; largura só nas barras (até 15 elementos);
  - nenhuma animação contínua com a janela escondida ou o painel parado;
  - a TUI parada mantém o custo de hoje.
- **Temas:** todo efeito usa tokens. Os temas de sistema (macOS, Windows 11, Ubuntu) seguem a duração e a curva nativas da plataforma, e os retrô podem desligar a interpolação.

## 7. Ordem de execução e verificação

1. **RF1:** tokens, movimento reduzido, opção de Aparência e reconciliação incremental (pré-requisito para qualquer transição funcionar).
2. **RF2:** efeitos do painel, um por versão, com captura em quatro temas.
3. **RF3:** diálogos, bandeja e View Transitions, com smoke no app Tauri.
4. **RF4:** motor e efeitos da TUI, com smoke em tmux e medição de redesenho parado.

Em cada etapa: `cargo fmt`, `clippy -D warnings`, testes do workspace e do desktop, `node --check`, e smoke visual no desktop e na TUI antes da versão.

O requisito só está entregue quando um refresh que muda valores mostra a transição nos dois frontends, a reordenação desliza os cartões, os diálogos abrem e fecham com movimento sem perder foco, e com movimento reduzido ou desligado tudo continua funcionando de forma instantânea e sem nenhuma animação ociosa.
