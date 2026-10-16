# PRD 0001 — Suporte do core a frontends gráficos

| Campo | Valor |
|---|---|
| ID | 0001 |
| Status | Proposto |
| Responsável | Backend (`cuotai-core`) |
| Consumidores | `apps/desktop` (Tauri), `cuotai-tui` |
| Data | 2026-10-01 |

## 1. Contexto

O `cuotai-core` já concentra as regras de edição de contas (`account_edit`), a persistência do TOML (`config_file`), a classificação de métricas (`metrics`) e o agendamento (`refresh`). O app desktop (`apps/desktop`) passou a consumir essas APIs e funciona para listar, adicionar, editar e remover contas.

Ao integrar, apareceram lacunas que só o core pode resolver. Hoje o desktop contorna algumas delas com código próprio em `apps/desktop/src-tauri/src/backend.rs`, o que duplica política e custa dinheiro em provedores cobrados por consulta.

## 2. Problema

1. **Toda edição refaz a consulta de todas as contas.** `RefreshCycle::new` sempre cria um ciclo sem snapshot e o `Monitor` guarda a configuração imutável. Salvar uma conta dispara uma rodada completa, inclusive em provedores pagos (AWS Cost Explorer).
2. **O login OAuth só existe na TUI.** A orquestração está em `crates/cuotai-tui/src/auth_ui.rs` (`LoginTask::oauth/aws/bailian_console/store`, `paste`, `open_browser`). O core expõe as primitivas (`OAuthSession::begin/finish/finish_with_codes/save_login/logout`), mas nenhum frontend gráfico consegue conduzir o fluxo.
3. **Remover conta deixa credencial órfã no cofre.** `Config::without_account` só altera o TOML; `OAuthSession::logout` é um passo separado.
4. **Descoberta e conflito de configuração não servem a apps gráficos.** `ConfigFile::load` tenta `./cuotai.toml` do diretório atual, que em um app gráfico é imprevisível. Erros de `save` e `build_account` são `String`, então a UI não distingue conflito, validação e I/O.
5. **Política de provider duplicada no frontend.** O desktop fixou `AwsBedrock → perfil` e `AlibabaTokenPlan → sem credencial`.
6. **Agendamento incompleto.** `RefreshCycle` calcula prazos, mas cada frontend reescreve o laço, o controle de conta em andamento e a proteção contra resultados de configuração antiga. Após `RateLimited` ou erro de autenticação, o reagendamento usa o mesmo intervalo. O mínimo de 30 s não distingue provedores cobrados por consulta.
7. **DTOs repetidos.** `MetricState` e `AccountClass` não são `Serialize`, e o desktop os mapeia manualmente.

## 3. Objetivos

- Permitir que qualquer frontend (TUI, desktop, futuros) edite contas, faça login e agende atualizações usando **somente** APIs do core, sem política própria.
- Evitar consultas desnecessárias a provedores após edições.
- Não deixar segredos órfãos nem expor segredos em eventos, erros ou snapshots.

## 4. Não-objetivos

- Telas, textos de UI ou logos (responsabilidade dos frontends).
- Alterar a semântica dos fetchers de provider ou do formato do `cuotai.toml`.
- Criar um daemon ou um serviço externo.
- Escrita de segredos no TOML (continua proibida).

## 5. Requisitos funcionais

### RF1 — Atualização parcial após edição (prioridade alta)

- **RF1.1** `RefreshCycle` oferece uma operação para aplicar uma edição, por exemplo `apply_edit(&mut self, new_config: &Config, edit: Edit, now: Instant) -> Vec<usize>`, com `Edit::Upsert(i)` e `Edit::Remove(i)`.
  - Preserva snapshot e prazos das contas não afetadas.
  - Reindexa snapshot e prazos após uma remoção.
  - Devolve os índices que precisam de nova consulta (a conta editada ou nova).
