# PRD 0002 — Catálogo de modelos sob demanda

| Campo | Valor |
|---|---|
| ID | 0002 |
| Status | Implementado (0.14.x) |
| Responsável | `cuotai-core`, TUI e Desktop |
| Consumidores | `crates/cuotai-tui`, `apps/desktop` |
| Data | 2026-10-16 |

## Estado da implementação (2026-10-17)

Todos os RFs foram implementados no core, na TUI e no desktop, um por vez, com validação (`cargo fmt`, `clippy -D warnings`, testes do workspace e do desktop, smoke da TUI e da UI desktop) antes de cada versão.

| RF | Versão | Entrega |
|---|---|---|
| RF1 | 0.8.0, 0.14.0 | Resolução única no core (TUI: `--config` > `CUOTAI_CONFIG` > `./cuotai.toml` existente > SO; desktop: `CUOTAI_CONFIG` > arquivo escolhido > SO), escolha do TOML nas Configurações do desktop, `ConfigIdentity` normalizada e aviso quando TUI e desktop usam arquivos diferentes. |
| RF2 | 0.9.0 | `ModelInfo`/`ModelCatalog`/`ModelState` e `Monitor::{model_state, load_models, sync_models}` reaproveitando cliente, OAuth, timeout e semáforo. |
| RF3 | 0.10.0 | `models.sqlite3` no diretório de dados (0700/0600, só digests), estados `fetching`/`ready`/`failed`/`interrupted`, reivindicação entre processos e `prune_models`. |
| RF4 | 0.11.0 | Adaptadores verificados (Groq, AWS Bedrock, Antigravity, Gemini AI Studio, Kimi, Poolside, Alibaba Model Studio intl), paginação limitada e matriz dos 36 provedores em `docs/providers.md`. |
| RF5 | 0.12.0 | Seção Modelos na TUI (detalhes e wizard com conta configurada), busca local e sincronização manual. |
| RF6 | 0.13.0 | Seção Modelos no desktop (detalhes, popover e diálogo de conta), busca acessível, sincronização manual e descarte de resultados de identidade antiga. |

As seções 1 e 2 descrevem a situação de 2026-10-16, antes da implementação. O primeiro conjunto de adaptadores do RF4.1 foi ampliado com Gemini AI Studio, Kimi, Poolside e Alibaba Model Studio (intl), cada um verificado em fonte oficial.

## 1. Contexto

O `Monitor::refresh_account` compartilha cliente HTTPS, OAuth, timeout e limite de concorrência, mas devolve apenas relatórios de uso (`crates/cuotai-core/src/lib.rs`). `RefreshCycle` conserva relatórios e prazos em memória, não um catálogo persistente. Groq já consulta `GET /openai/v1/models` para ler cabeçalhos de rate limit (`providers/router_keys.rs`); Antigravity usa `fetchAvailableModels` para quota dos planos pagos (`providers/antigravity.rs`). Isso não constitui um catálogo pesquisável nem autoriza consultar automaticamente listas de modelos durante refresh.

TUI e desktop podem carregar TOMLs diferentes: TUI prefere `./cuotai.toml` existente após `--config`, enquanto o desktop usa `CUOTAI_CONFIG` ou a configuração do SO (`config_file.rs::resolve_path`, `crates/cuotai-tui/src/main.rs`, `apps/desktop/src-tauri/src/backend.rs::ensure_loaded`). Não existe página autônoma de provedor: o wizard seleciona provedores, os cartões abrem detalhes de conta, e o desktop usa formulário e diálogo/popover de detalhes. Neste PRD, **página do provedor** significa a seleção/edição do provedor com uma seção Modelos; **detalhes** são os da conta configurada.

## 2. Problema

Não é possível descobrir e pesquisar modelos disponíveis para uma conta na TUI ou no desktop. Uma consulta por abertura de tela, refresh de quota ou busca digitada seria desnecessária e poderia ter custo; um cache apenas em memória ou `localStorage` não sobreviveria a reinício nem seria compartilhado. Cache por `Provider` confundiria contas, regiões e credenciais distintas. Mesmo um cache compartilhado não resolve o fato de os frontends poderem usar arquivos de configuração diferentes.

## 3. Objetivos

