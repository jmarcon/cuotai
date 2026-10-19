# PRD 0017 — Extensões por manifesto e script

| Campo | Valor |
|---|---|
| ID | 0017 |
| Status | Proposto |
| Responsável | `cuotai-core`, TUI e Desktop |
| Consumidores | `crates/cuotai-tui`, `apps/desktop`, autores de extensões |
| Depende de | PRD 0005 (RF1: nível `Extension`) |
| Data | 2026-10-19 |

## 1. Contexto

Todo provedor do Cuotai é um adaptador Rust compilado dentro do `cuotai-core` (`providers/*.rs`), escolhido por `match` exaustivo sobre `Provider` (36 variantes). Acrescentar uma fonte, por menor que seja (um painel interno da empresa, uma API de um provedor pequeno, o saldo de um proxy próprio), exige alterar o código, recompilar e publicar uma nova versão. O catálogo de modelos e a tela Sobre já seguem a ideia de que listas e textos pertencem ao core, mas não há ponto de extensão para dados de terceiros.

Na análise de mercado de 2026-10-19, o ClaudeBar aceita "qualquer fonte de quota que um script imprima" por um manifesto e um script em `~/.claudebar/extensions/`; o CodexBar documenta um guia de autoria de provedores e um desenho de provedor customizado.

## 2. Problema

- Fontes específicas de um usuário ou de uma empresa nunca entrarão no catálogo oficial.
- Hoje a única saída é um fork.
- Um mecanismo de extensão mal feito executa código arbitrário com os poderes do usuário e transforma o monitor somente-leitura em risco.

## 3. Objetivos

- Permitir que o usuário adicione fontes de consumo por meio de um manifesto declarativo e de um executável local que imprime JSON, sem recompilar o Cuotai.
- Manter o princípio somente-leitura: a extensão informa números, o Cuotai nunca lhe entrega credenciais do usuário nem lhe concede outros poderes.
- Tratar a extensão como código não confiável do ponto de vista dos dados: validar tudo o que ela devolve.

## 4. Não-objetivos

- Plugins carregados no processo (bibliotecas dinâmicas ou WASM) e API de interface gráfica.
- Distribuição, loja ou atualização de extensões.
- Executar extensões em segundo plano sem o usuário ter habilitado cada uma.
- Sandbox de sistema operacional completo (o PRD define mitigações, não promete isolamento forte).

## 5. Requisitos funcionais

### RF1 — Manifesto e descoberta (prioridade alta; pré-requisito)

- **RF1.1** Diretório de extensões no diretório de dados do usuário (`<dados>/cuotai/extensions/<id>/`). Cada extensão tem `manifest.toml` com: `id` (slug), `name`, `version`, `schema` (versão do contrato, 1), `command` como **vetor de argumentos** relativo ao diretório da extensão, `interval_seconds` (mínimo 60), `timeout_seconds` (padrão 15, máximo 60), `description` e `homepage` opcional, e `env` (lista dos **nomes** de variáveis de ambiente que a extensão pede).
- **RF1.2** O core descobre os manifestos sem executar nada, valida os campos (`deny_unknown_fields`) e expõe a lista com o motivo de cada rejeição. Manifesto inválido nunca derruba o carregamento das demais extensões nem do restante da configuração.
- **RF1.3** Uma extensão aparece como um tipo de conta de catálogo "Extensão: {name}" no assistente, com o nível de acesso `Extension` (PRD 0005) e o aviso "executa um programa local; revise o código antes de ativar".
- **Critério de aceite:** testes de descoberta com manifestos válidos, campos desconhecidos, `interval_seconds` abaixo do mínimo, comando fora do diretório da extensão e ids duplicados.

### RF2 — Contrato de saída (prioridade alta)

