# desktop-settings Specification

## Purpose

Reunir todas as configurações e informações do GameNode Desktop num modal com navegação por tópicos. Cada tópico (Conta, Lojas, Backups, Avançado, Sobre) tem um painel próprio, com aplicação previsível das mudanças, acesso completo por teclado e um sistema visual rastreável ao Figma.

## Requirements

### Requirement: Configurações abrem como modal sobre a janela
O aplicativo SHALL abrir as configurações num modal centralizado sobre a janela principal, com overlay escurecido, sem substituir a área de conteúdo. O modal SHALL abrir pelo item Settings da barra lateral, pelo item "Settings / About" do menu do app e pelo atalho `Ctrl+,`. Enquanto o modal estiver aberto, o conteúdo atrás dele SHALL ficar visível e inerte: não recebe foco nem cliques.

#### Scenario: Abrir pela barra lateral
- **WHEN** o usuário ativa Settings na barra lateral com a biblioteca aberta
- **THEN** o modal de configurações aparece sobre a biblioteca, que continua visível sob o overlay
- **AND** nenhum controle da biblioteca recebe foco ou cliques até o modal fechar

#### Scenario: Abrir pelo atalho
- **WHEN** a janela principal tem foco, nenhum outro diálogo está aberto e o usuário pressiona `Ctrl+,`
- **THEN** o modal de configurações abre no tópico mostrado por último nesta execução, ou em Conta se for a primeira vez

#### Scenario: Sem sessão
- **WHEN** o aplicativo não tem sessão e o usuário abre Settings
- **THEN** o modal abre normalmente e todos os tópicos estão disponíveis

### Requirement: Fechar o modal
O modal SHALL fechar pelo botão de fechar no cabeçalho do painel, pela tecla `Esc` e por clique no overlay fora do modal. Ao fechar, o foco SHALL voltar ao controle que abriu o modal ou, se ele não existir mais, ao conteúdo principal. Se houver edições não salvas, fechar SHALL seguir o requisito de confirmação de descarte.

#### Scenario: Fechar com Esc
- **WHEN** o modal está aberto sem edições pendentes nem diálogo interno e o usuário pressiona `Esc`
- **THEN** o modal fecha e o foco volta para o item Settings da barra lateral

#### Scenario: Clique dentro do modal
- **WHEN** o usuário clica numa área vazia dentro do modal
- **THEN** o modal continua aberto

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

### Requirement: Painel Conta
O painel Conta SHALL mostrar o estado da conta conectada: nome de exibição e indicador de conexão quando há sessão, ou a ação de entrar pelo navegador quando não há. Também SHALL mostrar o controle "Sync with GameNode" com sua explicação e o erro de sincronização de tempo de jogo, quando houver. Por fim, SHALL mostrar o método de armazenamento da sessão com a divulgação exigida por `desktop-shell` e a ação de sair, quando há sessão.

#### Scenario: Conta conectada
- **WHEN** há sessão válida e o usuário abre Conta
- **THEN** o painel mostra a identidade conectada, o controle de sincronização no estado atual, o método de armazenamento da sessão e a ação Sair

#### Scenario: Sem sessão
- **WHEN** não há sessão e o usuário abre Conta
- **THEN** o painel oferece entrar com o GameNode pelo navegador, não mostra identidade e não oferece Sair

#### Scenario: Sair pela conta
- **WHEN** o usuário ativa Sair no painel Conta
- **THEN** a saída ocorre como pelo menu do app, e o painel passa a mostrar o estado sem sessão

### Requirement: Painel Lojas
O painel Lojas SHALL listar as lojas dos provedores embutidos que esta plataforma carrega. Cada loja aparece com nome, estado da conta quando o provedor tiver catálogo, as quantidades de jogos próprios, compartilhados pelo Steam Family e instalados, e o resultado do último escaneamento: encontrados, nenhum encontrado ou falha. O painel SHALL oferecer Reescanear e a escolha global da fonte de tempo de jogo (Loja/Interno), com o texto de ajuda exigido por `desktop-shell`.

#### Scenario: Steam encontrada
- **WHEN** o último escaneamento encontrou 18 jogos instalados e a conta tem 312 jogos próprios e 8 compartilhados
- **THEN** a linha da Steam mostra as três contagens e o estado da conta

#### Scenario: Falha de escaneamento de uma loja
- **WHEN** o último escaneamento de uma loja falhou
- **THEN** a linha dessa loja mostra o estado de falha, e as demais lojas mantêm suas contagens