- Consultar modelos **da conta configurada** somente na primeira abertura explícita de Modelos e novamente apenas quando o usuário solicitar **Sincronizar modelos**.
- Persistir e compartilhar resultados entre TUI e desktop que apontem para o mesmo TOML; pesquisa local não consulta o provedor.
- Preservar a escolha da TUI por `./cuotai.toml`, além de permitir que o desktop escolha explicitamente esse mesmo arquivo.
- Exibir fonte, escopo, data e estado para não confundir catálogo com quota, permissão de inferência ou disponibilidade instantânea.

## 4. Não-objetivos

- Consultar todos os 36 provedores sem endpoint, permissão e credencial verificados; inventar modelos a partir de métricas ou tabelas estáticas.
- Usar requisições de inferência, sincronização por TTL/timer, prefetch, revalidação em segundo plano ou busca remota por caractere.
- Migrar/mesclar TOMLs automaticamente, armazenar tokens no TOML/cache ou mudar `Report`, `Snapshot` e agendamento de quotas.

## 5. Requisitos funcionais

### RF1 — Caminho de configuração compartilhável (prioridade alta; pré-requisito)

- **RF1.1** Centralizar a resolução em `ConfigFile`: na TUI, `--config` > `CUOTAI_CONFIG` > `./cuotai.toml` **existente** > `<configuração do SO>/cuotai/config.toml`. `--config ./cuotai.toml` continua permitindo criar o local explicitamente. Sem local nem override, o wizard cria no caminho do SO. Preservar a preferência atual da TUI pelo local existente.
- **RF1.2** O desktop usa `CUOTAI_CONFIG`, um arquivo escolhido explicitamente nas configurações do aplicativo, ou o caminho do SO; não deduz `./cuotai.toml` de seu `cwd`. Permitir escolher no desktop o mesmo TOML local da TUI. Exibir caminho efetivo em ambas as interfaces e informar quando os caminhos são diferentes; configs diferentes implicam caches isolados. Não migrar, mesclar nem sobrescrever automaticamente arquivos locais.
- **RF1.3** Normalizar caminhos relativos e symlinks para a identidade do mesmo arquivo, inclusive antes de sua criação (diretório-pai existente mais nome). `ConfigFile::path` resolvido alimenta a identidade do cache; integradores sem arquivo fornecem identidade estável explicitamente. Atualizar ajuda CLI, `docs/tui.md` e `apps/desktop/README.md` quando implementar.
- **Critério de aceite:** sem `./cuotai.toml`, ambos sem overrides veem caminho/contas do SO, mesmo com `cwd` distinto. Com local existente, a TUI continua usando-o; selecionar o mesmo arquivo no desktop faz ambos verem as mesmas contas após recarga e compartilhar cache. Testar precedência, criação local explícita, symlink/relativo e arquivos distintos.

### RF2 — Contrato de modelos por conta no core (prioridade alta)

- **RF2.1** Expor `ModelInfo` (ID real, nome/descrição opcionais), `ModelCatalog` (modelos, fonte/escopo, `fetched_at`) e estados/erros tipados; os textos apresentados aos frontends usam `text::Localized` com fallback português. Valores ausentes continuam ausentes; catálogo não prova autorização para inferência, preço ou disponibilidade atual.
- **RF2.2** Expor leitura do estado sem rede e operações explícitas `load_models(account_index)` e `sync_models(account_index)`. `load_models`: sem entrada faz a primeira consulta; com sucesso, lista vazia ou falha registrada, apenas lê estado. `sync_models`: única operação que reconsulta após a primeira tentativa, preservando catálogo anterior se falhar. `Unsupported` não faz rede. Índice inválido é erro tipado.
- **RF2.3** Reusar cliente, `Context`, OAuth compartilhado, timeout e limite de concorrência do `Monitor`; `Monitor::with_config` mantém acesso ao mesmo cache. A localização/identidade do cache é configurada explicitamente a partir de `ConfigFile::path`, sem adivinhar `cwd` nem exigir cache para `Monitor::refresh` existente. Estas operações não alteram `RefreshCycle`, `Report`, `Snapshot` ou prazos.
- **Critério de aceite:** duas contas do mesmo provedor com credenciais/regiões distintas retornam catálogos isolados; primeira abertura consulta uma vez; reabrir, reiniciar, pesquisar e refrescar quota não geram **consulta adicional do catálogo**. Resultado vazio bem-sucedido difere de ausência de consulta.