- **RF1.2** Um relatório pode estar pendente. Define-se um estado explícito (por exemplo `Status::Pending` ou `Option<Report>` no ciclo), para que a conta editada não exiba dados da configuração anterior.
- **RF1.3** `Monitor::with_config(&self, config) -> Result<Monitor, ConfigError>` reaproveita o cliente HTTP e a `OAuthSession`.
- **Critério de aceite:** salvar uma conta com N contas configuradas gera exatamente 1 consulta; remover uma conta gera 0. Teste com `wiremock` contando requisições.

**Contrato de edição:** o core deriva a diferença estrutural entre configuração anterior e nova (ou valida que ela corresponde exatamente à operação pedida), inclusive identidade e ordem das contas preservadas. Índices fornecidos pelo frontend não autorizam associar relatórios antigos a contas reordenadas. `Monitor::with_config` só reutiliza o cliente HTTP quando `timeout_seconds` permanece igual: o cliente reqwest atual é construído com esse timeout; quando mudar, criar cliente novo ou transferir integralmente o timeout para cada consulta antes de reutilizar.

### RF2 — Login OAuth sem UI (prioridade alta)

- **RF2.1** API de fluxo em `cuotai_core::oauth` que inicia o login e devolve um handle com:
  - eventos serializáveis: `Prompt { url, user_code, instrucoes }`, `AguardandoCodigo`, `Progresso(texto)`, `Concluido(ParametrosPublicos)`, `Erro(tipo)`;
  - `submit_code(texto)` (código colado manualmente) e `cancel()`.
- **RF2.2** Cobre todos os fluxos hoje presentes na TUI: OAuth no navegador e device code, importação do Cursor, AWS SSO (CLI) e console do Bailian.
- **RF2.3** `open_browser(url)` é movido para o core, com o mesmo comportamento da TUI.
- **RF2.4 (revisado)** Cofre do SO e TOML são armazenamentos independentes; não há commit atômico entre ambos. O core coordena preparação/validação, ordem de gravação, compensação segura e resultado tipado `PartialFailure { stage, config_saved, credential_saved, compensation }` quando uma etapa ou sua compensação falhar. Nunca apagar por compensação um token anterior substituído sem restaurá-lo com sucesso. A TUI apresenta o estado exato e opção de recuperação, sem inventar política. Cancelar antes do commit mantém o token apenas em memória. AWS/`bl` escrevem fora do cofre Cuotai e não prometem rollback.
- **RF2.5 (revisado)** O `user_code` público de device authorization é exibível ao usuário no evento de prompt. O código de autorização recebido no callback do navegador, tokens e URLs de redirecionamento com parâmetros de credencial nunca entram em eventos, erros ou logs. Não serializar indiscriminadamente o prompt.
- **RF2.6 (princípio)** A orquestração do login pertence ao core; os frontends só renderizam eventos e enviam entradas do usuário. A escolha do fluxo por provider (navegador com loopback, device code, importação do Cursor, código colado do MiMo, AWS SSO, console do Bailian) é feita no core. Nenhum frontend contém `if provider == ...` para decidir como autenticar.
- **RF2.7 (descoberta de modos)** O core expõe, por provider, os modos de autenticação aceitos (variável de ambiente, login nativo, perfil AWS) com seus descritores: opções não secretas do fluxo (método, região, URL), necessidade de registro próprio (`client_id`, tenant ou variável do client secret) e a razão quando o login não é oferecido. Hoje o desktop precisa combinar `supports_login`, `oauth_unsupported_reason`, `requires_client_registration` e `native_option_descriptors` por conta própria.
- **RF2.8 (reautenticação)** Reautenticar uma conta existente reaproveita o `credential_id` e sobrescreve o token, sem deixar credencial órfã. A decisão fica no core.
- **RF2.9 (abertura do navegador)** Se o navegador não puder ser aberto, o core emite um evento com a URL para o frontend exibir e copiar. Só URLs `https://` são abertas.
- **Critério adicional (RF2.6–RF2.9):** adicionar um fluxo novo no core o disponibiliza na TUI e no desktop sem alterar a lógica de login de nenhum deles.

- **Critério de aceite:** a TUI passa a usar a mesma API (sem lógica de login própria) e os testes de contrato OAuth existentes continuam passando.

