# PRD 0005 — Nível de acesso ao consumo por provedor

| Campo | Valor |
|---|---|
| ID | 0005 |
| Status | Proposto |
| Responsável | `cuotai-core`, TUI e Desktop |
| Consumidores | `crates/cuotai-tui` (`wizard.rs`, `dashboard.rs`), `apps/desktop` (`account-dialog.js`, cartões) |
| Depende de | nenhum (é pré-requisito dos PRDs 0016 e 0017) |
| Data | 2026-10-19 |

## 1. Contexto

O catálogo tem 36 provedores (`Provider::ALL` em `crates/cuotai-core/src/model.rs`). Cada adaptador consulta uma interface diferente, e a confiabilidade delas varia muito: APIs administrativas documentadas (OpenAI, Anthropic, AWS Cost Explorer), endpoints próprios acessíveis com a chave da conta (Kimi, Kimchi, OpenCode Go, Ollama Cloud), interfaces internas deduzidas de clientes comunitários e do 9router (ChatGPT/Codex, Claude, Kiro, Antigravity, Zed) e provedores sem nenhuma consulta pública (Qwen, NVIDIA NIM, Poolside, Alibaba Model Studio, Cloudflare AI, Gemini AI com chave de API).

Essa informação existe hoje só em texto livre: `docs/providers.md`, notas dos adaptadores e `Provider::cost_advisory()` (que cobre apenas o custo por consulta do AWS Bedrock). O catálogo de modelos já resolveu um problema parecido: `models::support(account)` devolve, por conta, uma fonte localizada (`models.source.live`, `models.source.undocumented`, `models.source.builtin`) ou um motivo de indisponibilidade, e as interfaces só exibem.

A análise de mercado de 2026-10-19 mostrou que nenhum concorrente descreve de forma estruturada a origem e a confiabilidade de cada número. O Quotum explica o risco de ler quotas não publicadas, mas só em texto de página.

## 2. Problema

- Quem adiciona uma conta não sabe se a consulta é oficial, se depende de uma interface interna que pode quebrar, se exige uma chave administrativa em vez da chave de inferência, se exige plano Enterprise ou se cobra por consulta.
- Escolher a classe errada de credencial é a falha mais comum e produz erros genéricos (caso real: Ollama Cloud, "Resposta incompatível com a interface de consulta").
- Provedores sem consulta pública aparecem como contas normais e terminam em erro ou em cartão vazio, sem explicação.
- Ao incluir um provedor novo, nada obriga o autor a declarar esse contexto.

## 3. Objetivos

- Descrever, no core e por conta, **como** o consumo é obtido: nível de confiabilidade, classe de credencial, requisito de plano, escopo, custo por consulta e evidência.
- Exibir isso no seletor de provedor, no passo de credencial e nos cartões, sem regras por provedor nas interfaces.
- Obrigar, por compilação e por teste, que todo provedor novo declare o descritor.
- Gerar a matriz de `docs/providers.md` a partir da mesma fonte.

## 4. Não-objetivos

- Mudar o que cada adaptador consulta, ou bloquear a criação de contas em provedores sem consulta (o aviso é obrigatório; o bloqueio não faz parte deste PRD).
- Verificar disponibilidade em tempo real (isso é o PRD 0011) ou idade do dado (PRD 0010).
- Alterar `Report`, `Snapshot` ou a saída `--json` atual.
- Provar a documentação do fabricante: o nível `Oficial` exige URL de documentação citada, mas o PRD não garante que a interface seja estável.

## 5. Requisitos funcionais

### RF1 — Descritor de acesso no core (prioridade alta; pré-requisito)

- **RF1.1** Criar o módulo `access` em `cuotai-core` com a função pura `describe(account: &Account) -> UsageAccess`, sem rede. O resultado varia por provedor, tipo de autenticação (credencial ou OAuth) e `params.product`/`params.mode` quando mudam a interface usada (Cursor pessoal x Teams, Copilot `quota` x `billing`, Kimi `default` x `coding`, Grok `management` x `cli`, Gemini AI `default` x `cli`).
- **RF1.2** `UsageAccess` contém:
  - `level`: `Official` (interface documentada pelo fabricante para essa credencial), `Undocumented` (interface interna ou deduzida), `Derived` (o consumo é inferido de outra fonte, como métricas de Cloud Monitoring ou cabeçalhos de rate limit), `Unavailable` (não há consulta; motivo obrigatório). Os PRDs 0016 e 0017 acrescentam `UserSession` e `Extension`.
  - `credential`: `InferenceKey`, `AdminKey`, `SubscriptionSession`, `CloudIdentity`, `LocalCli`, `UserSession`.
  - `plan_requirement`: texto opcional localizado ("Enterprise", "Teams ou Enterprise").
  - `scope`: `Account`, `Organization`, `WholeCloud` (fatura de todos os produtos, como Alibaba Cloud) ou `Product`.
  - `metered`: reutiliza `Provider::is_metered()`; não duplica a lista.
  - `evidence`: URL de documentação oficial **ou** referência ao contrato do 9router/cliente comunitário, mais a data (`verified_on`) da última verificação real.
  - `summary`, `credential_hint` e `risk` como `text::Localized` com fallback em português.
