# Roadmap WynCoin

Este documento registra o estado do projeto e a ordem recomendada de trabalho.
A rede atual é experimental; nenhuma etapa abaixo torna a WynCoin adequada para
valor real sem revisão de segurança independente.

## Concluído — v0.1: nó persistente e carteira

- SQLite para blocos, mempool e peers.
- UTXO reconstruído e validado na inicialização.
- Carteiras RSA persistentes e transações assinadas.
- Proteção contra gasto duplicado no mempool e nos blocos.
- API JSON administrativa local, CLI e serviço systemd.
- Explorer HTTP local e somente leitura.

## Concluído — v0.2: mineração e consenso inicial

- Proof of Work com alvo numérico sobre os primeiros 64 bits do hash.
- Seleção de chain por trabalho acumulado e desempate determinístico.
- Cancelamento do trabalho local quando um novo bloco é recebido.
- Reajuste de dificuldade por janela, sem bloqueio rígido entre blocos.

## Concluído — v0.3: rede P2P pública experimental

- Protocolo P2P separado da API administrativa.
- Handshake com identidade de rede e fingerprint de consenso.
- Seed público, persistência de peers e sincronização de chain.
- Propagação de blocos e transações entre nós.
- Serviços instaláveis por `install-service.sh`, atualizáveis e removíveis.

## Concluído — v0.4: regras da testnet atual

- Rede `wyncoin-public-testnet-v4`.
- Recompensa fixa de 10 WYN por bloco e emissão sem oferta máxima.
- Meta de 60 segundos por bloco; retarget a cada 20 blocos, limitado a 4×.
- Coinbase utilizável após 20 blocos.
- Taxa mínima zero: a taxa é opcional, mas o remetente a paga separadamente.
- Coinbase validada como recompensa base mais todas as taxas do bloco.
- Limite de 1.000 transações regulares e 2 MiB por bloco.
- Timestamp limitado a 30 segundos no futuro.
- Bits de sinalização reservados para futuras propostas de consenso.
- Explorer separa emissão base, taxas distribuídas e pagamento total em
  coinbases.
- `reinstall-service.sh` para reinstalação destrutiva do serviço preservando a
  carteira pessoal do operador.

As regras completas e a migração da v3 estão em
[TESTNET_STATUS.md](TESTNET_STATUS.md).

## Próxima prioridade — estabilizar a v4

Antes de adicionar recursos grandes, manter VPS, servidor residencial e desktop
minerando e sincronizando por várias janelas de retarget.

- Confirmar mesma altura e mesmo tip hash nos três nós.
- Medir se a média converge para aproximadamente 60 segundos por bloco.
- Confirmar que o retarget reage à entrada e saída de hashrate.
- Verificar que coinbases só aparecem como gastáveis após 20 blocos.
- Testar transferência com taxa: enviar 9 WYN com taxa de 1 WYN deve entregar
  9 WYN ao destinatário e acrescentar 1 WYN à coinbase do minerador.
- Registrar qualquer fork, rejeição de peer ou divergência de tip antes de
  prosseguir.

## Operação e experiência do operador

1. Ajustar o instalador para configurar `p2p.advertise` automaticamente na
   VPS, evitando edição manual depois de uma reinstalação.
2. Implementar cooldown local de mineração por nó. Ele deve limitar máquinas
   mais rápidas sem alterar o consenso nem favorecer um minerador na validação
   da rede.
3. Expor no comando `wyncoin status` e no Explorer: peers conectados, trabalho
   acumulado, ritmo de blocos, próximo retarget e dificuldade projetada.
4. Melhorar o Explorer com saldo disponível, saldo de coinbase bloqueado,
   maturidade restante, taxa da transação e recompensa base/taxas por bloco.

## P2P antes de abrir para terceiros

- Propagação por inventário, evitando retransmitir chains completas sem
  necessidade.
- Limites por peer, timeouts, rate limiting e penalidade temporária para peers
  inválidos.
- Sincronização incremental de headers e blocos.
- Testes de integração automatizados com ao menos três nós, forks e
  reorganizações.
- Revisar limites de memória, tamanho das mensagens e comportamento sob spam.

## Carteira e transações

- Criptografar a chave privada com senha e derivação de chave adequada.
- Backup, restauração e validação segura de arquivos de carteira.
- Exibir saldo disponível separadamente de UTXOs imaturos.
- Adicionar suporte a múltiplos destinatários, seleção de UTXO mais eficiente e
  estimativa de taxa opcional.

## Evolução de consenso

Uma mudança futura de consenso exige nova versão de rede, testes e migração.
Não deve ser entregue apenas por `update-service.sh`.

Para uma proposta futura, usar sinalização por blocos — não contagem de nós —
com a política atual de janela de 200 blocos, limiar de 90% e atraso de 100
blocos. A sinalização não atualiza nós automaticamente: uma release precisa
definir a regra e sua ativação antes de os mineradores sinalizarem apoio.

## Critérios antes de abrir a testnet publicamente

Não convidar operadores externos antes de concluir, no mínimo:

- estabilidade sustentada da v4 entre os três nós;
- testes financeiros de emissão, taxas e maturidade;
- proteção básica contra peers abusivos;
- sincronização P2P incremental confiável;
- carteira com backup seguro, idealmente protegida por senha;
- testes de integração e revisão de segurança do consenso.
