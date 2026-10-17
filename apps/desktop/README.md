# Cuotai Desktop (Tauri 2)

Front-end multiplataforma: janela principal + popover na bandeja. Sem npm/bundler:
a UI é HTML/CSS/JS puro em `ui/` e o shell é Rust em `src-tauri/`. Só lê `cuotai-core`.

As janelas de 5 horas/semanais e a exclusão de rate limits do resumo usam diretamente `Metric.kind` e `Metric.window` do core; o shell e o JS não classificam métricas pelo texto de `label` ou `period`.

```sh
cd apps/desktop/src-tauri
cargo run                 # desenvolvimento
cargo install tauri-cli   # opcional, para empacotar
cargo tauri build
```

Config: `CUOTAI_CONFIG` > arquivo escolhido em **Configurações → Arquivo de contas** (gravado em `desktop.json`, nunca no TOML) > `<config do SO>/cuotai/config.toml`; o desktop nunca lê `./cuotai.toml` do diretório atual. Para ver as mesmas contas da TUI, escolha o mesmo `cuotai.toml`; **Usar padrão do sistema** volta ao caminho do SO. Trocar o arquivo recarrega as contas pelo core e descarta o estado da configuração anterior. O caminho efetivo aparece com a origem (variável, escolhido ou padrão do sistema). Se o arquivo não existir, o primeiro `Salvar` o cria com permissão 0600.
Temas: `ui/css/themes.css` (+ `DARK_THEMES` em `ui/js/lib.js`); padrão escuro = Dracula.

No painel, **Adicionar conta** ou **Editar** abre o formulário. Escolha **Login no navegador** (ou **AWS SSO** / **Console Bailian**, conforme o provedor) e pressione **Entrar**. Para Kiro, escolha método, região e URL inicial quando usar Identity Center; cada conta deve ser autorizada separadamente. O diálogo exibe a URL e o código de dispositivo para copiar; Xiaomi MiMo também aceita o código colado da tentativa atual. **Cancelar login** interrompe a espera antes de gravar. **Salvar** persiste a credencial OAuth no cofre do SO e a conta no TOML; falha na gravação preserva a autorização em memória para tentar novamente. AWS CLI e `bl` mantêm suas próprias credenciais fora do cofre Cuotai. Os modos, as opções de fluxo e os campos de registro (`client_id`/`te…

No painel, arraste a alça do cartão, use as setas de mover conta ou foque a alça e pressione ↑/↓. A nova ordem é gravada em `cuotai.toml`, compartilhada com a TUI e não inicia consultas de provedores; comentários de cada conta são mantidos.

**Remover conta** abre o diálogo: para contas com credencial OAuth salva, escolha **Remover e manter no cofre** ou **Remover e apagar do cofre**. O TOML é sempre salvo primeiro; a purga do cofre roda depois e, se falhar (cofre indisponível) ou for recusada por outra conta sobrevivente compartilhar a credencial, o aviso do painel inclui um botão **Tentar apagar a credencial agora** que re-executa a purga contra a configuração atual (apaga a credencial assim que a conta sobrevivente também for removida; não deleta nada enquanto a sobrevivente existir). A recuperação usa um identificador opaco preservado em memória até a purga dar certo, e é descartado em qualquer recarga de configuração.

**Conflito de configuração (RF4):** se o `cuotai.toml` mudou no disco desde o carregamento (edição externa, `git pull`, segundo processo), o próximo **Salvar** recusa gravar e mostra a mensagem tipada `edit_error.conflict` com um botão **Recarregar arquivo do disco**. Recarregar reabre o arquivo, reconstrói o monitor/agenda/tokens, descarta identificadores de recuperação (eles referenciam a configuração anterior) e mantém o rascunho do diálogo pronto para nova tentativa. O mesmo detector barato roda no ganho de foco da janela e antes de abrir o editor de conta, oferecendo **Recarregar agora** ou **Continuar editando**. Erros de validação (campo vazio, nome duplicado, intervalo inválido, opção inválida) caem automaticamente no campo correspondente (`name`, `credential`, `params.<chave>`, `interval`, ...) em vez de só um aviso global.
