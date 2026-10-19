# PRD 0013 — `watch` e hooks de eventos

| Campo | Valor |
|---|---|
| ID | 0013 |
| Status | Proposto |
| Responsável | `cuotai-core` (motor de eventos e hooks), TUI/CLI e Desktop |
| Consumidores | `crates/cuotai-tui` (`--watch`), `apps/desktop` (Configurações → Integração), scripts do usuário |
| Depende de | PRD 0012 (RF1: contrato `export::v1`); aproveita os PRDs 0003, 0008, 0009 e 0011 quando existirem |
| Data | 2026-10-19 |

## 1. Contexto

O Cuotai só reage a mudanças de estado dentro das interfaces: o cartão muda de cor, a bandeja atualiza o anel. Quem quer acionar outra ferramenta (tocar um som, trocar de agente, escrever num arquivo, avisar num chat próprio) precisa consultar `--json` em laço e comparar manualmente os resultados, repetindo consultas e perdendo a noção de transição.

O CodexBar tem `hooks watch`, que consulta os provedores e dispara regras em **transições reais** de quota e de estado, com eventos disparados na borda (a condição que persiste não repete), limite de uma tentativa por 600 s por conta e erro reportado como categoria grosseira (`timeout`, `offline`, `auth_required`, `network_error`), sem repassar o erro bruto do provedor. O ClaudeBar oferece hooks de sessão do Claude Code.

## 2. Problema

- Integrar o Cuotai a scripts exige polling manual, com falsos positivos e custo.
- Não há um conjunto padronizado de eventos (quota cruzou limiar, esgotou, reiniciou, conta falhou).
- Rodar comandos arbitrários a partir de dados de provedores é perigoso se mal desenhado (injeção, vazamento).

## 3. Objetivos

- Definir um vocabulário estável de eventos derivados de transições entre leituras consecutivas.
- Oferecer `cuotai --watch`, que emite os eventos em fluxo (NDJSON) e/ou dispara hooks configurados.
- Garantir que hooks são opt-in, isolados, com tempo limite e sem segredos.

## 4. Não-objetivos

- Executar ações que alterem contas dos provedores (trocar plano, gastar crédito, fazer failover): hooks recebem dados, não controlam provedores.
- Fila persistente ou entrega garantida: eventos perdidos com o processo parado não são recuperados.
- Webhooks HTTP para a internet nesta versão (um hook pode chamar `curl` por conta e risco do usuário).
- Substituir as notificações nativas do desktop (PRDs 0003 e 0008).

## 5. Requisitos funcionais

### RF1 — Motor de eventos no core (prioridade alta; pré-requisito)

- **RF1.1** Criar `events::diff(prev: &ExportAccount, next: &ExportAccount, state: &mut EventState, now) -> Vec<Event>`, função pura que compara duas leituras consecutivas e emite eventos de borda. Reaproveita o planejador de reinícios (PRD 0003), o avaliador de limiares (PRD 0008) e a vinculante (PRD 0009) quando existirem, sem duplicar suas regras.
- **RF1.2** Tipos de evento (nomes estáveis): `quota_threshold_crossed`, `quota_exhausted`, `quota_recovered`, `quota_reset`, `account_blocked`, `account_unblocked`, `refresh_failed`, `refresh_recovered`, `auth_required`, `provider_incident` (se o PRD 0011 existir) e `usage_updated`.
- **RF1.3** Payload comum: `schema`, `event`, `occurred_at`, `account_key`, `account_name`, `provider`, `metric` (rótulo e janela, quando aplicável), valores relevantes e `previous`. `refresh_failed` carrega apenas uma categoria (`timeout`, `offline`, `auth_required`, `rate_limited`, `schema_changed`, `provider_error`), nunca a mensagem bruta, URL ou corpo.
- **RF1.4** Disparo na borda: uma condição que persiste não repete o evento. A primeira leitura só estabelece a linha de base e não emite eventos de transição; `usage_updated` é limitado a uma tentativa por 600 s por conta.
- **RF1.5** O estado de transição fica em memória; reiniciar o `watch` recria as linhas de base. Isso é documentado.
- **Critério de aceite:** testes de tabela com sequências de leituras (subida, queda, reinício, oscilação, erro e recuperação, primeira leitura), e teste de que nenhuma carga contém credenciais, URLs ou mensagens brutas.

### RF2 — `cuotai --watch` (prioridade alta)

