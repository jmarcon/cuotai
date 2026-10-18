# Catálogo de Provedores, Credenciais e Limitações

O catálogo do Cuotai contém **36 provedores**, combinando APIs administrativas de faturamento, Cloud Monitoring, assinaturas de assistentes de código e integrações compatíveis com o `9router`.

## Integrações principais e credenciais

| Identificador | Dados consultados | Configuração / restrições |
|---|---|---|
| `open_ai_api` | Custo por moeda; tokens e requisições de completions, mês UTC | `OPENAI_ADMIN_KEY`, **Admin API key**. `params.project_ids` e `params.models` opcionais, separados por vírgula |
| `anthropic_api` | Custo USD e tokens de mensagens incluindo cache, mês UTC | `ANTHROPIC_ADMIN_KEY`, chave de administração. `params.workspace_ids` e `params.models` opcionais. Priority Tier não incluído no relatório de custos |
| `chat_gpt` | Percentual das janelas **Codex** e saldo de créditos quando retornado | Login OAuth nativo ou `CHATGPT_ACCESS_TOKEN`; `params.account_id` opcional. Interface interna, não documentada. **Não consulta quota de mensagens do ChatGPT web** |
| `claude` | Janelas de utilização e renovação; uso extra quando retornado | Login OAuth nativo ou `CLAUDE_OAUTH_TOKEN`. Interface não documentada, contrato evidenciado por clientes comunitários. Unidades de uso extra não documentadas são preservadas como unidades do provedor |
| `gemini_api` | Limites e consumo recente via Cloud Monitoring | Login OAuth integrado ou `GOOGLE_CLOUD_ACCESS_TOKEN`, `params.project_id`; `monitoring.timeSeries.list`. Serviço `generativelanguage.googleapis.com` |
| `vertex_ai` | Limites e consumo recente via Cloud Monitoring | Mesmos requisitos; serviço `aiplatform.googleapis.com` |
| `aws_bedrock` | UnblendedCost do mês, filtro de serviço Amazon Bedrock | `params.profile` obrigatório, AWS CLI autenticada, `ce:GetCostAndUsage`. Não mede todas as quotas de inferência |
| `azure_open_ai` | Custo mensal do recurso explicitamente escolhido | Login OAuth integrado ou `AZURE_ACCESS_TOKEN`, `params.subscription_id`, `params.resource_id` ARM completo de `Microsoft.CognitiveServices/accounts`; Cost Management Reader. Recurso Foundry compartilhado inclui outros modelos |
| `cursor` | Gasto sob demanda/total da equipe ou resumo da assinatura pessoal | `CURSOR_ADMIN_API_KEY` para Admin API **Teams/Enterprise**, ou importação explícita da sessão SQLite local para conta pessoal |
| `copilot` | Quota pessoal de interações ou uso bruto/custo líquido de billing | Login OAuth nativo ou `GITHUB_TOKEN`. `params.mode=user` (padrão) ou `org` + `params.org`. `params.billing=premium_requests` (padrão) ou `ai_credits`. Conta pessoal não inclui licenças faturadas pela organização |
| `windsurf` | Uso de créditos, alocação e créditos adicionais no ciclo | `WINDSURF_SERVICE_KEY`. **Enterprise**, permissões Billing Read e Teams Read-Only. Alocação por assento não é saldo compartilhado; restante não inferido |

Chave de geração não substitui chave administrativa ou token de assinatura. Nenhum cookie, Keychain ou arquivo de credenciais de outro app é extraído automaticamente; só entradas do próprio Cuotai são lidas no cofre.

## Aplicabilidade das opções por autenticação

Os identificadores abaixo são valores de máquina. `credential` significa a credencial manual/variável do provedor; `oauth` significa login nativo. Uma opção sem restrição comprovada pelo adaptador permanece disponível em todos os tipos oferecidos pelo provedor. O core é a fonte desta tabela: TUI e Desktop recebem a lista filtrada e o índice completo da opção, sem regras específicas por provedor.

