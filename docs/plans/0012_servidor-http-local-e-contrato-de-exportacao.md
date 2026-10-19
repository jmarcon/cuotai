# PRD 0012 — Servidor HTTP local e contrato de exportação

| Campo | Valor |
|---|---|
| ID | 0012 |
| Status | Proposto |
| Responsável | `cuotai-core` (contrato e servidor), TUI/CLI e Desktop |
| Consumidores | `crates/cuotai-tui` (`--serve`), `apps/desktop` (servidor embutido), integrações de terceiros, PRDs 0013 e 0014 |
| Depende de | nenhum; incorpora o resultado dos PRDs 0005, 0007, 0009, 0010 e 0011 conforme forem entregues |
| Data | 2026-10-19 |

## 1. Contexto

Hoje a única saída para máquinas é `cuotai --json`, que serializa o `Snapshot` (`started_at`, `finished_at`, `reports`) e exige uma nova consulta completa a cada execução (`crates/cuotai-tui/src/main.rs`). Barras de status e scripts que chamam isso a cada poucos segundos repetiriam consultas aos provedores, com custo e risco de rate limit. Não há esquema versionado: qualquer campo novo é aditivo e silencioso.

Na análise de mercado de 2026-10-19: o CodexBar tem `codexbar serve` (rotas `/usage`, `/health`, painel web e snapshot protegido por token), o OpenUsage expõe `127.0.0.1:6736/v1/limits` e o Quotio tem API REST em loopback. O ecossistema do CodexBar (Waybar, KDE Plasma, GNOME, SketchyBar, tmux, Stream Deck, displays físicos) nasceu em cima dessa superfície estável.

## 2. Problema

- Integrações precisam de dados frescos sem disparar consultas a cada leitura.
- Sem contrato versionado, qualquer mudança no JSON pode quebrar scripts de terceiros.
- Um servidor mal desenhado vira vetor de vazamento: o OpenUsage documenta que páginas web também conseguem ler o endpoint local, o que precisa de decisão consciente.

## 3. Objetivos

- Definir um contrato de exportação versionado (`v1`), independente de transporte, sem segredos.
- Oferecer um servidor HTTP somente-leitura em loopback que serve o último estado conhecido, sem disparar consultas por requisição.
- Disponibilizar o mesmo servidor na TUI/CLI e no desktop, com a lógica no core.

## 4. Não-objetivos

- Escrita: nenhuma rota altera contas, credenciais ou configuração. Não há login nem edição por HTTP.
- Exposição na rede local ou na internet (fora do escopo; ver RF3.5).
- Substituir `--json`, que permanece igual e compatível.
- Painel web completo (pode virar PRD futuro).

## 5. Requisitos funcionais

### RF1 — Contrato de exportação v1 (prioridade alta; pré-requisito)

- **RF1.1** Criar `export::v1` em `cuotai-core` com `Export { schema: 1, generated_at, config_identity, accounts: Vec<AccountExport> }`. `AccountExport` traz: `index`, `key` (identidade estável, igual à do PRD 0003, RF1.3), `name`, `provider`, `class` (`AccountClass`), `report` (o `Report` atual, sem credenciais), `age_seconds`, `freshness`, `stale: bool` e, quando os PRDs correspondentes existirem, `binding` (0009), `pace` por métrica (0007), `access` (0005) e `incident` (0011).
- **RF1.2** O esquema é aditivo dentro da versão 1 e documentado em `docs/export-v1.md`, com JSON Schema gerado ou mantido junto do código. Remover ou mudar o significado de um campo exige `schema: 2`. Um teste compara a saída com um arquivo de referência para impedir quebra acidental.
- **RF1.3** Sem segredos: nenhuma credencial, nenhum nome de variável de ambiente de credencial, nenhuma URL autenticada e nenhuma nota que contenha corpo de resposta. E-mails de conta só aparecem se o usuário habilitar "incluir identidade" (padrão: omitidos).
- **RF1.4** Valores ausentes permanecem `null`; nunca viram zero.
- **Critério de aceite:** testes de contrato (arquivo de referência), de ausência de segredos (varredura por padrões de token em uma conta de fixtures) e de `null` em campos ausentes.

### RF2 — Servidor no core (prioridade alta)