- **RF2.1** `cuotai --watch [--interval S] [--events lista] [--jsonl]`: roda o ciclo de refresh do core (sem TUI), compara cada leitura com a anterior e emite os eventos selecionados. Intervalo mínimo de 60 s (valores menores são rejeitados, não ajustados em silêncio). Contas obedecem ao intervalo efetivo e ao piso de 300 s do AWS Bedrock.
- **RF2.2** `--jsonl` escreve um evento por linha em `stdout` (NDJSON) com `schema` e flush a cada linha; mensagens humanas e diagnósticos vão para `stderr`.
- **RF2.3** Sem `--jsonl` e sem hooks, imprime um resumo legível das transições.
- **RF2.4** Encerra de forma limpa em `SIGINT`, `SIGTERM` e `SIGHUP`. Códigos: 0 encerramento normal; 1 erro local; 2 se terminou com contas em falha persistente.
- **RF2.5** Pode coexistir com `--serve` (PRD 0012) no mesmo processo, compartilhando o ciclo.
- **Critério de aceite:** teste de integração com servidor `wiremock` e relógio acelerado; smoke com `jq` consumindo o NDJSON; encerramento limpo.

### RF3 — Hooks configuráveis (prioridade média)

- **RF3.1** Configuração em arquivo próprio, `hooks.toml`, ao lado do `cuotai.toml` (no mesmo diretório de configuração), nunca dentro das contas e sem segredos. Cada regra tem: `name`, `events` (lista), filtros opcionais (`provider`, `account`, `min_threshold`) e `command` como **vetor de argumentos** (`["/caminho/script", "--flag"]`), sem `sh -c` implícito.
- **RF3.2** Desligado por padrão. Os hooks só rodam com `--watch --hooks` ou ao serem habilitados no desktop, e a primeira execução mostra a lista de comandos e exige confirmação explícita do usuário. Alterar o arquivo exige nova confirmação (hash dos comandos).
- **RF3.3** Execução: o evento chega por `stdin` como JSON e por variáveis de ambiente `CUOTAI_EVENT`, `CUOTAI_ACCOUNT`, `CUOTAI_PROVIDER` e `CUOTAI_METRIC` (valores sempre tratados como dados; nunca interpolados no comando). Ambiente do processo filho reduzido a uma lista permitida (`PATH`, `HOME`, `LANG`), sem herdar variáveis de credencial. Diretório de trabalho neutro.
- **RF3.4** Limites: tempo limite padrão de 10 s (configurável até 60 s), saída capturada limitada a 16 KiB, no máximo 4 hooks simultâneos e fila limitada com descarte do mais antigo e aviso. Falhas de hook nunca derrubam o `watch` nem alteram o ciclo de consultas.
- **RF3.5** Cada execução registra, em log local sem payload, o nome da regra, o evento, a duração e o código de saída.
- **Critério de aceite:** testes com scripts de fixture: argumento com aspas e `;` chega como dado; ambiente reduzido (credenciais ausentes); tempo limite mata o processo; saída acima do limite é truncada; confirmação obrigatória; hash alterado exige reconfirmação.

### RF4 — Desktop (prioridade baixa)

- **RF4.1** Em **Configurações → Integração**, um editor da lista de regras (nome, eventos, comando como lista de argumentos), botão de teste que dispara um evento de exemplo e registro das últimas execuções. O desktop usa o mesmo motor do core sobre o ciclo já em curso.
- **RF4.2** O fluxo de confirmação do RF3.2 aparece como diálogo com a lista exata de comandos.
- **Critério de aceite:** smoke no app empacotado: criar regra, confirmar, disparar teste, ver o registro e desligar.

## 6. Requisitos não funcionais

- **Segurança:** hooks executam código do usuário. O desenho evita injeção (argumentos em vetor, dados via `stdin` e ambiente), isola o ambiente e exige confirmação. O Cuotai nunca baixa, gera nem modifica comandos.
- **Privacidade:** payloads não contêm credenciais, URLs, notas livres nem mensagens brutas de erro.
- **Custo:** `watch` usa exatamente o mesmo agendamento do app; não cria consultas extras.

## 7. Ordem de execução e verificação

1. **RF1:** motor de eventos com testes de tabela.
2. **RF2:** `--watch` e NDJSON.
3. **RF3:** hooks, com os testes de isolamento.
4. **RF4:** desktop.

Em cada etapa: `cargo fmt`, `clippy -D warnings`, testes do workspace e do desktop e smoke da interface alterada.

O requisito só está entregue quando um script recebe exatamente um evento por transição real, uma condição persistente não o repete, os hooks só rodam após confirmação, nunca recebem credenciais no ambiente nem no payload, e uma falha de hook não afeta o monitoramento.