| Provedor | Campo | Opção | Tipos de autenticação |
|---|---|---|---|
| Copilot | `query_mode` | `billing` | `credential`, `oauth` |
| Copilot | `query_mode` | `quota` | `credential`, `oauth` |
| Copilot | `mode` | `user` | `credential`, `oauth` |
| Copilot | `mode` | `org` | `credential`, `oauth` |
| Copilot | `billing` | `premium_requests` | `credential`, `oauth` |
| Copilot | `billing` | `ai_credits` | `credential`, `oauth` |
| Kimi | `product` | `default` | `credential`, `oauth` |
| Kimi | `product` | `coding` | `credential`, `oauth` |
| Grok | `product` | `management` | `credential`, `oauth` |
| Grok | `product` | `cli` | `credential`, `oauth` |
| Gemini AI | `product` | `default` | `credential`, `oauth` |
| Gemini AI | `product` | `cli` | `credential`, `oauth` |
| Qoder | `region` | `intl` | `credential`, `oauth` |
| Qoder | `region` | `cn` | `credential`, `oauth` |
| Qoder | `credential_type` | `auto` | `credential`, `oauth` |
| Qoder | `credential_type` | `pat` | `credential`, `oauth` |
| Qoder | `credential_type` | `bearer` | `credential`, `oauth` |
| Kiro | `auth_method` | `builder-id` | `oauth` |
| Kiro | `auth_method` | `idc` | `oauth` |
| Kiro | `auth_method` | `google` | `credential` |
| Kiro | `auth_method` | `github` | `credential` |
| Kiro | `auth_method` | `import` | `credential` |
| Kiro | `auth_method` | `api_key` | `credential` |
| Kiro | `auth_method` | `external_idp` | `credential` |
| CodeBuddy | `region` | `cn` | `credential`, `oauth` |
| CodeBuddy | `region` | `intl` | `credential`, `oauth` |
| Xiaomi MiMo | `region` | `sgp` | `credential`, `oauth` |
| Xiaomi MiMo | `region` | `cn` | `credential`, `oauth` |
| Xiaomi MiMo | `region` | `ams` | `credential`, `oauth` |
| Xiaomi MiMo | `region` | `ru` | `credential`, `oauth` |
| Xiaomi MiMo | `region` | `in` | `credential`, `oauth` |
| Alibaba Model Studio | `region` | `intl` | `credential` |
| Alibaba Model Studio | `region` | `cn` | `credential` |
| Alibaba Coding Plan | `region` | `intl` | `credential` |
| Alibaba Coding Plan | `region` | `cn` | `credential` |
| Alibaba Cloud | `region` | `intl` | `credential` |
| Alibaba Cloud | `region` | `cn` | `credential` |
| Alibaba Token Plan | `login.console_site` | `international` | `bailian_console` |

No Kiro, `auth_method` é um único descritor compartilhado. O login nativo recebe exatamente `builder-id` e `idc`; a credencial de ambiente recebe `google` (padrão explícito para contas novas), `github`, `import`, `api_key` e `external_idp`. Ao trocar de tipo, um valor incompatível é substituído pelo primeiro valor aplicável e a interface exibe o aviso localizado retornado pelo core.

Arquivos antigos não são reescritos ao carregar: a ausência de `auth_method` e combinações antigas como `credential` + `builder-id` continuam sendo lidas e consultadas com a semântica histórica do adaptador. Ao editar e salvar, porém, a combinação precisa obedecer à tabela.

## Catálogo de API keys, PATs e sessões

Os nomes abaixo são os defaults configurados no wizard. `credential_env` pode apontar para outra variável, sem copiar a chave para o TOML. Estar no catálogo **não significa** que o provedor disponibiliza quota por API key de inferência.

