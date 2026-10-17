# Arquitetura, Biblioteca Rust e Verificação

## Separação Core / TUI / Desktop

```text
crates/cuotai-core/       biblioteca Rust: contas e persistência, credenciais, provedores, métricas, consultas
crates/cuotai-tui/        binário cuotai: CLI, wizard de entrada, Ratatui, teclado/mouse
apps/desktop/             aplicativo Tauri 2: janela principal, popover de bandeja e menu nativo
```

`cuotai-core` é independente de UI (não depende de Ratatui, Crossterm nem de um loop gráfico):

- **`account_edit`**: `field_descriptors` e `native_option_descriptors` definem campos, opções e obrigatoriedade por provedor; `visible_field_indices` decide quais campos se aplicam conforme produto, modo e login; `build_account` valida o rascunho contra esse esquema; `Config::upsert_account`/`without_account` criam, editam ou removem contas.
- **`config_file::ConfigFile`**: descobre, carrega e grava o TOML atomicamente preservando comentários e recusando edições externas concorrentes.
- **`Monitor`**: `refresh` e `refresh_account` consultam provedores e normalizam relatórios; o semáforo interno limita em conjunto consultas completas e individuais (1–32), aplica prazo por conta (1–300 s), preserva a ordem configurada e isola falhas por conta.
- **`models`**: catálogo de modelos por conta, fora do refresh de quota. `Monitor::with_model_cache(ModelCache::in_data_dir(&file))` grava em `<dados persistentes do SO>/cuotai/models/models.sqlite3` (fora de cache, separado do TOML, cofre e SQLite de credenciais debug; diretório `0700` e arquivo `0600` no Unix, inclusive se já existiam). Cada linha guarda só digests: SHA-256 de `ConfigFile::identity()` e da identidade da conta (provedor, nome, `credential_env`/`oauth` e `params`, exceto `models` e `billing_cycle`, que só filtram/datam a quota); nunca segredos, corpos HTTP ou URLs. Mover conta preserva a entrada; renomear ou mudar credencial/produto/região cria outra. `Monitor::prune_models` apaga entradas da configuração ausentes do TOML recém-carregado ou salvo (TUI e desktop chamam ao abrir, salvar edição e remover), sem tocar contas vivas de outra janela. Estados: `not_fetched`, `fetching`, `ready`, `failed`, `interrupted`, `unsupported`. `model_state` só lê; `load_models` consulta só sem tentativa registrada (sucesso, lista vazia, falha ou interrupção ficam gravados e não se repetem); `sync_models` consulta e, se falhar, mantém o catálogo anterior com a data dele. Toda consulta exige uma reivindicação por conta (token do dono + lease = prazo da conta + 30 s) tomada em `BEGIN IMMEDIATE`: outra janela/processo com reivindicação viva não consulta, espera o resultado gravado (polling de 100 ms, limitado pelo lease), então syncs simultâneos viram uma requisição; só o dono publica, então conclusão antiga não sobrescreve resultado novo. Cancelamento expira o lease na hora e processo encerrado deixa o lease vencer: em ambos a linha vira `interrupted`, sem lista parcial nem nova consulta automática, até um `sync_models` manual. Erro de persistência é `ModelError::Storage`. Hoje só Groq (`GET /openai/v1/models`); os demais provedores retornam `Unsupported` sem rede.
- **`refresh::RefreshCycle`**: calcula os prazos efetivos de cada conta (incluindo piso de 300 s para provedores cobrados por consulta, como AWS Bedrock) e monta incrementalmente o `Snapshot`. Após `RateLimited`, `AuthenticationError` ou falha da tarefa, o intervalo da conta dobra a cada falha consecutiva até 1 h (intervalos configurados acima de 1 h são mantidos, nunca encurtados); um resultado de outro status restaura o intervalo configurado. Atualizações manuais continuam disponíveis.
- **`metrics`**: `classify_report` e `classify_metric` derivam disponibilidade e consumo (`AccountClass` e `MetricState`).
- **`oauth` e `login`**: `OAuthSession`, `OAuthConfig`, `OAuthLogin` e `TokenStore` gerenciam credenciais e renovação; `cuotai_core::login` expõe um driver cancelável independente de UI com eventos de prompt/progresso e commit estagiado entre cofre e TOML.

### Uso como biblioteca Rust

```rust
use cuotai_core::{Config, Monitor};

async fn consultar(config: Config) -> Result<(), Box<dyn std::error::Error>> {
    let monitor = Monitor::new(config)?;
    let snapshot = monitor.refresh().await;
    println!("{}", serde_json::to_string(&snapshot)?);
    Ok(())
}
```

