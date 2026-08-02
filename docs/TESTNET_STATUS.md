# Estado da testnet WynCoin

Atualizado em 29 de julho de 2026.

## Rede atual: v4

A próxima rede é a **`wyncoin-public-testnet-v4`**. A v4 substitui a v3 e
exige wipe dos dados de serviço: blocos v3 não são compatíveis com as novas
regras de consenso.

| Papel | Máquina | P2P |
| --- | --- | --- |
| Seed público | VPS | `191.252.204.223:9333` |
| Nó residencial | SRV-HOM-DEB | saída para a VPS |
| Nó desktop | DSK-HOM-DEB | saída para a VPS |

A API administrativa permanece privada em `127.0.0.1:9332`. O Explorer local
fica em `http://127.0.0.1:8080`. Apenas a porta P2P `9333` da VPS precisa estar
publicamente acessível.

## Regras de consenso v4

| Regra | Valor |
| --- | --- |
| Recompensa base | 10 WYN por bloco, permanente |
| Oferta máxima | Não há; a emissão é contínua enquanto houver mineração |
| Maturidade da coinbase | 20 blocos |
| Taxa mínima | Não há; transações com taxa zero são válidas |
| Meta de bloco | 60 segundos em média |
| Retarget | A cada 20 blocos |
| Variação máxima | 4× por janela |
| Transações regulares | Até 1.000 por bloco, além da coinbase |
| Tamanho do bloco | Até 2 MiB |
| Timestamp futuro | No máximo 30 segundos à frente do relógio local |

As taxas não criam moedas. Em uma transação UTXO, `taxa = total dos inputs -
total dos outputs`. O bloco só é aceito quando sua coinbase paga exatamente:

```text
10 WYN de subsídio + soma das taxas das transações do bloco
```

Assim, enviar 9 WYN com taxa de 1 WYN requer 10 WYN do remetente; o
destinatário recebe 9 WYN e o minerador recebe a taxa junto da recompensa.

O Explorer separa agora oferta emitida (somente subsídios) de taxas distribuídas
e do pagamento total das coinbases.

### Relógios dos nós

O limite de 30 segundos é propositalmente rigoroso. VPS, servidor e desktop
devem manter NTP ativo e o horário correto. Um nó muito adiantado pode criar
blocos rejeitados pelos demais.

## Atualizações futuras de consenso

O cabeçalho possui bits de sinalização reservados. A política v4 define uma
janela de 200 blocos, aprovação de 90% dos blocos e atraso de 100 blocos antes
da ativação.

Isso é voto pela produção de blocos, não por quantidade de nós — contar nós
permitiria voto Sybil. Sinalização não instala código novo automaticamente:
cada upgrade futuro ainda precisa ser publicado, definir qual bit representa a
proposta e programar a regra de ativação antes de mineradores começarem a
sinalizar. Nós desatualizados podem ficar em fork após uma ativação.

## Migração v3 para v4

1. Publique este código no repositório.
2. Em cada máquina: `git pull`.
3. Reinstale primeiro a VPS, depois o servidor residencial e por último o
   desktop:

   ```bash
   ./scripts/reinstall-service.sh
   ```

   O script pede duas confirmações, remove `/opt/wyncoin`, `/etc/wyncoin`,
   `/var/lib/wyncoin` e os serviços, depois compila e instala novamente. A
   carteira pessoal `~/.wyncoin/wallet.json` é preservada.

4. Na VPS, confirme antes de iniciar que `/etc/wyncoin/node.toml` contém:

   ```toml
   [p2p]
   advertise = "191.252.204.223:9333"
   ```

5. Confirme em cada máquina:

   ```bash
   wyncoin status
   sudo journalctl -u wyncoind -n 50 --no-pager
   ```

Todas devem mostrar `wyncoin-public-testnet-v4` e, após sincronizar, a mesma
altura e o mesmo tip hash.

## Operação

```bash
wyncoin status
wyncoin mining status
wyncoin mining on
wyncoin mining off
wyncoin wallet info
wyncoin wallet balance
```

Atualizações que não mudam consenso preservam dados:

```bash
git pull
./scripts/update-service.sh
```

Se uma atualização mudar rede, bloco, transação, assinatura, UTXO ou regras de
validação, ela exige uma nova versão de rede e procedimento explícito de
migração; `update-service.sh` não deve ser usado para isso.

## Segurança e limites atuais

- A testnet não tem valor real nem auditoria externa.
- As chaves das carteiras são JSON sem senha; faça backup protegido.
- Não exponha a API administrativa `9332` à internet.
- A VPS é seed de descoberta, não autoridade de consenso.
