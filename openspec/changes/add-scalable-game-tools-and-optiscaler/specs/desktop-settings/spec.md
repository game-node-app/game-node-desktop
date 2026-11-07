# Spec Delta

## MODIFIED Requirements

### Requirement: Navegação por tópicos
A navegação lateral do modal SHALL listar, nesta ordem, os tópicos Conta, Lojas, Backups, Ferramentas (`Tools`), Avançado e Sobre. Cada tópico tem ícone e rótulo, e exatamente um aparece selecionado. Selecionar um tópico SHALL trocar o painel à direita, mostrando título e descrição do tópico, e levar a rolagem do painel ao topo. O tópico selecionado SHALL ser lembrado enquanto o aplicativo estiver em execução, mas não SHALL ser persistido entre reinícios.

#### Scenario: Trocar de tópico
- **WHEN** o usuário seleciona Ferramentas (`Tools`) na navegação lateral
- **THEN** Ferramentas aparece selecionado, o painel mostra o título "Tools" (ou "Ferramentas" em pt-BR) com sua descrição, e o conteúdo começa no topo

#### Scenario: Reabrir na mesma execução
- **WHEN** o usuário fecha o modal com Avançado selecionado e o reabre sem reiniciar o aplicativo
- **THEN** o modal abre em Avançado

#### Scenario: Abrir num tópico específico
- **WHEN** a explicação de sincronização da primeira execução é dispensada com a opção de abrir as configurações
- **THEN** o modal abre no tópico Conta, que contém "Sync with GameNode"

## ADDED Requirements

### Requirement: Painel Ferramentas (Tools)
O painel Ferramentas (`Tools`) SHALL listar o catálogo de ferramentas suportadas (iniciando com OptiScaler), exibindo licença (`GPL-3.0`), link do código-fonte, versões em cache e espaço ocupado em disco, além de ações para buscar atualizações (`Check for updates`) e limpar pacotes em cache (`Clear cache`) sem afetar instalações ativas ou backups de reversão.

#### Scenario: Inspecionar e limpar o cache de ferramentas
- **WHEN** o usuário abre o tópico Ferramentas em Settings após baixar versões do OptiScaler e ativa `Clear cache`
- **THEN** os pacotes `.7z` em cache no disco são removidos e o tamanho do cache passa a zero, enquanto as instalações ativas nos jogos e seus backups de reversão (`tool-backups`) permanecem preservados

#### Scenario: Abrir link do repositório oficial e ver licença
- **WHEN** o usuário visualiza o cartão do OptiScaler no painel Ferramentas ou no modal da ferramenta
- **THEN** a licença `GPL-3.0` e a ação para abrir o repositório oficial no navegador do sistema estão visíveis e acessíveis