### RF3 — Persistência e sincronização manual (prioridade alta)

- **RF3.1** Persistir catálogo em SQLite separado do TOML/cofre/SQLite de credenciais debug (`rusqlite` já disponível), no diretório **persistente de dados** do SO, não em pasta sujeita a limpeza de cache. TUI e desktop usam o mesmo banco quando apontam para o mesmo arquivo. Erro de persistência é exposto, nunca tratado como êxito. Restringir permissões em Unix e não armazenar segredos, corpos HTTP, URLs com credenciais ou parâmetros sensíveis.
- **RF3.2** Isolar por digest do caminho normalizado do TOML e digest da identidade da conta (provedor, nome, referência de credencial e parâmetros relevantes), não por índice ou somente por provider. Mover conta preserva entrada; editar credencial/produto/região ou renomear invalida conservadoramente; remover conta descarta sua entrada. Não gravar identidade bruta potencialmente sensível.
- **RF3.3** Guardar estado da tentativa (`fetching`, `ready`, `failed`, `interrupted`), data do último êxito e catálogo completo somente após sucesso de todas as páginas. Falha inicial fica registrada e não é repetida automaticamente. Falha na sincronização mantém o último catálogo pesquisável com data anterior e erro visível. Resposta válida de zero modelos é `ready` com data. Cancelamento ou processo encerrado não publica lista parcial.
- **RF3.4** Serializar reivindicação por conta no SQLite: duas janelas/processos simultâneos não disparam duas primeiras consultas. Coalescer sincronizações concorrentes; conclusão antiga não sobrescreve resultado mais novo. Reivindicação abandonada vira `interrupted` visível após recuperação, sem consulta automática; a ação manual permite tentar novamente. Pesquisa e leitura do cache não fazem rede.
- **Critério de aceite:** testes de miss → 1 chamada, hit após reinício → 0, vazio → 0, falha inicial após reinício → 0, sync manual → 1, erro de sync preservando último resultado, duas instâncias simultâneas → 1, queda/interrupção → estado recuperável sem refetch, identidade editada/movida/removida e caminhos compartilhados/isolados.

### RF4 — Adaptadores e limitações verificadas (prioridade alta)