#### Scenario: Reescanear
- **WHEN** o usuário ativa Reescanear no painel Lojas
- **THEN** a biblioteca é reescaneada em segundo plano, o painel mostra que o escaneamento está em andamento, e as contagens são atualizadas ao terminar

### Requirement: Jogos DRM-free no painel Lojas
Quando houver suporte a jogos DRM-free, o painel Lojas SHALL ter uma seção DRM-free com as pastas monitoradas. A seção permite adicionar e remover pastas, reescanear pastas monitoradas, revisar descobertas pendentes e redefinir as descobertas ignoradas, com o mesmo comportamento definido em `desktop-drm-free-games`. Sem suporte, a seção SHALL indicar que não está disponível neste sistema.

#### Scenario: Pasta monitorada
- **WHEN** o usuário adiciona uma pasta monitorada pela seção DRM-free
- **THEN** a pasta aparece na lista, instalações reconhecidas são adicionadas automaticamente, e descobertas ambíguas aguardam revisão antes de entrar na biblioteca

#### Scenario: Sistema sem suporte
- **WHEN** o aplicativo roda num sistema sem suporte a jogos DRM-free
- **THEN** a seção DRM-free mostra uma pílula "Indisponível neste sistema" e nenhum controle de pastas

### Requirement: Cartão da conta Steam
O painel Lojas SHALL mostrar um cartão próprio para a conta Steam. O cartão SHALL apresentar, com texto e ícone, o estado da conexão, a identidade conectada quando houver, as contagens de jogos próprios, compartilhados pelo Steam Family e instalados, a hora da última verificação e o método de armazenamento da credencial. O cartão SHALL continuar disponível sem sessão do GameNode.

#### Scenario: Conta conectada
- **WHEN** a conta Steam está conectada como "cassi" com 312 jogos próprios, 8 compartilhados e 18 instalados
- **THEN** o cartão mostra a identidade, as três contagens, a última verificação e o armazenamento da credencial

#### Scenario: Sem conta
- **WHEN** nenhuma conta Steam está conectada
- **THEN** o cartão mostra "Não conectada" e explica que entrar inclui os jogos que não estão instalados

#### Scenario: Sessão expirada
- **WHEN** a sessão da Steam expirou
- **THEN** o cartão mostra o estado expirado e explica que os jogos instalados continuam funcionando

#### Scenario: Steam inalcançável
- **WHEN** a Steam não pode ser alcançada
- **THEN** o cartão mostra o estado de conexão e explica que a biblioteca salva continua sendo exibida

#### Scenario: Família indisponível
- **WHEN** a sincronização do Steam Family falha mas os jogos próprios sincronizam
- **THEN** o cartão mostra as contagens atualizadas e um aviso não bloqueante sobre o Steam Family

#### Scenario: Credencial ilegível
- **WHEN** a credencial salva não pode ser lida ou o keyring está bloqueado
- **THEN** o cartão oferece Tentar de novo, Entrar de novo e Remover a credencial salva, com uma explicação acionável

#### Scenario: Falha ao salvar a credencial
- **WHEN** a conta conecta mas a credencial não pode ser salva
- **THEN** o cartão mostra a conta conectada e avisa que ela não será restaurada ao reiniciar

#### Scenario: Sem sessão do GameNode
- **WHEN** o usuário abre Lojas sem sessão do GameNode
- **THEN** o cartão da Steam aparece e permite verificar ou conectar a conta normalmente

### Requirement: Ações da conta Steam
O cartão SHALL oferecer apenas as ações aplicáveis ao estado atual: Entrar com a Steam quando não houver conta; Trazer a janela para frente e Cancelar enquanto a janela de login estiver aberta; Sincronizar agora quando conectada; Entrar de novo quando expirada; Tentar de novo quando inalcançável; Remover a credencial salva quando ela não puder ser lida; e Sair quando houver credencial. Entrar SHALL abrir a janela de login da Steam e o cartão SHALL indicar que ela está aberta.

#### Scenario: Entrar
- **WHEN** o usuário ativa Entrar com a Steam
- **THEN** a janela de login abre e o cartão mostra que aguarda a conclusão

#### Scenario: Trazer a janela para frente
- **WHEN** a janela de login está atrás de outra janela e o usuário ativa Trazer para frente
- **THEN** a janela de login recebe o foco sem perder o estado da tentativa

