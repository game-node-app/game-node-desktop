# Spec Delta

## MODIFIED Requirements

### Requirement: Fechar o modal
O modal SHALL fechar pelo botão de fechar na barra de título, pela tecla `Esc` e por clique no overlay fora do modal. A barra de título SHALL mostrar a dica `Esc` como texto decorativo, sem ser um controle. Ao fechar, o foco SHALL voltar ao controle que abriu o modal ou, se ele não existir mais, ao conteúdo principal. Se houver alterações não salvas, fechar SHALL seguir o requisito de confirmação de descarte.

#### Scenario: Fechar com Esc
- **WHEN** o modal está aberto sem alterações pendentes nem diálogo interno e o usuário pressiona `Esc`
- **THEN** o modal fecha e o foco volta para o item Settings da barra lateral

#### Scenario: Clique dentro do modal
- **WHEN** o usuário clica numa área vazia dentro do modal
- **THEN** o modal continua aberto

#### Scenario: Esc com a busca preenchida
- **WHEN** o campo de busca das configurações tem texto e foco, e o usuário pressiona `Esc`
- **THEN** a busca é limpa e o modal continua aberto

### Requirement: Navegação por tópicos
A navegação lateral do modal SHALL listar os tópicos em três grupos rotulados, nesta ordem: Conta (Conta e sincronização); Biblioteca (Lojas, Jogos DRM-free, Backups); Aplicativo (Geral, Ferramentas, Plugins, Sobre). Cada tópico tem ícone e rótulo, e exatamente um aparece selecionado. Selecionar um tópico SHALL trocar o painel à direita, mostrando título e descrição do tópico, e levar a rolagem do painel ao topo. O tópico Plugins SHALL mostrar a quantidade de plugins listados. O rodapé da navegação SHALL mostrar o nome e a versão do aplicativo. O tópico selecionado SHALL ser lembrado enquanto o aplicativo estiver em execução, mas não SHALL ser persistido entre reinícios. Tópicos sem suporte nesta plataforma (Jogos DRM-free, Backups) SHALL continuar listados e explicar no painel que não estão disponíveis neste sistema.

#### Scenario: Trocar de tópico
- **WHEN** o usuário seleciona Jogos DRM-free, Plugins, Backups ou Ferramentas (`Tools`) na navegação lateral
- **THEN** o tópico escolhido aparece selecionado, o painel mostra seu título e descrição, e o conteúdo começa no topo

#### Scenario: Grupos da navegação
- **WHEN** o usuário abre as configurações
- **THEN** a navegação mostra os rótulos Conta, Biblioteca e Aplicativo com seus tópicos na ordem definida, e o rodapé mostra "GameNode Desktop" e a versão instalada

#### Scenario: Reabrir na mesma execução
- **WHEN** o usuário fecha o modal com Sobre selecionado e o reabre sem reiniciar o aplicativo
- **THEN** o modal abre em Sobre

#### Scenario: Abrir num tópico específico
- **WHEN** a explicação de sincronização da primeira execução é dispensada com a opção de abrir as configurações
- **THEN** o modal abre no tópico Conta e sincronização, que contém "Sync with GameNode"

### Requirement: Painel Conta
O painel Conta e sincronização SHALL mostrar o estado da conta: avatar (ou um marcador quando não houver imagem), nome de exibição, o método de armazenamento da sessão com a divulgação exigida por `desktop-shell`, o indicador Conectado, Abrir perfil e Sair quando há sessão; ou a ação de entrar pelo navegador quando não há. O painel SHALL mostrar "Sync with GameNode", a fonte do tempo de jogo (Loja/Interno), a última sincronização com Sincronizar agora e a lista do que é e do que não é sincronizado, conforme `desktop-sync`, além do erro de sincronização de tempo de jogo, quando houver.

#### Scenario: Conta conectada
- **WHEN** há sessão válida e o usuário abre Conta e sincronização
- **THEN** o painel mostra a identidade conectada, o método de armazenamento da sessão, Abrir perfil, Sair, o controle de sincronização no estado atual, a fonte do tempo de jogo e a última sincronização

