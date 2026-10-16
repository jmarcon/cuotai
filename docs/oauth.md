# Login OAuth Integrado, Sessões e Cofre de Credenciais

O Cuotai suporta fluxos de autenticação nativa (OAuth no navegador, Device Code, AWS SSO e importação assistida) tanto na TUI quanto no aplicativo Desktop, armazenando tokens no cofre nativo do sistema operacional (`cuotai.oauth`) e mantendo o `cuotai.toml` livre de segredos.

## Visão geral por provedor

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
| Zed | Navegador com chave RSA efêmera | Protocolo nativo, não OAuth padrão; `user_id` e `system_id` preenchidos automaticamente |
| Xiaomi MiMo | Navegador com callback X25519/AES-GCM **ou código para colar** | Cole o código da tentativa atual no campo oculto da TUI ou no campo do desktop. Obtém chave de inferência; quota semanal pode exigir cookie separado |
| Cursor | Importação explícita da sessão local em SQLite | Faça login no Cursor, escolha Tab → importação e confirme. Nenhuma leitura do token acontece só por abrir o wizard |
| Qwen Code | Device OAuth com PKCE + renovação | Fluxo legado descontinuado pelo provedor; a TUI mostra o aviso. O endpoint ainda emitiu desafio no smoke, mas isso não comprova concessão de acesso ou quota |
| Antigravity | OAuth Google no navegador + renovação | Sessão do IDE/Code Assist, distinta de uma API key Gemini; tokens no cofre, sem provisionar projeto |
| OpenAI/Anthropic API, Windsurf, Ollama e OpenCode Go | Mantidos os métodos por chave/variável | Não há botão que simule um login inexistente |

## Executar login nativo (9router) pela TUI ou Desktop

1. **Na TUI:** Inicie `cargo run -p cuotai-tui`, pressione **a** para adicionar ou **e** para editar uma conta. Escolha o provedor e o nome da conta e, na etapa **Credencial**, pressione **Tab**.
   **No Desktop:** Clique em **Adicionar conta** ou **Editar**, escolha **Login no navegador** e clique em **Entrar**.
2. Para login nativo, a interface não pede `client_id`. Kiro oferece método/região/start URL; Qoder, CodeBuddy e Xiaomi oferecem região.
3. Ao iniciar, o Cuotai abre o navegador e, quando necessário, exibe a URL e o código de dispositivo para copiar. Autorize na página do provedor.
4. Ao receber a autorização, os parâmetros de quota são preenchidos automaticamente. Salve para gravar os tokens no cofre do SO e a configuração no arquivo TOML.
5. Para renovar uma autorização revogada ou expirada, edite a conta e use **F2** (TUI) ou **Refazer login / Reautenticar** (Desktop).

O login nativo seleciona automaticamente o produto correto (Copilot quota pessoal, Kimi Coding, Gemini CLI, Grok CLI e Qoder bearer), sem substituir essas sessões por chaves comerciais de geração. IDs Zed, perfil Kiro, device ID Kimi e account ID Codex são derivados do retorno quando disponíveis; metadados de quota não secretos são preenchidos e credenciais de refresh ficam somente no cofre.

Os fluxos compatíveis usam os identificadores de clientes nativos e protocolos presentes na revisão de referência do 9router. Podem ser recusados, alterados ou restringidos pelo provedor. Em particular, a política publicada da Anthropic restringe login Claude.ai em aplicativos terceiros; implementar o protocolo **não constitui autorização ou garantia de aceitação**. Senhas e consentimento são tratados no navegador do provedor, nunca pelo Cuotai.

**Separação importante:** `gemini_api` e `vertex_ai` (Cloud Monitoring), além de Azure, conservam os fluxos com aplicativo próprio descritos abaixo. `gemini_ai` em login nativo acompanha Gemini CLI/Code Assist.

## Fluxos específicos por provedor

### Cursor pessoal: importar pelo wizard

1. Faça login normalmente no aplicativo **Cursor**.
2. No Cuotai, adicione/edite Cursor e selecione a importação local (Tab na TUI ou modo de autenticação no Desktop).
3. Deixe o caminho vazio para a instalação padrão ou informe um `state.vscdb` específico (aceita espaços no caminho, por exemplo Cursor Insiders).
4. Confirme a leitura e importação da sessão Cursor. A leitura é somente leitura, em uma transação SQLite, das chaves `cursorAuth/accessToken`/`cursorAuth/token` e machine ID; não modifica o banco e não lê todas as preferências/segredos.
5. Salve a conta. O token fica no Keychain; o TOML guarda apenas a referência da sessão, caminho opcional e metadados públicos. A importação não autentica de novo nem renova o token do Cursor; token expirado requer login no editor e reimportação.

Sessão pessoal usa `/api/usage-summary` em `cursor.com`, com cookie derivado do token da conta. **Nunca é enviada ao Admin API `/teams/spend`**. O modo por variável `CURSOR_ADMIN_API_KEY` permanece para Teams/Enterprise.

### Xiaomi MiMo: código manual

Quando a página exibir **Authentication Code / Paste this into MiMo Code**, volte à tela de login pendente do Cuotai, cole o código no campo correspondente e envie. A colagem aceita Base64URL sem padding e quebras de linha. Código inválido ou de outra sessão mostra erro sem encerrar a tentativa; a expiração original não é estendida.

O código é criptografado para a chave efêmera dessa tentativa. O callback automático continua aceito em paralelo. O código digitado nunca é persistido; só o token decodificado é gravado no cofre após a confirmação final.