- **RF4.1** Habilitar lista apenas para endpoint read-only com credencial, produto, região, permissões, custo e semântica comprovados. Primeiro conjunto de contratos a implementar/testar: Groq (`/openai/v1/models`, já usado para cabeçalhos), Bedrock por perfil/região (`aws bedrock list-foundation-models`, diferente de Cost Explorer) e Antigravity somente onde `fetchAvailableModels` retorna catálogo. **Antes de habilitar cada um**, validar que a credencial configurada dá acesso. Não acrescentar chamada ao refresh de quota existente.
- **RF4.2** Avaliar Gemini AI Studio apenas com chave Gemini API; `models.list` é paginado e a chave não se confunde com token de Cloud Monitoring/CLI. Não pressupor que chaves administrativas de OpenAI/Anthropic, Azure ou tokens de assinaturas listem modelos de inferência. Para os demais, investigar endpoint e escopo; sem evidência, `Unsupported` com motivo e zero rede. Publicar matriz dos 36 `Provider::ALL` com status e fonte em `docs/providers.md` na implementação.
- **RF4.3** Paginar até conclusão com limite de páginas e detecção de token repetido; schema/página incompleta rejeita a sincronização sem sobrescrever cache. Deduplicar IDs e ordenar para apresentação sem alterar IDs reais. Erros não exibem corpos, cabeçalhos, tokens ou URLs sensíveis.
- **Critério de aceite:** fixtures HTTP/CLI locais cobrem lista completa, paginação, página inválida/incompleta, 401/403/429, região/produto/credencial distintos e `Unsupported`; nenhum adaptador inventa lista a partir de métricas. [Gemini models.list](https://ai.google.dev/api/models) e [Bedrock ListFoundationModels](https://docs.aws.amazon.com/bedrock/latest/APIReference/API_ListFoundationModels.html) documentam seus contratos, não garantem acesso à conta do usuário.

### RF5 — Pesquisa na seleção e nos detalhes da TUI (prioridade média)

- **RF5.1** Em `ui.rs`/`dashboard.rs`, oferecer seção Modelos na seleção/edição do provedor e nos detalhes de conta. Na seleção, exigir escolha explícita de **conta já configurada** daquele provedor antes da primeira consulta; conta nova sem credencial mostra suporte e orientação, não lista fabricada. Não criar página autônoma vazia.
- **RF5.2** Ao abrir Modelos, ler cache/acionar `load_models` somente se elegível; campo de busca por ID/nome filtra localmente, separado do filtro global de contas. Mostrar status, fonte/escopo, contagem, `fetched_at`, vazios e erro com resultado antigo. Teclas, foco, scroll, mouse e `Esc` seguem o painel. Rodapé/atalho distinto **Sincronizar modelos**; operação assíncrona não trava navegação nem refresca quota.
- **Critério de aceite:** smoke TUI com conta suportada/sem cache, cache após reinício, duas contas do mesmo provedor, busca sem HTTP, indisponibilidade e erro de sync mantendo catálogo; `r`, `R`, `--once` e `--json` não criam chamadas adicionais de sincronização de modelos.

### RF6 — Pesquisa na seleção e nos detalhes do desktop (prioridade média)

- **RF6.1** Expor pelo backend Tauri leitura/primeira consulta/sync por conta validada, sem segredo no IPC. Em `account-dialog.js`, pesquisar modelos da conta em edição ou escolher explicitamente conta existente do provedor ao criar outra; sem conta, mostrar orientação. Em `detail.js`/`details-dialog.js`, exibir seção pesquisável nos detalhes da janela e popover. `lib.js` reutiliza as mesmas chamadas; `localStorage` não é fonte de verdade.
- **RF6.2** Busca local por ID/nome com `<input type="search">` rotulado, navegação por teclado, progresso anunciado (`aria-live`) e botão manual de sync desabilitado durante operação. Renderizar estados/fonte/data/erro sem descartar catálogo anterior; localizar em `pt-BR`, `en-US` e `es`. Se a configuração mudar enquanto consulta, descartar resultado da identidade antiga, nunca atribuí-lo ao novo índice.
- **Critério de aceite:** smoke desktop em três idiomas cobre primeira consulta, cache após fechar/reabrir, busca sem HTTP, sync manual, erro preservando dados, conta nova sem credencial, mudança/remoção/reordenação da conta e acesso no popover. O mesmo TOML escolhido na TUI fornece as mesmas contas e modelos em cache.

## 6. Requisitos não funcionais

- **Segurança:** nunca guardar ou serializar tokens/cookies, resposta bruta ou URL autenticada; consultas de modelos são somente leitura e não invocam inferência. Ausência de permissão é distinta de lista vazia.
- **Compatibilidade:** a TUI conserva `./cuotai.toml` e `--config`; o desktop não deduz `cwd`. Configuração/caches distintos continuam isolados; formato do TOML e saída de quotas não mudam.
- **Custo e honestidade:** nenhuma consulta de catálogo é disparada por refresh/scheduler, busca ou reinício. Groq/Antigravity já podem acessar seus endpoints em refresh de quota: o critério é não **acrescentar** uma chamada de sincronização nesses caminhos, não prometer tráfego zero aos endpoints existentes.

## 7. Ordem de execução e verificação

1. **RF1 primeiro:** resolver caminhos, escolher TOML no desktop, atualizar documentação e provar compartilhamento/isolamento.
2. **RF2–RF3:** contrato do core e cache SQLite com testes de concorrência, falha e reinício; manter `Monitor::refresh` independente.
3. **RF4:** validar contratos/credenciais com fontes, fixtures determinísticas e matriz de capacidade.
4. **RF5–RF6:** integrar TUI e desktop, provar visualmente pesquisa e sincronização manual em ambos; executar `cargo test --workspace`, Clippy, formatação, testes/build do desktop e smoke de CLI, TUI e interface desktop.

O requisito só está entregue quando um catálogo de conta suportada sobrevive ao fechamento e é pesquisável nos dois frontends configurados para **o mesmo arquivo**, sem nova consulta; outra consulta ocorre exclusivamente após **Sincronizar modelos**. Provedores não suportados exibem motivo sem rede e erro/página parcial nunca substitui catálogo válido.
