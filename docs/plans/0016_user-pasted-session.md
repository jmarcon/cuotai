# PRD 0016 — Sessão colada pelo usuário

| Campo | Valor |
|---|---|
| ID | 0016 |
| Status | Proposto |
| Responsável | `cuotai-core`, TUI e Desktop |
| Consumidores | `crates/cuotai-tui` (`wizard.rs`), `apps/desktop` (`account-dialog.js`) |
| Depende de | PRD 0005 (RF1: nível `UserSession`) |
| Data | 2026-10-19 |

## 1. Contexto

`docs/providers.md` estabelece que nenhum cookie, Keychain ou arquivo de credenciais de outro app é extraído automaticamente. Ainda assim, alguns provedores só expõem o consumo atrás de uma sessão web, e o Cuotai já aceita cookies exportados pelo usuário em casos pontuais:
- Xiaomi MIMO: `platform_session_env` (saldo e Token Plan do console) e `session_env`/`region` (quota semanal do Desktop);
- Alibaba Token Plan: `params.session_env = "QWEN_CLOUD_COOKIE"`;
- Alibaba Coding Plan: `params.session_env` com cookie explícito quando o gateway exige login;
- Cursor pessoal: importação **explícita** da sessão SQLite local.

Hoje cada caso tem seu próprio campo, texto de ajuda e tratamento de validade. O usuário precisa exportar o cookie por conta própria, colocá-lo numa variável de ambiente e referenciá-la, o que é frágil e deixa o valor em perfis de shell.

Na análise de mercado de 2026-10-19, o CodexBar cobre dezenas de provedores por cookie (cerca de 43 de 106 entradas), com modos `auto` (importa do navegador), `manual` (cabeçalho colado) e `off`; isso exige Acesso Total ao Disco (Safari) e acesso ao Keychain (Chromium). O WhatsMyUsage adota o caminho oposto: o usuário cola os cookies numa janela do app, **só as chaves de sessão** vão ao Keychain e o resto da colagem é descartado; duas sessões do mesmo provedor são duas linhas.

## 2. Problema

- Provedores que só têm consulta web ficam inacessíveis sem importação automática de cookies, que contradiz a política do projeto e abre uma superfície de permissões ampla.
- O mecanismo manual atual é inconsistente entre provedores e deixa o segredo em variáveis de ambiente.
- Uma sessão colada é um login completo: guardá-la sem cuidado é risco real.

## 3. Objetivos

- Oferecer uma forma única, explícita e opt-in de o usuário fornecer uma sessão web para os provedores que não têm outra via, sem varrer navegadores.
- Guardar o segredo no cofre do sistema, nunca no TOML, em logs, snapshots ou eventos.
- Tornar o risco claro (nível `UserSession` do PRD 0005) e detectar expiração.

## 4. Não-objetivos

- Importar cookies automaticamente de navegadores, ler o Keychain de outros apps ou pedir Acesso Total ao Disco.
- Automatizar o login web, resolver CAPTCHA ou usar senhas.
- Cobrir provedores sem evidência de que a consulta por sessão é viável e permitida; cada um entra por PRD ou revisão própria.

## 5. Requisitos funcionais

### RF1 — Tipo de credencial `UserSession` no core (prioridade alta; pré-requisito)

- **RF1.1** Acrescentar a `account_edit` e a `access` (PRD 0005) a classe de credencial `UserSession`. O descritor do provedor declara quais cookies/cabeçalhos são necessários (lista de nomes aceitos, por exemplo as chaves de sessão), o host de destino e como a sessão é enviada. A interface não contém regras por provedor.
- **RF1.2** Referência no TOML por **identificador do cofre** (`credential_ref`, a mesma convenção dos tokens OAuth), nunca pelo valor. Contas antigas com `session_env` continuam funcionando; o assistente oferece "migrar para o cofre" com consentimento explícito.
- **RF1.3** Armazenamento em entrada do cofre pertencente ao Cuotai (Keychain, Credential Manager ou Secret Service), com o mesmo mecanismo de `TokenStore`. No modo de desenvolvimento com SQLite em texto claro, o aviso existente continua valendo; builds de release o recusam.
- **Critério de aceite:** testes de armazenamento e leitura via `Credentials`, migração de `session_env` para o cofre, ausência do valor em TOML, `Debug`, logs e `Report`.

### RF2 — Colagem guiada e redução ao mínimo (prioridade alta)

