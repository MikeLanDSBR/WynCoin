# WynCoin Wallet

Aplicativo desktop local para a testnet WynCoin. Ele usa o `wyncoind` em
`127.0.0.1:9332`; não expõe a API do nó e não envia chaves privadas para a rede.

## O que a primeira versão faz

- cria uma carteira protegida por senha;
- importa uma carteira JSON legada uma única vez;
- bloqueia a chave privada após 15 minutos sem uso;
- recebe, gera QR Code, consulta saldo e histórico;
- assina e envia transações pelo nó local.

O arquivo protegido fica em `~/.wyncoin/wallets/default.wynwallet`. Guarde a
senha e um backup seguro desse arquivo. Não existe recuperação de senha.

Ao importar o JSON legado, ele é preservado deliberadamente para evitar perda
de fundos. Depois de testar o desbloqueio da nova carteira, mova o JSON antigo
para um backup protegido; não o deixe como carteira de uso diário sem senha.

## Executar em desenvolvimento

O ambiente Linux precisa das dependências de WebKitGTK usadas pelo Tauri. Com o
toolchain e as dependências instaladas, execute a partir da raiz do repositório:

```bash
cargo run -p wyncoin-wallet-app
```

Não use essa carteira em uma rede com valor real: a WynCoin permanece uma
testnet experimental.
