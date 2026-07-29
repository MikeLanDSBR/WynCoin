#!/usr/bin/env bash
set -euo pipefail

if [[ ${EUID} -eq 0 ]]; then
  echo "Execute sem sudo para compilar com o Cargo do seu usuário: ./scripts/install-service.sh" >&2
  exit 1
fi

PROJECT_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
GLOBAL_COMMAND="/usr/local/bin/wyncoin"
LEGACY_GLOBAL_TARGET="/opt/wyncoin/bin/wyncoin-cli"

is_managed_global_command() {
  [[ -L "$GLOBAL_COMMAND" && "$(readlink "$GLOBAL_COMMAND")" == "$LEGACY_GLOBAL_TARGET" ]] \
    || [[ -f "$GLOBAL_COMMAND" && "$(head -n 1 "$GLOBAL_COMMAND")" == "#!/usr/bin/env bash" \
      && "$(grep -Fxc '# WynCoin command launcher: node commands are the default; wallet is explicit.' "$GLOBAL_COMMAND")" == "1" ]]
}

if ! command -v cargo >/dev/null 2>&1; then
  echo "Cargo não encontrado. Instale o toolchain Rust antes de continuar." >&2
  exit 1
fi

if [[ ( -e "$GLOBAL_COMMAND" || -L "$GLOBAL_COMMAND" ) && ! is_managed_global_command ]]; then
  echo "Recusando sobrescrever comando não gerenciado: $GLOBAL_COMMAND" >&2
  exit 1
fi

cd "$PROJECT_ROOT"
cargo build --release --workspace

sudo -v

if ! sudo id wyncoin >/dev/null 2>&1; then
  sudo useradd --system --home-dir /var/lib/wyncoin --create-home --shell /usr/sbin/nologin wyncoin
fi

sudo install -d -o root -g root -m 0755 /opt/wyncoin/bin
sudo install -d -o root -g wyncoin -m 0750 /etc/wyncoin
sudo install -d -o wyncoin -g wyncoin -m 0700 /var/lib/wyncoin/wallets

sudo install -o root -g root -m 0755 target/release/wyncoind /opt/wyncoin/bin/wyncoind
sudo install -o root -g root -m 0755 target/release/wyncoin-cli /opt/wyncoin/bin/wyncoin-cli
sudo install -o root -g root -m 0755 target/release/wyncoin-wallet /opt/wyncoin/bin/wyncoin-wallet
sudo install -o root -g root -m 0755 target/release/wyncoin-explorer /opt/wyncoin/bin/wyncoin-explorer
sudo rm -f -- "$GLOBAL_COMMAND"
sudo install -o root -g root -m 0755 deploy/wyncoin "$GLOBAL_COMMAND"

if ! sudo test -f /etc/wyncoin/node.toml; then
  sudo install -o root -g wyncoin -m 0640 deploy/node.production.toml /etc/wyncoin/node.toml
else
  echo "Configuração existente preservada: /etc/wyncoin/node.toml"
fi

sudo install -o root -g root -m 0644 deploy/wyncoind.service /etc/systemd/system/wyncoind.service
sudo install -o root -g root -m 0644 wyncoin-explorer/deploy/wyncoin-explorer.service /etc/systemd/system/wyncoin-explorer.service
sudo systemctl daemon-reload
sudo systemctl enable --now wyncoind.service wyncoin-explorer.service
sudo systemctl --no-pager --full status wyncoind.service wyncoin-explorer.service
