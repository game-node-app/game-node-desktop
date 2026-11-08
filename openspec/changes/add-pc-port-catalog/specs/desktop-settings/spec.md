# Spec Delta

## MODIFIED Requirements

### Requirement: Painel Ferramentas (Tools)
O painel Ferramentas (`Tools`) SHALL listar o catálogo de ferramentas suportadas (iniciando com OptiScaler), exibindo licença (`GPL-3.0`), link do código-fonte, versões em cache e espaço ocupado em disco, além de ações para buscar atualizações (`Check for updates`) e limpar pacotes em cache (`Clear cache`) sem afetar instalações ativas ou backups de reversão. O painel SHALL listar também, em grupo próprio, as ferramentas auxiliares de extração usadas pela instalação de ports, com licença, link do código-fonte, versão fixada e espaço em cache, e o espaço ocupado pelos downloads de ports em cache, com `Clear cache` que não afeta ports instalados.

#### Scenario: Inspecionar e limpar o cache de ferramentas
- **WHEN** o usuário abre o tópico Ferramentas em Settings após baixar versões do OptiScaler e ativa `Clear cache`
- **THEN** os pacotes `.7z` em cache no disco são removidos e o tamanho do cache passa a zero, enquanto as instalações ativas nos jogos e seus backups de reversão (`tool-backups`) permanecem preservados

#### Scenario: Abrir link do repositório oficial e ver licença
- **WHEN** o usuário visualiza o cartão do OptiScaler no painel Ferramentas ou no modal da ferramenta
- **THEN** a licença `GPL-3.0` e a ação para abrir o repositório oficial no navegador do sistema estão visíveis e acessíveis

#### Scenario: Ver a ferramenta de extração de pacotes
- **WHEN** o usuário abre o tópico Ferramentas depois de uma instalação de port que extraiu um pacote de console
- **THEN** o grupo de ferramentas auxiliares mostra a ferramenta de extração com licença, versão fixada, link do código-fonte e espaço em cache

#### Scenario: Limpar o cache de ports
- **WHEN** o usuário ativa `Clear cache` para os downloads de ports
- **THEN** os arquivos de release e da ferramenta de extração em cache são removidos, e os ports instalados e suas entradas na biblioteca permanecem intactos
- **AND** a limpeza é recusada enquanto uma instalação de port estiver em andamento
