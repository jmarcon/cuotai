# Cuotai

Monitor de consumo e quota de AI em Rust, com backend reutilizável e TUI Ratatui.

## Executar

Requer Rust/Cargo. AWS Bedrock também requer AWS CLI v2 no PATH.

```sh
cargo run -p cuotai-tui -- --list-providers
cargo run -p cuotai-tui
```

Na primeira execução interativa sem arquivo de configuração (ou com lista de contas vazia), a TUI abre o **wizard de contas**. Não é necessário editar TOML. No painel, `a` adiciona uma conta e `e` edita a selecionada. O arquivo guarda **referências** às credenciais, nunca seus valores.

Para configuração manual, use `cuotai.example.toml` como referência. `--once`/`--json` sem arquivo continuam usando o catálogo padrão; não abrem o wizard. Consultas HTTP usam as variáveis padrão quando presentes; AWS só consulta após configurar explicitamente `params.profile`.

```sh
cargo run -p cuotai-tui -- --config cuotai.toml --once
cargo run -p cuotai-tui -- --config cuotai.toml --json
cargo install --path crates/cuotai-tui --locked
```

Precedência: `--config`, `./cuotai.toml`, diretório de configuração do sistema (`~/Library/Application Support/cuotai/config.toml` no macOS). Sem arquivo encontrado, o wizard propõe criar `./cuotai.toml`. Em modo interativo, `--config novo.toml` também permite criar o arquivo; em modo `--once`/`--json`, arquivo explícito ausente continua sendo erro. Arquivo inválido nunca é ignorado. `--once` e `--json`: saída 0 se todas as contas atualizarem; 2 para resultados com configuração ausente/falha; 1 para erro local.

TUI: `a` adiciona conta, `e` edita a selecionada, `↑↓`/`j k` selecionam, `PgUp/PgDn` paginam, `Home/End` vão às extremidades, `/` filtra, `Enter/Esc` encerram edição do filtro, `c` limpa, `[`/`]` rolam detalhes, `r` atualiza todas as contas e `R` atualiza somente a conta selecionada, `q`/`Ctrl-C` saem. Terminal mínimo do painel: 45 × 18; wizard: 60 × 20. Consultas independentes rodam em paralelo até `concurrency`; durante a atualização os dados anteriores permanecem visíveis. O cabeçalho mostra a contagem regressiva da próxima consulta da conta selecionada. Tema Dracula.

`K`/`J` movem a conta selecionada para cima/baixo na ordem persistida, sem consultar provedores; as ações também ficam no rodapé. A ordem é compartilhada com o desktop por `cuotai.toml`, preservando os comentários de cada conta.

Mouse: clique seleciona um cartão; clique no cartão já selecionado abre detalhes. Nos detalhes, clique na parte visível de uma fonte HTTPS única para abri-la no navegador; fontes compostas permanecem texto rolável. Botão direito volta dos detalhes. A roda navega pelas linhas de cartões e rola detalhes, listas do wizard e instruções de login. O rodapé possui ações clicáveis; o wizard oferece `[Voltar]`, `[Próximo]`, `[Gravar]` e `[Cancelar]`. A roda nunca confirma login/salvamento/exclusão. A captura é liberada ao sair ou em caso de panic. Alguns terminais reservam Shift+roda para o histórico do próprio terminal.

**Excluir conta:** selecione a conta no painel e pressione `d` ou `Delete`. A confirmação mostra o nome e o arquivo; `s` exclui e salva, `n`/`Esc` cancela sem alterações. O atalho respeita o filtro ativo e não exclui enquanto você digita no filtro ou no wizard. Falha de gravação mantém a conta e permite tentar novamente. Excluir remove apenas a entrada da configuração: não apaga variáveis de ambiente, entradas do Keychain nem revoga acesso no provedor.

### Painel de quotas

Os cartões (duas colunas em terminais largos) mantêm a ordem do arquivo de configuração, mesmo quando o resultado de uma consulta muda. `u` mostra/oculta contas sem dados (visíveis por padrão). `Enter` abre quotas, valores, barras de restante, períodos, contagem regressiva de renovação, observações e fonte. `Enter`/`Esc` voltam; `←→` mudam entre contas visíveis e mostram anterior/próxima; `↑↓` ou `[`/`]` rolam os detalhes. O filtro `/` continua funcionando.

Barras representam percentual **restante**; valores usados acima de 100% são preservados e apenas o desenho é limitado à largura da barra. Custos/saldos sem limite conhecido mostram valores, sem inventar percentual ou “ilimitado”. Saldo extra zero não torna esgotada uma assinatura que ainda tem quota limitada disponível. O controle `u` não cancela consultas nem remove contas do arquivo.

### Configurar uma conta pelo wizard

1. Para credencial por ambiente, exporte o access token no terminal que executará o Cuotai. Se a variável já existe no shell, `export MEU_TOKEN` a disponibiliza ao processo. O Cuotai não carrega `.env` automaticamente. Para login Google/Azure, registre seu aplicativo conforme a seção OAuth abaixo; não é necessário copiar access tokens.
2. Execute `cargo run -p cuotai-tui`. Se já houver configuração, pressione `a` para adicionar ou selecione uma conta e pressione `e` para editar.
3. **Provedor:** selecione com as setas e confirme com `Enter`.
4. **Nome da conta:** informe um nome único, como `Codex pessoal`.
5. **Credencial:** use o **nome da variável**, como `MEU_TOKEN`, ou pressione **Tab** para login OAuth nos provedores suportados. No modo OAuth, preencha o client ID do aplicativo e os campos seguintes; `Enter` em **Iniciar login** abre a autorização. O wizard indica `PRESENTE`/`AUSENTE` para variáveis, sem revelar valores. AWS solicita o perfil CLI e oferece **F2** para login SSO. Para Alibaba Token Plan, **F2** inicia `bl auth login --console --console-site international --config <perfil>` (requer `bl` instalado); o Cuotai consulta a quota com `bl usage token-plan --config <perfil> --output json`. `params.session_env` continua disponível para consulta HTTP direta com Cookie, sem CLI. `QWEN_API_KEY` autentica apenas inferência, não é necessária para a consulta de quota.
6. **Parâmetros:** preencha os campos específicos do provedor; `Enter` avança entre eles. Opcionais podem ficar vazios. Copilot oferece escolhas com `←→`, mostrando organização apenas no modo `org`.
7. **Intervalo:** informe de 30 a 86400 segundos para **esta conta** (mínimo de 300 s para AWS Bedrock); vazio herda o `refresh_seconds` global do TOML (padrão: 300). Contas com intervalos próprios são agendadas separadamente. AWS exibe um aviso de cobrança por consulta; considere 3600 segundos ou mais. Um intervalo global inferior a 300 s não é válido quando uma conta AWS o herda.
8. **Revisão:** confira conta, parâmetros e arquivo de destino; `↑↓` rolam o resumo. Só o `Enter` nesta etapa grava e aplica a configuração, iniciando a consulta no painel.

`Shift+Tab` volta mantendo o rascunho; `Ctrl+U` limpa o campo; `Backspace` apaga; `Esc` cancela sem salvar. Após salvar, use `a` para outra conta. A mudança de provedor redefine os campos e a referência de credencial para evitar reutilizar o token do provedor anterior.

A gravação usa arquivo temporário e substituição atômica, com permissão `0600` para arquivos novos no Unix. Edições em tabelas `[[accounts]]` preservam ordem, comentários da conta e demais parâmetros/configurações; a formatação dos parâmetros editados pode mudar. Se o arquivo mudar externamente, o wizard recusa sobrescrevê-lo e orienta fechar/reabrir o Cuotai. Em falha de gravação, o rascunho permanece na revisão: corrija a causa e pressione `Enter` para tentar novamente. A pasta de destino deve existir.

Tokens fornecidos por variável continuam sendo gerenciados externamente: se atualizar uma variável no shell, reinicie o Cuotai. Um processo aberto não recebe alterações posteriores no ambiente do terminal. Credenciais obtidas pelo login integrado são recuperadas do cofre e renovadas automaticamente antes da consulta quando próximas de expirar.

