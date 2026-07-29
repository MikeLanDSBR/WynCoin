#!/usr/bin/env bash
set -euo pipefail

if [[ ${EUID} -ne 0 ]]; then
  echo "Execute com sudo: sudo ./scripts/install-service.sh" >&2
  exit 1
fi

PROJECT_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

if ! command -v cargo >/dev/null 2>&1; then
  echo "Cargo não encontrado. Instale o toolchain Rust antes de continuar." >&2
  exit 1
fi

cd "$PROJECT_ROOT"
cargo build --release --workspace

if ! id wyncoin >/dev/null 2>&1; then
  useradd --system --home-dir /var/lib/wyncoin --create-home --shell /usr/sbin/nologin wyncoin
fi

install -d -o root -g root -m 0755 /opt/wyncoin/bin
install -d -o root -g wyncoin -m 0750 /etc/wyncoin
install -d -o wyncoin -g wyncoin -m 0700 /var/lib/wyncoin/wallets

install -o root -g root -m 0755 target/release/wyncoind /opt/wyncoin/bin/wyncoind
install -o root -g root -m 0755 target/release/wyncoin-cli /opt/wyncoin/bin/wyncoin-cli
install -o root -g root -m 0755 target/release/wyncoin-wallet /opt/wyncoin/bin/wyncoin-wallet

if [[ ! -f /etc/wyncoin/node.toml ]]; then
  install -o root -g wyncoin -m 0640 deploy/node.production.toml /etc/wyncoin/node.toml
else
  echo "Configuração existente preservada: /etc/wyncoin/node.toml"
fi

install -o root -g root -m 0644 deploy/wyncoind.service /etc/systemd/system/wyncoind.service
systemctl daemon-reload
systemctl enable --now wyncoind.service
systemctl --no-pager --full status wyncoind.service