#### Scenario: Cancelar a espera
- **WHEN** o usuário ativa Cancelar enquanto a janela de login está aberta
- **THEN** a janela fecha, nenhuma credencial é salva e o cartão volta ao estado anterior

#### Scenario: Sincronizar agora
- **WHEN** a conta está conectada e o usuário ativa Sincronizar agora
- **THEN** o cartão indica progresso e as contagens são atualizadas ao terminar

#### Scenario: Entrar de novo
- **WHEN** a conta está expirada e o usuário ativa Entrar de novo
- **THEN** uma nova janela de login abre e o cartão mantém a explicação sobre os jogos instalados

#### Scenario: Sair
- **WHEN** o usuário ativa Sair
- **THEN** a conta é desconectada, o cartão volta ao estado sem conta e os jogos instalados continuam na biblioteca

### Requirement: Aviso de credencial em arquivo privado
Quando a credencial da Steam for armazenada no arquivo privado de fallback do Linux, o cartão SHALL avisar que o arquivo é legível apenas pelo usuário atual e NÃO é criptografado, e SHALL recomendar a instalação de um keyring. O aviso SHALL aparecer antes de entrar e enquanto a conta estiver conectada.

#### Scenario: Linux sem Secret Service
- **WHEN** o sistema não tem Secret Service e o cartão da Steam é exibido
- **THEN** o aviso informa que a credencial será gravada em texto não criptografado em um arquivo privado

#### Scenario: Keyring disponível
- **WHEN** a credencial está no Windows Credential Manager ou no Secret Service
- **THEN** o aviso de texto não criptografado não aparece

### Requirement: Indicador de atenção no tópico Lojas
A navegação do modal SHALL mostrar um indicador de atenção no tópico Lojas quando a conta Steam precisar de nova autenticação ou a credencial não puder ser lida. O indicador SHALL ter nome acessível e NÃO SHALL depender apenas de cor. Ele SHALL desaparecer quando a conta voltar a funcionar ou for desconectada.

#### Scenario: Sessão expirada
- **WHEN** a conta Steam expira e o usuário abre as configurações em outro tópico
- **THEN** Lojas mostra um indicador de atenção com nome acessível

#### Scenario: Conta recuperada
- **WHEN** a conta volta a funcionar
- **THEN** o indicador desaparece sem reabrir o modal

#### Scenario: Conta desconectada
- **WHEN** o usuário sai da Steam
- **THEN** o indicador desaparece

### Requirement: Painel Backups
Quando houver suporte a backups locais, o painel Backups SHALL expor os controles globais de proteção local de saves: backup automático global e pasta de destino. O painel SHALL explicar que os backups ficam neste PC e que esses controles são independentes de "Sync with GameNode". Os controles por jogo SHALL continuar nos detalhes do jogo. Sem suporte, o painel SHALL indicar que backups não estão disponíveis neste sistema.

#### Scenario: Desativar backup automático
- **WHEN** o usuário desliga o backup automático no painel Backups
- **THEN** a preferência é aplicada e persistida imediatamente, e os backups existentes continuam disponíveis

#### Scenario: Alterar destino
- **WHEN** o usuário edita a pasta de destino e ativa Salvar alterações
- **THEN** o novo destino é validado e salvo em segundo plano, e um toast confirma o resultado

#### Scenario: Destino inválido
- **WHEN** o destino informado não pode ser usado
- **THEN** o campo mostra o erro abaixo dele, o destino anterior continua em uso, e a edição permanece pendente

### Requirement: Painel Ferramentas (Tools)
O painel Ferramentas (`Tools`) SHALL listar o catálogo de ferramentas suportadas (iniciando com OptiScaler), exibindo licença (`GPL-3.0`), link do código-fonte, versões em cache e espaço ocupado em disco, além de ações para buscar atualizações (`Check for updates`) e limpar pacotes em cache (`Clear cache`) sem afetar instalações ativas ou backups de reversão.

#### Scenario: Inspecionar e limpar o cache de ferramentas
- **WHEN** o usuário abre o tópico Ferramentas em Settings após baixar versões do OptiScaler e ativa `Clear cache`
- **THEN** os pacotes `.7z` em cache no disco são removidos e o tamanho do cache passa a zero, enquanto as instalações ativas nos jogos e seus backups de reversão (`tool-backups`) permanecem preservados

