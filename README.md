<div align="center">
  <img src="apps/desktop/ui/img/logo.svg" alt="Cuotai Logo" width="84" height="84" />
  <h1>Cuotai</h1>
  <p><strong>Todas as suas quotas, limites de uso e custos de IA em um único lugar.</strong></p>
  <p>
    Monitor multi-conta somente-leitura escrito em Rust, com aplicativo <strong>Desktop (Tauri 2)</strong>, <strong>TUI interativa (Ratatui)</strong> e <strong>CLI JSON</strong>.
  </p>
  <p>
    <a href="#aplicativo-desktop">Desktop</a> •
    <a href="#terminal-tui--cli">TUI &amp; CLI</a> •
    <a href="#provedores-suportados">36 Provedores</a> •
    <a href="#como-começar">Como Começar</a> •
    <a href="#documentação">Documentação</a>
  </p>
</div>

---

## Por que o Cuotai?

Quem trabalha com múltiplos assistentes e APIs de IA precisa alternar entre dezenas de painéis web para descobrir quanto resta da janela de 5 horas, da cota semanal ou do orçamento mensal. O **Cuotai** centraliza tudo em tempo real:

- **100% Somente-Leitura e Zero Gasto de Tokens:** Consulta apenas endpoints de quota, billing e cabeçalhos — **nunca** dispara chamadas de inferência que consumam seus créditos para medir limites.
- **Segurança por Padrão:** O arquivo `cuotai.toml` guarda apenas referências. Tokens OAuth ficam no cofre nativo do sistema operacional (**macOS Keychain**, **Windows Credential Manager**, **Linux Secret Service**) com renovação automática, ou em variáveis de ambiente.
- **Login Nativo Integrado:** Autentique contas direto pelo navegador, Device Code ou AWS SSO sem copiar tokens manualmente (Claude, ChatGPT/Codex, GitHub Copilot, Gemini CLI, Kiro, Antigravity, Cursor, Kimi, Zed, CodeBuddy e mais).
- **Três Interfaces, Um Único Core (`cuotai-core`):**
  - **Desktop & Bandeja do Sistema (Tauri 2):** Janela principal detalhada + popover rápido na barra de menus (visão em donuts ou lista) + menu nativo no clique direito.
  - **Terminal TUI (Ratatui):** Painel rápido com suporte a teclado e mouse, filtro instantâneo e wizard completo de contas.
  - **CLI (`--once` / `--json`):** Ideal para scripts, barras de status (Waybar, SketchyBar, tmux) e automações.
- **Temas e Internacionalização:** 14 temas integrados (Dracula, Catppuccin, Tokyo Night, One Dark, GitHub, macOS, Windows 11, Ubuntu, Synthwave, CRT, Win98/XP, Tron), suporte a temas/idiomas customizados e interface em **Português**, **Inglês** e **Espanhol**.

---

## Aplicativo Desktop

O aplicativo Desktop acompanha suas contas em segundo plano com agendamento incremental por conta, reordenação via *drag-and-drop* (sincronizada com a TUI) e alertas visuais imediatos quando uma cota entra em atenção ou se esgota.

<div align="center">
  <img src="docs/assets/desktop-main.png" alt="Painel Principal do Cuotai Desktop" width="100%" />
  <p><em>Janela principal do Cuotai Desktop exibindo janelas de 5h, semanal, rate limits e estado das contas.</em></p>
</div>

<div align="center">
  <img src="docs/assets/desktop-tray.png" alt="Popover da Bandeja do Cuotai Desktop" width="420" />
  <p><em>Popover da bandeja do sistema na visão compacta em anéis (donuts) com resumo de alertas no rodapé.</em></p>
</div>

---

## Provedores Suportados

O Cuotai possui **36 integrações** no catálogo. Para detalhes de parâmetros, variáveis padrão e limitações de cada provedor, consulte **[`docs/providers.md`](docs/providers.md)**.