### RF3 — Remoção coerente (prioridade média)

- **RF3.1 (revisado)** Operação única `remove_account(&mut ConfigFile, session, index, purge_credentials: bool)`: validar índice e referências, salvar TOML, então tentar purga se solicitada. Verificar a **chave efetiva do cofre** derivada de provider, configuração OAuth e `credential_id`; se outra conta restante a referenciar, manter o segredo. Nenhuma falha na gravação do TOML apaga token. Se a purga falhar após gravar, retornar resultado tipado `PartialFailure { stage, config_saved: true, credential_saved: true, compensation }` ou variante própria indicando **remoção salva, token mantido**; recuperação/repetição pertence ao core e não depende de índice que já foi removido.
- **Critério de aceite:** testes com `TokenStore` em memória para manter/apagar conforme opção, falha parcial e duas contas com a mesma chave efetiva preservando a credencial da sobrevivente.

### RF4 — Descoberta de configuração e erros tipados (prioridade média)

- **RF4.1** Modo de descoberta sem diretório atual: caminho explícito, depois diretório de configuração do sistema. Inclui criar o arquivo no primeiro uso.
- **RF4.2** `ConfigFile::save` e `build_account` retornam um enum de erro, por exemplo `EditError`, com ao menos `Validation { campo: Option<&'static str>, mensagem }`, `Conflict`, `Io` e `Unsupported`. As mensagens continuam sem ecoar valores digitados nem conteúdo do TOML.
- **RF4.3** `ConfigFile::reload()` e uma forma barata de saber se o arquivo mudou no disco, para a UI oferecer "recarregar" após `Conflict`.
- **Critério de aceite:** os testes atuais de conflito, symlink e permissões `0600` continuam passando; novo teste cobre `reload` após edição externa.

### RF5 — Política de credencial no core (prioridade média)

- **RF5.1** `account_edit::credential_kind(provider) -> CredentialKind` (`EnvVar`, `AwsProfile`, `None`) e indicação de quais modos o provider aceita (env, OAuth, ambos), eliminando o `match` do desktop.
- **Critério de aceite:** nenhum frontend contém `match` sobre `Provider` para decidir o formulário de credencial.

### RF6 — Driver de agenda e provedores cobrados (prioridade média)

- **RF6.1** `RefreshCycle` expõe `due_indices(now) -> Vec<usize>` e marcação de conta em andamento (`begin(index)` / `finish_account`), com proteção contra concluir uma conta de uma configuração antiga (geração).
- **RF6.2** Backoff exponencial limitado após `RateLimited` e `AuthenticationError`, respeitando um teto (por exemplo 1 h). Sucesso restaura o intervalo configurado.
- **RF6.3** Provedores cobrados por consulta são identificados (`Provider::is_metered()` ou equivalente). Para eles:
  - `Config::validate` aplica um mínimo de intervalo maior que 30 s;
  - o formulário recebe um aviso de custo vindo do descritor.
- **Critério de aceite:** testes de `RefreshCycle` para backoff, geração e mínimo de provedor cobrado.

**Decisão de execução RF6:** inicialmente apenas `AwsBedrock` é marcado cobrado por consulta, com intervalo efetivo mínimo de 300 s (o padrão atual); qualquer outro provider exige evidência de cobrança antes de entrar na lista. Para `RateLimited` e `AuthenticationError`, dobrar o intervalo de falha sucessiva com teto de 1 h e reiniciar após `Ready`; o usuário mantém atualização manual explícita. Registrar esse comportamento no formulário/README e testar também o fallback global.

### RF7 — Tipos serializáveis (prioridade baixa)

- **RF7.1** `MetricState` e `AccountClass` derivam `Serialize`, com representação estável (`snake_case`), documentada.

### RF8 — Mensagens do core localizáveis (prioridade média)

