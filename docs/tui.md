# Guia da TUI, CLI e Wizard de Contas

O binário `cuotai-tui` oferece tanto o painel interativo no terminal (Ratatui) quanto modos de execução única (`--once` e `--json`) para scripts e automação.

## Executar

Requer Rust/Cargo. AWS Bedrock também requer AWS CLI v2 no `PATH`.

```sh
cargo run -p cuotai-tui -- --list-providers
cargo run -p cuotai-tui
```

Para execução única ou instalação local:

```sh
cargo run -p cuotai-tui -- --config cuotai.toml --once
cargo run -p cuotai-tui -- --config cuotai.toml --json
cargo install --path crates/cuotai-tui --locked
```

### Precedência de configuração e códigos de saída

- **Precedência:** `--config`, `./cuotai.toml`, diretório de configuração do sistema (`~/Library/Application Support/cuotai/config.toml` no macOS).
- Sem arquivo encontrado em modo interativo, o wizard propõe criar `./cuotai.toml` (ou o caminho passado em `--config novo.toml`).
- Em modo `--once`/`--json`, sem arquivo continuam usando o catálogo padrão (não abrem o wizard), e um arquivo explícito ausente ou inválido retorna erro. Arquivo inválido nunca é ignorado.
- Consultas HTTP usam as variáveis padrão quando presentes; AWS só consulta após configurar explicitamente `params.profile`.
- **Códigos de saída (`--once` e `--json`):**
  - `0`: todas as contas atualizaram com sucesso (`ready`)
  - `2`: resultados com configuração ausente ou falha em conta
  - `1`: erro local (argumentos ou arquivo inválido)

## Atalhos de teclado e mouse na TUI

- **Contas:** `a` adiciona conta, `e` edita a selecionada, `d` ou `Delete` exclui a conta selecionada (com confirmação `s` para gravar ou `n`/`Esc` para cancelar).
- **Navegação:** `↑↓`/`j k` selecionam, `PgUp/PgDn` paginam, `Home/End` vão às extremidades, `Enter` abre detalhes da conta (`←→` alternam entre contas visíveis nos detalhes; `↑↓` ou `[`/`]` rolam detalhes; `Enter`/`Esc` voltam).
- **Filtro e visualização:** `/` filtra, `Enter/Esc` encerram edição do filtro, `c` limpa o filtro, `u` mostra/oculta contas sem dados (visíveis por padrão).
- **Atualização:** `r` atualiza todas as contas e `R` atualiza somente a conta selecionada.
- **Reordenação:** `K`/`J` movem a conta selecionada para cima/baixo na ordem persistida em `cuotai.toml` (compartilhada com o app Desktop, preservando comentários de cada conta), sem disparar consultas aos provedores.
- **Saída:** `q` ou `Ctrl-C`.
- **Dimensões mínimas do terminal:** painel `45 × 18`; wizard `60 × 20`. Tema Dracula.

### Suporte a mouse

- Clique seleciona um cartão; clique no cartão já selecionado abre detalhes.
- Nos detalhes, clique na parte visível de uma fonte HTTPS única para abri-la no navegador; fontes compostas permanecem texto rolável. Botão direito volta dos detalhes.
- A roda do mouse navega pelas linhas de cartões e rola detalhes, listas do wizard e instruções de login.
- O rodapé possui ações clicáveis; o wizard oferece `[Voltar]`, `[Próximo]`, `[Gravar]` e `[Cancelar]`. A roda nunca confirma login, salvamento ou exclusão. A captura de mouse é liberada ao sair ou em caso de panic.

## Painel de quotas

- Os cartões (duas colunas em terminais largos) mantêm a ordem do arquivo de configuração, mesmo quando o resultado de uma consulta muda.
- Consultas independentes rodam em paralelo até `concurrency`; durante a atualização os dados anteriores permanecem visíveis. O cabeçalho mostra a contagem regressiva da próxima consulta da conta selecionada.
- Barras representam percentual **restante**; valores usados acima de 100% são preservados e apenas o desenho é limitado à largura da barra.
- Custos/saldos sem limite conhecido mostram valores em texto, sem inventar percentual ou "ilimitado". Saldo extra zero não torna esgotada uma assinatura que ainda tem quota limitada disponível.
- Excluir uma conta remove apenas a entrada da configuração: não apaga variáveis de ambiente, entradas do Keychain nem revoga acesso no provedor.

## Configurar uma conta pelo wizard

Na primeira execução interativa sem arquivo de configuração (ou com lista de contas vazia), a TUI abre o **wizard de contas**. Não é necessário editar TOML manualmente (para configuração manual, consulte [`cuotai.example.toml`](../cuotai.example.toml)).

1. **Ambiente (opcional):** Para credencial por ambiente, exporte o access token no terminal que executará o Cuotai (`export MEU_TOKEN`). O Cuotai não carrega `.env` automaticamente. Para login OAuth integrado, veja [Autenticação OAuth e Cofre](oauth.md).
2. **Abrir o wizard:** Execute `cargo run -p cuotai-tui`. Se já houver configuração, pressione `a` para adicionar ou selecione uma conta e pressione `e` para editar.
3. **Provedor:** Selecione com as setas e confirme com `Enter`.
4. **Nome da conta:** Informe um nome único, como `Codex pessoal`.
5. **Credencial:** Use o **nome da variável** (ex.: `MEU_TOKEN`), ou pressione **Tab** para login OAuth nos provedores suportados. O wizard indica `PRESENTE`/`AUSENTE` para variáveis, sem revelar valores.
   - AWS solicita o perfil CLI e oferece **F2** para login SSO.
   - Para Alibaba Token Plan, **F2** inicia `bl auth login --console --console-site international --config <perfil>` (requer `bl` instalado); o Cuotai consulta a quota com `bl usage token-plan --config <perfil> --output json`. `params.session_env` continua disponível para consulta HTTP direta com Cookie, sem CLI.
6. **Parâmetros:** Preencha os campos específicos do provedor; `Enter` avança entre eles. Opcionais podem ficar vazios. Copilot oferece escolhas com `←→`, mostrando organização apenas no modo `org`.
7. **Intervalo:** Informe de `30` a `86400` segundos para **esta conta** (mínimo de `300` s para AWS Bedrock); vazio herda o `refresh_seconds` global do TOML (padrão: `300`). Contas com intervalos próprios são agendadas separadamente. AWS exibe um aviso de cobrança por consulta; considere `3600` segundos ou mais.
8. **Revisão:** Confira conta, parâmetros e arquivo de destino; `↑↓` rolam o resumo. Só o `Enter` nesta etapa grava e aplica a configuração, iniciando a consulta no painel.

`Shift+Tab` volta mantendo o rascunho; `Ctrl+U` limpa o campo; `Backspace` apaga; `Esc` cancela sem salvar. A mudança de provedor redefine os campos e a referência de credencial para evitar reutilizar o token do provedor anterior.

### Persistência atômica do TOML

A gravação usa arquivo temporário e substituição atômica, com permissão `0600` para arquivos novos no Unix. Edições em tabelas `[[accounts]]` preservam ordem, comentários da conta e demais parâmetros/configurações. Se o arquivo mudar externamente, o wizard recusa sobrescrevê-lo e orienta fechar/reabrir o Cuotai. Em falha de gravação, o rascunho permanece na revisão para tentar novamente.

Tokens fornecidos por variável continuam sendo gerenciados externamente: se atualizar uma variável no shell, reinicie o Cuotai. Credenciais obtidas pelo login integrado são recuperadas do cofre e renovadas automaticamente antes da consulta quando próximas de expirar.