### Login OAuth integrado

| Provedor | Login oferecido | Pré-requisito |
|---|---|---|
| Gemini API / Vertex AI | Navegador do sistema, authorization code + PKCE, callback em `127.0.0.1` | Seu client ID OAuth do tipo **Desktop**, API Monitoring habilitada e permissão de leitura no projeto |
| Azure OpenAI | Código de dispositivo, navegador Microsoft | Seu aplicativo Entra **public client**, tenant e permissões delegadas/RBAC de custos |
| AWS Bedrock | **F2** na TUI ou **AWS SSO** no desktop executa o login SSO oficial da AWS CLI; URL e código aparecem no diálogo | AWS CLI v2.22+ e perfil SSO previamente configurado |
| GitHub Copilot, Kimi Coding, Grok CLI | Código de dispositivo no navegador | Login nativo compatível com o 9router; não exige cadastrar aplicativo próprio |
| Kiro | AWS SSO OIDC com registro dinâmico de cliente e código de dispositivo | Builder ID (padrão) ou Identity Center; região e start URL configuráveis, sem perfil da AWS CLI obrigatório |
| ChatGPT/Codex, Claude, Gemini CLI | Navegador com callback local e PKCE | Login nativo experimental; Codex requer porta local 1455 livre. Não equivale a suporte oficial do provedor |
| Cline | Navegador com callback local | Sessão Cline recebida no callback ou trocada por token; não exige copiar access token |
| Qoder, CodeBuddy | Navegador com polling do estado/dispositivo | Região internacional/CN selecionável antes do login |
| Zed | Navegador com chave RSA efêmera | Protocolo nativo, não OAuth padrão; user_id e system_id preenchidos automaticamente |
| Xiaomi MiMo | Navegador com callback X25519/AES-GCM **ou código para colar** | Cole o código da tentativa atual no campo oculto da TUI ou no campo do desktop. Obtém chave de inferência; quota semanal pode exigir cookie separado |
| Cursor | Importação explícita da sessão local em SQLite | Faça login no Cursor, escolha Tab → importação e confirme. Nenhuma leitura do token acontece só por abrir o wizard |
| Qwen Code | Device OAuth com PKCE + renovação | Fluxo legado descontinuado pelo provedor; a TUI mostra o aviso. O endpoint ainda emitiu desafio no smoke, mas isso não comprova concessão de acesso ou quota |
| Antigravity | OAuth Google no navegador + renovação | Sessão do IDE/Code Assist, distinta de uma API key Gemini; tokens no cofre, sem provisionar projeto |
| OpenAI/Anthropic API, Windsurf, Ollama e OpenCode Go | Mantidos os métodos por chave/variável | Não há botão que simule um login inexistente |

#### Executar login do 9router pela TUI

1. Inicie `cargo run -p cuotai-tui`, pressione **a** para adicionar ou **e** para editar uma conta.
2. Escolha o provedor e o nome da conta. Na etapa **Credencial**, pressione **Tab**.
3. Para login nativo, a tela mostra **Entrar com <provedor> no navegador** — não pede client ID. Kiro oferece método/região/start URL; Qoder, CodeBuddy e Xiaomi oferecem região.
4. Pressione **Enter** na confirmação. O Cuotai abre o navegador e, quando necessário, mostra o código de dispositivo. Autorize na página do provedor. **Esc** cancela a espera e volta ao formulário.
5. Ao receber a autorização, o wizard avança e preenche os campos de quota automaticamente. Em **Revisão**, pressione **Enter** para gravar tokens no cofre e a configuração no arquivo.
6. Para renovar uma autorização revogada ou expirada, edite a conta e use **F2** no modo login. Não é preciso copiar tokens para variáveis.

Login nativo seleciona automaticamente o produto correto: Copilot quota pessoal, Kimi Coding, Gemini CLI, Grok CLI e Qoder bearer. Não substitui essas sessões por chaves comerciais de geração. IDs Zed, perfil Kiro, device ID Kimi e account ID Codex são derivados do retorno quando disponíveis; metadados de quota não secretos são preenchidos, credenciais de refresh ficam somente no cofre.

Os fluxos compatíveis usam os identificadores de clientes nativos e protocolos presentes na revisão do 9router indicada abaixo. Podem ser recusados, alterados ou restringidos pelo provedor. Em particular, a política publicada da Anthropic restringe login Claude.ai em aplicativos terceiros; implementar o protocolo **não constitui autorização ou garantia de aceitação**. Senhas e consentimento são tratados no navegador do provedor, nunca pela TUI; não há proxy de senha nem extração automática de cookies/SQLite.

**Separação importante:** `gemini_api` e `vertex_ai` (Cloud Monitoring), além de Azure, conservam os fluxos com aplicativo próprio descritos abaixo. `gemini_ai` em login nativo acompanha Gemini CLI/Code Assist.

#### Cursor pessoal: importar pelo wizard

1. Faça login normalmente no aplicativo **Cursor**.
2. No Cuotai, adicione/edite Cursor e pressione **Tab** na etapa Credencial.
3. Deixe o caminho vazio para a instalação padrão ou informe um `state.vscdb` específico (aceita espaços no caminho, por exemplo Cursor Insiders).
4. Confirme **“CONFIRMAR leitura e importação da sessão Cursor”**. A leitura é somente leitura, em uma transação SQLite, das chaves `cursorAuth/accessToken`/`cursorAuth/token` e machine ID; não modifica o banco e não lê todas as preferências/segredos.
5. Na revisão, grave. O token fica no Keychain; o TOML guarda apenas a referência da sessão, caminho opcional e metadados públicos. A importação não autentica de novo nem renova o token do Cursor; token expirado requer login no editor e reimportação com **F2**.

Sessão pessoal usa `/api/usage-summary` em `cursor.com`, com cookie derivado do token da conta. **Nunca é enviada ao Admin API `/teams/spend`**. O modo por variável `CURSOR_ADMIN_API_KEY` permanece para Teams/Enterprise. Uma sessão importada não garante acesso: a resposta do provedor é exibida na consulta. Cancele com Esc antes de confirmar para não ler o banco, ou antes de gravar para descartar a cópia em memória.

#### Xiaomi MiMo: código manual

Quando a página exibir **Authentication Code / Paste this into MiMo Code**, volte à tela de login pendente do Cuotai, cole o código no campo **Código MiMo (oculto)** e pressione **Enter**. A colagem aceita Base64URL sem padding e quebras de linha. `Ctrl-U` limpa o campo; `Esc` cancela. Código inválido/de outra sessão mostra erro sem encerrar a tentativa; a expiração original não é estendida.

O código é criptografado para a chave efêmera dessa tentativa. Não use códigos de outra execução, não cole no chat e não trate o código como API key. Se reiniciou o Cuotai ou expirou a tentativa, gere um novo código. O callback automático continua aceito em paralelo. O código digitado nunca é exibido nem persistido; só o token decodificado é gravado no cofre após a confirmação final.

O formato atual coloca a chave pública efêmera antes do nonce; a referência antiga do 9router tinha a ordem inversa. Ambos são aceitos somente após validação AES-GCM. Um código sintético de **251 caracteres**, no formato atual, foi exercitado de ponta a ponta na TUI (colagem mascarada → autorização), sem gravar token. O tamanho 251 não é uma restrição do campo.

**Autorização por código não é sessão de quota.** O código entrega uma chave `sk-` de inferência; não entrega cookies do console. Para saldo e Token Plan mensal, edite a conta e configure `params.platform_session_env` com o **nome** da variável contendo o Cookie de `platform.xiaomimimo.com` (`api-platform_serviceToken` e `userId`). O backend consulta somente `/api/v1/balance`, `/tokenPlan/detail` e `/tokenPlan/usage`. Não lê a chave no Keychain para esse modo. A quota semanal do MiMo Desktop permanece separada em `session_env`/`region`; se ambos os campos forem preenchidos, o modo Desktop tem precedência. Não repita o código esperando obter esses cookies: são credenciais distintas. Nenhum cookie é extraído automaticamente nem salvo no TOML.

