# Arquitetura do WynCoin

Este documento define a divisão de responsabilidades do projeto. Ela é
estrutural: não altera consenso, banco persistente, protocolo P2P ou rede.

## Crates executáveis

| Crate | Responsabilidade |
| --- | --- |
| `wyncoind` | Processo do nó: ciclo de vida, API local, mineração e P2P. |
| `wyncoin-cli` | Cliente administrativo da API local. |
| `wyncoin-wallet` | Criação, consulta e assinatura de transações de carteira. |
| `wyncoin-explorer` | HTTP local, consultas somente leitura e interface web. |

`wyncoin-cli` e `wyncoin-wallet` permanecem em um arquivo porque ainda são
pequenos. A divisão deve acontecer quando houver responsabilidades reais novas,
e não apenas para aumentar a quantidade de arquivos.

## `wyncoin-core`

```text
src/
├── consensus/   blocos, transações, UTXO, cadeia e regras de retarget
├── node/        configuração TOML e persistência SQLite do nó
├── network/     protocolo da API local, handshake e mensagens P2P
├── wallet/      chaves RSA, endereços e assinatura
└── support/     valores WYN e tipos de erro
```

O caminho público `wyncoin_core::blockchain` continua como reexport compatível
de `consensus`. Código novo deve preferir `wyncoin_core::consensus`.

## Pontos de entrada

Os `main.rs` dos binários são somente inicializadores. A implementação fica em
`app.rs`, reduzindo o acoplamento do binário ao ponto de entrada e preparando a
separação progressiva das áreas internas.

Ao acrescentar funcionalidades, use estas regras:

- consenso e validação determinística: `wyncoin-core/src/consensus`;
- mensagens entre nós ou API TCP: `wyncoin-core/src/network`;
- acesso SQLite/configuração de nó: `wyncoin-core/src/node`;
- criptografia e arquivos de carteira: `wyncoin-core/src/wallet`;
- interface HTTP e agregações somente leitura: `wyncoin-explorer`.

## Compatibilidade

Movimentos de arquivo não devem alterar serialização de `Block`, `Transaction`,
mensagens `P2pMessage`, esquema SQLite ou valores de consenso. Mudanças nesses
contratos exigem uma alteração de rede documentada, como ocorreu na v3.