**Autorização por código não é sessão de quota:** O código entrega uma chave `sk-` de inferência, não cookies do console. Para saldo e Token Plan mensal, configure `params.platform_session_env` com o **nome** da variável contendo o Cookie de `platform.xiaomimimo.com` (`api-platform_serviceToken` e `userId`). A quota semanal do MiMo Desktop permanece separada em `session_env`/`region`; se ambos os campos forem preenchidos, o modo Desktop tem precedência.

### Qwen Code OAuth

Selecione **Qwen → Login OAuth**. O fluxo usa o device grant com PKCE do cliente oficial Qwen Code (`chat.qwen.ai/api/v1/oauth2/device/code` e `/token`, escopo `openid profile email model.completion`). Access/refresh tokens ficam no cofre, separados de `QWEN_API_KEY` do DashScope.

**Limitação do provedor:** a [documentação oficial](https://github.com/QwenLM/qwen-code/blob/81582ca19d0772afdd06ace9cad4845c207001f8/docs/users/configuration/auth.md) informa que o OAuth gratuito foi descontinuado em 15/04/2026. A interface disponibiliza a tentativa com aviso; não foi encontrado endpoint de quota Qwen Code e login bem-sucedido não é convertido em falsa medição de consumo.

### Antigravity

Autorize a sessão Google no navegador pelo modo OAuth. O cadastro não usa a API key Gemini. As consultas são somente leitura, sem geração nem `onboardUser`. Percentuais e renovação aparecem nos cartões e nos detalhes; a escala 1.000 usada pelo 9router não é apresentada como quantidade de requisições. Quotas ausentes continuam desconhecidas e falhas parciais ficam nas observações.

### Google Cloud Monitoring (Gemini API / Vertex AI): preparação única

1. Em [Google Cloud](https://console.cloud.google.com/apis/credentials), habilite **Cloud Monitoring API** no projeto consultado e configure a tela de consentimento OAuth. Em aplicativo externo em teste, inclua sua conta entre os usuários de teste.
2. Crie um **OAuth client ID → Desktop app** para o Cuotai. Não use client ID de outro produto.
3. No Cuotai, escolha Gemini API ou Vertex AI, selecione o modo OAuth e informe o `client_id`.
4. Se o registro Google exigir `client_secret`, exporte-o em uma variável e informe **o nome dessa variável** no campo seguinte. Segredos não entram no TOML.
5. Inicie o login. O navegador solicita `https://www.googleapis.com/auth/monitoring.read`; o retorno usa uma porta local livre.
6. Preencha `project_id`, revise e grave. A identidade Google precisa de `monitoring.timeSeries.list` (ex.: Monitoring Viewer no projeto).

### Azure OpenAI: preparação única

1. Em [Microsoft Entra](https://portal.azure.com), registre seu aplicativo. Em **Authentication → Advanced settings**, habilite **Allow public client flows**.
2. Configure a permissão delegada **Azure Service Management → user_impersonation** e conceda o consentimento exigido pelo tenant. A conta autorizadora precisa de **Cost Management Reader** no recurso/assinatura consultado.
3. No Cuotai, escolha Azure OpenAI no modo OAuth e informe o **Application (client) ID** e o tenant (GUID/domínio; vazio usa `organizations`). Não é necessário client secret para esse public client.
4. Confirme o login e digite o código exibido na página Microsoft aberta pelo Cuotai (`https://management.azure.com/user_impersonation` e `offline_access`).
5. Informe `subscription_id` e `resource_id`, revise e grave.

### AWS Bedrock: perfil SSO

Prepare uma vez com `aws configure sso`. Ao configurar AWS Bedrock, informe o perfil e acione o login SSO (`F2` na TUI ou modo **AWS SSO** no Desktop). O Cuotai mostra a URL e o código fornecidos por `aws sso login --profile ... --use-device-code --no-browser`. A própria AWS CLI mantém e usa seu cache SSO; o Cuotai não copia esses tokens para o Keychain. Login SSO não dispensa `ce:GetCostAndUsage` e não elimina a cobrança das consultas de Cost Explorer.

## Persistência, cancelamento e renovação

- **Cofre do sistema:** Por padrão, os logins integrados armazenam access token, refresh token e contexto secreto no cofre nativo, serviço **`cuotai.oauth`** (Keychain no macOS, Windows Credential Manager e Linux Secret Service). Sem acesso autorizado, há erro explícito; nunca há fallback automático para SQLite.
- **Formato no TOML:** O TOML guarda `accounts.oauth.credential_id`, client ID/tenant/referência de client secret quando aplicável, e `options` não secretas (`region`, `auth_method`, `start_url`). `oauth` e `credential_env` não podem coexistir na mesma conta.
- **Commit estagiado:** Tokens de login novo ficam em memória até o salvamento final da conta. O core grava primeiro no cofre e depois no TOML; se o arquivo falhar, restaura a credencial anterior ou remove a nova.
- **Renovação automática:** Antes de consultar, tokens com menos de 60 segundos de validade são renovados quando existe refresh token e o protocolo o suporta (Claude, Codex, Google, Kimi, Kiro, Grok, CodeBuddy, Cline e GitHub quando aplicável). Uma rotação já iniciada termina sua persistência mesmo que a consulta seja cancelada, evitando perder o refresh token novo.

## Credenciais SQLite — somente desenvolvimento

Para evitar os prompts do Keychain a cada recompilação durante o desenvolvimento local:

```sh
cargo run -p cuotai-tui -- --dev-credentials-sqlite ./cuotai-dev.sqlite3
```

O banco guarda **tokens sem criptografia**, incluindo refresh tokens e contexto secreto. Use apenas em ambiente local confiável. No Unix, arquivos novos têm modo `0600`; arquivos existentes acessíveis a grupo/outros e links simbólicos são recusados. Builds `release` recusam a opção antes de criar ou abrir o banco (a implementação SQLite nem é compilada sem `debug_assertions`).