| Provedor / identificador | Variável / credencial padrão | Consulta implementada / limitação |
|---|---|---|
| Minimax.io / `mini_max` | `MINIMAX_IO_API_KEY` | Token/Coding Plan: janelas por contagem ou percentual, incluindo M-series. Fallback entre endpoints usados pelo 9router; chave `sk-api-` mantém consulta de saldo |
| Kimi / `kimi` | `KIMI_API_KEY` | Saldo Moonshot internacional por padrão. `params.product=coding` (ou login nativo) consulta quota semanal e rate limit do Kimi Coding |
| Qwen / `qwen` | `QWEN_API_KEY` | DashScope por chave continua sem quota compatível. OAuth Qwen Code disponível com aviso de descontinuação; nenhum endpoint de quota confirmado para essa sessão |
| Alibaba Model Studio / `alibaba_model_studio` | `DASHSCOPE_API_KEY` | Cadastro separado, região `intl` ou `cn`. API key de inferência não concede consulta pública de saldo/quota |
| Alibaba Coding Plan / `alibaba_coding_plan` | `BAILIAN_CODING_PLAN_API_KEY` ou variável configurada | RPC regional `queryCodingPlanInstanceInfoV2`: uso/limite de 5h, semana e mês. Se o gateway exigir login, `params.session_env` referencia cookie explícito; não extrai sessão do navegador nem tenta outra região |
| Alibaba Token Plan (Qwen Cloud) / `alibaba_token_plan` | `params.cli_profile = "cuotai-qwen"` (login `bl` internacional) **ou** `params.session_env = "QWEN_CLOUD_COOKIE"` (Cookie) | Quota 5h/semana via `bl usage token-plan --output json` no perfil indicado; `session_env` usa HTTP direto do console Qwen Cloud e tem precedência quando presente. A chave `QWEN_API_KEY` só autentica inferência |
| Alibaba Cloud / `alibaba_cloud` | `ALIBABA_CLOUD_ACCESS_KEY_ID` | Saldo BSS via AccessKey Secret referenciada em `params.secret_env`; STS opcional em `security_token_env`. `billing_cycle=YYYY-MM` consulta fatura de **todos os produtos**, não apenas AI |
| Antigravity / `antigravity` | OAuth Google ou `ANTIGRAVITY_ACCESS_TOKEN` | Projeto/plano via `loadCodeAssist`; quotas por modelo via `fetchAvailableModels` nos planos pagos; sessão/semana via `retrieveUserQuotaSummary`. Modelo sem fração de quota mantém valores desconhecidos e não força a sessão a 0%. `project_id` opcional |
| Groq / `groq` | `GROQ_API_KEY` | GET `/openai/v1/models`, lendo `x-ratelimit-*` como o 9router. Sem geração. Se a resposta não trouxer cabeçalhos, indica conectado sem quota reportada, nunca zero inventado |
| Perplexity / `perplexity` | `PERPLEXITY_API_KEY` | Sonar: indisponível. **Computer Enterprise Analytics** é opt-in separado (`params.product=computer`), com chave Analytics; soma créditos dos dias UTC completos |
| Mistral / `mistral` | `MISTRAL_API_KEY` | `/v1/admin/spend-limit` com **Admin API Key**, cabeçalho `x-api-key`. Valores ausentes continuam desconhecidos; estado de limite mensal aparece nas observações |
| Cloudflare AI / `cloudflare_ai` | `CLOUDFLARE_AI_API_KEY` | Sem consulta de Neurons/quota Workers AI confirmada nesta integração. Invocações genéricas de Workers e AI Gateway não são substitutos |
| Ollama Cloud / `ollama_cloud` | `OLLAMA_CLOUD_API_KEY` | GET `/api/usage`: janelas mensal/semanal/sessão conforme plano. POST `/api/me` complementa plano/cadastro; não exige cookie. Renovação gratuita estimada pelo aniversário de cadastro |
| Xiaomi MiMo / `xiaomi_mimo` | Sessão explícita por variável | `platform_session_env`: saldo e Token Plan mensal do console; `session_env`/`region`: quota semanal do Desktop. Código nativo gera chave de inferência e não substitui essas sessões |
| OpenCode Go / `open_code_go` | `OPENCODE_GO_API_KEY` | GET `https://opencode.ai/zen/go/v1/usage`: percentual rolling, semanal e mensal, com renovação |
| Qoder / `qoder` | `QODER_API_KEY` | PAT `pt-` trocado por job token e GET `/api/v2/quota/usage`; `params.region=intl\|cn`, `params.credential_type=auto\|pat\|bearer` |
| Gemini AI / `gemini_ai` | `GEMINI_API_KEY` | API key AI Studio continua sem quota nesta interface. `params.product=cli` (ou login nativo) usa sessão Gemini CLI e `retrieveUserQuota`; não confunde produtos |
| Grok (xAI) / `grok` | `X_GROK_API_KEY` | Management API por padrão. `params.product=cli` (ou login nativo) usa sessão Grok CLI para créditos sob demanda, pré-pagos e pool SuperGrok via REST/gRPC |
| NVIDIA NIM / `nvidia_nim` | `NVIDIA_NIM_API_KEY` | Sem consulta pública de saldo por chave confirmada; não usa inferência para extrair rate-limit headers |
| Poolside / `poolside` | `POOLSIDE_API_KEY` | Sem consulta pública de saldo/quota da conta confirmada; métricas de infraestrutura não são saldo |
| Kimchi / `kimchi` | `KIMCHI_API_KEY` | GET `/v1/credits` e `/v1/budget`: saldo USD, orçamento por escopo/provedor e renovação. Falha no orçamento preserva créditos válidos com observação |