- **RF8.1** Textos que o core entrega à interface (rótulos e ajuda dos descritores de campos, avisos de custo, motivos de login indisponível, erros de edição, notas de relatório) saem hoje em português fixo, então o desktop não consegue traduzi-los (a UI já traduz todo o restante em pt-BR, en-US e es).
- **RF8.2** Cada texto passa a ter um identificador estável (`field.kiro.auth_method.label`, `edit_error.duplicate_name`, …) acompanhado de parâmetros, e o texto em português continua como valor padrão. Os frontends resolvem o identificador no seu catálogo e usam o padrão quando ele não existir.
- **Critério de aceite:** nenhum frontend precisa fazer correspondência por texto; adicionar um idioma novo é só acrescentar um catálogo.

### RF9 — Ordem das contas compartilhada (prioridade baixa)

- **RF9.1** Hoje a ordem escolhida pelo usuário no desktop é só uma preferência de exibição do próprio app (não muda o `cuotai.toml`), então a TUI e o desktop podem mostrar ordens diferentes. Para uma ordem única, o core oferece `Config::move_account(from, to)` e `RefreshCycle::apply_edit` com `Edit::Move { from, to }` (reindexando snapshot e prazos sem nova consulta).
- **Critério de aceite:** reordenar não gera nenhuma consulta e a nova ordem persiste no `cuotai.toml` preservando comentários.

### RF10 — Tipo da métrica no modelo (prioridade média)

- **RF10.1** `Metric` ganha um tipo explícito (`Quota`, `RateLimit`, `Balance`, …) preenchido pelos adaptadores. Hoje o rate limit só se identifica por texto livre (`period: "janela corrente do rate limit"`, rótulo "Rate limit"), e o desktop precisa reconhecer esses termos para não tratar uma janela de minutos como o reinício da cota (ordenação por reinício e % disponível).
- **RF10.2** `Metric` também informa a **janela** da cota (`Window5h`, `Weekly`, `Monthly`, `Other`), preenchida pelos adaptadores. A tela principal mostra as janelas de 5 horas e semanal lado a lado, e hoje o desktop as reconhece por texto livre (`label`/`period`: "Sessão (5 horas)", "M-series (5h)", "Coding Plan: 5h", "Semanal", "weekly (7d)"). Com o campo no core, essa correspondência por texto sai do shell.
- **Critério de aceite:** nenhum frontend compara texto de `label` ou `period`; os adaptadores existentes (Kimi, Groq, MiniMax…) classificam as métricas e testes de contrato cobrem cada tipo.

### RF11 — Aplicabilidade das opções por tipo de autenticação (prioridade média)

**Problema.** Alguns campos de provedor oferecem uma lista de métodos ou origens de credencial que não vale para todos os tipos de autenticação. O caso do Kiro: o campo `auth_method` (descritor de campo, usado com **variável de ambiente**) lista 7 valores (`builder-id`, `idc`, `google`, `github`, `import`, `api_key`, `external_idp`), enquanto o login nativo (opção do modo `oauth`) suporta só `builder-id` e `idc`. Hoje isso funciona porque são duas listas independentes, e o frontend apenas troca uma pela outra. Mas o core não declara **quais valores fazem sentido com uma variável de ambiente**: `builder-id` e `idc` emitem tokens de curta duração, que o Cuotai não renova a partir de uma variável, e o adaptador aceita os 7 sem distinção (`router_sessions.rs`: só `api_key` e `external_idp` mudam o cabeçalho). Resultado: o usuário pode escolher uma combinação sem sentido, e cada frontend teria de inventar a regra.

**Requisitos**