#### Scenario: Sem sessão
- **WHEN** não há sessão e o usuário abre Conta e sincronização
- **THEN** o painel oferece entrar com o GameNode pelo navegador, não mostra identidade, não oferece Sair nem Sincronizar agora, e continua mostrando a fonte do tempo de jogo

#### Scenario: Sair pela conta
- **WHEN** o usuário ativa Sair no painel Conta e sincronização
- **THEN** a saída ocorre como pelo menu do app, e o painel passa a mostrar o estado sem sessão

#### Scenario: Abrir perfil
- **WHEN** o usuário ativa Abrir perfil
- **THEN** a página do usuário no GameNode abre no navegador do sistema e o modal continua aberto

#### Scenario: O que é sincronizado
- **WHEN** o usuário abre Conta e sincronização
- **THEN** o painel lista como enviados o tempo de jogo com a última sessão e o status e as coleções escolhidos, e como nunca enviados os arquivos de save e os caminhos e executáveis locais

### Requirement: Painel Lojas
O painel Lojas SHALL mostrar um cartão para cada provedor embutido com conta de loja, um cartão de armazenamento de credenciais e um cartão "Detectadas neste PC" com cada loja que esta plataforma escaneia. Cada loja detectada aparece com nome, local de instalação e quantidade de pastas de biblioteca quando o provedor os informar, e o resultado do último escaneamento: detectada, não encontrada ou falha. A quantidade de jogos compartilhados e o aviso de falha do catálogo compartilhado SHALL aparecer somente para provedores que declaram acesso compartilhado e SHALL usar o rótulo declarado, como `Steam Family`. O painel SHALL oferecer Reescanear e Mostrar jogos ocultos.

#### Scenario: Steam encontrada
- **WHEN** o último escaneamento encontrou 18 jogos instalados e a conta tem 312 jogos próprios e 8 compartilhados
- **THEN** o cartão da Steam mostra as três contagens, com a contagem de compartilhados identificada por `Steam Family`, e o estado da conta

#### Scenario: Loja sem acesso compartilhado
- **WHEN** a conta Epic está conectada com jogos próprios
- **THEN** o cartão da Epic mostra as contagens de próprios e instalados e não mostra contagem, aviso nem interruptor de compartilhados

#### Scenario: Falha do catálogo compartilhado
- **WHEN** a sincronização do catálogo compartilhado da Steam falha e a dos jogos próprios sincroniza
- **THEN** o cartão da Steam mostra as contagens atualizadas e um aviso não bloqueante que nomeia `Steam Family`

#### Scenario: Loja detectada com local
- **WHEN** o escaneamento encontra a Steam instalada em `C:\Program Files (x86)\Steam` com 4 pastas de biblioteca
- **THEN** "Detectadas neste PC" mostra a Steam com esse caminho, "4 bibliotecas" e o estado detectada

#### Scenario: Falha de escaneamento de uma loja
- **WHEN** o último escaneamento de uma loja falhou
- **THEN** a linha dessa loja em "Detectadas neste PC" mostra o estado de falha, e as demais lojas mantêm suas informações

#### Scenario: Reescanear
- **WHEN** o usuário ativa Reescanear no painel Lojas
- **THEN** a biblioteca é reescaneada em segundo plano, o painel mostra que o escaneamento está em andamento, e as contagens são atualizadas ao terminar

#### Scenario: Armazenamento de credenciais
- **WHEN** o usuário abre Lojas no Windows
- **THEN** o cartão de armazenamento de credenciais mostra "Gerenciador de Credenciais do Windows" com o estado disponível ou indisponível

### Requirement: Cartão da conta Steam
O painel Lojas SHALL mostrar um cartão próprio para cada conta de loja, incluindo a Steam. O cartão SHALL apresentar, com texto e ícone, o nome da loja, o estado da conexão, a identidade conectada e a última sincronização quando houver, e as contagens de jogos na biblioteca, compartilhados (somente com acesso compartilhado declarado) e instalados neste PC. Um cartão que precisa de nova autenticação ou com credencial ilegível SHALL ter destaque de atenção que não dependa só de cor. Para provedores com acesso compartilhado, o cartão SHALL oferecer o interruptor "Incluir jogos compartilhados", definido em `desktop-library`. O cartão SHALL continuar disponível sem sessão do GameNode.

