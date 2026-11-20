# Spec Delta

## MODIFIED Requirements

### Requirement: Confirmação de descarte
Fechar o modal ou trocar de tópico com edições pendentes SHALL abrir um diálogo de alerta sobre o modal. O diálogo pergunta se as alterações devem ser descartadas, lista o nome de cada campo com edição pendente e oferece Continuar editando e Descartar. Continuar editando SHALL manter o modal e a edição. Descartar SHALL desfazer a edição e concluir a ação pedida. Enquanto o alerta estiver aberto, o modal atrás dele SHALL ficar inerte.

#### Scenario: Fechar com edição pendente
- **WHEN** a pasta de destino tem edição pendente e o usuário pressiona `Esc`
- **THEN** o diálogo de alerta abre com o foco em Continuar editando, lista "Destino dos backups", e o modal continua aberto atrás dele

#### Scenario: Confirmar descarte ao trocar de tópico
- **WHEN** o alerta foi aberto ao selecionar outro tópico e o usuário ativa Descartar
- **THEN** a edição é desfeita e o tópico escolhido é exibido
