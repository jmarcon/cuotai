# PRD 0019 — Empacotamento, licença e distribuição

| Campo | Valor |
|---|---|
| ID | 0019 |
| Status | Proposto |
| Responsável | Repositório, CI e release (`.github/`, `apps/desktop`, `crates/cuotai-tui`) |
| Consumidores | usuários finais, PRDs 0014 e 0018 |
| Depende de | nenhum; é pré-requisito dos PRDs 0014 e 0018 |
| Data | 2026-10-19 |

## 1. Contexto

O Cuotai hoje só pode ser usado a partir do código-fonte: o README manda executar `cargo run` em `apps/desktop/src-tauri` e `cargo run -p cuotai-tui` na raiz, e gerar o pacote instalável localmente. Faltam:
- **Licença:** os manifestos declaram `license = "MIT"` (workspace e desktop), mas o repositório público não tem arquivo `LICENSE` (o GitHub não reconhece nenhuma licença), nem `CONTRIBUTING`, nem política de segurança.
- **CI e releases:** não existe `.github/workflows`, tags ou releases publicadas; a versão está duplicada em `Cargo.toml` e `tauri.conf.json` (0.17.0).
- **Pacotes:** `tauri.conf.json` tem `bundle.targets = "all"`, mas nada gera, assina ou publica artefatos; a TUI não tem binários distribuídos.
- **Plataformas:** o README cita cofres do Keychain, Credential Manager e Secret Service, mas não há pacotes nem testes automáticos para Windows e Linux.

Na análise de mercado de 2026-10-19, todo concorrente aberto com tração distribui binários: CodexBar (DMG, cask do Homebrew, AUR, tarballs de CLI para macOS e Linux, app Qt para Linux), ClaudeBar (DMG assinado e notarizado, cask), Quotio (DMG, cask, npm e Homebrew para a CLI), OpenUsage (DMG universal, cask), OpenQuota (instaladores Windows x64/ARM64, DMG universal, AppImage e deb), QuotaTempo (ZIP assinado e notarizado com `SHA256SUMS`). O OpenQuota informa abertamente quando a assinatura nativa está desligada.

## 2. Problema

Sem binários, sem licença formal e sem releases, o Cuotai não tem como ser adotado fora de quem compila Rust. A ausência de licença impede contribuições e redistribuição por terceiros, apesar de o manifesto declarar MIT.

## 3. Objetivos

- Formalizar licença, política de contribuição e de segurança.
- Automatizar build, teste, empacotamento, assinatura e publicação de releases para macOS, Windows e Linux.
- Distribuir também a TUI/CLI, e definir os canais de instalação suportados.
- Fornecer verificação de integridade (somas e assinaturas) e os artefatos que o atualizador do PRD 0018 usa.

## 4. Não-objetivos

- Lojas de aplicativos (Mac App Store, Microsoft Store) nesta versão.
- Paridade imediata de recursos nas três plataformas: o PRD define o que é suportado, o que é "melhor esforço" e o que fica de fora, com testes.
- Telemetria de instalação.

## 5. Requisitos funcionais

### RF1 — Licença e governança (prioridade alta; pré-requisito)

- **RF1.1** Adicionar `LICENSE` na raiz com o texto da licença que o responsável do projeto decidir. Os manifestos já declaram MIT; a decisão é do dono do projeto e deve ser registrada antes de qualquer release. O `apps/desktop/ui/img/providers/NOTICE.md` e o `THIRD_PARTY_NOTICES` permanecem coerentes com a escolha.
- **RF1.2** Adicionar `CONTRIBUTING.md` (como compilar, testar, política de commits semânticos em português sem acentos no assunto, regras de segurança do projeto), `SECURITY.md` (canal privado de reporte, escopo, prazos de resposta, política de divulgação) e modelos de *issue* (relatório de provedor quebrado sem credenciais, pedido de provedor novo).
- **RF1.3** Verificar a conformidade das licenças das dependências (`cargo deny` ou equivalente, via Cargo) no CI, e manter a tela Sobre e o `THIRD_PARTY_NOTICES` em dia (o teste de divergência de `about.rs` já protege os manifestos).
- **Critério de aceite:** o GitHub reconhece a licença; o CI falha com dependência de licença incompatível; os documentos existem e são linkados no README.

### RF2 — Integração contínua (prioridade alta)

- **RF2.1** Fluxo de trabalho de CI em cada *push* e *pull request*: `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`, e os testes do desktop (workspace separado em `apps/desktop/src-tauri`), em macOS, Windows e Linux.
- **RF2.2** Cache de compilação (`target/`), tempo limite por job, permissões mínimas do `GITHUB_TOKEN` e ações fixadas por *commit hash*. Nenhum segredo disponível para *pull requests* de forks.
- **RF2.3** Falhas dependentes de plataforma (cofre de credenciais, bandeja, notificações) são isoladas por `cfg`/testes marcados, com a lista do que é coberto em cada sistema documentada em `docs/architecture.md`.
- **Critério de aceite:** CI verde nos três sistemas; um *pull request* com `clippy` quebrado falha.

### RF3 — Build, assinatura e publicação (prioridade alta)

