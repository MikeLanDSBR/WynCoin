# Changelog

## 0.1.0

- Substituição da blockchain temporária por armazenamento SQLite.
- Separação em `wyncoind`, `wyncoin-cli`, `wyncoin-wallet` e `wyncoin-core`.
- Remoção dos dados de Alice/Bob/Charlie e das assinaturas placeholder.
- Validação de TXID, assinatura, propriedade do UTXO, saldo e double-spend.
- Mempool persistente.
- Mineração contínua configurável.
- API local e unidade systemd.