- **RF2.1** Passo "Colar sessão" no assistente de conta: instruções localizadas por provedor ("onde copiar", sempre genéricas e sem capturas de contas reais), campo de colagem com máscara, e botão de teste que faz uma consulta somente-leitura.
- **RF2.2** O core aceita colagens em formatos comuns (linha `Cookie:`, lista `nome=valor; ...`, tabela copiada do inspetor, `cURL` copiado) e **extrai somente os nomes permitidos** pelo descritor, descartando todo o resto antes de gravar. Cookies cujo nome não está na lista nunca chegam ao cofre. A normalização reconstrói cookies fragmentados em partes numeradas (por exemplo, `.0`, `.1`) na ordem correta, para não gerar sessão silenciosamente inválida.
- **RF2.3** Nada do conteúdo colado é registrado: o campo é limpo ao fechar, e mensagens de erro descrevem o problema ("faltou o cookie X") sem repetir valores.
- **Critério de aceite:** testes de tabela para cada formato, fragmentos fora de ordem, nomes ausentes, nomes extras descartados e verificação de que nenhum valor aparece em erros ou logs.

### RF3 — Validade, expiração e uso (prioridade alta)

- **RF3.1** Uma consulta com sessão rejeitada (401/403 ou redirecionamento para login) produz `AuthenticationError` com mensagem específica "sessão expirada ou revogada; cole uma nova", sem expor corpo ou URL.
- **RF3.2** Quando o cookie carrega data de expiração conhecida, o core a lê (apenas a data, nunca o valor) e informa "expira em N dias" no cartão; o aviso de expiração próxima (menos de 3 dias) integra os alertas (PRD 0008) como evento próprio quando existir.
- **RF3.3** A sessão só é enviada ao host declarado no descritor, por HTTPS, e nunca é reutilizada em outro provedor ou conta. Redirecionamentos para outro host não carregam a sessão.
- **RF3.4** Atualizar a sessão de uma conta existente substitui o valor na mesma entrada do cofre (reautenticação), sem duplicar contas.
- **Critério de aceite:** testes com `wiremock`: 401, redirecionamento para host externo (sessão não enviada), expiração conhecida e desconhecida e substituição da sessão.

### RF4 — Transparência e aviso de risco (prioridade média)

- **RF4.1** O cartão e o passo de colagem mostram o nível `UserSession` e um aviso curto e localizado: "Esta é uma sessão de login completa. O Cuotai a guarda no cofre do sistema e a usa somente para consultar este serviço."
- **RF4.2** Opção "Esquecer esta sessão" na conta, que remove a entrada do cofre.
- **RF4.3** `docs/providers.md` e `docs/oauth.md` descrevem o fluxo, o que é guardado, o que é descartado e os riscos (termos de uso do provedor, revogação, vazamento).
- **Critério de aceite:** smoke na TUI e no desktop em três idiomas; remoção apaga a entrada do cofre.

### RF5 — Primeiros provedores (prioridade média)

- **RF5.1** Migrar os casos existentes (Xiaomi MIMO, Alibaba Token Plan, Alibaba Coding Plan) para o descritor `UserSession`, mantendo `session_env` como alternativa.
- **RF5.2** Novos provedores só entram com evidência (documentação do fabricante ou contrato verificado no 9router/cliente comunitário) de que a consulta é somente-leitura e de que a sessão é suficiente, e passam pelo PRD 0005.
- **Critério de aceite:** os testes atuais dos adaptadores migrados continuam passando; a matriz de `docs/providers.md` reflete a mudança.

## 6. Requisitos não funcionais

- **Segurança:** sessão é segredo de alto valor. Nunca em TOML, snapshots, eventos, logs, exportações (PRD 0012), hooks (PRD 0013) ou mensagens de erro; zeragem do buffer quando possível; sem telemetria.
- **Privacidade e termos:** o uso depende dos termos do provedor. A documentação diz isso e o nível de acesso expõe o risco.
- **Compatibilidade:** contas e TOML atuais não mudam sem consentimento.

## 7. Ordem de execução e verificação

1. **RF1:** credencial, cofre e migração, com testes.
2. **RF2 e RF3:** colagem, extração mínima, expiração e redirecionamentos.
3. **RF4:** avisos e remoção.
4. **RF5:** migração dos três provedores existentes.

Em cada etapa: `cargo fmt`, `clippy -D warnings`, testes do workspace e do desktop e smoke da interface alterada.

O requisito só está entregue quando o usuário cola uma sessão uma única vez, só os cookies permitidos chegam ao cofre, o valor nunca aparece em TOML, logs ou erros, a sessão expirada é identificada com uma mensagem clara, e esquecê-la remove a entrada do cofre.