#### Scenario: Conta conectada
- **WHEN** a conta Steam está conectada como "cassi" com 312 jogos próprios, 8 compartilhados e 18 instalados
- **THEN** o cartão mostra a identidade, a última sincronização, as três contagens e o interruptor de jogos compartilhados

#### Scenario: Sem conta
- **WHEN** nenhuma conta Steam está conectada
- **THEN** o cartão mostra "Não conectada" e explica que entrar inclui os jogos que não estão instalados

#### Scenario: Sessão expirada
- **WHEN** a sessão da Steam expirou
- **THEN** o cartão mostra o estado expirado com destaque de atenção, explica que a biblioteca salva continua sendo exibida com a quantidade de jogos e que os jogos instalados continuam funcionando

#### Scenario: Steam inalcançável
- **WHEN** a Steam não pode ser alcançada
- **THEN** o cartão mostra o estado de conexão e explica que a biblioteca salva continua sendo exibida

#### Scenario: Família indisponível
- **WHEN** a sincronização do Steam Family falha mas os jogos próprios sincronizam
- **THEN** o cartão mostra as contagens atualizadas e um aviso não bloqueante sobre o Steam Family

#### Scenario: Credencial ilegível
- **WHEN** a credencial salva não pode ser lida ou o keyring está bloqueado
- **THEN** o cartão oferece Tentar de novo, Entrar de novo e Remover login salvo, com uma explicação acionável

#### Scenario: Falha ao salvar a credencial
- **WHEN** a conta conecta mas a credencial não pode ser salva
- **THEN** o cartão mostra a conta conectada e avisa que ela não será restaurada ao reiniciar

#### Scenario: Sem sessão do GameNode
- **WHEN** o usuário abre Lojas sem sessão do GameNode
- **THEN** o cartão da Steam aparece e permite verificar ou conectar a conta normalmente

### Requirement: Ações da conta Steam
O cartão SHALL mostrar a ação principal do estado atual como botão e as demais ações aplicáveis num menu "Mais ações": Entrar quando não houver conta; Trazer a janela para frente e Cancelar enquanto a janela de login estiver aberta; Sincronizar agora quando conectada; Entrar de novo quando expirada; Tentar de novo quando inalcançável; Remover login salvo quando a credencial não puder ser lida ou a sessão tiver expirado; e Sair quando houver credencial. Entrar SHALL abrir a janela de login da loja e o cartão SHALL indicar que ela está aberta.

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

#### Scenario: Remover login salvo de uma conta expirada
- **WHEN** a sessão da Epic expirou e o usuário ativa Remover login salvo
- **THEN** a credencial salva é apagada, o cartão volta ao estado sem conta e os jogos instalados continuam na biblioteca

#### Scenario: Sair
- **WHEN** o usuário abre "Mais ações" numa conta conectada e ativa Sair
- **THEN** a conta é desconectada, o cartão volta ao estado sem conta e os jogos instalados continuam na biblioteca

### Requirement: Painel Backups
Quando houver suporte a backups locais, o painel Backups SHALL expor, num cartão Proteção automática, o backup automático global, a pasta de destino e a quantidade padrão de cópias mantidas por jogo (5, 10, 20 ou 50), e, num cartão Uso do espaço, o total ocupado pelos backups e o espaço e a quantidade de cópias de cada jogo. O painel SHALL explicar que os backups ficam neste PC e que esses controles são independentes de "Sync with GameNode". Os controles por jogo SHALL continuar nos detalhes do jogo. Sem suporte, o painel SHALL indicar que backups não estão disponíveis neste sistema.

#### Scenario: Desativar backup automático
- **WHEN** o usuário desliga o backup automático e ativa Salvar alterações
- **THEN** a preferência é persistida, nenhum novo backup automático roda, e os backups existentes continuam disponíveis

#### Scenario: Alterar destino
- **WHEN** o usuário escolhe ou digita uma nova pasta de destino e ativa Salvar alterações
- **THEN** o novo destino é validado e salvo em segundo plano, e um toast confirma o resultado

#### Scenario: Destino inválido
- **WHEN** o destino informado não pode ser usado
- **THEN** o campo mostra o erro abaixo dele, o destino anterior continua em uso, e a alteração permanece pendente