#### Scenario: Abrir link do repositório oficial e ver licença
- **WHEN** o usuário visualiza o cartão do OptiScaler no painel Ferramentas ou no modal da ferramenta
- **THEN** a licença `GPL-3.0` e a ação para abrir o repositório oficial no navegador do sistema estão visíveis e acessíveis

### Requirement: Painel Sobre
O painel Sobre SHALL mostrar o nome e a versão instalada do aplicativo, a licença GPL-3.0-only, o resumo das licenças de arte, fonte e runtime, e uma ação para ler os avisos de licença completos em visualização rolável nativa sobre o modal, que funciona sem conexão.

#### Scenario: Ler avisos de licença
- **WHEN** o usuário ativa "Show license notices" no painel Sobre
- **THEN** os avisos completos abrem num diálogo rolável sobre o modal, e fechá-lo devolve o foco ao painel Sobre

### Requirement: Preferências aplicam imediatamente
Interruptores, rádios e controles segmentados do modal SHALL aplicar e persistir a mudança no momento da interação, sem botão de salvar. Se a persistência falhar, o controle SHALL voltar ao valor efetivamente salvo e mostrar um erro no próprio painel.

#### Scenario: Trocar fonte de tempo de jogo
- **WHEN** o usuário escolhe Interno na fonte de tempo de jogo
- **THEN** a escolha é aplicada e persistida imediatamente, sem rodapé de salvar

#### Scenario: Falha ao salvar uma preferência
- **WHEN** a gravação de uma preferência alterada falha
- **THEN** o controle volta ao valor salvo e o painel mostra o erro

### Requirement: Edições de texto exigem salvar
Campos de texto do modal SHALL manter a edição como pendente até o usuário salvar. Enquanto houver edição pendente, o painel SHALL mostrar um rodapé de ações com o aviso "Alterações não salvas", Descartar e Salvar alterações. Descartar SHALL restaurar o valor salvo.

#### Scenario: Descartar edição
- **WHEN** o usuário altera a pasta de destino e ativa Descartar
- **THEN** o campo volta ao valor salvo e o rodapé de ações desaparece

### Requirement: Confirmação de descarte
Fechar o modal ou trocar de tópico com edições pendentes SHALL abrir um diálogo de alerta sobre o modal. O diálogo pergunta se as alterações devem ser descartadas e oferece Cancelar e Descartar. Cancelar SHALL manter o modal e a edição. Descartar SHALL desfazer a edição e concluir a ação pedida. Enquanto o alerta estiver aberto, o modal atrás dele SHALL ficar inerte.

#### Scenario: Fechar com edição pendente
- **WHEN** a pasta de destino tem edição pendente e o usuário pressiona `Esc`
- **THEN** o diálogo de alerta abre com o foco em Cancelar, e o modal continua aberto atrás dele

#### Scenario: Confirmar descarte ao trocar de tópico
- **WHEN** o alerta foi aberto ao selecionar outro tópico e o usuário ativa Descartar
- **THEN** a edição é desfeita e o tópico escolhido é exibido

### Requirement: Feedback por toast
Resultados de ações concluídas em segundo plano a partir do modal (salvar destino, concluir reescaneamento, conectar ou desconectar a Steam) SHALL ser anunciados num toast dentro do modal, com ícone de estado, texto curto e ação Fechar. O toast SHALL ser anunciado por leitores de tela, desaparecer sozinho após alguns segundos e não roubar o foco.

#### Scenario: Destino salvo
- **WHEN** o novo destino de backup termina de ser salvo
- **THEN** um toast de sucesso aparece no modal, o foco continua no controle anterior, e o toast some sozinho

#### Scenario: Steam conectada
- **WHEN** a autenticação da Steam termina e o catálogo é importado
- **THEN** um toast informa a quantidade de jogos importados e o foco continua no cartão da Steam

#### Scenario: Steam desconectada
- **WHEN** o usuário sai da Steam
- **THEN** um toast informa que os jogos instalados continuam na biblioteca

### Requirement: Teclado e foco no modal
O modal SHALL prender o foco enquanto aberto: Tab e Shift+Tab circulam apenas pelos controles do modal ou do diálogo mais interno. As setas para cima e para baixo SHALL mover a seleção entre os tópicos quando o foco estiver na navegação lateral. Todo controle focado SHALL ter um indicador de foco visível. `Esc` SHALL fechar primeiro o diálogo mais interno: alerta ou avisos de licença, depois o modal.