Para Mistral, Perplexity Computer e Grok, a variável da etapa Credencial deve conter a **classe correta** de chave. Se guardar a chave administrativa em outra variável, configure `params.admin_key_env`, `params.analytics_key_env` ou `params.management_key_env`, respectivamente. A referência alternativa tem precedência; apenas a chave efetivamente usada é lida.

## Outros assistentes

- **Cline (`cline`)**: consulta saldo pessoal com `CLINE_API_KEY` (API key ou account auth token Cline) ou login nativo; `/users/me` descobre o ID antes da consulta de saldo. `params.organization_id` seleciona saldo da organização. Não agrega histórico incompleto nem quotas dos provedores BYOK.
- **Kiro (`kiro`)**: login nativo recomendado (**AWS Builder ID** ou **IAM Identity Center**) ou `KIRO_ACCESS_TOKEN`, consulta interna `getUsageLimits` em CodeWhisperer/Q. No login, `params.auth_method=builder-id|idc`; por variável, `google` (padrão), `github`, `import`, `api_key` ou `external_idp`. `params.profile_arn` permite indicar seu perfil. Quota de Kiro é distinta do faturamento Bedrock.
  - Com `profile_arn` da própria conta (descoberto pelo login ou informado no campo opcional), consulta créditos com POST `{profileArn}` no host regional conhecido (`us-east-1` ou `eu-central-1`) antes das três tentativas legadas do 9router. Mostra plano, teste gratuito, excedente habilitado e renovação; bônus não separáveis são sinalizados.
  - **Várias contas:** adicione uma entrada Kiro por identidade, com nomes distintos. Faça o login nativo separadamente em cada entrada; cada uma recebe seu próprio `credential_id` e consulta sua própria quota.
- **CodeBuddy (`code_buddy`)**: login nativo ou `CODEBUDDY_ACCESS_TOKEN`, POST interno `/v2/billing/meter/get-user-resource`. `params.region=cn|intl`; pacotes de renovação e bônus aparecem separados.
- **Zed (`zed`)**: login nativo ou `ZED_ACCESS_TOKEN` + `params.user_id` obrigatório; GET `cloud.zed.dev/client/users/me`, `Authorization: <user_id> <token>`. `params.system_id` opcional. Não extrai tokens do editor automaticamente.
- **Cursor pessoal**: a montagem do cookie preserva maiúsculas/minúsculas do identificador do JWT. A importação recusa tokens com menos de 60 segundos de validade.
- **Poolside**: permanece bloqueado (o cliente oficial revela `GET /admin/account/{account_id}/usage` para consumo de rate limit autenticado por token administrativo da plataforma, não pela chave de inferência).

## Compatibilidade com o 9router