#### Scenario: Cópias padrão
- **WHEN** o usuário escolhe 20 cópias e salva
- **THEN** jogos sem uma escolha própria passam a manter 20 cópias, e jogos com escolha própria mantêm a sua

#### Scenario: Uso do espaço
- **WHEN** existem backups de três jogos
- **THEN** o cartão mostra o total ocupado e, para cada jogo, capa, nome, espaço e quantidade de cópias, do maior para o menor

### Requirement: Painel Ferramentas (Tools)
O painel Ferramentas (`Tools`) SHALL listar as ferramentas suportadas (iniciando com OptiScaler) com resumo, licença (`GPL-3.0`), link do código-fonte e em quantos jogos estão instaladas, com Ver jogos. O painel SHALL oferecer as preferências de procurar atualizações automaticamente e de canal padrão (Estável/Nightly), definidas em `desktop-game-tools`, e um cartão Cache de downloads com o espaço total, a quantidade de versões e cada versão em cache, indicando a versão em uso, as Nightly e a verificação SHA-256, com `Clear cache` que não afeta instalações ativas nem backups de reversão. O painel SHALL listar também, em grupo próprio, as ferramentas auxiliares de extração usadas pela instalação de ports, com licença, link do código-fonte, versão fixada e espaço em cache, e o espaço ocupado pelos downloads de ports em cache, com `Clear cache` que não afeta ports instalados.

#### Scenario: Inspecionar e limpar o cache de ferramentas
- **WHEN** o usuário abre o tópico Ferramentas em Settings após baixar versões do OptiScaler e ativa `Clear cache`
- **THEN** os pacotes `.7z` em cache no disco são removidos e o tamanho do cache passa a zero, enquanto as instalações ativas nos jogos e seus backups de reversão (`tool-backups`) permanecem preservados

#### Scenario: Abrir link do repositório oficial e ver licença
- **WHEN** o usuário visualiza o cartão do OptiScaler no painel Ferramentas ou no modal da ferramenta
- **THEN** a licença `GPL-3.0` e a ação para abrir o repositório oficial no navegador do sistema estão visíveis e acessíveis

#### Scenario: Ver jogos com a ferramenta
- **WHEN** o OptiScaler está instalado em dois jogos e o usuário ativa Ver jogos
- **THEN** o cartão mostra "Em 2 jogos" e um menu lista os dois jogos; escolher um fecha o modal e seleciona o jogo na biblioteca

#### Scenario: Versões em cache
- **WHEN** o cache tem as versões 0.7.4 (instalada num jogo), 0.7.3 e uma Nightly
- **THEN** cada versão aparece com seu tamanho e verificação SHA-256, a 0.7.4 marcada Em uso e a Nightly marcada Nightly

#### Scenario: Ver a ferramenta de extração de pacotes
- **WHEN** o usuário abre o tópico Ferramentas depois de uma instalação de port que extraiu um pacote de console
- **THEN** o grupo de ferramentas auxiliares mostra a ferramenta de extração com licença, versão fixada, link do código-fonte e espaço em cache

#### Scenario: Limpar o cache de ports
- **WHEN** o usuário ativa `Clear cache` para os downloads de ports
- **THEN** os arquivos de release e da ferramenta de extração em cache são removidos, e os ports instalados e suas entradas na biblioteca permanecem intactos
- **AND** a limpeza é recusada enquanto uma instalação de port estiver em andamento

### Requirement: Painel Sobre
O painel Sobre SHALL mostrar a marca, o nome e a versão instalada do aplicativo com a data do build, o sistema e a arquitetura; Diagnóstico com Mostrar log recente, Abrir pasta de logs e Copiar informações do sistema; Licenças com a licença GPL-3.0-only, o resumo das licenças de arte, fonte e runtime e uma ação para ler os avisos de licença completos em visualização rolável nativa sobre o modal, que funciona sem conexão; e links para o site do GameNode, termos de uso, privacidade e relatar um problema, abertos no navegador do sistema. O painel SHALL NOT oferecer atualização do aplicativo nem canal de atualização.

