# WynCoin v0.1.0

Versão experimental da WynCoin com nó persistente e testnet P2P. Ela não é
compatível com Bitcoin, não deve custodiar dinheiro real e suas regras ainda
podem mudar de forma incompatível.

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

A configuração padrão minera automaticamente um bloco por ciclo de 60
segundos. A mineração não é exposta pela CLI administrativa.

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

A transação entra no mempool e será confirmada automaticamente no próximo
ciclo de mineração, em até aproximadamente 60 segundos:

```bash
cargo run -p wyncoin-cli -- mempool
# aguarde o próximo bloco automático
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

Em uma máquina Linux com systemd:

```bash
./scripts/install-service.sh
sudo systemctl status wyncoind
sudo systemctl status wyncoin-explorer
sudo journalctl -u wyncoind -f
```

Em instalação nova, o instalador cria `~/.wyncoin/wallet.json` antes de iniciar
o serviço e usa o endereço dessa carteira para receber as recompensas. A chave
privada continua apenas no usuário que instalou; o serviço não precisa dela.

O instalador mantém o nó e o Explorer ativos no boot. O Explorer fica disponível
somente no próprio servidor em `http://127.0.0.1:8080`; use túnel SSH ou um
proxy autenticado caso precise acessá-lo de outra máquina.

Comandos instalados:

```bash
/opt/wyncoin/bin/wyncoin-cli status
/opt/wyncoin/bin/wyncoin-wallet balance --address WYN...
wyncoin status
wyncoin blocks --limit 5
wyncoin wallet create
wyncoin wallet info
wyncoin wallet balance
```

Uma carteira pessoal é um arquivo JSON que contém a chave privada. Para criar
ou usar outra carteira:

```bash
wyncoin wallet create
wyncoin wallet info
wyncoin wallet balance
wyncoin wallet send \
  --to WYN_ENDERECO_DESTINO \
  --amount 10 \
  --fee 0.001
```

A carteira recém-criada começa com saldo zero. Nesta versão, sem P2P, cada
instalação opera a própria chain local; transferências só alcançam o nó local.
Por padrão a carteira fica em `~/.wyncoin/wallet.json`; defina
`WYNCOIN_WALLET=/outro/caminho.json` para usar outra carteira.

## Atualizar ou zerar

Para compilar uma nova versão, instalar os binários e reiniciar os dois
serviços sem apagar a configuração, a blockchain ou as carteiras:

```bash
./scripts/update-service.sh
```

Para zerar ambientes, use o menu interativo:

```bash
./scripts/wipe.sh
```

`local` remove somente a chain e as carteiras ativas em `data/`, preservando
`data/node.toml` e backups em `data/archive/`. `wallet` remove somente a
carteira pessoal padrão em `~/.wyncoin/wallet.json`. `service` exige `sudo` e
remove os serviços, binários, configuração e dados de produção em
`/var/lib/wyncoin`. Todas as operações exigem confirmação textual explícita.

## Testnet P2P

Instalações novas entram na `wyncoin-public-testnet-v1` e usam
`191.252.204.223:9333` como seed inicial. A porta `9332` permanece uma API
administrativa apenas local. A porta P2P é `9333`; ela pode ficar acessível na
VPS seed, mas nós atrás de NAT podem somente fazer conexão de saída ao seed.

O nó sincroniza com um peer compatível antes de começar a minerar. O handshake
valida versão, gênesis e regras de consenso; blocos e transações são propagados
em mensagens P2P separadas da API. Para tornar um nó acessível como peer
descoberto, configure seu endereço público em `p2p.advertise` no
`/etc/wyncoin/node.toml`, por exemplo `"191.252.204.223:9333"` na VPS, e
reinicie o serviço. Nunca configure `p2p.advertise` com `0.0.0.0`.

Uma instalação privada já existente é preservada por segurança. Para migrá-la
para a testnet, faça backup, execute o wipe de serviço conscientemente e instale
novamente; não misture os bancos das duas redes.

## Segurança e limitações conhecidas

- A chave privada é salva em JSON sem senha, com permissão `0600` no Unix.
- RSA será mantido apenas durante a fase experimental.
- A dificuldade é fixa e representada por zeros hexadecimais.
- A testnet usa dificuldade fixa; não há reajuste, halving, emissão máxima ou
  maturidade de coinbase.
- O P2P ainda não possui autenticação criptográfica de identidade, proteção
  Sybil, NAT traversal ou auditoria independente.
- Não existe halving, limite final de emissão ou maturidade da coinbase.
- A mineração é single-thread.
- O protocolo TCP é exclusivamente local e administrativo.

Consulte `docs/ROADMAP.md` antes de transformar a rede privada em testnet.