- **RF1.3** A função usa `match` exaustivo sobre `Provider`, sem braço curinga, de modo que um provedor novo não compila sem classificação.
- **RF1.4** Classificação inicial, a confirmar contra a documentação de cada fabricante antes de marcar `Official`:
  - `Official`: OpenAI e Anthropic (chave administrativa), AWS Bedrock (Cost Explorer, `metered`), Azure OpenAI, Alibaba Cloud (`WholeCloud`), Cursor Teams, Windsurf (Enterprise), Mistral (chave administrativa), Perplexity Computer (Analytics), Kimi por chave (saldo), Grok `management`.
  - `Derived`: Gemini API e Vertex AI (Cloud Monitoring), Groq (cabeçalhos `x-ratelimit-*`).
  - `Undocumented`: ChatGPT/Codex, Claude, Antigravity, Gemini CLI, Grok CLI, Kiro, CodeBuddy, Zed, Cursor pessoal, Copilot `quota`, Alibaba Coding Plan e Token Plan, Xiaomi MIMO, Qoder, MiniMax, OpenCode Go, Ollama Cloud, Kimchi, Cline.
  - `Unavailable`: Qwen (chave), Alibaba Model Studio, NVIDIA NIM, Cloudflare AI, Gemini AI com chave de API, Poolside (o endpoint conhecido exige token de plataforma, não a chave de inferência).
- **Critério de aceite:** testes de tabela cobrem os 36 provedores e os modos que alteram a interface; `Unavailable` sempre traz motivo; `Official` sempre traz URL; todo `id` emitido existe no catálogo `en-US` (o teste `every_descriptor_id_is_translated_in_every_catalog` já cobre o padrão).

### RF2 — Textos e catálogos (prioridade alta)

- **RF2.1** Criar ids estáveis `access.level.*`, `access.credential.*`, `access.scope.*` e `access.summary.<slug>` (um por provedor/modo) em `locales/en-US.json` e `es.json`; o português fica no `fallback`.
- **RF2.2** Nenhum texto cita URL autenticada, token ou corpo de resposta. A evidência é uma URL pública de documentação ou o nome do arquivo de referência.
- **Critério de aceite:** teste de catálogo completo nos três idiomas; nenhuma string de acesso aparece em `ui/locales` do desktop (o core é dono).

### RF3 — Exibição no seletor e no passo de credencial (prioridade alta)

- **RF3.1** TUI (`wizard.rs`) e desktop (`account-dialog.js`) mostram, ao lado de cada provedor, uma etiqueta curta do nível (`Oficial`, `Não documentado`, `Derivado`, `Indisponível`), vinda de `describe`.
- **RF3.2** No passo de credencial, mostrar `credential_hint`: "Precisa de chave administrativa, não a de inferência", "Interface interna, pode mudar sem aviso", "Exige plano Enterprise". Reusa a área onde `credential_help.<slug>` já aparece, sem criar um segundo bloco para o mesmo assunto.
- **RF3.3** Provedores `metered` mantêm o aviso de custo atual (`cost_advisory`), agora derivado do mesmo descritor.
- **RF3.4** Provedores `Unavailable` continuam selecionáveis. Ao escolher um, a interface exibe o motivo antes de salvar e pede confirmação explícita ("Salvar mesmo assim").
- **Critério de aceite:** smoke na TUI e no desktop (três idiomas) mostrando um provedor de cada nível; a escolha de `Unavailable` exige confirmação; nenhuma ramificação por `Provider` nas interfaces.

### RF4 — Exibição no cartão e nos detalhes (prioridade média)

- **RF4.1** O cartão e os detalhes da conta exibem o nível e o resumo, de forma discreta, sem competir com a quota.
- **RF4.2** Uma conta `Unavailable` mostra o resumo e o motivo no lugar de "Erro" ou de um cartão vazio. Uma conta `Undocumented` com `Status::Error` por esquema incompatível mostra que a interface pode ter mudado e aponta para a documentação do provedor.
- **RF4.3** O JSON de exportação (PRD 0012) inclui `access` por conta; `--json` atual não muda.
- **Critério de aceite:** teste de apresentação com um relatório `Error` de esquema em provedor `Undocumented` e um `Unsupported` em provedor `Unavailable`.

### RF5 — Documentação gerada e teste de divergência (prioridade média)

- **RF5.1** `docs/providers.md` ganha as colunas Nível, Credencial, Plano, Escopo e Evidência, geradas por um teste que compara o arquivo com `access::describe` (mesmo padrão dos testes de divergência de `about.rs`). O teste falha com instrução para regenerar.
- **RF5.2** O guia de novo provedor em `AGENTS.md` e `docs/architecture.md` passa a listar o descritor de acesso como etapa obrigatória.
- **Critério de aceite:** alterar um nível sem atualizar o documento quebra o teste.

## 6. Requisitos não funcionais

- **Honestidade:** `Undocumented` nunca é apresentado como garantia; o texto diz "pode mudar sem aviso". `Official` não promete permissão na conta do usuário.
- **Sem rede e sem segredos:** `describe` é pura e não lê credenciais.
- **Compatibilidade:** TOML, `Report`, `Snapshot` e `--json` atuais não mudam.

## 7. Ordem de execução e verificação

1. **RF1:** módulo, classificação inicial e testes de tabela.
2. **RF2:** ids e catálogos.
3. **RF3 e RF4:** TUI e desktop, um de cada vez, com smoke.
4. **RF5:** matriz gerada e guia de novo provedor.

Em cada etapa: `cargo fmt`, `clippy -D warnings`, testes do workspace e do desktop e smoke da interface alterada.

O requisito só está entregue quando um provedor novo não compila sem declarar seu acesso, o wizard exibe nível e dica de credencial nos três idiomas, uma conta `Unavailable` explica o motivo em vez de mostrar erro genérico, e a matriz de `docs/providers.md` é verificada por teste.
