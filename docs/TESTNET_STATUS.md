# Estado da testnet WynCoin — retomada de trabalho

Atualizado em 29 de julho de 2026.

## Estado atual

A próxima rede é a **`wyncoin-public-testnet-v3`**. Ela é uma testnet
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

## Consenso da v3: retarget automático

A v1 exigia um intervalo mínimo rígido de 60 segundos entre blocos. Isso fazia
o nó que criou o bloco anterior chegar primeiro à próxima janela, impedindo uma
disputa PoW real entre máquinas.

A v3 mantém a disputa PoW paralela e substitui a dificuldade fixa por um alvo
numérico, calculado sobre os primeiros 64 bits do hash. Menor alvo significa
mais trabalho. A configuração contém:

```toml
[chain]
initial_target = 1099511627775
target_block_time_seconds = 60
retarget_interval_blocks = 20
max_retarget_factor = 4
```

Após cada janela de 20 blocos, o próximo alvo é recalculado a partir do tempo
real da janela. A mudança por janela é limitada a 4× para evitar oscilações
extremas. A meta é uma média próxima de 60 segundos por bloco, não um bloqueio
rígido de exatamente um minuto.

Essa regra é consenso: todos os nós calculam o mesmo alvo esperado e rejeitam
um bloco com alvo diferente. Ela também é usada para comparar o trabalho de
chains concorrentes.

## Limitação importante no encerramento desta sessão

O desktop Ryzen 5600 encontrou blocos muito mais rápido que os outros nós na
v2. A v3 deve elevar o trabalho quando a janela ficar rápida e reduzi-lo quando
a rede perder hashrate. As primeiras 40 alturas usam o alvo inicial antes do
primeiro retarget, para não usar o timestamp artificial do gênesis no cálculo.

O campo `mining.interval_seconds` **ainda não é um limitador pós-bloco**. Não
altere os parâmetros `[chain]` em apenas uma máquina: isso quebra a identidade
da rede e o handshake P2P a rejeitará.

Para parar a mineração do desktop temporariamente sem tirá-lo da rede:

```bash
sudoedit /etc/wyncoin/node.toml
# em [mining]: enabled = false
sudo systemctl restart wyncoind
```

O nó continuará sincronizando e o Explorer continuará funcionando.

## Migração obrigatória v2 → v3

Não execute apenas `update-service.sh` em uma instalação v2: o banco v2 possui
blocos com o formato antigo e o novo nó os rejeitará corretamente.

1. Pare a mineração nos três nós: `wyncoin mining off`.
2. Publique este código no repositório.
3. Em cada máquina, execute `git pull` e depois `sudo ./scripts/wipe.sh service`.
4. Reinstale com `sudo ./scripts/install-service.sh`.
5. Na VPS, mantenha `p2p.advertise = "191.252.204.223:9333"` em
   `/etc/wyncoin/node.toml` antes de iniciar o serviço.
6. Instale primeiro a VPS; depois servidor residencial e desktop.
7. Confirme a rede com `wyncoin status`: deve aparecer
   `wyncoin-public-testnet-v3`.

O wipe remove a chain, moedas mineradas e a carteira de minerador do serviço.
A carteira pessoal em `~/.wyncoin/wallet.json` não deve ser apagada, mas os
fundos v2 não existirão na v3.

## Próximos passos recomendados

1. Implementar `mining.interval_seconds` como cooldown **local** depois de um
   bloco confirmado pelo próprio nó. Isso permite limitar o desktop sem mudar
   consenso.
2. Adicionar telemetria de hashrate, peers conectados, altura, dificuldade e
   tempo médio de bloco ao comando `wyncoin status` e ao Explorer.
3. Melhorar a propagação P2P/anti-flood e adicionar testes de integração com
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