Referência de código: [9router, revisão `f01fb909e37189008080632ddaf404f096345cde`](https://github.com/decolua/9router/tree/f01fb909e37189008080632ddaf404f096345cde). Os módulos Rust `providers/router_*.rs` reproduzem consultas e `oauth/reference*.rs` implementam os fluxos de login/renovação a partir de `src/lib/oauth/providers/` e helpers nativos. A licença MIT e atribuição estão em [`THIRD_PARTY_NOTICES`](../THIRD_PARTY_NOTICES).

Essas consultas incluem **interfaces internas/não oficiais**. Podem mudar, expirar ou exigir sessão específica. Não é necessário executar o 9router junto com o Cuotai. O Cuotai não importa seus bancos de credenciais, cookies do navegador ou arquivos de sessão de outros apps, não gera respostas de IA para medir limites e não chama operações que gastam créditos para renovar quota.

No modo **variável de ambiente**, selecione o produto em **Parâmetros** conforme a tabela:

| Caminho | Configuração |
|---|---|
| Kimi Coding | `product = "coding"`; `KIMI_API_KEY`. OAuth alternativo: `token_env` (nome de variável); a API key tem precedência quando ambas existem |
| Copilot pessoal | `query_mode = "quota"`; `GITHUB_TOKEN` da conta, não copilotToken. Campos de billing são ocultados nesse modo |
| Gemini CLI | `product = "cli"`; `session_env` opcional (default `GEMINI_CLI_ACCESS_TOKEN`), `project_id` opcional (resolvido por `loadCodeAssist`). O token Cloud Monitoring de escopo restrito não substitui essa sessão |
| Grok CLI | `product = "cli"`; `session_env` opcional (default `GROK_CLI_ACCESS_TOKEN`), `email`/`user_id` opcionais. Não utiliza a Management Key nesse modo |
| Xiaomi MiMo | `session_env = "XIAOMI_MIMO_COOKIE"`, cookie de conta exportado pelo usuário; `region = "sgp"` (ou cn/ams/ru/in) |
| Qoder | PAT/job token em `QODER_API_KEY`; `region = "intl"` (ou cn), `credential_type = "auto"` (ou pat/bearer). Troca PAT→job token sem fazer inferência |
| Kiro/CodeBuddy/Zed | Preferencialmente login nativo no wizard/diálogo; tokens e metadados são preenchidos automaticamente. O método manual por variável permanece disponível; no Kiro ele usa `google` por padrão e não oferece `builder-id`/`idc` |

Diferenças deliberadas em relação ao 9router: campos ausentes não viram zero ou ilimitado; Gemini CLI mostra `remainingFraction` em **%**, não como 1000 requisições reais; Ollama identifica a renovação calculada a partir do cadastro como estimativa; respostas de erro não expõem corpo bruto; datas sem timezone de pacotes CodeBuddy são interpretadas como UTC.

## Semântica e custos das consultas

- `used`, `limit` e `remaining` são opcionais. Ausência **não** significa zero. Não se calcula saldo de API a partir de gasto, nem mistura moedas.
- `status` descreve a **consulta**, não a saúde global do provedor. `ready` significa que a consulta terminou, não que existe quota disponível. HTTP 401/403, 429, rede, timeout e resposta incompatível são distinguidos.
- Custos mensais são acumulados; `resets_at` nesses relatórios indica início do próximo período de acompanhamento, não concessão garantida de créditos.
- **OpenAI e Anthropic:** tokens limitados a completions/mensagens; custos têm escopo mais amplo. Filtrar `models` altera tokens, **não** o escopo dos custos.
- **Google Cloud Monitoring:** limites amostrados diariamente; consumo é soma dos últimos 10 minutos com atraso. Séries de limite e uso são separadas, não percentuais comparáveis. Também pode cobrar leituras (duas consultas iniciais por atualização, mais páginas).
- **AWS Bedrock:** custos com atraso, possivelmente estimados. **Cost Explorer cobra por requisição** (consulte preço vigente; referência de US$ 0,01). Atualização a cada 300 s, rodando 24 h, pode gerar cerca de US$ 2,88/dia por conta. O Cuotai exige intervalo mínimo de 300 s para AWS Bedrock e avisa sobre o custo na edição da conta (TUI e Desktop). Defina `accounts.refresh_seconds = 3600` ou mais para contas AWS.
- **Windsurf:** analytics em buckets horários; alocação e consumo podem ter escopos distintos. O app não inventa saldo restante a partir dessa comparação.

## Catálogo de modelos por conta

A lista só é consultada sob demanda (`load_models`/`sync_models`), nunca pelo refresh de quota. Habilita-se apenas quando o endpoint aceita **a mesma credencial** que a conta já guarda, em leitura e sem custo de inferência. Quase todos são contratos oficiais; as duas exceções são as assinaturas **Claude** e **Codex**, listadas por interfaces não documentadas para o token OAuth, verificadas contra contas reais e sujeitas a mudar sem aviso (como as consultas de quota delas); caso contrário a conta mostra `Unsupported` com o motivo e nenhuma requisição. Paginação segue tokens até o fim (limite de 50 páginas, token repetido rejeitado); página inválida/incompleta, HTTP 401/403/429 ou cancelamento falham sem sobrescrever o catálogo salvo. IDs são deduplicados e ordenados sem reescrita.

- **AWS Bedrock:** `aws bedrock list-foundation-models --profile <params.profile>`, com `--region <params.region>` quando definido; sem `params.region` vale a região padrão do perfil na AWS CLI. Exige `bedrock:ListFoundationModels`; não é Cost Explorer e nunca roda no refresh.
- **Gemini AI:** só com a chave AI Studio (`product` padrão, sem login nativo); o token Gemini CLI não é usado. O ID é o `name` retornado (`models/...`).
- **Claude (assinatura):** `GET https://api.anthropic.com/v1/models?limit=1000` com o token OAuth e os cabeçalhos `anthropic-beta: oauth-2025-04-20` e `anthropic-version`, paginado por `after_id` enquanto `has_more`. O contrato público é de chave de API; o uso com o token da assinatura não é documentado. `has_more` sem `last_id` rejeita a listagem.
- **ChatGPT / Codex (assinatura):** `GET https://chatgpt.com/backend-api/codex/models?client_version=<versão do Codex CLI>` com o token da sessão (e `chatgpt-account-id` se `params.account_id`), a lista lida pelo Codex CLI. Entradas com `visibility` diferente de `list` não aparecem no seletor do Codex e ficam de fora. Interface interna e não documentada.
- **Antigravity:** mesma sessão e requisição `fetchAvailableModels` da quota; plano gratuito ou resposta sem `models` é `Unsupported`.
- **Alibaba Model Studio:** só `region = "intl"` (Singapura); as demais regiões exigem host com `WorkspaceId`, que a conta não guarda.

Status: `supported`, `unsupported-incompatible-credential` (existe endpoint, mas não para a credencial armazenada) ou `unsupported-no-verified-endpoint`.

| Identificador | Status | Contrato / motivo | Fonte |
|---|---|---|---|
| `open_ai_api` | unsupported-incompatible-credential | Lista exige API key de projeto; conta guarda Admin key | [OpenAI](https://developers.openai.com/api/reference/resources/models/methods/list) |
| `anthropic_api` | unsupported-incompatible-credential | Lista exige API key; conta guarda Admin key | [Anthropic](https://platform.claude.com/docs/en/api/models/list) |
| `chat_gpt` | supported (interface interna) | GET `chatgpt.com/backend-api/codex/models` com o token da sessão; não documentada | [Codex CLI](https://github.com/openai/codex/blob/main/codex-rs/codex-api/src/endpoint/models.rs) |
| `claude` | supported (interface não documentada para o token) | GET `api.anthropic.com/v1/models` com o token OAuth da assinatura e `anthropic-beta: oauth-2025-04-20` | [Anthropic](https://platform.claude.com/docs/en/api/models/list) |
| `gemini_api` | unsupported-incompatible-credential | Token de escopo Cloud Monitoring, não Generative Language | [Gemini](https://ai.google.dev/api/models) |
| `vertex_ai` | unsupported-incompatible-credential | Token de escopo Cloud Monitoring, não Vertex AI | [Vertex AI](https://docs.cloud.google.com/gemini-enterprise-agent-platform/reference/rest/v1beta1/publishers.models/list) |
| `aws_bedrock` | supported | `ListFoundationModels` via AWS CLI no perfil/região | [AWS CLI](https://docs.aws.amazon.com/cli/latest/reference/bedrock/list-foundation-models.html), [API](https://docs.aws.amazon.com/bedrock/latest/APIReference/API_ListFoundationModels.html) |
| `azure_open_ai` | unsupported-incompatible-credential | Papel Cost Management Reader não comprova leitura do recurso | [Azure](https://learn.microsoft.com/en-us/rest/api/aiservices/accountmanagement/accounts/list-models) |
| `cursor` | unsupported-no-verified-endpoint | Admin API/sessão sem lista de modelos | [Cursor](https://cursor.com/docs/account/teams/admin-api) |
| `copilot` | unsupported-no-verified-endpoint | Catálogo GitHub Models é outro produto e escopo | [GitHub](https://docs.github.com/en/rest/models/catalog) |
| `windsurf` | unsupported-incompatible-credential | Service key Enterprise de analytics | — |
| `mini_max` | unsupported-incompatible-credential | Chave pode ser Subscription Key do plano, separada da API key | [MiniMax](https://platform.minimax.io/docs/api-reference/models/openai/list-models), [Subscription Key](https://platform.minimax.io/docs/token-plan/faq.md) |
| `kimi` | supported (padrão por chave) | GET `api.moonshot.ai/v1/models`; `product = "coding"` ou login nativo: incompatível | [Kimi](https://platform.kimi.ai/docs/api/list-models) |
| `qwen` | unsupported-incompatible-credential | Conta sem região; a lista sem `WorkspaceId` é só de Singapura: use `alibaba_model_studio` com `region = "intl"`. OAuth Qwen Code não lista | [Model Studio](https://www.alibabacloud.com/help/en/model-studio/list-models) |
| `groq` | supported | GET `/openai/v1/models` com a chave | [Groq](https://console.groq.com/docs/api-reference) |
| `perplexity` | unsupported-no-verified-endpoint | `/v1/models` é público (sem credencial), não da conta | [Perplexity](https://docs.perplexity.ai/api-reference/models-get) |
| `mistral` | unsupported-incompatible-credential | Lista exige chave de inferência; conta guarda Admin key | [Mistral](https://docs.mistral.ai/api/endpoint/models) |
| `cloudflare_ai` | unsupported-incompatible-credential | Lista exige `account_id`, que a conta não guarda | [Cloudflare](https://developers.cloudflare.com/api/resources/ai/subresources/models/methods/list/) |
| `ollama_cloud` | unsupported-no-verified-endpoint | `/api/tags` da nuvem é público, não da conta | [Ollama](https://docs.ollama.com/cloud) |
| `xiaomi_mimo` | unsupported-no-verified-endpoint | Sessões do console sem lista oficial | — |
| `open_code_go` | unsupported-no-verified-endpoint | Sem lista oficial para a chave | [OpenCode Go](https://opencode.ai/go/) |
| `qoder` | unsupported-no-verified-endpoint | PAT/job token sem lista oficial | — |
| `gemini_ai` | supported (chave AI Studio) | `models.list` paginado, `x-goog-api-key`; `product = "cli"`/login nativo: incompatível | [Gemini](https://ai.google.dev/api/models) |
| `grok` | unsupported-incompatible-credential | Lista exige API key de inferência; conta guarda Management key/sessão CLI | [xAI](https://docs.x.ai/developers/rest-api-reference/inference/models) |
| `nvidia_nim` | unsupported-no-verified-endpoint | `/v1/models` documentado só para NIM auto-hospedado | [NIM](https://docs.nvidia.com/nim/large-language-models/latest/api-reference.html) |
| `poolside` | supported | GET `inference.poolside.ai/v1/models` com a chave | [Poolside](https://docs.poolside.ai/api/overview.md) |
| `kiro` | unsupported-no-verified-endpoint | Token interno sem lista oficial | — |
| `cline` | unsupported-no-verified-endpoint | API documenta só chat completions | [Cline](https://docs.cline.bot/api/models.md) |
| `code_buddy` | unsupported-no-verified-endpoint | Sessão interna sem lista oficial | — |
| `zed` | unsupported-no-verified-endpoint | Token de cliente sem lista oficial | — |
| `alibaba_model_studio` | supported (`region = "intl"`) | GET `dashscope-intl.aliyuncs.com/api/v1/models`, `page_no` até `output.total`; `cn`: incompatível | [Model Studio](https://www.alibabacloud.com/help/en/model-studio/list-models) |
| `alibaba_coding_plan` | unsupported-incompatible-credential | Chave de assinatura `sk-sp-`, não API key padrão | — |
| `alibaba_token_plan` | unsupported-no-verified-endpoint | Perfil `bl`/cookie sem lista oficial | — |
| `alibaba_cloud` | unsupported-incompatible-credential | AccessKey de faturamento, não API key do Model Studio | — |
| `antigravity` | supported (planos pagos) | `fetchAvailableModels` da sessão (interface interna) | [9router](https://github.com/decolua/9router/blob/f01fb909e37189008080632ddaf404f096345cde/open-sse/services/usage.js) |
| `kimchi` | unsupported-no-verified-endpoint | Sem lista oficial para a chave | — |

## Referências

- [OpenAI: custos e uso da organização](https://developers.openai.com/api/reference/resources/admin/subresources/organization)
- [Anthropic: Usage and Cost API](https://platform.claude.com/docs/en/manage-claude/usage-cost-api)
- [Codex: cliente de consulta de limites](https://github.com/openai/codex/blob/main/codex-rs/backend-client/src/client/rate_limit_resets.rs)
- [Claude: evidência comunitária do contrato](https://github.com/loocor/codmate/blob/main/services/ClaudeUsageAPIClient.swift)
- [Google: métricas de quota Service Runtime](https://cloud.google.com/monitoring/api/metrics_gcp_p_z)
- [AWS: get-cost-and-usage](https://docs.aws.amazon.com/cli/latest/reference/ce/get-cost-and-usage.html)
- [Azure: Cost Management Query](https://learn.microsoft.com/en-us/rest/api/cost-management/query/usage?view=rest-cost-management-2025-03-01) e [atribuição de custos por recurso](https://learn.microsoft.com/en-us/azure/foundry/concepts/manage-costs)
- [Cursor: Admin API](https://cursor.com/docs/account/teams/admin-api), [contrato pessoal no CodexBar](https://github.com/steipete/CodexBar/blob/main/Sources/CodexBarCore/Providers/Cursor/CursorStatusProbe.swift) e [identidade da sessão importada](https://github.com/steipete/CodexBar/blob/main/Sources/CodexBarCore/Providers/Cursor/CursorAppAuth.swift)
- [GitHub: billing usage](https://docs.github.com/en/rest/billing/usage) e [autenticação para relatórios de consumo](https://docs.github.com/en/billing/tutorials/automate-usage-reporting)
- [Windsurf: créditos da equipe](https://docs.windsurf.com/windsurf/accounts/api-reference/get-team-credit-balance) e [analytics e unidades](https://docs.windsurf.com/windsurf/accounts/api-reference/user-page-analytics)
- [Google OAuth para apps Desktop](https://developers.google.com/identity/protocols/oauth2/native-app)
- [Microsoft OAuth device code](https://learn.microsoft.com/en-us/entra/identity-platform/v2-oauth2-device-code)
- [AWS CLI SSO login](https://docs.aws.amazon.com/cli/latest/reference/sso/login.html)
- [Anthropic: restrições de autenticação](https://code.claude.com/docs/en/legal-and-compliance#authentication-and-credential-use)
- [MiniMax Token Plan](https://platform.minimax.io/docs/token-plan/faq.md) e [interpretação de quota no CLI oficial](https://github.com/MiniMax-AI/cli/blob/main/src/utils/quota.ts)
- [Kimi: saldo da conta internacional](https://platform.kimi.ai/docs/api/balance.md)
- [Mistral: autenticação Admin API](https://docs.mistral.ai/admin/admin-api/authentication.md) e [uso e limites](https://docs.mistral.ai/admin/admin-api/usage-metrics.md)
- [Perplexity Computer Analytics](https://docs.perplexity.ai/docs/admin/computer-analytics-api.md)
- [xAI Management Billing](https://docs.x.ai/developers/rest-api-reference/management/billing.md)
- [Cline: API de conta e saldo](https://docs.cline.bot/enterprise-solutions/api-reference.md)
- [Groq: limites de gasto](https://console.groq.com/docs/spend-limits)
- [Cloudflare Workers AI: preços e acompanhamento](https://developers.cloudflare.com/workers-ai/platform/pricing/)
- [Ollama: autenticação](https://docs.ollama.com/api/authentication)
- [Gemini: limites](https://ai.google.dev/gemini-api/docs/rate-limits)
- [Qwen via DashScope](https://www.alibabacloud.com/help/en/model-studio/qwen-api-via-dashscope) e [Qwen Code device OAuth](https://github.com/QwenLM/qwen-code/blob/81582ca19d0772afdd06ace9cad4845c207001f8/packages/core/src/qwen/qwenOAuth2.ts)
- [Alibaba Token Plan CLI oficial](https://github.com/modelstudioai/cli/blob/main/packages/commands/src/commands/usage/token-plan.ts), [Token Plan no CodexBar](https://github.com/steipete/CodexBar/pull/2361) e [Coding Plan RPC no CodexBar](https://github.com/steipete/CodexBar/blob/main/Sources/CodexBarCore/Providers/Alibaba/AlibabaCodingPlanUsageFetcher.swift)
- [Xiaomi MiMo: documentação](https://mimo.mi.com/llms-full.txt) e [saldo/Token Plan no CodexBar](https://github.com/steipete/CodexBar/blob/main/Sources/CodexBarCore/Providers/MiMo/MiMoUsageFetcher.swift)
- [OpenCode Go](https://opencode.ai/go/) e [Qoder: uso](https://docs.qoder.com/cli/usage.md)
- [NVIDIA NIM](https://docs.api.nvidia.com/nim/reference/llm-apis) e [Poolside API](https://docs.poolside.ai/api/overview.md)
- [Kiro: faturamento](https://kiro.dev/docs/billing.md), [contrato getUsageLimits no 9router](https://github.com/decolua/9router/blob/f01fb909e37189008080632ddaf404f096345cde/open-sse/services/usage/kiro.js) e [créditos no CodexBar](https://github.com/steipete/CodexBar/blob/main/Sources/CodexBarCore/Providers/Kiro/KiroUsageLimitsAPI.swift)
- [CodeBuddy: uso](https://www.codebuddy.cn/docs/ide/Account/usage.md) e [Zed: planos](https://zed.dev/docs/account/plans-and-pricing.md)
- [Kimchi: contrato oficial de créditos e orçamentos](https://github.com/getkimchi/kimchi/blob/36cc0406339a50f5ced984266d0306fb8a150408/src/extensions/billing/status.ts)
- [9router: despacho de consultas](https://github.com/decolua/9router/blob/f01fb909e37189008080632ddaf404f096345cde/open-sse/services/usage.js) e [fluxos de autenticação](https://github.com/decolua/9router/tree/f01fb909e37189008080632ddaf404f096345cde/src/lib/oauth/providers)