#### Scenario: Ler avisos de licença
- **WHEN** o usuário ativa "Show license notices" no painel Sobre
- **THEN** os avisos completos abrem num diálogo rolável sobre o modal, e fechá-lo devolve o foco ao painel Sobre

#### Scenario: Copiar informações do sistema
- **WHEN** o usuário ativa Copiar em Informações do sistema
- **THEN** a área de transferência recebe a versão, o build, o sistema e a arquitetura, o idioma, os plugins com origem, versão e estado, e as lojas detectadas, sem tokens, credenciais, nomes de conta ou caminhos do perfil do usuário, e um toast confirma a cópia

#### Scenario: Abrir um link
- **WHEN** o usuário ativa "Relatar um problema"
- **THEN** a página correspondente abre no navegador do sistema e o modal continua aberto

### Requirement: Preferências aplicam imediatamente
Interruptores, rádios, seletores e controles segmentados do modal SHALL aplicar e persistir a mudança no momento da interação, sem botão de salvar, exceto os controles do cartão Proteção automática do painel Backups, que seguem o requisito de alterações salvas em grupo. Se a persistência falhar, o controle SHALL voltar ao valor efetivamente salvo e mostrar um erro no próprio painel.

#### Scenario: Trocar fonte de tempo de jogo
- **WHEN** o usuário escolhe Interno na fonte de tempo de jogo
- **THEN** a escolha é aplicada e persistida imediatamente, sem barra de alterações não salvas

#### Scenario: Falha ao salvar uma preferência
- **WHEN** a gravação de uma preferência alterada falha
- **THEN** o controle volta ao valor salvo e o painel mostra o erro

### Requirement: Edições de texto exigem salvar
Campos de texto do modal e os controles do cartão Proteção automática do painel Backups SHALL manter a alteração como pendente até o usuário salvar. Cada linha alterada SHALL mostrar um marcador de alteração com nome acessível e o valor salvo anterior ("Antes: …") quando ele for um texto. Enquanto houver alteração pendente, uma barra fixa na base do painel SHALL mostrar "Alterações não salvas", a quantidade de configurações alteradas, Reverter e Salvar alterações. Reverter SHALL restaurar os valores salvos. Salvar SHALL gravar todas as alterações pendentes do painel juntas.

#### Scenario: Descartar edição
- **WHEN** o usuário altera a pasta de destino e desliga o backup automático, e ativa Reverter
- **THEN** os dois controles voltam aos valores salvos, os marcadores somem e a barra de alterações desaparece

#### Scenario: Marcar linhas alteradas
- **WHEN** o usuário altera a pasta de destino
- **THEN** a linha mostra o marcador de alteração e "Antes:" com a pasta salva, e a barra informa "1 configuração alterada"

#### Scenario: Salvar com o teclado
- **WHEN** há alterações pendentes e o usuário pressiona `Ctrl+S`
- **THEN** as alterações são salvas como por Salvar alterações

### Requirement: Teclado e foco no modal
O modal SHALL prender o foco enquanto aberto: Tab e Shift+Tab circulam apenas pelos controles do modal ou do diálogo mais interno. As setas para cima e para baixo SHALL mover a seleção entre os tópicos visíveis quando o foco estiver na navegação lateral, pulando os rótulos de grupo. `Ctrl+F` com o modal aberto SHALL focar a busca das configurações. Todo controle focado SHALL ter um indicador de foco visível. `Esc` SHALL fechar primeiro o diálogo mais interno ou o menu aberto, depois limpar a busca focada, depois fechar o modal.

#### Scenario: Navegar tópicos pelo teclado
- **WHEN** o foco está em Conta e sincronização na navegação lateral e o usuário pressiona a seta para baixo duas vezes
- **THEN** Jogos DRM-free fica selecionado e focado, e o painel mostra Jogos DRM-free

#### Scenario: Esc com diálogo interno
- **WHEN** os avisos de licença estão abertos sobre o modal e o usuário pressiona `Esc`
- **THEN** só os avisos fecham, e o modal continua aberto no painel Sobre

#### Scenario: Focar a busca
- **WHEN** o modal está aberto e o usuário pressiona `Ctrl+F`
- **THEN** o campo de busca das configurações recebe o foco e a busca da biblioteca não é ativada

