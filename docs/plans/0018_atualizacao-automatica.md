# PRD 0018 — Atualização automática do desktop

| Campo | Valor |
|---|---|
| ID | 0018 |
| Status | Proposto |
| Responsável | Desktop (`apps/desktop`) e pipeline de release |
| Consumidores | `apps/desktop/src-tauri`, `apps/desktop/ui` (Configurações → Atualizações e Sobre) |
| Depende de | PRD 0019 (RF2 e RF3: pacotes assinados e hospedagem dos artefatos) |
| Data | 2026-10-19 |

## 1. Contexto

O desktop é um app Tauri 2 (`cuotai-desktop` 0.17.0, identificador `dev.cuotai.desktop`) que depende apenas de `tauri-plugin-dialog`. Não há plugin de atualização, não há `pubkey` em `tauri.conf.json` e não há local de onde buscar versões novas. O app fica residente na bandeja, então o usuário raramente o reinicia por conta própria e pode ficar meses numa versão antiga, que talvez dependa de interfaces de provedores que já mudaram.

Na análise de mercado de 2026-10-19: o CodexBar usa Sparkle e o Homebrew para atualização; o Quotio atualiza versões estáveis e beta pelo Sparkle, com apps separados para o canal beta; o OpenUsage usa Sparkle assinado e notarizado com canal beta opcional; o OpenQuota atualiza por pacotes assinados com chave do projeto, independente da assinatura do sistema; o WhatsMyUsage avisa que não consegue se atualizar quando roda do disco de instalação.

## 2. Problema

- Corrigir um adaptador quebrado por mudança de provedor só ajuda quem consegue atualizar o app.
- Sem canal confiável, o usuário instala manualmente e perde a continuidade.
- Atualização automática mal feita é um vetor clássico de comprometimento: precisa de assinatura, de verificação e de consentimento.

## 3. Objetivos

- Verificar versões novas e instalá-las com consentimento do usuário, pelo mecanismo oficial do Tauri 2.
- Garantir que só artefatos assinados com a chave do projeto sejam aceitos.
- Manter a privacidade: a verificação não envia dados do usuário, e o recurso pode ser desligado.

## 4. Não-objetivos

- Atualizar a TUI/CLI: ela chega por gerenciadores de pacote ou download (PRD 0019).
- Atualização silenciosa sem aviso, telemetria de uso ou coleta de identificadores.
- Reverter versões automaticamente (a documentação orienta a reinstalar uma versão anterior).
- Canal beta nesta versão (pode ser um PRD futuro).

## 5. Requisitos funcionais

### RF1 — Mecanismo e chaves (prioridade alta; pré-requisito)

- **RF1.1** Adotar `tauri-plugin-updater` 2.x, com a permissão correspondente em `capabilities/default.json`, e `bundle.createUpdaterArtifacts` habilitado.
- **RF1.2** Gerar um par de chaves de assinatura de atualização do Tauri. A chave **pública** vai em `tauri.conf.json` (`plugins.updater.pubkey`); a **privada** nunca entra no repositório e fica em um segredo do pipeline de release (PRD 0019), com procedimento documentado de rotação e de perda.
- **RF1.3** Endpoint de atualização estático (um `latest.json` por plataforma e arquitetura, hospedado junto dos artefatos de release), acessível por HTTPS. O plugin só aceita artefatos cuja assinatura confere com a chave pública embutida.
- **RF1.4** Versionamento: `latest.json` traz versão semântica, notas da versão em texto simples, data, URL e assinatura. Só se atualiza para versão **maior**; a mesma versão ou menor é ignorada.
- **Critério de aceite:** teste de integração com um servidor local simulando `latest.json`: versão maior com assinatura válida (aceita), assinatura inválida (rejeitada), versão igual ou menor (ignorada), manifesto malformado (erro tratado).

### RF2 — Fluxo no app (prioridade alta)

