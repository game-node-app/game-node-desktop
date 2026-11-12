# Spec Delta

## MODIFIED Requirements

### Requirement: Painel Lojas
O painel Lojas SHALL listar as lojas dos provedores embutidos que esta plataforma carrega. Cada loja aparece com nome, estado da conta quando o provedor tiver catálogo, as quantidades de jogos próprios e instalados e o resultado do último escaneamento: encontrados, nenhum encontrado ou falha. A quantidade de jogos compartilhados e o aviso de falha do catálogo compartilhado SHALL aparecer somente para provedores que declaram acesso compartilhado e SHALL usar o rótulo declarado, como `Steam Family`. O painel SHALL oferecer Reescanear e a escolha global da fonte de tempo de jogo (Loja/Interno), com o texto de ajuda exigido por `desktop-shell`.

#### Scenario: Steam encontrada
- **WHEN** o último escaneamento encontrou 18 jogos instalados e a conta tem 312 jogos próprios e 8 compartilhados
- **THEN** a linha da Steam mostra as três contagens, com a contagem de compartilhados identificada por `Steam Family`, e o estado da conta

#### Scenario: Loja sem acesso compartilhado
- **WHEN** a conta Epic está conectada com jogos próprios
- **THEN** o cartão da Epic mostra as contagens de próprios e instalados e não mostra contagem nem aviso de compartilhados

#### Scenario: Falha do catálogo compartilhado
- **WHEN** a sincronização do catálogo compartilhado da Steam falha e a dos jogos próprios sincroniza
- **THEN** o cartão da Steam mostra as contagens atualizadas e um aviso não bloqueante que nomeia `Steam Family`

#### Scenario: Falha de escaneamento de uma loja
- **WHEN** o último escaneamento de uma loja falhou
- **THEN** a linha dessa loja mostra o estado de falha, e as demais lojas mantêm suas contagens

#### Scenario: Reescanear
- **WHEN** o usuário ativa Reescanear no painel Lojas
- **THEN** a biblioteca é reescaneada em segundo plano, o painel mostra que o escaneamento está em andamento, e as contagens são atualizadas ao terminar