- **RF3.1** Release disparado por tag `v*` na `main`. A versão vem de uma única fonte: o `Cargo.toml` do workspace, com verificação no CI de que `apps/desktop/src-tauri/Cargo.toml` e `tauri.conf.json` coincidem (hoje estão duplicados).
- **RF3.2** Artefatos do desktop: macOS universal (`.dmg`), Windows x64 e ARM64 (instalador), Linux x64 e ARM64 (`AppImage` e `.deb`). Artefatos da TUI/CLI: tarballs para macOS (arm64 e x86_64) e Linux (glibc e musl, quando viável) e zip para Windows.
- **RF3.3** Assinatura:
  - macOS: Developer ID e notarização;
  - Windows: Authenticode, se houver certificado, ou aviso explícito nas notas da versão de que o instalador não está assinado (como o OpenQuota faz);
  - todas as plataformas: soma `SHA256SUMS` e assinatura dos artefatos de atualização do Tauri (PRD 0018).
  Os certificados e chaves ficam em segredos de CI, nunca no repositório ou em logs.
- **RF3.4** Notas da versão geradas dos commits semânticos, com seção de mudanças de comportamento e de compatibilidade de configuração, e o estado de assinatura de cada artefato.
- **RF3.5** Publicação no GitHub Releases, com `latest.json` do atualizador e as somas. O processo deve ser repetível em um fork para testes (sem os segredos de produção).
- **Critério de aceite:** uma tag de teste gera os artefatos de todas as plataformas; instalação limpa em cada sistema abre o app, carrega a configuração e cria uma conta de exemplo; `shasum -c` confere as somas; o pacote macOS passa na verificação do Gatekeeper.

### RF4 — Canais de instalação (prioridade média)

- **RF4.1** Homebrew: *tap* próprio com *cask* do desktop e *formula* da CLI; após estabilizar, avaliar o repositório oficial.
- **RF4.2** Windows: manifesto do `winget` e, opcionalmente, `scoop`. Linux: pacote para o AUR e instruções de `AppImage`/`.deb`. Cargo: `cargo install` da TUI a partir do repositório, documentado.
- **RF4.3** Instalações por gerenciador devem poder desligar o atualizador interno (PRD 0018, RF3.2) para evitar duas fontes de atualização.
- **RF4.4** README reorganizado com a instalação por plataforma como o primeiro passo e a compilação a partir do código-fonte como alternativa.
- **Critério de aceite:** instalar e remover pelo `brew`, `winget` (ou instalador) e pacote Linux em máquinas limpas; verificar que a desinstalação não apaga contas nem preferências sem perguntar.

### RF5 — Suporte por plataforma (prioridade média)

- **RF5.1** Declarar em `docs/platforms.md` o que funciona em cada sistema: cofre de credenciais, bandeja (no GNOME pode exigir extensão), notificações, login nativo por navegador, aplicativo de linha de comando (`aws`, `bl`) quando necessário.
- **RF5.2** Executar um *smoke test* automatizado do binário da TUI em cada sistema (`--list-providers`, `--once` com configuração de exemplo sem rede) e um roteiro manual de verificação do desktop para cada release.
- **RF5.3** Recursos sem suporte numa plataforma aparecem desabilitados com explicação, nunca falham de forma silenciosa.
- **Critério de aceite:** a matriz de suporte é revisada a cada release; o teste de fumaça da TUI roda no CI nos três sistemas.

### RF6 — Políticas de release (prioridade baixa)

- **RF6.1** Política de versão (semântica, o que é mudança incompatível: TOML, `--json`, contrato `export::v1`), de *changelog* e de suporte (apenas a última versão estável).
- **RF6.2** Reprodutibilidade: `Cargo.lock` versionado, versões de toolchain fixadas no CI e SBOM gerado por release.
- **Critério de aceite:** documento publicado; o SBOM é anexado ao release.

## 6. Requisitos não funcionais

- **Segurança da cadeia de suprimentos:** segredos só em CI protegido, ações fixadas por hash, dependências auditadas, artefatos verificáveis por soma e assinatura.
- **Honestidade:** o estado de assinatura de cada artefato é declarado nas notas da versão.
- **Custo:** o pipeline deve caber no plano gratuito do GitHub Actions para repositórios públicos, com cache e *matrix* limitada.
- **Dependências externas:** os certificados da Apple e (opcionalmente) do Windows são decisões e custos do dono do projeto.

## 7. Ordem de execução e verificação

1. **RF1:** licença e governança (rápido e desbloqueia o resto).
2. **RF2:** CI nos três sistemas.
3. **RF3:** build, assinatura e publicação, com uma tag de teste em fork.
4. **RF4 e RF5:** canais, matriz de suporte e testes de fumaça.
5. **RF6:** políticas.

Em cada etapa: `cargo fmt`, `clippy -D warnings`, testes do workspace e do desktop e smoke da interface alterada.

O requisito só está entregue quando um usuário sem Rust instalado baixa um pacote da página de releases, verifica a soma, instala em macOS, Windows ou Linux, abre o app com a configuração preservada, a licença do repositório é reconhecida, e o pipeline publica o `latest.json` que o atualizador do PRD 0018 consome.
