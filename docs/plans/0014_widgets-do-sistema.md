# PRD 0014 — Widgets do sistema

| Campo | Valor |
|---|---|
| ID | 0014 |
| Status | Proposto |
| Responsável | Desktop (macOS) e `cuotai-core` (arquivo de snapshot) |
| Consumidores | `apps/desktop`, extensão de widget (Swift), integrações de terceiros (Waybar, GNOME, KDE) que leem o mesmo arquivo |
| Depende de | PRD 0012 (RF1: contrato `export::v1`); PRD 0019 (assinatura e perfil do desenvolvedor); aproveita os PRDs 0006, 0007, 0009 e 0010 |
| Data | 2026-10-19 |

## 1. Contexto

O desktop do Cuotai é um aplicativo Tauri 2 (`apps/desktop`, identificador `dev.cuotai.desktop`) com janela principal e popover de bandeja. Não existe widget do sistema. O backend mantém o `RefreshCycle` e o estado de todas as contas em memória; a webview pode ser suspensa quando a janela está escondida.

Na análise de mercado de 2026-10-19, o CodexBar publica 8 tipos de widget do macOS (WidgetKit): Switcher, Usage, Account Usage, Accounts, History, Metric, Burn Down e Burn Down combinado, em pequeno, médio e grande. Eles funcionam assim: o app grava um snapshot JSON compacto no container do *app group* e a extensão apenas lê e desenha. Há cuidados explícitos: nunca mostrar outra conta quando a escolhida falha, manter a idade original da medição em dados antigos e não inventar percentual quando a capacidade é desconhecida. O AIQuota e o Quotio (iOS) também têm widgets.

## 2. Problema

Ver a quota exige abrir o app ou o popover. Quem mantém o painel de widgets do macOS à vista não encontra o Cuotai ali, embora o dado já exista no backend.

## 3. Objetivos

- Publicar o estado do Cuotai em um arquivo de snapshot estável e pequeno, legível por extensões de widget e por integrações de terceiros.
- Entregar widgets do macOS (WidgetKit) que apenas leem esse arquivo.
- Manter a regra do projeto: nenhuma lógica de negócio na extensão; ela desenha o que o core já calculou.

## 4. Não-objetivos

- Widgets para iOS/iPadOS, Windows e Linux nesta versão. Para esses sistemas, o arquivo de snapshot e o servidor do PRD 0012 permitem integrações de terceiros.
- Consultar provedores a partir da extensão: a extensão nunca faz rede nem lê credenciais.
- Widgets interativos que alterem contas ou configuração.

## 5. Requisitos funcionais

### RF1 — Arquivo de snapshot para widgets (prioridade alta; pré-requisito)

- **RF1.1** Em `cuotai-core`, `widget_snapshot::write(export: &Export, path)` grava um subconjunto compacto do contrato `export::v1` do PRD 0012: por conta, `key`, `name`, `provider`, a vinculante (PRD 0009), até N métricas elegíveis com fração, `unit`, `window`, `resets_at`, `checked_at`, `freshness` e o ritmo (PRD 0007, quando existir). Sem credenciais, sem notas livres, sem e-mail por padrão.
- **RF1.2** Escrita atômica (arquivo temporário e `rename`), permissão `0600`, tamanho máximo de 256 KiB com truncamento determinístico (contas ordenadas pela vinculante), campo `schema` próprio e versionado.
- **RF1.3** Dados desatualizados mantêm o `checked_at` original; contas em falha sem leitura anterior ficam como `unavailable`, nunca com valores de outra conta.
- **RF1.4** A escrita acontece depois de cada mudança relevante do `RefreshCycle` (ciclo completo ou conta) e é limitada a uma gravação a cada 15 segundos, em tarefa separada, para não atrasar o refresh.
- **Critério de aceite:** testes de gravação atômica, limite de tamanho, ausência de segredos, conta indisponível sem dados emprestados e limitação de frequência.

### RF2 — Localização e acesso ao arquivo no macOS (prioridade alta)

- **RF2.1** O arquivo fica no container do *app group* (`group.dev.cuotai.desktop`, nome a confirmar), compartilhado entre o app e a extensão. O app (não sandboxed) escreve; a extensão (sandboxed, como o WidgetKit exige) lê.
- **RF2.2** Esse caminho só é usado quando os widgets estão ligados em **Configurações → Widgets**; com o recurso desligado, nenhum arquivo é escrito e o existente é apagado.
- **RF2.3** Documentar a necessidade de conta de desenvolvedor Apple, *entitlements* de *app group* e *provisioning profile* (dependência do PRD 0019). Sem eles, o recurso aparece desabilitado com explicação.
- **Critério de aceite:** teste do caminho e do ciclo ligar/desligar; verificação manual dos *entitlements* no app assinado.