#### Scenario: Navegar tópicos pelo teclado
- **WHEN** o foco está em Conta na navegação lateral e o usuário pressiona a seta para baixo duas vezes
- **THEN** Backups fica selecionado e focado, e o painel mostra Backups

#### Scenario: Esc com diálogo interno
- **WHEN** os avisos de licença estão abertos sobre o modal e o usuário pressiona `Esc`
- **THEN** só os avisos fecham, e o modal continua aberto no painel Sobre

### Requirement: Controles acessíveis
Cada controle do modal SHALL expor a leitores de tela papel, nome e estado. Isso inclui itens de navegação (selecionado), interruptores (ligado/desligado), rádios e segmentados (escolhido), botões e campos (rótulo e erro), e o estado da conta Steam. Estado SHALL nunca ser comunicado apenas por cor.

#### Scenario: Interruptor anunciado
- **WHEN** um leitor de tela foca o interruptor "Sync with GameNode"
- **THEN** ele anuncia o rótulo, o papel de interruptor e se está ligado ou desligado

#### Scenario: Estado da conta Steam anunciado
- **WHEN** um leitor de tela foca o cartão da conta Steam
- **THEN** ele anuncia o estado da conexão, a identidade quando houver e as contagens de jogos

### Requirement: Modal se adapta ao tamanho da janela
O modal SHALL usar no máximo 960×720 px lógicos e caber na janela com margem mínima em todos os lados, até o tamanho mínimo da janela principal. O cabeçalho do painel SHALL ficar fixo, e só o conteúdo do painel SHALL rolar. Quando há conteúdo além da área visível, o painel SHALL indicar isso com um esmaecimento na base.

#### Scenario: Janela no tamanho mínimo
- **WHEN** a janela principal está em 640×420 e o modal está aberto
- **THEN** o modal inteiro cabe na janela, a navegação lateral e o cabeçalho continuam visíveis, e o conteúdo do painel rola

### Requirement: Modal não bloqueia a interface
Operações iniciadas no modal que tocam disco, rede ou processos SHALL rodar fora da thread de interface. Exemplos: salvar preferências, validar destino, reescanear, ler avisos de licença, verificar a sessão da Steam e sincronizar o catálogo. Enquanto uma operação estiver em andamento, o controle afetado SHALL indicar progresso e evitar reenvio, e o resto do modal SHALL continuar responsivo.

#### Scenario: Validação lenta do destino
- **WHEN** a validação de uma nova pasta de destino demora
- **THEN** Salvar alterações indica progresso e fica indisponível, e o usuário ainda consegue navegar entre tópicos e fechar o alerta

#### Scenario: Verificação lenta da Steam
- **WHEN** a verificação da conta Steam demora
- **THEN** o cartão indica progresso e o usuário ainda consegue navegar entre tópicos, rolar o painel e fechar o modal

### Requirement: Textos do modal traduzidos
Todos os rótulos, descrições, estados, erros, alertas e toasts do modal SHALL estar disponíveis em inglês e português do Brasil, usando a mesma seleção de idioma do restante do shell. Nomes de lojas, nomes de exibição e caminhos SHALL aparecer como recebidos.

#### Scenario: Desktop em português do Brasil
- **WHEN** o aplicativo inicia com locale pt-BR e o usuário abre o modal
- **THEN** a navegação, os títulos dos painéis, os controles e os alertas aparecem em português do Brasil, e o nome "Steam" e os caminhos não mudam

### Requirement: Sistema visual do modal rastreável
O modal SHALL usar apenas os tokens de estilo do modal de configurações. Isso cobre cores, tipografia Roboto embutida, espaçamentos, raios, sombras e overlay. Cada token SHALL ter procedência registrada num nó ou variável da seção `4987:10202` do Figma GameNode ou numa adaptação desktop documentada. Itens, textos e funcionalidades das telas web de referência SHALL NOT entrar no aplicativo.

#### Scenario: Revisar contra a referência
- **WHEN** os painéis do modal são revisados contra a seção `4987:10202`
- **THEN** cada cor, tamanho de fonte, espaçamento e raio em uso aparece no registro de procedência com o nó ou variável de origem, ou com uma adaptação desktop documentada

#### Scenario: Funcionalidade web ausente
- **WHEN** o modal é inspecionado
- **THEN** não há tópicos ou controles de Premium, edição de perfil, notificações, privacidade, aparência ou MCP vindos da referência web