#### Qwen Code OAuth

Selecione **Qwen → Tab → Entrar com Qwen**. O fluxo usa o device grant com PKCE do cliente oficial Qwen Code: `chat.qwen.ai/api/v1/oauth2/device/code` e `/token`, escopo `openid profile email model.completion`. Access/refresh tokens ficam no cofre, separados de `QWEN_API_KEY` do DashScope.

**Limitação do provedor:** a [documentação oficial](https://github.com/QwenLM/qwen-code/blob/81582ca19d0772afdd06ace9cad4845c207001f8/docs/users/configuration/auth.md) informa que o OAuth gratuito foi descontinuado em 15/04/2026. A TUI disponibiliza a tentativa com aviso; o endpoint real emitiu um desafio durante o smoke, sem consentimento concluído. Não foi encontrado endpoint de quota Qwen Code; login bem-sucedido não é convertido em falsa medição de consumo e não envia o token ao faturamento DashScope.

#### Antigravity

Selecione **Antigravity → Credencial → Tab** para autorizar a sessão Google no navegador. O cadastro não usa a API key Gemini. As consultas são somente leitura, sem geração nem `onboardUser`. Percentuais e renovação aparecem nos cartões e em **Enter → detalhes**; a escala 1.000 usada pelo 9router não é apresentada como quantidade de requisições. Quotas ausentes continuam desconhecidas e falhas parciais ficam nas observações. Os endpoints internos podem mudar ou recusar o cliente; autorizar o login não garante acesso às quotas.

#### Google: preparação única

1. Em [Google Cloud](https://console.cloud.google.com/apis/credentials), habilite **Cloud Monitoring API** no projeto consultado e configure a tela de consentimento OAuth. Em aplicativo externo em teste, inclua sua conta entre os usuários de teste.
2. Crie um **OAuth client ID → Desktop app** para o Cuotai. Não use client ID de outro produto.
3. Na TUI, escolha Gemini API ou Vertex AI; no passo **Credencial**, pressione `Tab` e informe o client ID.
4. Se o registro Google exigir `client_secret`, exporte-o em uma variável e informe **o nome dessa variável** no campo seguinte. Segredos não entram no TOML. Clientes Desktop são públicos; esse campo não substitui PKCE.
5. Confirme **Iniciar login**. O navegador solicita `https://www.googleapis.com/auth/monitoring.read`; o retorno usa uma porta local livre. Autorize no Google e volte à TUI.
6. Preencha `project_id`, revise e grave. A identidade Google precisa de `monitoring.timeSeries.list`, por exemplo via Monitoring Viewer no projeto. OAuth não concede permissões IAM que sua conta não possui.

O callback expira em cinco minutos. Aplicativos Google externos em modo teste podem ter refresh tokens com validade limitada pela política do provedor; refaça o login quando necessário.

#### Azure: preparação única

1. Em [Microsoft Entra](https://portal.azure.com), registre seu aplicativo. Em **Authentication → Advanced settings**, habilite **Allow public client flows**.
2. Configure a permissão delegada **Azure Service Management → user_impersonation** e conceda o consentimento exigido pelo tenant. A conta autorizadora precisa de **Cost Management Reader** no recurso/assinatura consultado.
3. Na TUI, escolha Azure OpenAI e pressione `Tab` no passo Credencial. Informe o **Application (client) ID** e o tenant (GUID/domínio; vazio usa `organizations`). Não é necessário client secret para esse public client.
4. Confirme o login. Digite o código exibido na página Microsoft aberta pelo Cuotai. O pedido inclui `https://management.azure.com/user_impersonation` e `offline_access`.
5. Após autorizar, informe `subscription_id` e `resource_id`, revise e grave. Políticas de Conditional Access podem bloquear device flow; nesse caso, mantenha o método por variável ou ajuste a política com o administrador.

#### AWS: perfil SSO

Prepare uma vez com `aws configure sso`. No wizard Bedrock, informe o perfil e pressione **F2**. A TUI mostra a URL e o código fornecidos por `aws sso login --profile ... --use-device-code --no-browser`; abra a URL para autorizar. A própria AWS CLI mantém e usa seu cache SSO. O Cuotai não copia esses tokens para o Keychain. `Enter` sem F2 mantém o caminho de perfil já autenticado. Login SSO não dispensa `ce:GetCostAndUsage` e não elimina a cobrança das consultas de Cost Explorer.

#### Persistência, cancelamento e renovação

- Por padrão, os logins integrados armazenam access token, refresh token e contexto secreto no cofre nativo, serviço **`cuotai.oauth`**. No macOS é o **Keychain**; Windows Credential Manager e Linux Secret Service também têm implementação, mas não foram exercitados neste ambiente. O sistema pode solicitar permissão por binário. Sem acesso autorizado, há erro explícito; nunca há fallback automático para SQLite.
- O TOML guarda `accounts.oauth.credential_id`, client ID/tenant/referência de client secret quando aplicável, e `options` não secretas (`region`, `auth_method`, `start_url`). O client ID é omitido nos logins nativos padrão. `oauth` e `credential_env` não podem coexistir na mesma conta. A identidade de cofre inclui provedor, cliente, tenant e opções, sem quebrar configurações OAuth anteriores.
- Tokens de login novo ficam em memória até **Revisão → Enter GRAVAR** na TUI ou **Salvar** no desktop. O core grava primeiro no cofre e depois no TOML; se o arquivo falhar, restaura a credencial anterior ou remove a nova. Se a compensação também falhar, mostra o estado parcial; a autorização permanece em memória para tentar salvar novamente. Não há transação atômica entre Keychain e arquivo.
- `Esc` durante autorização interrompe o callback/polling e volta ao formulário. O consentimento já concedido no provedor não é revogado automaticamente. AWS gerencia seu próprio cache; login SSO concluído permanece na AWS CLI mesmo se cancelar a configuração do Cuotai.
- Para refazer um login: selecione a conta, `e`, avance até Credencial e pressione **F2** no modo OAuth. Para trocar para variável, use `Tab`. A credencial antiga não é revogada nem apagada automaticamente.
- Antes de consultar, tokens com menos de 60 segundos de validade são renovados quando existe refresh token e o protocolo o suporta. Claude/Codex/Google/Kimi/Kiro/Grok/CodeBuddy/Cline têm caminhos de renovação; GitHub usa refresh quando fornecido. Qoder e Zed exigem novo login quando revogados/expirados; a chave Xiaomi obtida pelo fluxo nativo não tem refresh. Tokens rotacionados e metadados secretos substituem os anteriores no cofre. Variáveis de ambiente não recebem renovação automática.
- Uma rotação já iniciada termina sua persistência mesmo que a consulta seja cancelada, evitando perder o refresh token novo. Compartilhe uma `OAuthSession` dentro do app; evite executar duas instâncias sobre a mesma credencial durante rotação (não há coordenação entre processos).

O Cuotai não possui aplicativos OAuth publicados próprios. Nos fluxos Cloud Monitoring/Azure, informe o registro do seu aplicativo. Nos fluxos nativos acima, a TUI usa a compatibilidade do cliente de referência e não exige que você registre outro aplicativo. A disponibilidade final é controlada pelo provedor.

#### Credenciais SQLite — somente desenvolvimento

Para evitar os prompts do Keychain a cada recompilação, ative explicitamente:

```sh
cargo run -p cuotai-tui -- --dev-credentials-sqlite ./cuotai-dev.sqlite3
```

A mesma opção funciona com `--once` e `--json`. O banco guarda **tokens sem criptografia**, incluindo refresh tokens e contexto secreto. Use apenas em ambiente local confiável: qualquer processo com acesso ao arquivo pode usar essas credenciais. No Unix, arquivos novos têm modo `0600`; arquivos existentes acessíveis a grupo/outros e links simbólicos são recusados. O diretório pai precisa existir. Bancos `.sqlite3` e seus sidecars são ignorados pelo Git; não compartilhe nem faça backup público desses arquivos.

Não há migração automática do Keychain. Na primeira execução com SQLite, refaça os logins/importações desejados; as próximas execuções e recompilações reutilizam o mesmo arquivo. Use sempre a flag enquanto estiver desenvolvendo. Sem ela, o aplicativo volta ao cofre do sistema e não lê o SQLite. Variáveis de API key e o cache AWS CLI continuam onde estavam.

Builds release recusam a opção **antes de criar ou abrir o banco**; a implementação SQLite nem é compilada sem `debug_assertions`. O backend permite opt-in explícito via `OAuthSession::development_sqlite`; `system()` e `Monitor::new()` permanecem com cofre nativo.

Verificação: 433 testes passaram, Clippy sem avisos; um token sintético foi importado e salvo no SQLite, recuperado por outro processo sem Keychain, e o CLI debug abriu o banco. O binário release recusou a opção sem criar arquivo.


## Integrações e credenciais

| Identificador | Dados consultados | Configuração / restrições |
|---|---|---|
| `open_ai_api` | Custo por moeda; tokens e requisições de completions, mês UTC | `OPENAI_ADMIN_KEY`, **Admin API key**. `params.project_ids` e `params.models` opcionais, separados por vírgula |
| `anthropic_api` | Custo USD e tokens de mensagens incluindo cache, mês UTC | `ANTHROPIC_ADMIN_KEY`, chave de administração. `params.workspace_ids` e `params.models` opcionais. Priority Tier não incluído no relatório de custos |
| `chat_gpt` | Percentual das janelas **Codex** e saldo de créditos quando retornado | `CHATGPT_ACCESS_TOKEN`, token OAuth fornecido pelo usuário; `params.account_id` opcional. Interface interna, não documentada. **Não consulta quota de mensagens do ChatGPT web** |
| `claude` | Janelas de utilização e renovação; uso extra quando retornado | `CLAUDE_OAUTH_TOKEN`. Interface não documentada, contrato evidenciado por clientes comunitários. Unidades de uso extra não documentadas são preservadas como unidades do provedor |
| `gemini_api` | Limites e consumo recente via Cloud Monitoring | Login OAuth integrado ou `GOOGLE_CLOUD_ACCESS_TOKEN`, `params.project_id`; `monitoring.timeSeries.list`. Serviço `generativelanguage.googleapis.com` |
| `vertex_ai` | Limites e consumo recente via Cloud Monitoring | Mesmos requisitos; serviço `aiplatform.googleapis.com` |
| `aws_bedrock` | UnblendedCost do mês, filtro de serviço Amazon Bedrock | `params.profile` obrigatório, AWS CLI autenticada, `ce:GetCostAndUsage`. Não mede todas as quotas de inferência |
| `azure_open_ai` | Custo mensal do recurso explicitamente escolhido | Login OAuth integrado ou `AZURE_ACCESS_TOKEN`, `params.subscription_id`, `params.resource_id` ARM completo de `Microsoft.CognitiveServices/accounts`; Cost Management Reader. Recurso Foundry compartilhado inclui outros modelos |
| `cursor` | Gasto sob demanda e total do ciclo da equipe | `CURSOR_ADMIN_API_KEY`. Admin API **Teams/Enterprise**, não assinatura individual |
| `copilot` | Uso bruto e custo líquido de solicitações premium ou créditos AI | `GITHUB_TOKEN`. `params.mode=user` (padrão) ou `org` + `params.org`. `params.billing=premium_requests` (padrão) ou `ai_credits`. Conta pessoal não inclui licenças faturadas pela organização |
| `windsurf` | Uso de créditos, alocação e créditos adicionais no ciclo | `WINDSURF_SERVICE_KEY`. **Enterprise**, permissões Billing Read e Teams Read-Only. Alocação por assento não é saldo compartilhado; restante não inferido |

OAuth integrado está disponível para Google/Azure e SSO via CLI para AWS conforme descrito acima. Chave de geração não substitui chave administrativa ou token de assinatura. Nenhum cookie, Keychain ou arquivo de credenciais de outro app é extraído automaticamente; só entradas do próprio Cuotai são lidas no cofre. Os métodos legados por variável permanecem disponíveis, sujeitos às regras do provedor.

### Catálogo de API keys e PATs

Os nomes abaixo são os defaults **exatos** solicitados e já vêm preenchidos no wizard. `credential_env` pode apontar para outra variável, sem copiar a chave para o TOML. Estar no catálogo **não significa** que o provedor disponibiliza quota por API key.

| Provedor / identificador | Variável padrão | Consulta implementada / limitação |
|---|---|---|
| Minimax.io / `mini_max` | `MINIMAX_IO_API_KEY` | Token/Coding Plan: janelas por contagem ou percentual, incluindo M-series. Fallback entre endpoints usados pelo 9router; chave `sk-api-` mantém consulta de saldo |
| Kimi / `kimi` | `KIMI_API_KEY` | Saldo Moonshot internacional por padrão. `params.product=coding` consulta quota semanal e rate limit do Kimi Coding com a própria chave |
| Qwen / `qwen` | `QWEN_API_KEY` | DashScope por chave continua sem quota compatível. OAuth Qwen Code disponível via Tab com aviso de descontinuação; nenhum endpoint de quota confirmado para essa sessão |
| Alibaba Model Studio / `alibaba_model_studio` | `DASHSCOPE_API_KEY` | Cadastro separado, região `intl` ou `cn`. API key de inferência não concede consulta pública de saldo/quota |
| Alibaba Coding Plan / `alibaba_coding_plan` | `BAILIAN_CODING_PLAN_API_KEY` ou variável configurada, como `QWEN_API_KEY` | RPC regional `queryCodingPlanInstanceInfoV2`: uso/limite de 5h, semana e mês. Se o gateway exigir login, `params.session_env` referencia cookie explícito; não extrai sessão do navegador nem tenta outra região |
| Alibaba Token Plan (Qwen Cloud) / `alibaba_token_plan` | `params.cli_profile = "cuotai-qwen"` (login `bl` internacional) **ou** `params.session_env = "QWEN_CLOUD_COOKIE"` (Cookie) | Quota 5h/semana via `bl usage token-plan --output json` no perfil indicado; `session_env` usa HTTP direto do console Qwen Cloud e tem precedência quando presente. A chave `QWEN_API_KEY` só autentica inferência em `https://token-plan.maas.qwencloudapi.com/compatible-mode/v1` ou `/apps/anthropic`; esses caminhos não foram confirmados como fontes de quota. O CLI grava sua própria sessão fora do Cuotai; nunca coloque Cookie/chave no TOML |
| Alibaba Cloud / `alibaba_cloud` | `ALIBABA_CLOUD_ACCESS_KEY_ID` | Saldo BSS via AccessKey Secret referenciada em `params.secret_env`; STS opcional em `security_token_env`. `billing_cycle=YYYY-MM` consulta fatura de **todos os produtos**, não apenas AI |
| Antigravity / `antigravity` | OAuth Google ou `ANTIGRAVITY_ACCESS_TOKEN` | Projeto/plano via `loadCodeAssist`; quotas por modelo via `fetchAvailableModels` nos planos pagos; sessão/semana via `retrieveUserQuotaSummary`. Modelo sem fração de quota mantém valores desconhecidos e não força a sessão a 0%. `project_id` opcional |
| Groq / `groq` | `GROQ_API_KEY` | GET `/openai/v1/models`, lendo `x-ratelimit-*` como o 9router. Sem geração. Se a resposta não trouxer cabeçalhos, indica conectado sem quota reportada, nunca zero inventado |
| Perplexity / `perplexity` | `PERPLEXITY_API_KEY` | Sonar: indisponível. **Computer Enterprise Analytics** é opt-in separado (`params.product=computer`), com chave Analytics; soma créditos dos dias UTC completos |
| Mistral / `mistral` | `MISTRAL_API_KEY` | `/v1/admin/spend-limit` com **Admin API Key**, cabeçalho `x-api-key`. Valores ausentes continuam desconhecidos; estado de limite mensal aparece nas observações |
| Cloudflare AI / `cloudflare_ai` | `CLOUDFLARE_AI_API_KEY` | Sem consulta de Neurons/quota Workers AI confirmada nesta integração. Invocações genéricas de Workers e AI Gateway não são substitutos |
| Ollama Cloud / `ollama_cloud` | `OLLAMA_CLOUD_API_KEY` | GET `/api/usage`: janelas mensal/semanal/sessão conforme plano. POST `/api/me` complementa plano/cadastro; não exige cookie. Renovação gratuita estimada pelo aniversário de cadastro |
| Xiaomi MIMO / `xiaomi_mimo` | Sessão explícita por variável | `platform_session_env`: saldo e Token Plan mensal do console; `session_env`/`region`: quota semanal do Desktop. Código nativo gera chave de inferência e não substitui essas sessões |
| Opencode Go / `open_code_go` | `OPENCODE_GO_API_KEY` | GET `https://opencode.ai/zen/go/v1/usage`: percentual rolling, semanal e mensal, com renovação |
| Qoder / `qoder` | `QODER_API_KEY` | PAT `pt-` trocado por job token e GET `/api/v2/quota/usage`; `params.region=intl|cn`, `params.credential_type=auto|pat|bearer` |
| Gemini AI / `gemini_ai` | `GEMINI_API_KEY` | API key AI Studio continua sem quota nesta interface. `params.product=cli` usa sessão Gemini CLI e `retrieveUserQuota`; não confunde produtos |
| Grok (xAI) / `grok` | `X_GROK_API_KEY` | Management API por padrão. `params.product=cli` usa sessão Grok CLI para créditos sob demanda, pré-pagos e pool SuperGrok via REST/gRPC, como no 9router |
| NVidia NIM / `nvidia_nim` | `NVIDIA_NIM_API_KEY` | Sem consulta pública de saldo por chave confirmada; não usa inferência para extrair rate-limit headers |
| Poolside / `poolside` | `POOLSIDE_API_KEY` | Sem consulta pública de saldo/quota da conta confirmada; métricas de infraestrutura não são saldo |
| Kimchi / `kimchi` | `KIMCHI_API_KEY` | GET `/v1/credits` e `/v1/budget`: saldo USD, orçamento por escopo/provedor e renovação. Falha no orçamento preserva créditos válidos com observação |

Para Mistral, Perplexity Computer e Grok, a variável da etapa Credencial deve conter a **classe correta** de chave. Se guardar a chave administrativa em outra variável, configure `params.admin_key_env`, `params.analytics_key_env` ou `params.management_key_env`, respectivamente. A referência alternativa tem precedência; apenas a chave efetivamente usada é lida. Não é necessário fornecer duas chaves. O wizard oferece esses campos e preserva os nomes padrão acima.

### Outros assistentes

- **Cline (`cline`)**: consulta saldo pessoal com `CLINE_API_KEY` (API key ou account auth token Cline); `/users/me` descobre o ID antes da consulta de saldo. `params.organization_id` seleciona saldo da organização. Não agrega histórico incompleto nem quotas dos provedores BYOK.
- **Kiro (`kiro`)**: `KIRO_ACCESS_TOKEN`, consulta interna `getUsageLimits` em CodeWhisperer/Q. `params.auth_method` seleciona builder-id, idc, google, github, import, api_key ou external_idp; `params.profile_arn` permite indicar seu perfil. Quota de Kiro é distinta do faturamento Bedrock.
  - Login recomendado: `a` → **Kiro (AWS)** → Credencial → `Tab` → **AWS Builder ID** → confirmar no navegador → gravar na revisão. Identity Center usa sua região e URL `https://…awsapps.com/start`. Access/refresh tokens ficam no cofre; não exige perfil AWS CLI nem AccessKey ID/Secret.
  - Com `profile_arn` da própria conta (descoberto pelo login ou informado no campo opcional), consulta créditos com POST `{profileArn}` no host regional conhecido (`us-east-1` ou `eu-central-1`) antes das três tentativas legadas do 9router. Mostra plano, teste gratuito, excedente habilitado e renovação; bônus não separáveis são sinalizados. API keys usam `tokentype: API_KEY`, sem inventar ARN de outra conta.
  - **Várias contas:** adicione uma entrada Kiro por identidade, com nomes distintos. No wizard, faça o login nativo separadamente em cada entrada; cada uma recebe seu próprio `credential_id` e consulta sua própria quota. Com TOML manual, use `credential_env` diferente para cada token (`KIRO_ACCESS_TOKEN`, `KIRO_SECOND_ACCESS_TOKEN`) e `params.profile_arn` da respectiva conta, quando disponível. O Cuotai não importa automaticamente sessões do 9router nem acessa suas credenciais; a interface de quota é não oficial e acesso real depende do provedor.
- **CodeBuddy (`code_buddy`)**: `CODEBUDDY_ACCESS_TOKEN`, POST interno `/v2/billing/meter/get-user-resource`. `params.region=cn|intl`; pacotes de renovação e bônus aparecem separados.
- **Zed (`zed`)**: `ZED_ACCESS_TOKEN` + `params.user_id` obrigatório; GET `cloud.zed.dev/client/users/me`, `Authorization: <user_id> <token>`. `params.system_id` opcional. Não extrai tokens do editor automaticamente.
- **Claude e ChatGPT/Codex** mantêm consultas internas. **Copilot** oferece quota pessoal e billing. **Cursor** oferece Admin API Teams/Enterprise e sessão pessoal importada com confirmação; o contrato pessoal segue a implementação pública do CodexBar, pois o 9router analisado não tem handler de quota Cursor.

Cursor importado: a montagem do cookie preserva maiúsculas/minúsculas do identificador do JWT. Tokens já salvos e válidos usam a correção na próxima consulta; tokens expirados/revogados exigem login no IDE e nova importação explícita. A importação recusa tokens com menos de 60 segundos de validade.

Poolside permanece **bloqueado**, não implementado: o cliente oficial revela `GET /admin/account/{account_id}/usage` para consumo de rate limit, autenticado por token administrativo da plataforma, não pela chave de inferência. Faltam autorização apropriada e schema/resposta redigida dessa rota. Não se presume que esse consumo seja saldo de faturamento.

O catálogo contém **36 entradas**, incluindo os provedores anteriores. A configuração existente do usuário não é sobrescrita nem ganha contas automaticamente: use `a` no painel para adicioná-las ou consulte `cuotai.example.toml`.

### Compatibilidade com o 9router

Referência de código: [9router, revisão `f01fb909e37189008080632ddaf404f096345cde`](https://github.com/decolua/9router/tree/f01fb909e37189008080632ddaf404f096345cde). Os módulos Rust `providers/router_*.rs` reproduzem consultas e `oauth/reference*.rs` implementam os fluxos de login/renovação a partir de `src/lib/oauth/providers/` e helpers nativos. A licença MIT e atribuição estão em `THIRD_PARTY_NOTICES`.

Essas consultas incluem **interfaces internas/não oficiais**. Podem mudar, expirar ou exigir sessão específica. Não é necessário executar o 9router junto com o Cuotai. O Cuotai não importa seus bancos de credenciais, cookies do navegador ou arquivos de sessão de outros apps, não gera respostas de IA para medir limites e não chama operações que gastam créditos para renovar quota.
Token Plan é separado de Coding Plan. O [dispatcher de quota do 9router](https://raw.githubusercontent.com/decolua/9router/master/open-sse/services/usage.js) não possui handler para Alibaba Token Plan: cadastrar `QWEN_API_KEY` para inferência não demonstra que a chave consulte quota. A [CLI oficial](https://github.com/modelstudioai/cli/blob/main/packages/commands/src/commands/usage/token-plan.ts) consulta o RPC do console com autenticação de console; o wizard pode iniciar o login internacional e salvar somente o nome do perfil para consultas posteriores. Alternativamente, a consulta HTTP direta segue o [contrato observado pelo CodexBar](https://github.com/steipete/CodexBar/pull/2361), com Cookie explícito via `params.session_env` e sem leitura automática de cookies do navegador. O login `bl` não configura esse Cookie. Interface interna sujeita a mudanças; não foi obtida quota de uma conta Token Plan real neste ambiente.

Sem sessão do console no perfil, o `bl` retorna código 3 com erro JSON em stdout. O Cuotai mostra **erro de autenticação** sem exibir a resposta; refaça o login pelo **F2** ou execute `bl auth login --console --console-site international --config <perfil>`. Login e quota real dependem de uma conta Token Plan e não foram confirmados neste ambiente.

Uma consulta `bl usage token-plan` bem-sucedida pode retornar `{}` ou somente uma das janelas: a [CLI oficial](https://github.com/modelstudioai/cli/blob/main/packages/commands/src/commands/usage/token-plan.ts) omite limites ausentes e orienta conferir o console. O Cuotai marca a quota ausente como desconhecida; não presume limite ilimitado nem consumo zero.

No modo **variável de ambiente**, selecione o produto em **Parâmetros** conforme a tabela. No **login nativo**, produto e metadados de autenticação são preenchidos e protegidos contra mudanças incompatíveis com a sessão.

| Caminho | Configuração |
|---|---|
| Kimi Coding | `product = "coding"`; `KIMI_API_KEY`. OAuth alternativo: `token_env` (nome de variável); a API key tem precedência quando ambas existem |
| Copilot pessoal | `query_mode = "quota"`; `GITHUB_TOKEN` da conta, não copilotToken. Campos de billing são ocultados nesse modo |
| Gemini CLI | `product = "cli"`; `session_env` opcional (default `GEMINI_CLI_ACCESS_TOKEN`), `project_id` opcional (resolvido por `loadCodeAssist`). O token Cloud Monitoring de escopo restrito não substitui essa sessão |
| Grok CLI | `product = "cli"`; `session_env` opcional (default `GROK_CLI_ACCESS_TOKEN`), `email`/`user_id` opcionais. Não utiliza a Management Key nesse modo |
| Xiaomi MiMo | `session_env = "XIAOMI_MIMO_COOKIE"`, cookie de conta exportado pelo usuário; `region = "sgp"` (ou cn/ams/ru/in). A integração usa a sessão pronta; não implementa a extração de passToken do Desktop/SSO feita pelo 9router |
| Qoder | PAT/job token em `QODER_API_KEY`; `region = "intl"` (ou cn), `credential_type = "auto"` (ou pat/bearer). Troca PAT→job token sem fazer inferência |
| Kiro/CodeBuddy/Zed | Preferencialmente login nativo via **Tab → Entrar** no wizard; tokens e metadados são preenchidos automaticamente. O método manual por variável permanece disponível |

Diferenças deliberadas: campos ausentes não viram zero ou ilimitado; Gemini CLI mostra `remainingFraction` em **%**, não como 1000 requisições reais; Ollama identifica a renovação calculada a partir do cadastro como estimativa; respostas de erro não expõem corpo bruto. Datas sem timezone de pacotes CodeBuddy são interpretadas como UTC (o JavaScript original usa o timezone do processo), portanto podem diferir do painel.

O 9router distingue quota do provedor de **uso registrado pelo seu proxy** (`usageDb`). O despacho consultado não implementa quota para Cloudflare AI, Qwen/DashScope, Perplexity API, Mistral, NVIDIA NIM, Poolside ou Cursor. Não extrapolamos suporte de inferência/telemetria para prometer saldo nesses casos; mantemos as consultas próprias já existentes de Mistral e Perplexity Computer.

### Semântica e custos das consultas

- `used`, `limit` e `remaining` são opcionais. Ausência **não** significa zero. Não se calcula saldo de API a partir de gasto, nem mistura moedas.
- `status` descreve a **consulta**, não a saúde global do provedor. `ready` significa que a consulta terminou, não que existe quota disponível. HTTP 401/403, 429, rede, timeout e resposta incompatível são distinguidos.
- Custos mensais são acumulados; `resets_at` nesses relatórios indica início do próximo período de acompanhamento, não concessão garantida de créditos.
- OpenAI: tokens limitados a completions; custos têm escopo mais amplo. Filtrar `models` altera tokens, **não** o escopo dos custos. O mesmo vale para Anthropic.
- Google: limites amostrados diariamente; consumo é soma dos últimos 10 minutos com atraso. Séries de limite e uso são separadas, não percentuais comparáveis. Nem toda quota de Gemini/AI Studio está publicada no Monitoring. Lista vazia significa que nenhuma série foi retornada, não utilização zero.
- AWS: custos com atraso, possivelmente estimados. **Cost Explorer cobra por requisição** (consulte preço vigente; referência de US$ 0,01). Atualização a cada 300 s, rodando 24 h, pode gerar cerca de US$ 2,88/dia por conta. O Cuotai exige intervalo mínimo de 300 s para AWS Bedrock e avisa sobre o custo na edição da conta (TUI e desktop). Defina `accounts.refresh_seconds = 3600` ou mais para contas AWS, mantendo o global para as demais.
- Google Monitoring também pode cobrar leituras. Há duas consultas iniciais por atualização, mais páginas. AWS/Azure não fornecem consumo instantâneo por esse caminho.
- Windsurf: analytics em buckets horários; alocação e consumo podem ter escopos distintos. O app não inventa saldo restante a partir dessa comparação.
- Codex, Claude e Cursor pessoal usam interfaces internas e podem deixar de funcionar sem aviso. Windsurf continua exigindo Enterprise. As limitações são exibidas sem inventar consumo.

## Separação TUI / backend / desktop

```text
crates/cuotai-core/       biblioteca Rust: contas e persistência, credenciais, provedores, métricas, consultas
crates/cuotai-tui/        binário cuotai: CLI, wizard de entrada, Ratatui, teclado
apps/desktop/             aplicativo Tauri: janelas, bandeja e apresentação das consultas
```

`cuotai-core` não depende de Ratatui, Crossterm nem de um loop de UI. `account_edit::field_descriptors` e `native_option_descriptors` definem campos, opções e obrigatoriedade por provedor; `visible_field_indices` decide quais campos se aplicam conforme produto, modo e login. `build_account` valida o rascunho contra esse esquema, e `Config::upsert_account`/`without_account` criam, editam ou removem contas. `config_file::ConfigFile` descobre/carrega/grava o TOML preservando comentários e recusando edições externas. `Monitor::refresh`/`refresh_account` consultam provedores e normalizam relatórios; o semáforo do próprio `Monitor` limita em conjunto consultas completas e individuais. `refresh::RefreshCycle` calcula os prazos efetivos de cada conta e monta incrementalmente o `Snapshot`; `metrics::classify_report`/`classify_metric` derivam disponibilidade e consumo. A TUI transforma descritores em widgets, mantém entrada/rascunho/navegação, solicita consultas nos momentos indicados pelo ciclo e apresenta relatórios; não decide regras de provedores, intervalos efetivos, mesclagem de relatórios nem grava TOML por conta própria. O desktop é outro consumidor do core, mantido separadamente. A interface principal é:

```rust
use cuotai_core::{Config, Monitor};

async fn consultar(config: Config) -> Result<(), Box<dyn std::error::Error>> {
    let monitor = Monitor::new(config)?;
    let snapshot = monitor.refresh().await;
    println!("{}", serde_json::to_string(&snapshot)?);
    Ok(())
}
```

O chamador fornece um runtime Tokio e decide quando solicitar trabalho. `Monitor` reutiliza conexões HTTPS, limita em conjunto as consultas completas e individuais (1–32), aplica prazo por conta (1–300 s, incluindo autenticação e páginas), preserva ordem e isola falhas. Para atualização incremental, `RefreshCycle` informa vencimentos, mantém o snapshot mais recente e substitui o relatório da conta concluída. A TUI e o desktop aplicam recuo exponencial por conta em falhas, até 3600 s; uma edição consulta somente a conta alterada, sem repetir consultas pagas das demais. Não há retries dentro de uma consulta; OAuth device flow usa o polling exigido pelo protocolo. Cancelar o future cancela consultas e o subprocesso AWS; uma renovação OAuth iniciada pode continuar até persistir tokens rotacionados. Paginação incompleta é erro, nunca um total parcial apresentado como completo.

`AccountClass` serializa como `"available"`, `"exhausted"` ou `"unmeasured"`; `MetricState` como `"unmeasured"`, `{"exhausted":{"no_cap":true}}` ou `{"available":{"ratio":0.25,"uncapped":false}}`. Campos são `snake_case`; essa classificação não altera o formato existente de `Snapshot`/`--json`.

Textos apresentados pelo core carregam `Localized { id, params, fallback }`: o desktop resolve `id` nos catálogos `apps/desktop/ui/locales/{en-US,es}.json` e usa o texto português de `fallback` se não houver tradução; a TUI usa esse padrão português. IDs incluem `field.<provider>.<key>.label`, `edit_error.<case>` e `note.<adapter>.<case>`; parâmetros são estruturados, sem tokens. `Report.notes` continua texto em português para consumidores existentes e `Report.notes_l10n` acompanha as notas na mesma ordem para interfaces traduzidas. Para adicionar idioma, adicione um catálogo, sem comparação pelo texto português.

O aplicativo Tauri em `apps/desktop/` consome a biblioteca diretamente e não implementa protocolos de provedor. `Monitor::with_credentials(config, Arc<dyn Credentials>)` permite a outros frontends fornecer segredos vindos de um cofre. `Credentials::get` deve ser rápido e não interativo; carregue credenciais antes da consulta. Exporte apenas `Snapshot`, não tokens. A saída `--json` também permite integração por processo, se desejada.

O módulo público `cuotai_core::oauth` mantém `OAuthSession`, `OAuthConfig`, `OAuthLogin` e `TokenStore`. `cuotai_core::login` expõe modos por provedor e um driver cancelável com eventos de prompt/progresso, entrada opcional de código e abertura do navegador. TUI e desktop usam esse driver; só o core escolhe o protocolo e coordena a gravação do cofre e do TOML. Eventos mostram URL de autorização e código de dispositivo quando necessários, nunca tokens ou callback com credenciais. `Monitor::with_oauth` compartilha a sessão/cofre entre consultas.

Os adapters ficam privados em `providers/`; para adicionar um provedor, acrescente o identificador em `Provider`, implemente consulta/normalização e conecte o despacho do backend. A TUI renderiza os relatórios genericamente.

## Verificação

```sh
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo build --workspace
```

Testes de contrato usam HTTP local controlado: paginação, centavos/moeda, overflow, schemas incompletos, filtros de cobrança, consumo bruto versus líquido, saldo versus gasto e credenciais ausentes. Eles não comprovam acesso às contas reais.

Smokes iniciais exercitaram o binário em PTY e JSON, sem credenciais reais. Posteriormente, a comparação 9router abaixo também exerceu credenciais exportadas para Groq, Ollama, Kimi Coding e MiniMax.

Wizard exercitado em PTY: seis etapas dos 11 provedores, referência de variável presente sem revelar valor, validação de campos, voltar/cancelar, adicionar/renomear e reabrir configuração, preservação das outras contas e parâmetros, mudança de provedor, conflito externo e falha de escrita com nova tentativa. A suíte inclui testes isolados de persistência, permissões e links simbólicos.

Smoke de rede: OpenAI API, Anthropic API, Codex, Claude, Cursor, Copilot e Windsurf retornaram HTTP 401 com credenciais deliberadamente inválidas. A saída não incluiu o token nem métricas inventadas. Nenhuma conta cloud real foi consultada.

OAuth: 79 testes passaram, incluindo callback/CSRF, negação, polling/slow_down/expiração, rotação de refresh token e isolamento por app/provedor. Smoke no macOS confirmou Keychain real (gravar/ler/apagar), sessão persistida após recriar o backend, callback loopback com PKCE e negação, TUI com abertura da URL Google/cancelamento, Microsoft rejeitando app não registrado e AWS CLI rejeitando perfil SSO ausente. **Não houve autorização completa com conta de produção nem validação de consumo pós-login**, pois não foram fornecidos client IDs registrados/consentimento. Fixtures locais de sucesso não substituem essa verificação.

Catálogo ampliado: **132 testes** passaram e o Clippy terminou sem avisos. Smoke do binário real em PTY confirmou os 15 defaults de variáveis, navegação dos 30 provedores, aviso de integração indisponível e cancelamento sem gravação. O exemplo TOML completo foi carregado. Com tokens deliberadamente inválidos, Kimi/Mistral/Perplexity Computer/Cline retornaram HTTP 401; MiniMax saldo e Grok retornaram 400; MiniMax Token Plan retornou resposta incompatível. Nenhum resultado falso de consumo ou token apareceu na saída. Isso valida caminhos de erro, **não consumo autenticado com suas chaves**.

Comparação 9router: **191 testes** passaram após portar os handlers. O código JavaScript original foi executado em harness com transporte real, em paralelo lógico com as mesmas consultas do Cuotai: **Ollama**, **Kimi Coding** e **MiniMax** retornaram HTTP 200 e as mesmas quotas (diferenças apenas de precisão temporal/instante da coleta). **Groq** retornou HTTP 200 sem cabeçalhos de rate limit em ambos. Corrigida divergência real: `/api/me` Ollama exige `Content-Length: 0`; sem ele, HTTP 411 impedia metadados e renovação mensal estimada. O teste de regressão cobre esse comportamento.

Wizard real exercitado para Kimi Coding, Gemini CLI, Copilot pessoal, Kiro, CodeBuddy, Zed e cookie/região Xiaomi; salvar e reabrir manteve os modos e referências. OpenCode Go, Qoder e provedores de sessão sem credenciais exportadas foram validados com fixtures do 9router/HTTP local, **não em contas reais**. Nenhuma chave foi exibida ou gravada no repositório.

Login nativo na TUI: **248 testes** passaram, incluindo estados de device flow, PKCE, criptografia RSA/X25519/AES-GCM, callbacks inválidos, expiração/rotação, preservação de metadados e isolamento de opções. Clippy terminou sem avisos. Smoke em terminal real percorreu os 12 novos logins nativos e iniciou Codex, Copilot, Kimi, Zed e CodeBuddy, cancelando sem persistência. Chamadas reais também receberam desafios GitHub/Kimi/Kiro/CodeBuddy e URLs Qoder/Zed. Um callback Cline **sintético** exercitou autorização → wizard → Keychain → TOML e leitura por nova sessão; a entrada de teste foi removida. Uma ferramenta de limpeza com outro binário recebeu negação do Keychain; a remoção foi concluída com `security delete-generic-password` direcionado apenas à entrada sintética. **Não foi concluído consentimento em conta real nesta verificação**, nem geração de tokens de IA. Concluir login e acessar a quota real ainda depende da autorização/assinatura da conta no provedor.

Última atualização (Cursor pessoal, MiMo manual, cartões e Qwen OAuth): **326 testes permanentes passaram**, Clippy sem avisos e workspace compilado. A TUI real foi exercitada com SQLite sintético Cursor (confirmação, caminho com espaços, leitura sem alteração e cancelamento), além de Keychain real com token fictício (salvar/reabrir/remover). Código MiMo criptografado sintético foi colado com Base64URL sem padding e quebras de linha, rejeitado quando inválido e aceito na mesma sessão, sempre mascarado. Painel em terminal real mostrou disponíveis → esgotadas → sem dados, toggle `u` e detalhes completos; nenhuma barra ilimitada foi inventada para custos sem limite. Qwen emitiu desafio OAuth real pela TUI e foi cancelado sem salvar; não houve consentimento completo nem confirmação de acesso ao serviço descontinuado.

Atualização Alibaba/Antigravity: **395 testes passaram**, incluindo respostas HTTP controladas de quota e OAuth; Clippy sem avisos e build concluído. Na TUI real, os três cadastros Alibaba foram salvos com referências de variáveis, e o login Antigravity abriu o fluxo Google e foi cancelado sem persistência. Não foi concluído consentimento Google nem consultada quota de uma conta Antigravity real; nenhuma credencial Antigravity estava disponível no ambiente. Faturamento Alibaba foi verificado com respostas controladas, não com AccessKey real.

Atualização Kiro/MiMo/mouse: **431 testes passaram**, Clippy sem avisos e build concluído. Smoke em terminal real verificou captura/liberação de mouse, clique em cartões/rodapé, scroll, retorno por botão direito e navegação/cancelamento no wizard. O login Kiro obteve desafio real AWS e foi cancelado sem salvar; quotas Kiro foram verificadas com respostas HTTP controladas, não com conta real autorizada. MiMo foi exercitado editando sessão OAuth salva sem abrir Keychain nem repetir código; consulta real com cookie fictício foi rejeitada e saldo/plano foram testados com respostas controladas. Não foi fornecido cookie MiMo válido para validar consumo real.

## Referências

- [OpenAI: custos e uso da organização](https://developers.openai.com/api/reference/resources/admin/subresources/organization)
- [Anthropic: Usage and Cost API](https://platform.claude.com/docs/en/manage-claude/usage-cost-api)
- [Codex: cliente de consulta de limites](https://github.com/openai/codex/blob/main/codex-rs/backend-client/src/client/rate_limit_resets.rs)
- [Claude: evidência comunitária do contrato](https://github.com/loocor/codmate/blob/main/services/ClaudeUsageAPIClient.swift)
- [Google: métricas de quota Service Runtime](https://cloud.google.com/monitoring/api/metrics_gcp_p_z)
- [AWS: get-cost-and-usage](https://docs.aws.amazon.com/cli/latest/reference/ce/get-cost-and-usage.html)
- [Azure: Cost Management Query](https://learn.microsoft.com/en-us/rest/api/cost-management/query/usage?view=rest-cost-management-2025-03-01)
- [Azure: atribuição de custos por recurso](https://learn.microsoft.com/en-us/azure/foundry/concepts/manage-costs)
- [Cursor: Admin API](https://cursor.com/docs/account/teams/admin-api)
- [GitHub: billing usage](https://docs.github.com/en/rest/billing/usage)
- [Windsurf: créditos da equipe](https://docs.windsurf.com/windsurf/accounts/api-reference/get-team-credit-balance)
- [Windsurf: analytics e unidades](https://docs.windsurf.com/windsurf/accounts/api-reference/user-page-analytics)
- [Google OAuth para apps Desktop](https://developers.google.com/identity/protocols/oauth2/native-app)
- [Microsoft OAuth device code](https://learn.microsoft.com/en-us/entra/identity-platform/v2-oauth2-device-code)
- [AWS CLI SSO login](https://docs.aws.amazon.com/cli/latest/reference/sso/login.html)
- [GitHub: autenticação para relatórios de consumo](https://docs.github.com/en/billing/tutorials/automate-usage-reporting)
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
- [Qwen via DashScope](https://www.alibabacloud.com/help/en/model-studio/qwen-api-via-dashscope)
- [Xiaomi MiMo: documentação](https://mimo.mi.com/llms-full.txt)
- [OpenCode Go](https://opencode.ai/go/) e [Qoder: uso](https://docs.qoder.com/cli/usage.md)
- [NVIDIA NIM](https://docs.api.nvidia.com/nim/reference/llm-apis) e [Poolside API](https://docs.poolside.ai/api/overview.md)
- [Kiro: faturamento](https://kiro.dev/docs/billing.md), [CodeBuddy: uso](https://www.codebuddy.cn/docs/ide/Account/usage.md) e [Zed: planos](https://zed.dev/docs/account/plans-and-pricing.md)
- [9router: despacho de consultas](https://github.com/decolua/9router/blob/f01fb909e37189008080632ddaf404f096345cde/open-sse/services/usage.js)
- [9router: OpenCode Go](https://github.com/decolua/9router/blob/f01fb909e37189008080632ddaf404f096345cde/open-sse/services/usage/opencode-go.js), [Groq](https://github.com/decolua/9router/blob/f01fb909e37189008080632ddaf404f096345cde/open-sse/services/usage/groq.js), [Kimi](https://github.com/decolua/9router/blob/f01fb909e37189008080632ddaf404f096345cde/open-sse/services/usage/kimi.js)
- [9router: Ollama e Qoder](https://github.com/decolua/9router/blob/f01fb909e37189008080632ddaf404f096345cde/open-sse/services/usage/misc.js), [MiniMax](https://github.com/decolua/9router/blob/f01fb909e37189008080632ddaf404f096345cde/open-sse/services/usage/minimax.js), [Zed](https://github.com/decolua/9router/blob/f01fb909e37189008080632ddaf404f096345cde/open-sse/services/usage/zed.js)
- [9router: fluxos de autenticação](https://github.com/decolua/9router/tree/f01fb909e37189008080632ddaf404f096345cde/src/lib/oauth/providers)
- [9router: login nativo Zed](https://github.com/decolua/9router/blob/f01fb909e37189008080632ddaf404f096345cde/open-sse/shared/zedAuth.js)
- [9router: callback Xiaomi](https://github.com/decolua/9router/blob/f01fb909e37189008080632ddaf404f096345cde/src/lib/oauth/providers/xiaomi-mimo.js)
- [Cursor pessoal: contrato de sessão e consumo no CodexBar](https://github.com/steipete/CodexBar/blob/main/Sources/CodexBarCore/Providers/Cursor/CursorStatusProbe.swift)
- [Qwen Code: implementação de device OAuth](https://github.com/QwenLM/qwen-code/blob/81582ca19d0772afdd06ace9cad4845c207001f8/packages/core/src/qwen/qwenOAuth2.ts)
- [9router: quotas Google/Antigravity](https://github.com/decolua/9router/blob/f01fb909e37189008080632ddaf404f096345cde/open-sse/services/usage/google.js)
- [9router: resumo de sessão e semana Antigravity](https://github.com/decolua/9router/blob/f01fb909e37189008080632ddaf404f096345cde/open-sse/services/usage/antigravity-weekly.js)

- [Kimchi: contrato oficial de créditos e orçamentos](https://github.com/getkimchi/kimchi/blob/36cc0406339a50f5ced984266d0306fb8a150408/src/extensions/billing/status.ts)
- [Coding Plan: referência comunitária do RPC](https://github.com/steipete/CodexBar/blob/main/Sources/CodexBarCore/Providers/Alibaba/AlibabaCodingPlanUsageFetcher.swift)
- [Cursor: identidade da sessão importada](https://github.com/steipete/CodexBar/blob/main/Sources/CodexBarCore/Providers/Cursor/CursorAppAuth.swift)

Verificação da correção Coding Plan/Cursor e integração Kimchi/Ollama: **406 testes passaram**, Clippy sem avisos e build concluído. Consulta real com `QWEN_API_KEY` exigiu sessão do console na região internacional; o modo cookie foi exercitado com servidor HTTP controlado e o campo `session_env` na TUI. Ollama retornou quota mensal real com a API key exportada. Kimchi retornou HTTP 401 para chave fictícia; saldo/orçamentos foram verificados com respostas controladas, sem chave real disponível. Importação Cursor com SQLite sintético preservou `user_MixedCase_42`, sem alterar o banco nem persistir token; não foi acessada a sessão real do usuário. Poolside permanece bloqueado conforme descrito acima.
- [Kiro: contrato getUsageLimits no 9router](https://github.com/decolua/9router/blob/f01fb909e37189008080632ddaf404f096345cde/open-sse/services/usage/kiro.js)
- [Kiro: créditos e excedentes no CodexBar](https://github.com/steipete/CodexBar/blob/main/Sources/CodexBarCore/Providers/Kiro/KiroUsageLimitsAPI.swift)
- [MiMo: saldo e Token Plan no CodexBar](https://github.com/steipete/CodexBar/blob/main/Sources/CodexBarCore/Providers/MiMo/MiMoUsageFetcher.swift)