- **RF2.1** `serve::LocalServer` em `cuotai-core`, atrás de feature `serve` (desligada por padrão no core e ligada nos binários que a usam), sobre `tokio` e uma biblioteca HTTP mínima escolhida na implementação (via Cargo, sem dependências de JavaScript). Escuta em `127.0.0.1` por padrão, porta configurável (padrão 6738 como sugestão, sujeita a conferência de conflitos), com escolha de porta alternativa em caso de ocupação.
- **RF2.2** Rotas somente `GET`: `/health` (`{"status":"ok","version":"...","schema":1}`), `/v1/snapshot` (o `Export` completo), `/v1/accounts/{key}` e `/v1/limits` (versão resumida: por conta, a vinculante e a idade). Qualquer outro método responde `405`; rotas desconhecidas, `404`.
- **RF2.3** O servidor lê do estado já mantido pelo `RefreshCycle`; **nunca** dispara consulta ao provedor por requisição. Se ainda não há dados, responde `503` com `Retry-After`. Um parâmetro opcional `?refresh=1` não existe nesta versão.
- **RF2.4** Limites defensivos: tamanho de cabeçalho e de URL, número de conexões simultâneas, tempo por requisição, e resposta sempre com `Content-Type: application/json` e `Cache-Control: no-store`.
- **Critério de aceite:** testes de integração com `reqwest` contra uma porta efêmera: rotas, métodos proibidos, `503` sem dados, ausência de consultas ao provedor durante 100 requisições, respeito ao limite de conexões.

### RF3 — Segurança do servidor (prioridade alta)

- **RF3.1** Ligado somente em loopback (`127.0.0.1` e `::1`). Pedir outro endereço de vínculo exige uma flag explícita e resulta em erro de configuração, não em exposição silenciosa.
- **RF3.2** Token opcional (`Authorization: Bearer`) gerado pelo Cuotai e guardado no cofre do SO, não no TOML. Sem token, o servidor só aceita requisições cuja origem seja loopback e que **não** tenham cabeçalho `Origin` de navegador.
- **RF3.3** Proteção contra leitura por páginas web: sem `Access-Control-Allow-Origin` por padrão (bloqueia leituras de outras origens) e verificação do cabeçalho `Host` contra `127.0.0.1`/`localhost` para mitigar *DNS rebinding*. Um modo "permitir navegadores" pode ser ligado explicitamente, com aviso do que isso significa.
- **RF3.4** Nenhuma rota devolve credenciais, e o log de acesso não grava cabeçalhos de autorização nem parâmetros.
- **RF3.5** Acesso pela rede local fica fora do escopo. Quem precisar deve fazê-lo por túnel próprio.
- **Critério de aceite:** testes de *DNS rebinding* (`Host` estranho recusado), `Origin` de navegador recusado, token inválido (`401`), vínculo não loopback recusado sem a flag e ausência de segredos nas respostas e nos logs.

### RF4 — TUI/CLI (prioridade alta)

- **RF4.1** `cuotai --serve [--port N] [--token-file ARQ]` roda em primeiro plano sem abrir a TUI, mantém o ciclo de refresh do core e serve os dados. Encerra de forma limpa em `SIGINT`/`SIGTERM`. Sem terminal interativo é permitido.
- **RF4.2** Códigos de saída: 0 encerramento normal; 1 erro local (porta, configuração, token); 2 se o ciclo ficou sem nenhuma conta saudável até o encerramento.
- **RF4.3** `--once`/`--json` permanecem idênticos aos atuais; um novo `--json-v1` emite o `Export` completo.
- **Critério de aceite:** smoke com `curl` contra `--serve`, encerramento limpo e códigos de saída.

### RF5 — Desktop (prioridade média)

- **RF5.1** Em **Configurações → Integração**: ligar e desligar o servidor (desligado por padrão), porta, token (gerar, copiar, revogar), "permitir navegadores", e o endereço efetivo. O servidor usa o mesmo ciclo de refresh do app; não cria um segundo.
- **RF5.2** Estado do servidor (ligado, porta, erro de bind) exibido de forma clara, incluindo conflito de porta com uma instância `cuotai --serve`.
- **Critério de aceite:** smoke no app empacotado: ligar, consultar por `curl`, revogar o token, desligar e confirmar que a porta fecha.

## 6. Requisitos não funcionais

- **Privacidade:** o servidor serve só o que o `--json` já expõe, acrescido de campos derivados, e nenhuma credencial. Fica claro na documentação que qualquer processo local do mesmo usuário pode ler os dados.
- **Estabilidade:** contrato versionado, testes de referência e política de depreciação documentada.
- **Custo:** nenhuma requisição ao servidor causa consulta a provedores.
- **Dependências:** a biblioteca HTTP entra via Cargo, atrás da feature `serve`, e fica registrada em `THIRD_PARTY_NOTICES` e na tela Sobre.

## 7. Ordem de execução e verificação

1. **RF1:** contrato e testes de referência, sem servidor.
2. **RF2 e RF3:** servidor com segurança e testes de integração.
3. **RF4:** `--serve` e `--json-v1`.
4. **RF5:** desktop.

Em cada etapa: `cargo fmt`, `clippy -D warnings`, testes do workspace e do desktop e smoke da interface alterada.

O requisito só está entregue quando um script lê `/v1/snapshot` repetidamente sem causar nenhuma consulta adicional ao provedor, uma página web hostil não consegue ler os dados, o esquema `v1` é protegido por teste de referência e nenhuma resposta ou log contém credenciais.