- `AccountClass` serializa como `"available"`, `"exhausted"` ou `"unmeasured"`.
- `MetricState` serializa com a tag `state`: `{"state":"unmeasured"}`, `{"state":"exhausted","no_cap":true}` ou `{"state":"available","ratio":0.25,"uncapped":false}` (quando `uncapped` é `true`, `ratio` não representa disponibilidade).
- Cada `Metric` traz `kind` (`"quota"`, `"rate_limit"`, `"balance"`, `"credits"`, `"spend"` ou `"usage"`) e `window` (`"window_5h"`, `"daily"`, `"weekly"`, `"monthly"`, `"other"` ou `null`). Os adaptadores atribuem esses campos pelo contrato do provedor; frontends não devem inferi-los de `label` ou `period`. Os campos são aditivos ao `Snapshot` e ao JSON da CLI.
- `account_edit::CredentialKind` serializa como `"env"`, `"profile"` ou `"none"`.
- `Monitor::with_credentials(config, Arc<dyn Credentials>)` permite injetar busca de segredos customizada (`Credentials::get` deve ser rápido e não interativo). `Monitor::with_oauth` compartilha a `OAuthSession` entre consultas.

### Internacionalização (`Localized`)

Textos estruturados emitidos pelo core carregam `Localized { id, params, fallback }`:

- O **core é dono do catálogo dos ids que ele mesmo emite**:
  `crates/cuotai-core/locales/{en-US,es}.json` (carregados por
  `include_str!` e `serde_json` atrás de `LazyLock`). Cada id do core
  (field.*, native.*, login.*, credential_help.*, oauth.*,
  cost_advisory.*, edit_error.*, query.*, note.*, remove_error.*,
  remove.kept.*, status.*, provider.*) vive em `en-US.json` e `es.json`
  com o mesmo conjunto de chaves; `pt-BR` não consulta catálogo e usa
  o `fallback` em português embutido em cada `Localized`. A resolução
  (`Localized::resolve`, `cuotai_core::text::core_catalog`) segue a ordem
  idioma exato → mesma língua → `en-US` → `fallback`.
- O aplicativo Desktop recebe o catálogo do core via o comando Tauri
  `core_catalog(locale)` e o `js/i18n.js` mescla esse catálogo sob o da
  UI (`apps/desktop/ui/locales/*.json`); pacotes do usuário (em
  `<user dir>/locales/<code>.json`) cobrem ambos. Frontends **não fazem
  correspondência por texto** — `t()` e `tl()` consultam o catálogo.
- A TUI resolve os `Localized` do core via `Localized::resolve` na
  locale ativa (`CUOTAI_LANG`, padrão `pt-BR`); as próprias legendas do
  Ratatui (atalhos, molduras, ...) ficam fora do escopo do RF8 e
  permanecem em português.
- `Report.notes` mantém texto em português para compatibilidade com
  consumidores CLI/JSON; `Report.notes_l10n` acompanha as notas na
  mesma ordem com IDs e parâmetros estruturados (nunca contendo
  tokens). Acrescentar um idioma significa apenas acrescentar um
  catálogo.

## Comandos de desenvolvimento e verificação

Execute na raiz do workspace:

```sh
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo build --workspace
cargo fmt --all -- --check
```

Para o aplicativo Desktop (Tauri):

```sh
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml
cargo clippy --manifest-path apps/desktop/src-tauri/Cargo.toml --all-targets -- -D warnings
```

### Escopo dos testes e verificação

- **Testes de contrato (`wiremock`):** Exercitam paginação, centavos/moeda, overflow, schemas incompletos, filtros de cobrança, consumo bruto versus líquido, saldo versus gasto e credenciais ausentes/inválidas sem depender de rede externa.
- **Testes de OAuth e cofre:** Cobrem callback/CSRF, negação, polling/`slow_down`/expiração, criptografia RSA/X25519/AES-GCM, rotação de refresh token, isolamento por conta/provedor (ex.: múltiplas contas Kiro) e commit estagiado.
- **Testes de persistência e TUI:** Validam edição e reordenação em `ConfigFile` preservando comentários, permissões `0600`, recusa de symlinks/edições concorrentes, e renderização do painel/wizard.
- Respostas HTTP simuladas validam contratos e tratamento de erros, mas não substituem autorização e consulta em contas reais de produção.
