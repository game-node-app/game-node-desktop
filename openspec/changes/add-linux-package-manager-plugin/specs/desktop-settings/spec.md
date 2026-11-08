# Spec Delta

## MODIFIED Requirements

### Requirement: Navegação por tópicos
A navegação lateral do modal SHALL listar, nesta ordem, os tópicos Conta, Lojas, Plugins, Backups, Ferramentas (`Tools`) e Sobre. Cada tópico tem ícone e rótulo, e exatamente um aparece selecionado. Selecionar um tópico SHALL trocar o painel à direita, mostrando título e descrição do tópico, e levar a rolagem do painel ao topo. O tópico selecionado SHALL ser lembrado enquanto o aplicativo estiver em execução, mas não SHALL ser persistido entre reinícios.

#### Scenario: Trocar de tópico
- **WHEN** o usuário seleciona Plugins, Backups ou Ferramentas (`Tools`) na navegação lateral
- **THEN** o tópico escolhido aparece selecionado, o painel mostra seu título e descrição, e o conteúdo começa no topo

#### Scenario: Reabrir na mesma execução
- **WHEN** o usuário fecha o modal com Sobre selecionado e o reabre sem reiniciar o aplicativo
- **THEN** o modal abre em Sobre

#### Scenario: Abrir num tópico específico
- **WHEN** a explicação de sincronização da primeira execução é dispensada com a opção de abrir as configurações
- **THEN** o modal abre no tópico Conta, que contém "Sync with GameNode"

## ADDED Requirements

### Requirement: Inventário selecionável de plugins
O tópico Plugins SHALL listar os plugins disponíveis nesta plataforma e permitir selecionar cada item para consultar seu identificador, origem, estado e sessão do processo. Os detalhes SHALL mostrar as permissões concedidas pela API separadamente dos acessos nativos ao sistema operacional.

#### Scenario: Plugin oficial de pacotes no Linux
- **WHEN** o usuário abre Plugins num pacote Linux com o plugin de gerenciador de pacotes disponível
- **THEN** o inventário lista "Gerenciador de pacotes Linux" como oficial e permite consultar `provider.context.read` e `library.discovery`, o estado do executável e seus acessos nativos

#### Scenario: Plugin não disponível nesta plataforma
- **WHEN** o usuário abre Plugins no Windows
- **THEN** o inventário não lista o plugin de gerenciador de pacotes Linux

#### Scenario: Selecionar um plugin
- **WHEN** o usuário seleciona um plugin no inventário
- **THEN** o item fica marcado como selecionado e os detalhes correspondentes são exibidos
