# Spec Delta

## ADDED Requirements

### Requirement: Cartão da conta Steam
O painel Lojas SHALL mostrar um cartão próprio para a conta Steam. O cartão SHALL apresentar, com
texto e ícone, o estado da conexão, a identidade conectada quando houver, as contagens de jogos
próprios, compartilhados pelo Steam Family e instalados, a hora da última verificação e o método de
armazenamento da credencial. O cartão SHALL continuar disponível sem sessão do GameNode.

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
O cartão SHALL oferecer apenas as ações aplicáveis ao estado atual: Entrar com a Steam quando não
houver conta; Trazer a janela para frente e Cancelar enquanto a janela de login estiver aberta;
Sincronizar agora quando conectada; Entrar de novo quando expirada; Tentar de novo quando
inalcançável; Remover a credencial salva quando ela não puder ser lida; e Sair quando houver
credencial. Entrar SHALL abrir a janela de login da Steam e o cartão SHALL indicar que ela está
aberta.

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
Quando a credencial da Steam for armazenada no arquivo privado de fallback do Linux, o cartão
SHALL avisar que o arquivo é legível apenas pelo usuário atual e NÃO é criptografado, e SHALL
recomendar a instalação de um keyring. O aviso SHALL aparecer antes de entrar e enquanto a conta
estiver conectada.

#### Scenario: Linux sem Secret Service
- **WHEN** o sistema não tem Secret Service e o cartão da Steam é exibido
- **THEN** o aviso informa que a credencial será gravada em texto não criptografado em um arquivo privado

#### Scenario: Keyring disponível
- **WHEN** a credencial está no Windows Credential Manager ou no Secret Service
- **THEN** o aviso de texto não criptografado não aparece

### Requirement: Indicador de atenção no tópico Lojas
A navegação do modal SHALL mostrar um indicador de atenção no tópico Lojas quando a conta Steam
precisar de nova autenticação ou a credencial não puder ser lida. O indicador SHALL ter nome
acessível e NÃO SHALL depender apenas de cor. Ele SHALL desaparecer quando a conta voltar a
funcionar ou for desconectada.

#### Scenario: Sessão expirada
- **WHEN** a conta Steam expira e o usuário abre as configurações em outro tópico
- **THEN** Lojas mostra um indicador de atenção com nome acessível

#### Scenario: Conta recuperada
- **WHEN** a conta volta a funcionar
- **THEN** o indicador desaparece sem reabrir o modal

#### Scenario: Conta desconectada
- **WHEN** o usuário sai da Steam
- **THEN** o indicador desaparece

## MODIFIED Requirements

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

### Requirement: Controles acessíveis
Cada controle do modal SHALL expor a leitores de tela papel, nome e estado. Isso inclui itens de navegação (selecionado), interruptores (ligado/desligado), rádios e segmentados (escolhido), botões e campos (rótulo e erro), e o estado da conta Steam. Estado SHALL nunca ser comunicado apenas por cor.

#### Scenario: Interruptor anunciado
- **WHEN** um leitor de tela foca o interruptor "Sync with GameNode"
- **THEN** ele anuncia o rótulo, o papel de interruptor e se está ligado ou desligado

#### Scenario: Estado da conta Steam anunciado
- **WHEN** um leitor de tela foca o cartão da conta Steam
- **THEN** ele anuncia o estado da conexão, a identidade quando houver e as contagens de jogos

### Requirement: Modal não bloqueia a interface
Operações iniciadas no modal que tocam disco, rede ou processos SHALL rodar fora da thread de interface. Exemplos: salvar preferências, validar destino, reescanear, ler avisos de licença, verificar a sessão da Steam e sincronizar o catálogo. Enquanto uma operação estiver em andamento, o controle afetado SHALL indicar progresso e evitar reenvio, e o resto do modal SHALL continuar responsivo.

#### Scenario: Validação lenta do destino
- **WHEN** a validação de uma nova pasta de destino demora
- **THEN** Salvar alterações indica progresso e fica indisponível, e o usuário ainda consegue navegar entre tópicos e fechar o alerta

#### Scenario: Verificação lenta da Steam
- **WHEN** a verificação da conta Steam demora
- **THEN** o cartão indica progresso e o usuário ainda consegue navegar entre tópicos, rolar o painel e fechar o modal
