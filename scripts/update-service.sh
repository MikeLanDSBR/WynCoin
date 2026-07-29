#!/usr/bin/env bash
set -euo pipefail

# Atualiza os binários e as units sem tocar na configuração, banco ou carteiras.
if [[ ${EUID} -eq 0 ]]; then
  echo "Execute sem sudo para compilar com o Cargo do seu usuário: ./scripts/update-service.sh" >&2
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
echo "Compilando workspace WynCoin em release..."
cargo build --release --workspace

sudo -v
if ! sudo test -f /etc/systemd/system/wyncoind.service || ! sudo test -d /opt/wyncoin || ! sudo test -d /var/lib/wyncoin; then
  echo "A instalação do WynCoin não foi encontrada. Execute primeiro ./scripts/install-service.sh." >&2
  exit 1
fi

sudo install -o root -g root -m 0755 target/release/wyncoind /opt/wyncoin/bin/wyncoind
sudo install -o root -g root -m 0755 target/release/wyncoin-cli /opt/wyncoin/bin/wyncoin-cli
sudo install -o root -g root -m 0755 target/release/wyncoin-wallet /opt/wyncoin/bin/wyncoin-wallet
sudo install -o root -g root -m 0755 target/release/wyncoin-explorer /opt/wyncoin/bin/wyncoin-explorer
sudo install -o root -g root -m 0644 deploy/wyncoind.service /etc/systemd/system/wyncoind.service
sudo install -o root -g root -m 0644 wyncoin-explorer/deploy/wyncoin-explorer.service /etc/systemd/system/wyncoin-explorer.service
sudo rm -f -- "$GLOBAL_COMMAND"
sudo install -o root -g root -m 0755 deploy/wyncoin "$GLOBAL_COMMAND"

sudo systemctl daemon-reload
sudo systemctl enable wyncoind.service wyncoin-explorer.service
sudo systemctl restart wyncoind.service
sudo systemctl restart wyncoin-explorer.service
sudo systemctl --no-pager --full status wyncoind.service wyncoin-explorer.service