- **RF11.1 (metadado)** Cada opção de um campo de escolha (`FieldDescriptor` do tipo `Choice` e opções de login) declara em quais **tipos de autenticação** vale: `credential` (variável de ambiente ou perfil) e/ou os ids dos modos de login (`oauth`, `aws_sso`, `bailian_console`). Opção sem declaração vale em todos (compatibilidade).
- **RF11.2 (consulta)** O core expõe, dado `provider` e o tipo de autenticação escolhido, a lista de opções válidas de cada campo (por exemplo `field_options(provider, key, auth_mode)`), com o valor padrão para aquele tipo. Os frontends deixam de manter listas próprias; trocar o tipo de autenticação troca a lista.
- **RF11.3 (troca de tipo)** Ao trocar o tipo de autenticação, se o valor atual do campo deixa de ser válido, o core indica o valor substituto (o padrão do tipo) e o frontend avisa a troca em vez de manter um valor inválido em silêncio.
- **RF11.4 (validação)** `build_account` rejeita uma combinação inválida (por exemplo, `auth_method = idc` com variável de ambiente) com erro tipado e identificador estável (`edit_error.option_not_valid_for_auth_mode`, parâmetros: campo, valor, tipo de autenticação), sem ecoar o valor digitado além do que já é visível ao usuário. A validação vale também para contas existentes ao serem editadas; contas já gravadas com combinação inválida continuam sendo lidas e consultadas, e o aviso aparece só ao editar.
- **RF11.5 (auditoria dos provedores)** Revisar todos os campos de escolha dos provedores (Kiro, Gemini AI e Grok `product`, Kimi `product`, Copilot `mode`, CodeBuddy `region`, entre outros) e declarar a aplicabilidade de cada opção. Entregar a tabela resultante (provedor, campo, opção, tipos de autenticação) na documentação.
- **RF11.6 (textos)** Os motivos de indisponibilidade de uma opção para um tipo de autenticação usam `Localized` (RF8), com identificadores estáveis.

**Critérios de aceite**

- Para o Kiro, a lista do campo `auth_method` com variável de ambiente **não** contém `builder-id` nem `idc` (ver questões), e a do login contém exatamente os valores que o fluxo nativo suporta.
- Trocar de variável de ambiente para login e vice-versa produz a lista correta e o valor substituto, nos dois frontends (TUI e desktop), sem lógica própria de cada um.
- Teste de contrato por provedor: cada opção declarada vale nos tipos informados, e `build_account` recusa as demais.
- Contas existentes com combinação antiga continuam carregando e sendo consultadas.

**Questões em aberto (decidir antes de implementar)**

1. Quais métodos do Kiro valem com variável de ambiente? Proposta inicial: `api_key` e `external_idp` (tokens de longa duração ou emitidos por outro sistema); `google`, `github` e `import` dependem de como o token chega à variável; `builder-id` e `idc` ficam só no login.
2. O campo `auth_method` do modo de credencial e a opção `auth_method` do login devem ser unificados num só descritor com aplicabilidade por opção, em vez de duas listas?
3. Outros provedores com o mesmo problema além do Kiro (ver RF11.5)?

## 6. Requisitos não funcionais

- **Segurança:** segredos nunca aparecem em snapshots, eventos, erros ou logs; mensagens de erro não ecoam entrada do usuário.
- **Compatibilidade:** a TUI continua funcionando e passa a consumir as mesmas APIs. Sem mudança de formato no `cuotai.toml`.
- **Independência de UI:** o core não depende de Tauri, Ratatui ou qualquer framework de janela.
- **Qualidade:** `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings` e `cargo fmt --all -- --check` passam. Testes de provider usam fixtures locais determinísticas.

## Avaliação técnica — core e TUI (2026-10-01)

Decisões abaixo avaliam os requisitos, não aprovam automaticamente as assinaturas sugeridas. O core mantém política, estado e persistência; a TUI mantém tarefas Tokio, teclas, renderização, textos e confirmação humana. `Snapshot`/CLI e formato TOML permanecem compatíveis.

