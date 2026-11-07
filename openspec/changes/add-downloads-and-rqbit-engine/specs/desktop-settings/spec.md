# Spec Delta

## MODIFIED Requirements

### Requirement: Navegação por tópicos
A navegação lateral do modal SHALL listar, nesta ordem, os tópicos Conta, Lojas, Downloads, Backups, Avançado e Sobre. Cada tópico tem ícone e rótulo, e exatamente um aparece selecionado. Selecionar um tópico SHALL trocar o painel à direita, mostrando título e descrição do tópico, e levar a rolagem do painel ao topo. O tópico selecionado SHALL ser lembrado enquanto o aplicativo estiver em execução, mas não SHALL ser persistido entre reinícios.

#### Scenario: Trocar de tópico
- **WHEN** o usuário seleciona Downloads na navegação lateral
- **THEN** Downloads aparece selecionado, o painel mostra o título "Downloads" com sua descrição, e o conteúdo começa no topo

#### Scenario: Reabrir na mesma execução
- **WHEN** o usuário fecha o modal com Avançado selecionado e o reabre sem reiniciar o aplicativo
- **THEN** o modal abre em Avançado

#### Scenario: Abrir num tópico específico
- **WHEN** a explicação de sincronização da primeira execução é dispensada com a opção de abrir as configurações
- **THEN** o modal abre no tópico Conta, que contém "Sync with GameNode"

## ADDED Requirements

### Requirement: Painel Downloads
O painel Downloads SHALL permitir ativar ou desativar o recurso, escolher a pasta de destino, definir limites de download e upload e escolher a política de semeadura. Desativar SHALL parar o motor e preservar o estado resumível das transferências. As alterações de texto SHALL seguir a confirmação de descarte do modal.

#### Scenario: Ativar Downloads
- **WHEN** o usuário ativa Downloads no painel depois de aceitar o termo
- **THEN** o recurso fica disponível e o motor inicia na primeira utilização

#### Scenario: Desativar Downloads
- **WHEN** o usuário desativa Downloads com uma transferência ativa
- **THEN** o motor para, nenhuma porta permanece aberta e a transferência mantém seu estado resumível

#### Scenario: Alterar a pasta de destino
- **WHEN** o usuário edita a pasta de destino e salva
- **THEN** a nova pasta é validada, persistida e usada nas próximas transferências

#### Scenario: Destino inválido
- **WHEN** a pasta informada não pode ser escrita
- **THEN** o campo mostra o erro, o destino anterior continua em uso e a edição permanece pendente

#### Scenario: Limites de velocidade
- **WHEN** o usuário define um limite de download ou upload
- **THEN** o limite é aplicado ao motor e persiste após reiniciar

### Requirement: Estado do motor no painel
O painel Downloads SHALL mostrar o nome e a versão do motor embarcado, seu estado (parado, iniciando, pronto ou erro) e uma ação de tentar novamente quando houver falha. O estado SHALL vir do host, não de cache da interface.

#### Scenario: Motor pronto
- **WHEN** o motor está ativo
- **THEN** o painel mostra rqbit, a versão e o estado "Pronto"

#### Scenario: Motor parado
- **WHEN** o recurso está desativado
- **THEN** o painel mostra o motor como parado e não apresenta uma porta em uso

#### Scenario: Falha do motor
- **WHEN** o motor não inicia ou cai repetidamente
- **THEN** o painel mostra o erro localizado e oferece tentar novamente

### Requirement: Licença do motor
O painel Downloads SHALL divulgar a licença e a origem do motor de transferências embarcado, e os avisos de licença completos SHALL continuar disponíveis sem conexão pelo painel Sobre.

#### Scenario: Licença visível
- **WHEN** o usuário abre o painel Downloads
- **THEN** a licença Apache-2.0 e a origem do rqbit são exibidas

#### Scenario: Avisos offline
- **WHEN** o usuário abre os avisos de licença sem rede
- **THEN** o aviso do motor embarcado aparece junto dos demais
