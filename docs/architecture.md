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
- `MetricState` serializa como `"unmeasured"`, `{"exhausted":{"no_cap":true}}` ou `{"available":{"ratio":0.25,"uncapped":false}}`.
- `Monitor::with_credentials(config, Arc<dyn Credentials>)` permite injetar busca de segredos customizada (`Credentials::get` deve ser rápido e não interativo). `Monitor::with_oauth` compartilha a `OAuthSession` entre consultas.

### Internacionalização (`Localized`)

Textos estruturados emitidos pelo core carregam `Localized { id, params, fallback }`:

- O aplicativo Desktop resolve `id` nos catálogos `apps/desktop/ui/locales/{pt-BR,en-US,es}.json` (e em pacotes de idioma do usuário) e usa `fallback` em português caso não haja tradução.
- A TUI utiliza diretamente o `fallback` em português.
- `Report.notes` mantém texto em português para compatibilidade com consumidores CLI/JSON e `Report.notes_l10n` acompanha as notas na mesma ordem com IDs e parâmetros estruturados (nunca contendo tokens).

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
