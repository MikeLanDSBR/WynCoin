# WynCoin Explorer v0.1.0

Explorador web local e somente leitura para a WynCoin v0.1.0.

Ele inicia um servidor HTTP em `http://127.0.0.1:8080`, consulta o `wyncoind` em `127.0.0.1:9332` e lê o mesmo `blockchain.db` para mostrar:

- status do nó e da mineração;
- altura, tip hash, dificuldade, recompensa e uptime;
- painel da rede com oferta emitida por coinbase, ritmo médio recente dos blocos e valor pendente no mempool;
- páginas navegáveis por URL para blocos, transações, endereços, mempool e estado da rede;
- lista paginada de blocos e transações;
- detalhes de cabeçalho, Merkle root, nonce e tamanho;
- transações confirmadas e pendentes;
- inputs, outputs e valores em WYN;
- pesquisa por altura, block hash, TXID ou endereço;
- saldo, UTXOs confirmados e histórico recente de um endereço;
- mempool com atualização automática.

A interface não possui endpoints para minerar, enviar transações ou alterar o banco.

## 1. Colocar no projeto

A pasta precisa ficar no mesmo nível das outras crates:

```text
WynCoin/
├── Cargo.toml
├── data/
├── wyncoin-core/
├── wyncoind/
├── wyncoin-cli/
├── wyncoin-wallet/
└── wyncoin-explorer/       # esta pasta
```

## 2. Registrar no workspace

Modo automático:

```bash
chmod +x wyncoin-explorer/scripts/integrate-workspace.sh
./wyncoin-explorer/scripts/integrate-workspace.sh
```

Ou edite o `Cargo.toml` da raiz manualmente:

```toml
[workspace]
members = [
    "wyncoin-core",
    "wyncoind",
    "wyncoin-cli",
    "wyncoin-wallet",
    "wyncoin-explorer",
]
```

## 3. Compilar

Na raiz do projeto:

```bash
cargo build -p wyncoin-explorer
cargo test -p wyncoin-explorer
```

## 4. Executar

Primeiro mantenha o nó aberto:

```bash
cargo run -p wyncoind -- --config data/node.toml
```

Em outro terminal:

```bash
cargo run -p wyncoin-explorer
```

O navegador será aberto automaticamente em:

```text
http://127.0.0.1:8080
```

Caso o `xdg-open` não esteja disponível:

```bash
cargo run -p wyncoin-explorer -- --no-open
```

E abra manualmente `http://127.0.0.1:8080`.

## Argumentos

```text
--listen <IP:PORTA>       padrão: 127.0.0.1:8080
--node <IP:PORTA>         padrão: 127.0.0.1:9332
--database <CAMINHO>      opcional; usa o caminho informado pelo nó
--no-open                 não abre o navegador
--cache-seconds <N>       padrão: 2
```

Exemplo:

```bash
cargo run -p wyncoin-explorer -- \
  --listen 127.0.0.1:8080 \
  --node 127.0.0.1:9332 \
  --database data/blockchain.db
```

## Endpoints HTTP

Todos são somente leitura:

```text
GET /api/health
GET /api/status
GET /api/blocks?limit=20&before=100
GET /api/block/<altura-ou-hash>
GET /api/transactions?limit=25&before_height=100
GET /api/transaction/<txid>
GET /api/address/<address>?limit=50
GET /api/mempool
GET /api/search?q=<altura|hash|txid|endereço>
```

## Segurança

Esta versão rejeita `0.0.0.0` e qualquer IP que não seja loopback. Ela foi feita para uso local.

Não altere isso para publicar o explorador na internet sem antes adicionar:

- proxy reverso com TLS;
- rate limit;
- cache/paginação de banco mais eficiente;
- proteção contra consultas muito caras;
- política de CORS;
- autenticação para qualquer futura função administrativa.

## Executar como serviço

O arquivo `deploy/wyncoin-explorer.service` considera:

```text
/opt/wyncoin/bin/wyncoin-explorer
/var/lib/wyncoin/blockchain.db
127.0.0.1:9332
127.0.0.1:8080
```

Depois de compilar e copiar o binário:

```bash
sudo cp target/release/wyncoin-explorer /opt/wyncoin/bin/
sudo cp wyncoin-explorer/deploy/wyncoin-explorer.service /etc/systemd/system/
sudo systemctl daemon-reload
sudo systemctl enable --now wyncoin-explorer
sudo systemctl status wyncoin-explorer
```

Logs:

```bash
sudo journalctl -u wyncoin-explorer -f
```

## Limites desta primeira versão

O explorador recarrega os blocos do SQLite em memória e mantém cache curto. Isso é adequado para a rede local v0.1.0, mas deverá ser substituído por índices próprios quando a chain crescer muito.

As métricas exibidas são somente aquelas verificáveis pelo banco ou pelo status local do nó. Em particular, hashrate global, quantidade de peers e estimativa de taxas não são exibidos: o protocolo atual ainda não fornece telemetria confiável para calculá-los. Esses dados exigem uma evolução separada de `wyncoind` e `wyncoin-core`.