| Categoria | Provedores |
|---|---|
| **Assistentes & Assinaturas** | **Claude** (`claude`), **OpenAI Codex / ChatGPT** (`chat_gpt`), **GitHub Copilot** (`copilot`), **Cursor** (`cursor` — pessoal e Teams), **Windsurf** (`windsurf`), **Kiro / AWS** (`kiro`), **Antigravity** (`antigravity`), **Cline** (`cline`), **Zed** (`zed`), **CodeBuddy** (`code_buddy`), **Qoder** (`qoder`), **OpenCode Go** (`open_code_go`) |
| **CLIs & Planos de Coding** | **Gemini CLI** (`gemini_ai`), **Grok CLI / xAI** (`grok`), **Kimi Coding & API** (`kimi`), **MiniMax Token/Coding Plan** (`mini_max`), **Alibaba Coding Plan** (`alibaba_coding_plan`), **Alibaba Token Plan / Qwen Cloud** (`alibaba_token_plan`), **Xiaomi MiMo** (`xiaomi_mimo`), **Ollama Cloud** (`ollama_cloud`), **Qwen Code** (`qwen`) |
| **Cloud Billing & Admin APIs** | **OpenAI Admin API** (`open_ai_api`), **Anthropic Admin API** (`anthropic_api`), **Google Gemini API Monitoring** (`gemini_api`), **Google Vertex AI** (`vertex_ai`), **AWS Bedrock Cost Explorer** (`aws_bedrock`), **Azure OpenAI Cost Management** (`azure_open_ai`), **Alibaba Cloud BSS** (`alibaba_cloud`), **Mistral Admin** (`mistral`), **Perplexity Computer Analytics** (`perplexity`), **Kimchi** (`kimchi`), **Groq** (`groq`) |
| **Catálogo Adicional** | **Alibaba Model Studio** (`alibaba_model_studio`), **Cloudflare AI** (`cloudflare_ai`), **NVIDIA NIM** (`nvidia_nim`), **Poolside** (`poolside`) |

---

## Como Começar

### Pré-requisitos

- **Rust / Cargo** (edição 2021)
- *(Opcional)* **AWS CLI v2** no `PATH` caso utilize contas `aws_bedrock`
- *(Opcional)* **Bailian CLI (`bl`)** caso utilize login de console para `alibaba_token_plan`

### 1. Aplicativo Desktop (Tauri)

```sh
cd apps/desktop/src-tauri
cargo run
```

Para gerar o pacote instalável para o seu sistema operacional:

```sh
cargo install tauri-cli
cd apps/desktop/src-tauri && cargo tauri build
```

### 2. Terminal TUI & CLI

Execute o painel interativo no terminal (abre automaticamente o wizard na primeira execução):

```sh
cargo run -p cuotai-tui
```

Ou instale o binário `cuotai` localmente:

```sh
cargo install --path crates/cuotai-tui --locked
```

Consultas rápidas via linha de comando (sem abrir a interface interativa):

```sh
# Listar os 36 provedores disponíveis
cargo run -p cuotai-tui -- --list-providers

# Resumo legível no terminal (execução única)
cargo run -p cuotai-tui -- --config cuotai.toml --once

# Snapshot estruturado em JSON para scripts e barras de status
cargo run -p cuotai-tui -- --config cuotai.toml --json
```

No JSON, cada métrica inclui `kind` (`quota`, `rate_limit`, `balance`, `credits`, `spend` ou `usage`) e `window` (`window_5h`, `daily`, `weekly`, `monthly`, `other` ou `null`), classificados pelo adaptador do provedor. Consumidores não precisam interpretar `label` ou `period` para distinguir cotas, rate limits e janelas.

---

## Documentação

Toda a documentação técnica detalhada está organizada na pasta **[`docs/`](docs/)**:

- **[Guia da TUI, CLI e Wizard de Contas (`docs/tui.md`)](docs/tui.md)** — Atalhos de teclado e mouse (incluindo a seção Modelos, `m`, com busca local e `s` Sincronizar modelos), fluxo passo a passo do wizard, precedência de arquivos de configuração e códigos de saída da CLI.
- **[Login OAuth, Sessões e Cofre (`docs/oauth.md`)](docs/oauth.md)** — Como funcionam os logins nativos no navegador/Device Code, importação assistida do Cursor, preparação OAuth para Google/Azure/AWS SSO e armazenamento seguro no Keychain.
- **[Catálogo de Provedores e Limitações (`docs/providers.md`)](docs/providers.md)** — Tabela completa dos 36 provedores, variáveis de ambiente padrão, compatibilidade com o `9router`, semântica das métricas e avisos de custos por consulta (como AWS Cost Explorer).
- **[Arquitetura, Biblioteca Rust e Verificação (`docs/architecture.md`)](docs/architecture.md)** — Separação entre `cuotai-core`, `cuotai-tui` e `apps/desktop`, uso do `cuotai-core` como biblioteca Rust, contrato de internacionalização e suíte de testes.
- **[Guia do App Desktop (`apps/desktop/README.md`)](apps/desktop/README.md)** — Detalhes específicos do shell Tauri 2, temas e pasta de personalização do usuário.
- **[Exemplo de Configuração (`cuotai.example.toml`)](cuotai.example.toml)** — Referência comentada de todas as entradas `[[accounts]]` e parâmetros TOML.
