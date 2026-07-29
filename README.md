# WynCoin v0.1.0

Primeira versão persistente e executável da WynCoin. Este projeto é uma rede
privada experimental: ele executa Proof of Work real segundo as regras da
WynCoin, mas ainda não é descentralizado, não é compatível com Bitcoin e não
deve custodiar dinheiro real.

## Estrutura

```text
WynCoin-v0.1.0/
├── wyncoin-core/      regras, blocos, transações, carteira e SQLite
├── wyncoind/          serviço que mantém a chain e minera
├── wyncoin-cli/       administração e consulta do nó
├── wyncoin-wallet/    criação de carteira e envio de transações
├── data/node.toml     configuração de desenvolvimento
├── deploy/            arquivos para systemd
└── scripts/           instalação e backup
```

## O que mudou em relação à demo

- O nó não cria outra blockchain a cada comando.
- Blocos e mempool são gravados em `data/blockchain.db`.
- O UTXO é reconstruído a partir dos blocos sempre que o nó inicia.
- A carteira do minerador permanece em `data/wallets/miner.json`.
- Transações usam assinatura RSA válida; não existe `DEMO_SIG_PLACEHOLDER`.
- O nó rejeita UTXO inexistente, assinatura inválida, saldo insuficiente e
  gasto duplicado.
- `wyncoin-cli` e `wyncoin-wallet` conversam com o processo `wyncoind`.

## Requisitos

- Rust 1.85 ou superior com Cargo.
- Linux, macOS ou Windows para desenvolvimento.
- `systemd` apenas para a instalação como serviço no Linux.

Confirme o ambiente:

```bash
rustc --version
cargo --version
```

## Primeiro uso local

Entre na raiz do projeto e compile:

```bash
cargo build --workspace
cargo test --workspace
```

O banco da demo anterior não deve ser reutilizado sem migração. Faça backup:

```bash
./scripts/backup-demo-data.sh
mv data/blockchain.db data/blockchain.db.old 2>/dev/null || true
```

Inicie o nó em um terminal:

```bash
cargo run -p wyncoind -- --config data/node.toml
```

Em outro terminal, consulte o status:

```bash
cargo run -p wyncoin-cli -- status
cargo run -p wyncoin-cli -- blocks --limit 5
```

A configuração padrão minera um bloco por ciclo de 60 segundos. Para testar
imediatamente:

```bash
cargo run -p wyncoin-cli -- mine 1
cargo run -p wyncoin-cli -- status
```

## Criar uma carteira

```bash
cargo run -p wyncoin-wallet -- new --output data/wallets/alice.json
cargo run -p wyncoin-wallet -- info --file data/wallets/alice.json
```

Consultar o saldo:

```bash
cargo run -p wyncoin-wallet -- balance --file data/wallets/alice.json
```

O minerador recebe as recompensas na carteira indicada em
`data/node.toml`. Para testar uma transferência, copie o endereço de Alice e
envie a partir da carteira do minerador:

```bash
cargo run -p wyncoin-wallet -- send \
  --file data/wallets/miner.json \
  --to WYN_ENDERECO_DE_ALICE \
  --amount 10 \
  --fee 0.001
```

A transação entra no mempool. Confirme-a minerando:

```bash
cargo run -p wyncoin-cli -- mempool
cargo run -p wyncoin-cli -- mine 1
cargo run -p wyncoin-wallet -- balance --file data/wallets/alice.json
```

## Persistência

Teste a persistência:

```bash
cargo run -p wyncoin-cli -- status
# encerre o wyncoind com Ctrl+C
cargo run -p wyncoind -- --config data/node.toml
cargo run -p wyncoin-cli -- status
```

A altura e os saldos devem continuar iguais depois da reinicialização.

## Instalar como serviço

Em um VPS Debian/Ubuntu:

```bash
sudo ./scripts/install-service.sh
sudo systemctl status wyncoind
sudo journalctl -u wyncoind -f
```

Comandos instalados:

```bash
/opt/wyncoin/bin/wyncoin-cli status
/opt/wyncoin/bin/wyncoin-wallet balance --address WYN...
```

A API permanece apenas em `127.0.0.1:9332`. Não altere para `0.0.0.0` nesta
versão: o protocolo ainda não possui autenticação nem TLS.

## Segurança e limitações conhecidas

- A chave privada é salva em JSON sem senha, com permissão `0600` no Unix.
- RSA será mantido apenas durante a fase experimental.
- A dificuldade é fixa e representada por zeros hexadecimais.
- Não existe rede P2P, forks, reorganização ou consenso entre vários nós.
- Não existe halving, limite final de emissão ou maturidade da coinbase.
- A mineração é single-thread.
- O protocolo TCP é exclusivamente local e administrativo.

Consulte `docs/ROADMAP.md` antes de transformar a rede privada em testnet.