| RF | Decisão e lacuna atual | Responsabilidade proposta e critério de aceite |
|---|---|---|
| **RF1** | **Aceitar, revisar a representação pendente.** `refresh.rs` exige recriar o ciclo em edições, sem snapshot; `ui.rs` recria ciclo e dispara consulta completa. `Monitor` já compartilha semáforo, mas não cliente HTTP entre configurações. | Core aplica edição/remoção preservando relatórios e prazos das contas intactas, invalida somente o relatório editado e fornece monitor da nova configuração com cliente/sessão reaproveitados. Internamente, slots `Option<Report>` deixam explícita a pendência; uma visão por conta não expõe dados antigos, e só se produz `Snapshot` completo quando todos os slots têm relatório, sem mudar seu `Vec<Report>` nem o JSON da CLI. TUI cancela/ignora tarefas antigas, inicia só a consulta indicada e mostra a conta pendente. Testar 1 requisição para edição e 0 para remoção, incluindo TUI; não consultar contas pagas por efeito colateral. |
| **RF2** | **Revisar, não bloquear RF1/RF3/RF4.** `OAuthSession` já oferece `begin`, `finish`, importação e armazenamento; `auth_ui.rs` ainda orquestra OAuth, AWS CLI, Bailian CLI, navegador e códigos. | Core oferece operação cancelável e eventos tipados para fluxos que controla; TUI fica com apresentação, entrada de código, confirmação e, preferencialmente, abertura de navegador pela plataforma. Separar OAuth armazenado por Cuotai de AWS/`bl`, que gravam fora do cofre Cuotai: não prometer rollback impossível. `LoginPrompt` hoje contém URL e código de dispositivo; entregar prompts só ao consumidor de UI que os exibirá, nunca serializar/logar indiscriminadamente nem incluir URLs com credencial ou códigos de autorização em eventos (RF2.1 × RF2.5). Coordenar as escritas no cofre e no TOML com recuperação explícita de falhas parciais; não há transação atômica entre ambos, nem é seguro apagar token anterior em rollback. Testar cancelamento, falha de cada etapa e ausência de vazamento. Retomada após fechar o app: escolher fluxo reiniciado (sem persistência nova). |
| **RF3** | **Revisar ordem/falhas, implementar na fase 1.** `without_account` só remove da configuração; `logout` apaga do cofre; TUI salva sem purgar. | Core oferece remoção coordenada: validar índice, salvar TOML primeiro e só então, se solicitado e se a chave derivada não for compartilhada por outra conta, apagar o segredo. Falha do cofre após salvar deve ser informada como **config salva, segredo não apagado**, não como rollback fictício; permitir nova tentativa de limpeza. TUI pergunta se deve purgar e informa resultado parcial, sem acessar o cofre diretamente. Política de purga padrão continua decisão de produto (questão 4), mas não impede a API/testes para ambos os valores. |
| **RF4** | **Aceitar com erros separados.** `ConfigFile::load` prioriza `./cuotai.toml`; `save` já compara bytes e preserva comentários/symlinks/permissões, mas retorna `String` e não oferece reload. `build_account` também retorna `String`. | Core adiciona descoberta explícita sem CWD para GUI, sem alterar a precedência CLI/TUI; erros tipados de validação, conflito, I/O e não suportado sem ecoar entrada; reload lê novamente e troca estado apenas em sucesso. `changed_on_disk` usa a comparação de bytes já exigida para evitar falsos negativos (mtime sozinho não basta). TUI usa tipos para permitir recarregar após conflito e preservar o rascunho; não leva fs/TOML para `ui.rs`. Testar conflito, reload externo, symlink e 0600. Sem questão de produto pendente. |
| **RF5** | **Aceitar, antecipar quando útil.** O core já fornece `default_env`, `credential_help`, `resolve_credential_env` e `supports_login`, mas não um descritor completo de tipo/modos; ainda há `match` de formulário no desktop. | Core publica tipo de credencial e modos permitidos coerentes com `build_account` (AWS perfil, Alibaba Token Plan perfil `bl`/override, env/OAuth conforme provider), sem converter default vazio em política. TUI consome o descritor e só decide apresentação. Testar todos os providers e impedir `match` de política em frontend. Sem decisão aberta. |
| **RF6** | **Revisar/particionar.** `RefreshCycle` só tem `is_due`/`finish_*`; TUI mantém `Vec<JoinHandle>` e desktop mantém `busy`/geração. Todos os intervalos aceitam 30 s; erros não alteram o prazo. | Core centraliza elegibilidade, prazo e geração de resultados após RF1; TUI continua dona dos handles/cancelamento e chama o core antes de aceitar resultado, inclusive refresh completo/manual. `due_indices` é opcional se não remover política duplicada. Backoff com teto após `RateLimited` é razoável; `AuthenticationError` exige sinal de relogin e política explícita antes de repetição. Marcar providers cobrados e impor mínimo pelo **intervalo efetivo**, incluindo fallback global, somente após definir valor/lista (questão 2); aviso vem do core e estilo da TUI. Não acoplar driver a runtime/framework. |
| **RF7** | **Aceitar, independente.** `AccountClass` e `MetricState` em `metrics.rs` não implementam `Serialize`; TUI já usa os enums sem strings de política. | Core deriva `Serialize` com `snake_case` para variantes e campos estruturados estáveis, documenta o formato e testa serialização representativa. TUI mantém cores/rótulos; não precisa serializar para renderizar. Contrato existente de `Snapshot`/CLI fica intacto. Sem decisão aberta. |