### Requirement: Modal se adapta ao tamanho da janela
O modal SHALL abrir com 960×720 px lógicos, ou menor para caber na janela com margem mínima em todos os lados, até o tamanho mínimo da janela principal, e SHALL poder ser movido pela barra de título e redimensionado, lembrando a geometria. A barra de título, a navegação lateral e a barra de alterações não salvas SHALL ficar fixas; só o conteúdo do painel, incluindo seu título, SHALL rolar. Quando há conteúdo além da área visível, o painel SHALL indicar isso com um esmaecimento na base, que some ao chegar ao fim.

#### Scenario: Janela no tamanho mínimo
- **WHEN** a janela principal está em 640×420 e o modal está aberto
- **THEN** o modal inteiro cabe na janela, a barra de título e a navegação lateral continuam visíveis, e o conteúdo do painel rola

#### Scenario: Esmaecimento no fim da rolagem
- **WHEN** o usuário rola o painel Lojas até o fim
- **THEN** o esmaecimento da base desaparece, e volta ao rolar para cima

### Requirement: Sistema visual do modal rastreável
O modal SHALL usar apenas os tokens de estilo compartilhados do aplicativo. Isso cobre cores, tipografia Roboto embutida, espaçamentos, raios, sombras e overlay. Cada token em uso SHALL ter procedência registrada num nó ou variável da seção `5373:3196` ("03 · Configurações") do Figma GameNode ou numa adaptação desktop documentada. Marcações de design como "NOVO" SHALL NOT aparecer no aplicativo. Itens das telas web de referência SHALL NOT entrar no aplicativo.

#### Scenario: Revisar contra a referência
- **WHEN** os painéis do modal são revisados contra a seção `5373:3196`
- **THEN** cada cor, tamanho de fonte, espaçamento e raio em uso aparece no registro de procedência com o nó ou variável de origem, ou com uma adaptação desktop documentada

#### Scenario: Funcionalidade web ausente
- **WHEN** o modal é inspecionado
- **THEN** não há Premium, edição de perfil, notificações, "Jogando agora" no perfil, atualização do aplicativo, limite de espaço de backups nem MCP, e nenhuma etiqueta "NOVO"

## ADDED Requirements

### Requirement: Busca nas configurações
A navegação SHALL ter um campo "Buscar configurações". O texto digitado SHALL filtrar os tópicos pelos seus títulos, descrições e rótulos de configuração, sem diferenciar maiúsculas nem acentos. O primeiro tópico correspondente SHALL ser exibido e as configurações correspondentes SHALL ser destacadas no painel. Sem correspondência, a navegação SHALL mostrar "Nenhuma configuração encontrada".

#### Scenario: Buscar uma configuração
- **WHEN** o usuário digita "bandeja" na busca
- **THEN** a navegação mostra só Geral, o painel Geral é exibido, e as linhas sobre a bandeja ficam destacadas

#### Scenario: Busca sem resultado
- **WHEN** o usuário digita um texto que não corresponde a nenhuma configuração
- **THEN** a navegação mostra "Nenhuma configuração encontrada" e o painel atual continua exibido

#### Scenario: Limpar a busca
- **WHEN** o usuário apaga o texto da busca
- **THEN** todos os tópicos voltam a aparecer e o tópico selecionado continua selecionado

### Requirement: Painel Jogos DRM-free
Com suporte a jogos DRM-free, o tópico Jogos DRM-free SHALL listar as pastas monitoradas com o estado de `desktop-drm-free-games`, Verificar e Parar de monitorar por pasta, Adicionar pasta pelo seletor nativo e Verificar tudo agora; as preferências de descoberta; as descobertas ignoradas com Redefinir; e Adicionar a partir do executável. Sem suporte, SHALL indicar que não está disponível neste sistema.

#### Scenario: Pasta monitorada
- **WHEN** o usuário adiciona uma pasta pelo seletor nativo
- **THEN** a pasta aparece na lista em verificação, instalações reconhecidas são adicionadas automaticamente, descobertas ambíguas aguardam revisão, e a linha mostra a quantidade de jogos ao terminar

