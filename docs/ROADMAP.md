# Roadmap WynCoin

## v0.1.0 — nó local persistente

- SQLite para blocos e mempool.
- UTXO reconstruído e validado ao iniciar.
- Carteiras RSA persistentes.
- Transações assinadas e verificadas.
- Proteção contra gasto duplicado no mempool e nos blocos.
- Proof of Work real com dificuldade fixa.
- API JSON local por TCP.
- CLI administrativo e CLI de carteira.
- Execução contínua por systemd.

## v0.2.0 — consenso e operação

- Target numérico de 256 bits.
- Reajuste de dificuldade por janela.
- Halving e política de emissão máxima.
- Coinbase maturity.
- Cancelamento do trabalho quando o topo mudar.
- Mineração multithread com limite configurável de CPU.
- Snapshots e ferramenta de reindexação.
- Carteira privada criptografada por senha.
- Testes de integração e fuzzing dos parsers.

## v0.3.0 — rede privada

- Protocolo P2P versionado.
- Handshake e identificação de rede.
- Descoberta e persistência de peers.
- Sincronização de headers e blocos.
- Propagação de transações e blocos.
- Trabalho acumulado e reorganização de chain.
- Rate limiting, timeouts e penalidade de peers inválidos.

## v1.0.0 — testnet pública

Somente depois de congelar as regras de consenso, criar um gênesis definitivo,
executar auditoria independente e testar vários nós durante um período longo.