**Sequência recomendada:** fase 1 mantém RF1 + RF3 + RF4; RF5 e RF7 podem ser feitos independentemente, sem esperar RF2 ou RF6. Na fase 2, migrar a TUI com RF2 sem assumir atomicidade entre sistemas de armazenamento. Dividir RF6: proteção de geração junto a RF1; política de custo/backoff após decisão de intervalo e revisão dos erros. Só as escolhas de pendência (RF1), custo (RF6), retomada (RF2) e padrão de purga (RF3) correspondem às questões em aberto; as demais lacunas não dependem delas.

**Fontes no repositório:** `crates/cuotai-core/src/refresh.rs:13-90`, `lib.rs:54-81,89-169`, `config_file.rs:33-164`, `account_edit.rs:35-126,1136-1185,1419-1459`, `oauth/session.rs:51-105,175-204`, `oauth/store.rs:47-57`, `metrics.rs:25-38,132-148`; `crates/cuotai-tui/src/ui.rs:203-259,375-479,645-730`, `auth_ui.rs:21-145`, `dashboard.rs:41-94`; `apps/desktop/src-tauri/src/backend.rs:27-35,168-205,332-388,505-528` (apenas evidência do consumidor).

## 7. Plano em fases

1. **Fase 1 — custo e consistência:** RF1, RF3, RF4.
2. **Fase 2 — login:** RF2 (maior), com migração da TUI.
3. **Fase 3 — acabamento:** RF5, RF6, RF7, RF11 (depende das decisões das questões do RF11).

## 8. Impacto nos consumidores

- **Desktop:** substituir a recriação completa do ciclo por `apply_edit`; remover o `match` de credencial; adicionar o botão "Entrar" e o acompanhamento do login; exibir erros por campo.
- **TUI:** trocar `auth_ui.rs` pela API de login do core; usar `remove_account` e os erros tipados.

## 9. Questões em aberto

1. O estado pendente deve ser um `Status` novo ou um `Option<Report>` no ciclo? (afeta o formato JSON da CLI)
2. Qual o mínimo de intervalo para provedores cobrados, e quais entram na lista inicial além do AWS Cost Explorer?
3. O login deve poder ser retomado após o app fechar, ou é sempre reiniciado?
4. A purga de credenciais deve ser o padrão ao remover, ou opcional na UI?
5. RF11: quais métodos de autenticação do Kiro valem com variável de ambiente, e as listas de credencial e de login são unificadas em um descritor só?

## 10. Referências no código

- `crates/cuotai-core/src/refresh.rs` — `RefreshCycle`
- `crates/cuotai-core/src/lib.rs` — `Monitor`
- `crates/cuotai-core/src/account_edit.rs` — `build_account`, `Config::upsert_account`, `Config::without_account`
- `crates/cuotai-core/src/config_file.rs` — `ConfigFile::load`, `save`
- `crates/cuotai-core/src/metrics.rs` — `MetricState`, `AccountClass`
- `crates/cuotai-core/src/oauth/session.rs` — `OAuthSession`
- `crates/cuotai-tui/src/auth_ui.rs` — orquestração de login a migrar
- `apps/desktop/src-tauri/src/backend.rs` — contornos temporários no desktop