### RF3 — Extensão WidgetKit (prioridade alta)

- **RF3.1** Projeto Swift mínimo em `apps/desktop/widgets/macos`, com a extensão de widget empacotada no bundle (`PlugIns/`). A forma de embutir o `.appex` no pacote do Tauri (por exemplo, a configuração `bundle.macOS.files`) deve ser validada na implementação; se não for suportada, descrever o passo de pós-processamento no pipeline do PRD 0019.
- **RF3.2** Conjunto inicial de widgets:
  - **Resumo**: a vinculante de cada conta (pequeno, médio, grande);
  - **Conta**: uma conta fixada por escolha do usuário (*App Intent*), sem cair para outra conta se ela desaparecer;
  - **Histórico**: gráfico de área com os dados do PRD 0006, apenas se o histórico estiver ligado (médio).
- **RF3.3** Cada widget mostra a idade da medição (texto relativo nativo, que o sistema avança sozinho) e o estado `Desatualizado` quando aplicável; barras atenuadas para dados velhos; nenhuma animação.
- **RF3.4** Textos nos três idiomas, vindos do próprio arquivo de snapshot (já localizados pelo core) ou de catálogo Swift mínimo para rótulos fixos. Acessibilidade: rótulos legíveis por VoiceOver e contraste em tema claro e escuro.
- **RF3.5** Estado vazio claro ("Abra o Cuotai para começar") quando não há arquivo; nunca dados de exemplo disfarçados de reais.
- **Critério de aceite:** verificação em Mac limpo: adicionar os três widgets, mudar uma conta no app, observar a atualização, fechar o app (widgets mantêm o último dado com a idade), remover uma conta fixada (widget mostra indisponível), desligar o recurso (widgets mostram o estado vazio).

### RF4 — Configurações (prioridade média)

- **RF4.1** **Configurações → Widgets**: chave global, "incluir identidade (e-mail)" (desligada por padrão), "máximo de contas publicadas" (padrão 6), local do arquivo e botão "Apagar dados dos widgets".
- **RF4.2** Atualização dos widgets depende do sistema; o texto explica que o macOS decide quando redesenhar e que o dado exibido pode atrasar.
- **Critério de aceite:** smoke em três idiomas; preferências persistem em `desktop.json`.

### RF5 — Compatibilidade fora do macOS (prioridade baixa)

- **RF5.1** Em Windows e Linux, o mesmo `widget_snapshot` pode ser escrito em `<dados locais>/cuotai/widget.json` com `0600`, para integrações de terceiros (Waybar, painéis GNOME e KDE, barras de status), documentado em `docs/export-v1.md`. Nenhum widget nativo é entregue nessas plataformas por este PRD.
- **Critério de aceite:** teste do arquivo nos três sistemas; documentação do formato.

## 6. Requisitos não funcionais

- **Privacidade:** o arquivo pode ser lido por qualquer processo do mesmo usuário (e pela extensão); por isso não contém credenciais, e-mails por padrão nem notas livres.
- **Honestidade:** dados antigos mantêm a idade original e nunca são apresentados como atuais.
- **Energia:** gravação limitada em frequência; nenhum polling na extensão.
- **Risco principal:** a combinação Tauri + extensão Swift + *app group* exige conta paga da Apple e ajustes de assinatura; validar em um protótipo antes de comprometer o escopo.

## 7. Ordem de execução e verificação

1. **RF1:** arquivo de snapshot, testes de contrato e de segredos.
2. **Protótipo de empacotamento** (parte do RF3.1): provar que o `.appex` sobrevive à geração do bundle e à assinatura.
3. **RF2 e RF3:** local do arquivo e widgets.
4. **RF4 e RF5:** configurações e arquivo para outros sistemas.

Em cada etapa: `cargo fmt`, `clippy -D warnings`, testes do workspace e do desktop e smoke da interface alterada.

O requisito só está entregue quando, em um macOS limpo com o app assinado, o widget de resumo mostra a conta vinculante com a idade da medição, continua exibindo o último dado com idade correta com o app fechado, nunca exibe os dados de outra conta, e desligar o recurso apaga o arquivo e esvazia os widgets.
