# PRD 0011 — Status de incidentes dos provedores

| Campo | Valor |
|---|---|
| ID | 0011 |
| Status | Proposto |
| Responsável | `cuotai-core`, Desktop e TUI |
| Consumidores | `apps/desktop` (cartões, bandeja), `crates/cuotai-tui`, PRDs 0012 e 0013 |
| Depende de | PRD 0005 (RF1) para distinguir "provedor com incidente" de "interface mudou"; não é exigido |
| Data | 2026-10-19 |

## 1. Contexto

Quando uma consulta falha, o `Report` só informa `Status` (`AuthenticationError`, `RateLimited`, `Error`…) e uma nota. O usuário não sabe se o problema é a credencial dele, a interface do provedor ou uma indisponibilidade pública. Isso gera suporte inútil, como a troca desnecessária de credenciais durante uma queda do serviço.

O CodexBar consulta as páginas de status dos provedores e mostra selos de incidente no menu e uma sobreposição no ícone. Essas páginas são públicas e não exigem credencial, o que combina com a regra do projeto de ser somente-leitura e sem gasto de tokens.

## 2. Problema

- Falhas de consulta provocadas por incidentes do provedor parecem falhas do Cuotai ou da conta.
- Não há contexto para decidir entre reautenticar, esperar ou ignorar.

## 3. Objetivos

- Consultar fontes públicas de status por provedor, de forma leve e cacheada, e exibir o estado atual junto da conta.
- Relacionar uma falha de consulta com um incidente em andamento, sem afirmar causa.
- Funcionar sem credenciais e sem custo.

## 4. Não-objetivos

- Monitorar a saúde de provedores que não publicam status em formato legível por máquina.
- Raspar páginas HTML ou usar endpoints que exijam login.
- Alertas por e-mail ou push (o PRD 0013 permite um evento `provider_incident` para quem quiser integrar).
- Mudar o `Status` do relatório: o incidente é informação adicional, não substitui o estado da consulta.

## 5. Requisitos funcionais

### RF1 — Fontes e mapeamento verificado (prioridade alta; pré-requisito)

- **RF1.1** Criar `incidents::source(provider) -> Option<StatusSource>` com `match` exaustivo sobre `Provider`. `StatusSource` traz: formato (`StatuspageV2`, `AtomRss`, `JsonCustom`), URL pública, escopo (componentes relevantes para a API ou para o produto) e a data da última verificação.
- **RF1.2** Só entram fontes com URL pública **verificada na implementação** (formato, estabilidade e termos de uso). Provedores sem fonte confiável devolvem `None` e não aparecem como "sem incidentes". O mapeamento inicial deve cobrir os provedores com página de status no formato Statuspage v2 (`/api/v2/summary.json`), como é comum entre fornecedores de IA, mais os formatos nativos de AWS e Google Cloud, quando verificados.
- **RF1.3** Provedores que compartilham uma plataforma (por exemplo, vários produtos da mesma empresa) compartilham a mesma fonte, com filtro por componente para não alarmar por serviços sem relação.
- **Critério de aceite:** teste de tabela garante que todo `Provider` tem fonte verificada ou `None` explícito; fixtures com respostas reais anonimizadas de cada formato; documento `docs/providers.md` lista as fontes.

### RF2 — Cliente de status no core (prioridade alta)

- **RF2.1** `IncidentMonitor` em `cuotai-core` reaproveita o cliente HTTP e o semáforo do `Monitor`, com timeout curto, tamanho máximo de resposta e `User-Agent` identificado. Não envia credenciais, cookies nem dados da conta; o log nunca grava a URL com parâmetros sensíveis.
- **RF2.2** Cache por fonte com TTL de 5 minutos e uma única consulta por fonte por ciclo, mesmo com várias contas do mesmo provedor. Falha de status não afeta a consulta de quota nem o `Status` da conta e usa o último resultado conhecido, marcado como antigo.
- **RF2.3** Estados normalizados: `Operational`, `Degraded`, `PartialOutage`, `MajorOutage`, `Maintenance`, `Unknown`. O incidente ativo traz título, início e link público para a página de status. Texto livre do provedor passa por truncamento e remoção de marcação antes de chegar às interfaces.
- **RF2.4** Consulta somente quando há contas do provedor configuradas e o recurso está ligado.
- **Critério de aceite:** testes com `wiremock` para cada formato, resposta inválida, limite de tamanho, timeout, cache e coalescência por fonte, e falha sem efeito na consulta de quota.

### RF3 — Correlação com falhas (prioridade média)

- **RF3.1** Se a consulta de uma conta falha (`Error`, `RateLimited` ou `AuthenticationError`) e o provedor tem incidente `Degraded` ou pior, o core anexa uma nota localizada: "O provedor informa um incidente em andamento; a falha pode não ser da sua conta." A mensagem não altera o `Status` e não diz que o incidente é a causa.
- **RF3.2** `AuthenticationError` com incidente ativo sugere esperar antes de reautenticar, sem impedir a ação.
- **Critério de aceite:** testes de tabela por status e estado do incidente.

### RF4 — Exibição no desktop (prioridade média)

- **RF4.1** Selo de incidente no cartão (cor, ícone e texto) com link "Abrir página de status", aberto pelo `open_external` com lista de permissões já existente.
- **RF4.2** O ícone da bandeja ganha indicador quando qualquer provedor configurado tem incidente de nível `PartialOutage` ou pior.
- **RF4.3** Em **Configurações → Privacidade**: ligar e desligar a consulta de status (desligada por padrão no primeiro uso, com explicação de que o app fará pedidos públicos a esses domínios) e lista dos domínios consultados.
- **Critério de aceite:** smoke em três idiomas com incidente simulado, desligado e sem rede.

### RF5 — TUI e CLI (prioridade média)

- **RF5.1** A linha da conta mostra o selo; os detalhes listam o incidente. `--once` imprime a nota de incidente; `--json` permanece igual, e o estado do provedor entra no contrato do PRD 0012.
- **RF5.2** Flag `--no-status` desliga a consulta de status no modo não interativo.
- **Critério de aceite:** testes de renderização e snapshot textual.

## 6. Requisitos não funcionais

- **Privacidade:** pedidos públicos a domínios dos provedores revelam o IP do usuário e o fato de que ele usa aquele provedor; por isso o recurso é opt-in e documentado.
- **Robustez:** estado `Unknown` é a resposta correta para qualquer dúvida; nunca se afirma "operacional" sem leitura válida.
- **Custo:** pedidos leves, cacheados e sem credencial; nenhum efeito no agendamento de quotas.
- **Direitos:** respeitar termos de uso das páginas e `robots`/limites de taxa; sem raspagem de HTML.

## 7. Ordem de execução e verificação

1. **RF1:** mapeamento verificado e testes de tabela.
2. **RF2 e RF3:** cliente, cache e correlação.
3. **RF4:** desktop.
4. **RF5:** TUI e CLI.

Em cada etapa: `cargo fmt`, `clippy -D warnings`, testes do workspace e do desktop e smoke da interface alterada.

O requisito só está entregue quando uma queda simulada do provedor aparece como selo no cartão e na bandeja, a falha de consulta correspondente ganha a nota explicativa sem mudar seu `Status`, provedores sem fonte verificada nunca aparecem como "operacionais", e o recurso desligado não faz nenhum pedido de rede.