#### Scenario: Pasta indisponível
- **WHEN** a unidade de uma pasta monitorada é desconectada
- **THEN** a linha mostra o ícone e o estado Indisponível desde quando, mantém a quantidade de jogos conhecida e não oferece Verificar

#### Scenario: Verificar uma pasta
- **WHEN** o usuário ativa Verificar numa pasta disponível
- **THEN** só essa pasta é verificada em segundo plano e sua linha mostra o progresso e a nova hora de verificação

#### Scenario: Redefinir descobertas ignoradas
- **WHEN** há 4 descobertas ignoradas e o usuário ativa Redefinir descobertas ignoradas
- **THEN** a contagem passa a zero e as instalações voltam a ser consideradas na próxima verificação

#### Scenario: Sistema sem suporte
- **WHEN** o aplicativo roda num sistema sem suporte a jogos DRM-free
- **THEN** o painel mostra uma pílula "Indisponível neste sistema" e nenhum controle de pastas

### Requirement: Painel Geral
O tópico Geral SHALL reunir as preferências deste PC: inicialização (iniciar com o Windows, iniciar minimizado, fechar para a bandeja), biblioteca (visualização padrão, tamanho das capas, abrir em, barra de status), idioma e acesso aos atalhos de teclado, com o comportamento de `desktop-lifecycle`, `desktop-library` e `desktop-shell`. Preferências sem suporte nesta plataforma SHALL NOT aparecer.

#### Scenario: Abrir Geral no Windows
- **WHEN** o usuário abre Geral no Windows
- **THEN** o painel mostra os cartões Inicialização, Biblioteca e Idioma e atalhos com os valores atuais

#### Scenario: Abrir Geral no Linux
- **WHEN** o usuário abre Geral no Linux
- **THEN** o cartão Inicialização não aparece, e Biblioteca e Idioma e atalhos aparecem

#### Scenario: Ver atalhos
- **WHEN** o usuário ativa Ver atalhos
- **THEN** a lista de atalhos de teclado abre sobre o modal e, ao fechá-la, o foco volta para Ver atalhos

### Requirement: Painel Plugins
O tópico Plugins SHALL mostrar um cartão por plugin com nome, versão, origem confiável (Oficial ou Comunidade), identificador copiável, estado da sessão e os acessos nativos declarados, e SHALL filtrar por origem com contagens. Detalhes SHALL mostrar as permissões da API separadas dos acessos nativos, sem descrever o plugin como isolado em sandbox. Parar e Iniciar SHALL encerrar ou iniciar a sessão do processo do plugin. Uma nota SHALL avisar que plugins da comunidade não são revisados.

#### Scenario: Filtrar por origem
- **WHEN** há dois plugins oficiais e um da comunidade e o usuário escolhe Comunidade
- **THEN** só o cartão do plugin da comunidade aparece, e o filtro mostra "Todos · 3", "Oficiais · 2" e "Comunidade · 1"

#### Scenario: Parar um plugin
- **WHEN** o usuário ativa Parar no plugin da Epic
- **THEN** o processo do plugin termina, o cartão mostra Parada com Iniciar, a biblioteca continua mostrando os jogos salvos da Epic, e as ações de conta e sincronização da Epic ficam indisponíveis até Iniciar ou reiniciar o aplicativo

#### Scenario: Iniciar um plugin
- **WHEN** o usuário ativa Iniciar num plugin parado
- **THEN** uma nova sessão do processo começa, o cartão mostra Sessão ativa, e a conta da loja é restaurada

#### Scenario: Sessão da loja expirada
- **WHEN** a sessão da loja de um plugin expirou
- **THEN** o cartão do plugin tem destaque de atenção e uma faixa que leva ao tópico Lojas

#### Scenario: Copiar identificador
- **WHEN** o usuário ativa o identificador de um plugin
- **THEN** o identificador é copiado e um toast confirma a cópia

## REMOVED Requirements

### Requirement: Jogos DRM-free no painel Lojas
**Reason**: Jogos DRM-free passa a ser um tópico próprio da navegação, com estado por pasta e preferências de descoberta.
**Migration**: Use o requisito "Painel Jogos DRM-free"; o comportamento das pastas, revisões e redefinição de descobertas é o mesmo de `desktop-drm-free-games`.
