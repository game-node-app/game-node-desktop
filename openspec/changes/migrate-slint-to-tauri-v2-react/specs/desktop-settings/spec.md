# Spec Delta

## MODIFIED Requirements

### Requirement: Navegação por tópicos
A navegação lateral do modal SHALL listar, nesta ordem, os tópicos Conta, Lojas, Backups, Ferramentas (`Tools`) e Sobre. Cada tópico tem ícone e rótulo, e exatamente um aparece selecionado. Selecionar um tópico SHALL trocar o painel à direita, mostrando título e descrição do tópico, e levar a rolagem do painel ao topo. O tópico selecionado SHALL ser lembrado enquanto o aplicativo estiver em execução, mas não SHALL ser persistido entre reinícios.

#### Scenario: Trocar de tópico
- **WHEN** o usuário seleciona Backups ou Ferramentas (`Tools`) na navegação lateral
- **THEN** o tópico escolhido aparece selecionado, o painel mostra seu título e descrição, e o conteúdo começa no topo

#### Scenario: Reabrir na mesma execução
- **WHEN** o usuário fecha o modal com Sobre selecionado e o reabre sem reiniciar o aplicativo
- **THEN** o modal abre em Sobre

#### Scenario: Abrir num tópico específico
- **WHEN** a explicação de sincronização da primeira execução é dispensada com a opção de abrir as configurações
- **THEN** o modal abre no tópico Conta, que contém "Sync with GameNode"

### Requirement: Painel Sobre
O painel Sobre SHALL mostrar o nome e a versão instalada do aplicativo, a licença GPL-3.0-only, o resumo das licenças de arte, fonte e runtime, e uma ação para ler os avisos de licença completos em visualização rolável nativa sobre o modal, que funciona sem conexão.

#### Scenario: Ler avisos de licença
- **WHEN** o usuário ativa "Show license notices" no painel Sobre
- **THEN** os avisos completos abrem num diálogo rolável sobre o modal, e fechá-lo devolve o foco ao painel Sobre

## REMOVED Requirements

### Requirement: Painel Avançado
**Reason**: The `Avançado` (`Advanced`) topic only existed to expose Slint's GPU-vs-software renderer toggle (`RendererPreference`) and fallback status, which are obsolete in Tauri v2.
**Migration**: Remove the `Avançado` topic from the Settings sidebar and delete `RendererPreference` controls.
