# Estado da testnet WynCoin — retomada de trabalho

Atualizado em 29 de julho de 2026.

## Estado atual

A rede em execução é a **`wyncoin-public-testnet-v2`**. Ela é uma testnet
experimental: não deve receber valor real e suas regras de consenso ainda podem
mudar com wipe obrigatório.

Nós atuais:

| Papel | Máquina | P2P | Observação |
| --- | --- | --- | --- |
| Seed público | VPS | `191.252.204.223:9333` | Porta TCP 9333 liberada; anuncia o próprio endereço. |
| Nó residencial | SRV-HOM-DEB | saída para a VPS | Não precisa encaminhar porta no roteador. |
| Nó desktop | DSK-HOM-DEB | saída para a VPS | Pode ser desligado sem interromper a rede. |

Cada nó mantém a API administrativa em `127.0.0.1:9332`. Essa porta **não é
pública**. A porta P2P é `9333`; somente a VPS precisa ficar acessível de fora.

O Explorer é local em cada máquina:

```text
http://127.0.0.1:8080
```

Não usar apenas `http://127.0.0.1`, pois isso tenta a porta 80.

## O que foi implementado

- Nó persistente com SQLite, UTXO, transações assinadas e carteira RSA.
- Serviços systemd: `wyncoind` e `wyncoin-explorer`.
- Comando global `wyncoin`, incluindo `wyncoin wallet create`, `info`,
  `balance` e `send`.
- Instalador que cria/reutiliza `~/.wyncoin/wallet.json` e usa o endereço como
  destino das recompensas de mineração.
- P2P separado da API: handshake versionado, seed, sincronização de blocos,
  armazenamento de peers, propagação de transações/blocos e seleção por
  trabalho acumulado.
- Configuração pública em `deploy/node.public-testnet.toml`.
- `p2p.advertise` na VPS configurado como `191.252.204.223:9333`.
- `wipe.sh` para ambiente local, carteira pessoal ou instalação de serviço.
- `update-service.sh` para atualizar binários sem apagar chain/configuração.

## Consenso atual da v2

A v1 exigia um intervalo mínimo rígido de 60 segundos entre blocos. Isso fazia
o nó que criou o bloco anterior chegar primeiro à próxima janela, impedindo uma
disputa PoW real entre máquinas.

A v2 removeu essa trava rígida. Agora os nós mineram após receber o último
bloco e disputam o nonce em paralelo. A configuração contém:

```toml
[chain]
difficulty = 6
target_block_time_seconds = 60
```

`target_block_time_seconds` é a meta da rede, mas **ainda não existe retarget
automático**. Portanto a dificuldade 6 é apenas uma calibração inicial e não
garante blocos a cada 60 segundos.

## Limitação importante no encerramento desta sessão

O desktop Ryzen 5600 está encontrando blocos muito mais rápido que os outros
nós. Os logs mostram VPS e servidor abandonando trabalho ao receberem um novo
topo. Isso é esperado com dificuldade fixa baixa para o hashrate atual, mas não
é aceitável como política final da rede.

O campo `mining.interval_seconds` **ainda não é um limitador pós-bloco** na v2.
Não altere `chain.difficulty` em apenas uma máquina: isso quebra a identidade
da rede e o handshake P2P a rejeitará.

Para parar a mineração do desktop temporariamente sem tirá-lo da rede:

```bash
sudoedit /etc/wyncoin/node.toml
# em [mining]: enabled = false
sudo systemctl restart wyncoind
```

O nó continuará sincronizando e o Explorer continuará funcionando.

## Próximos passos recomendados

1. Implementar `mining.interval_seconds` como cooldown **local** depois de um
   bloco confirmado pelo próprio nó. Isso permite limitar o desktop sem mudar
   consenso.
2. Implementar reajuste determinístico de dificuldade por janela de blocos,
   usando `target_block_time_seconds = 60` como meta.
3. Após definir regras finais de retarget, criar uma nova versão de testnet e
   fazer wipe consciente dos nós, pois a mudança é de consenso.
4. Adicionar telemetria de hashrate, peers conectados, altura, dificuldade e
   tempo médio de bloco ao comando `wyncoin status` e ao Explorer.
5. Melhorar a propagação P2P/anti-flood e adicionar testes de integração com
   múltiplos nós antes de abrir a rede para terceiros.

## Operação diária

Ver estado do nó:

```bash
wyncoin status
sudo systemctl status wyncoind wyncoin-explorer
```

Ver logs:

```bash
sudo journalctl -u wyncoind -f
sudo journalctl -u wyncoin-explorer -f
```

Atualizar código já compatível com a chain atual:

```bash
git pull
./scripts/update-service.sh
```

`update-service.sh` preserva banco, carteira e `/etc/wyncoin/node.toml`.

## Wipes e carteiras

`sudo ./scripts/wipe.sh service` remove os serviços, `/opt/wyncoin`,
`/etc/wyncoin` e `/var/lib/wyncoin`. Ele preserva a carteira pessoal do usuário
em `~/.wyncoin/wallet.json`.

Não execute `./scripts/wipe.sh wallet` depois de instalar um serviço que usa
essa carteira como `mining.miner_address`: apagar a chave privada elimina o
acesso às recompensas daquele endereço. Caso seja necessária uma carteira nova,
crie-a e atualize `mining.miner_address` em `/etc/wyncoin/node.toml` antes de
reiniciar o nó.

Após um `wipe service`, o instalador recria `/var/lib/wyncoin` com permissões
do usuário de serviço `wyncoin`, inclusive quando esse usuário já existia.

## Segurança e escopo

- A rede não possui auditoria, maturidade de coinbase, emissão máxima, halving
  ou proteção Sybil.
- As chaves de carteira ainda são JSON sem senha; proteger permissões e backups.
- A API `9332` não deve ser exposta à internet.
- A VPS é o seed inicial, não uma autoridade de consenso: blocos válidos devem
  ser comparados pelo trabalho acumulado.
