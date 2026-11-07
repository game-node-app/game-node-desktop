# Arquitetura de plugins hospedada pelo Tauri

**Status:** direção aprovada para implementação incremental; contrato e tarefas em
[OpenSpec `host-tauri`](../openspec/changes/host-tauri/).

## Direção atual

- O processo desktop Tauri é o único dono do Engine, perfil, persistência e ciclo de
  vida dos plugins. Não haverá daemon Core nem CLI de produto nesta direção.
- `gamenode-core` continua como crate de domínio e `gamenode-app` como crate do
  Engine, adaptadores, composição desktop e host da Plugin API. Os crates são
  compilados no mesmo processo; a fronteira entre eles não é IPC.
- Plugins são processos separados. Eles conectam a uma Plugin API autenticada e
  versionada hospedada pelo processo Tauri. Essa API é distinta do IPC `invoke` que
  liga o React ao host e da API REST do GameNode.
- O primeiro fluxo é o plugin oficial de descoberta Steam no Linux. O plugin envia
  observações tipadas; o host conserva reconciliação, associação, persistência e
  publicação de snapshots.
- A API Linux usa gRPC/Protobuf sobre Unix socket privado, valida UID e uma
  credencial temporária por execução. Isso autentica a sessão local; não cria uma
  sandbox para código nativo executado pelo mesmo usuário.
- O host inicia o plugin sob demanda, limita a uma execução por operação coordenada,
  reporta falhas sem substituir o último scan válido e termina os processos filhos
  quando a aplicação fecha.
- Windows mantém o scanner Steam atual no processo Tauri até haver transporte da
  Plugin API para named pipes e validação nativa correspondente.

## Fora deste primeiro marco

Instalação de plugins comunitários, UX geral de permissões, marketplace, assinatura,
sandbox, capacidades sociais e suporte de transporte Windows exigem decisões e
mudanças próprias. Plugins não recebem credenciais REST nem escrevem diretamente no
banco do aplicativo.

## Histórico

O plano anterior de Core/Daemon + CLI foi substituído. Seu registro está em
[archive/plugin-architecture-plan-core-cli.md](archive/plugin-architecture-plan-core-cli.md)
e não orienta a implementação atual. A proposta CLI/daemon também foi marcada como
superada em [core-cli-contract-proposal.md](core-cli-contract-proposal.md).