- **RF2.1** A extensão imprime em `stdout` um único objeto JSON `{ "schema": 1, "status": "ready", "metrics": [...], "notes": [...] }`. Cada métrica usa os mesmos campos de `Metric` (`label`, `used`, `limit`, `remaining`, `unit`, `kind`, `window`, `period`, `resets_at`); ausência é `null` e nunca zero.
- **RF2.2** O core valida o JSON contra o contrato: tamanho máximo de 256 KiB, no máximo 64 métricas, textos truncados, números finitos, datas válidas, `kind` e `window` conhecidos, `unit` curta. Resposta inválida produz um relatório `Error` com categoria `invalid_output`, sem repassar o conteúdo.
- **RF2.3** O `Report` resultante usa `source = "extension:<id>"`, e as notas livres da extensão passam por limpeza de marcação e truncamento. O core nunca confia em `status` ou texto vindos da extensão para decidir comportamento.
- **RF2.4** Um comando `cuotai --extension-check <id>` executa a extensão uma vez, mostra o relatório normalizado e as violações do contrato.
- **Critério de aceite:** testes de tabela com saídas válidas, parciais, grandes demais, com `NaN`/infinito, `kind` desconhecido e JSON malformado.

### RF3 — Execução isolada (prioridade alta)

- **RF3.1** Execução sem shell, com o vetor do manifesto como `argv`, diretório de trabalho na pasta da extensão e `stdin` fechado.
- **RF3.2** Ambiente do processo filho reduzido a `PATH`, `HOME`, `LANG` e **apenas** as variáveis que o manifesto lista em `env` e que o usuário aprovou. O Cuotai nunca injeta tokens do cofre nem credenciais de outras contas.
- **RF3.3** Limites: tempo máximo do manifesto, saída limitada, processos órfãos encerrados ao fim do tempo (grupo de processos), no máximo 2 extensões simultâneas e prioridade baixa. Rede e arquivos não são restringidos pelo Cuotai; a documentação diz isso claramente.
- **RF3.4** Falha ou lentidão de uma extensão não afeta o ciclo das demais contas, aplicando o mesmo prazo por conta e o mesmo backoff do `RefreshCycle`.
- **Critério de aceite:** testes com scripts de fixture: ambiente reduzido, tempo limite com filho órfão, saída excessiva, e extensão lenta que não atrasa outras contas.

### RF4 — Consentimento e integridade (prioridade alta)

- **RF4.1** Toda extensão nasce **desabilitada**. Habilitar exige uma tela com o comando completo, o diretório, as variáveis de ambiente pedidas e o intervalo, e uma confirmação explícita.
- **RF4.2** O core guarda o digest (SHA-256) do manifesto e do executável no momento da aprovação. Qualquer mudança desativa a extensão até uma nova aprovação.
- **RF4.3** Nenhuma extensão é instalada, baixada ou atualizada pelo Cuotai.
- **Critério de aceite:** testes de aprovação, mudança do executável (desativa) e mudança do manifesto (desativa).

### RF5 — Interfaces (prioridade média)

- **RF5.1** TUI e desktop listam extensões (estado, última execução, erro), permitem habilitar, desabilitar e testar, e criam a conta correspondente com o mesmo fluxo de contas de catálogo. As interfaces não têm regras específicas de extensão além da lista fornecida pelo core.
- **RF5.2** Documentação `docs/extensions.md` com o contrato, um exemplo mínimo em shell e outro em Python, orientação de segurança e política de compatibilidade do `schema`.
- **Critério de aceite:** smoke em três idiomas: habilitar, ver métricas no cartão com a etiqueta `Extensão`, desabilitar e remover.

## 6. Requisitos não funcionais

- **Segurança:** extensões executam com os poderes do usuário. O desenho limita o que o Cuotai entrega a elas (nada de credenciais), valida o que elas devolvem e exige consentimento e integridade. O risco residual (a extensão faz o que quiser com a rede e os arquivos do usuário) é documentado.
- **Somente-leitura:** o contrato não tem ações; a extensão devolve dados.
- **Compatibilidade:** `schema` versionado, aditivo dentro da versão.

## 7. Ordem de execução e verificação

1. **RF1 e RF2:** manifesto, contrato e `--extension-check`.
2. **RF3 e RF4:** execução isolada, consentimento e integridade.
3. **RF5:** interfaces e documentação.

Em cada etapa: `cargo fmt`, `clippy -D warnings`, testes do workspace e do desktop e smoke da interface alterada.

O requisito só está entregue quando uma extensão de exemplo aparece como conta, só roda depois de aprovada, deixa de rodar se seu executável mudar, nunca recebe credenciais do Cuotai, e uma saída inválida ou lenta vira um erro isolado sem afetar as demais contas.