- **RF2.1** Verificação automática ao abrir e depois uma vez a cada 24 horas, apenas se o usuário não desligou (padrão: ligado, com explicação clara na primeira execução do que é enviado). A requisição leva apenas a versão atual, o sistema operacional e a arquitetura exigidos pelo endpoint, sem identificador de usuário ou de máquina.
- **RF2.2** Ao encontrar versão nova, exibir um aviso na janela principal e no menu da bandeja ("Atualização 0.18.0 disponível"), com as notas da versão (a seção correspondente do `CHANGELOG.md`, PRD 0019 RF7.5) e os botões **Atualizar agora** e **Lembrar depois**. Nada baixa ou instala sem clique.
- **RF2.3** Instalação: baixar, verificar a assinatura, aplicar e pedir reinício, mostrando progresso e permitindo cancelar o download. Em caso de falha (rede, assinatura, espaço, permissão), mostrar uma mensagem localizada, manter a versão atual intacta e oferecer o link manual de download.
- **RF2.4** Antes de reiniciar, salvar o estado necessário (configuração, preferências). O ciclo de refresh e os logins em andamento são interrompidos de forma limpa; um login nativo em curso bloqueia a atualização com aviso.
- **RF2.5** Quando o app roda de um local que não permite se atualizar (imagem de disco montada, pasta somente leitura, instalação gerenciada por pacote), explicar e orientar o usuário em vez de falhar.
- **Critério de aceite:** smoke no macOS, Windows e Linux (quando houver pacote) atualizando de uma versão antiga para a nova, com falha simulada de rede e de assinatura; a configuração e as contas persistem.

### RF3 — Configurações e Sobre (prioridade média)

- **RF3.1** **Configurações → Atualizações**: chave para verificar automaticamente, botão **Verificar agora**, data da última verificação, versão atual e canal (estável). A tela Sobre mostra a versão e o estado.
- **RF3.2** Quando o recurso está desligado, o app não faz nenhuma requisição de atualização. A instalação gerenciada por Homebrew, winget ou AUR deve poder desligar o atualizador interno (variável de ambiente ou chave de build), para não competir com o gerenciador.
- **RF3.3** Preferências em `desktop.json`, textos nos três idiomas.
- **Critério de aceite:** smoke em três idiomas; com o recurso desligado, captura de rede mostra zero requisições do atualizador.

### RF4 — Segurança operacional (prioridade alta)

- **RF4.1** A chave privada de assinatura é usada apenas no pipeline de release, em ambiente protegido, com acesso restrito e registro de uso; o repositório e os logs de CI nunca a exibem.
- **RF4.2** Documento de resposta a incidentes: o que fazer se a chave vazar (nova chave, versão de transição assinada com a chave antiga que embute a nova, comunicação aos usuários).
- **RF4.3** O endpoint de atualização é servido apenas por HTTPS; falha de certificado é erro, nunca é ignorada.
- **Critério de aceite:** revisão do pipeline (PRD 0019) confirmando que a chave privada só existe como segredo; teste de recusa de endpoint HTTP e de certificado inválido.

## 6. Requisitos não funcionais

- **Privacidade:** nenhum identificador persistente, nenhum dado de contas ou de uso é enviado ao verificar atualizações. A documentação lista exatamente o que a requisição contém.
- **Confiança:** assinatura obrigatória; atualização por consentimento.
- **Compatibilidade:** contas, TOML, histórico e preferências sobrevivem à atualização; migrações de esquema são para frente.

## 7. Ordem de execução e verificação

1. **RF1:** plugin, chaves e teste com servidor simulado.
2. **RF2 e RF3:** fluxo no app e configurações.
3. **RF4:** procedimentos e verificação do pipeline, junto do PRD 0019.

Em cada etapa: `cargo fmt`, `clippy -D warnings`, testes do workspace e do desktop e smoke da interface alterada.

O requisito só está entregue quando um app instalado numa versão anterior atualiza para a seguinte com consentimento, rejeita qualquer pacote com assinatura inválida, preserva todos os dados do usuário, e com o recurso desligado não faz nenhuma requisição de atualização.
