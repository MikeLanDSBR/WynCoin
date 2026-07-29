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
# O aplicativo desktop Tauri é destinado ao computador do usuário e pode exigir
# bibliotecas gráficas ausentes em VPS/headless. O serviço instala somente os
# binários de nó e operação local.
cargo build --release \
  -p wyncoind \
  -p wyncoin-cli \
  -p wyncoin-wallet \
  -p wyncoin-explorer

sudo -v

if ! sudo id wyncoin >/dev/null 2>&1; then
  sudo useradd --system --home-dir /var/lib/wyncoin --create-home --shell /usr/sbin/nologin wyncoin
fi

sudo install -d -o root -g root -m 0755 /opt/wyncoin/bin
sudo install -d -o root -g wyncoin -m 0750 /etc/wyncoin
# O usuário de sistema pode sobreviver a um wipe; recrie também o diretório
# pai para que SQLite consiga criar o banco na reinstalação.
sudo install -d -o wyncoin -g wyncoin -m 0700 /var/lib/wyncoin
sudo install -d -o wyncoin -g wyncoin -m 0700 /var/lib/wyncoin/wallets

sudo install -o root -g root -m 0755 target/release/wyncoind /opt/wyncoin/bin/wyncoind
sudo install -o root -g root -m 0755 target/release/wyncoin-cli /opt/wyncoin/bin/wyncoin-cli
sudo install -o root -g root -m 0755 target/release/wyncoin-wallet /opt/wyncoin/bin/wyncoin-wallet
sudo install -o root -g root -m 0755 target/release/wyncoin-explorer /opt/wyncoin/bin/wyncoin-explorer
sudo rm -f -- "$GLOBAL_COMMAND"
sudo install -o root -g root -m 0755 deploy/wyncoin "$GLOBAL_COMMAND"

if ! sudo test -f /etc/wyncoin/node.toml; then
  PERSONAL_WALLET="${WYNCOIN_WALLET:-$HOME/.wyncoin/wallet.json}"
  if [[ ! -f "$PERSONAL_WALLET" ]]; then
    install -d -m 0700 "$(dirname "$PERSONAL_WALLET")"
    (umask 077; target/release/wyncoin-wallet new --output "$PERSONAL_WALLET")
    echo "Carteira pessoal criada para receber a mineração: $PERSONAL_WALLET"
  fi
  MINER_ADDRESS="$(target/release/wyncoin-wallet address --file "$PERSONAL_WALLET")"
  if [[ ! "$MINER_ADDRESS" =~ ^WYN[0-9a-f]+$ ]]; then
    echo "Não foi possível obter um endereço válido da carteira pessoal." >&2
    exit 1
  fi
  sed "s/__WYNCOIN_MINER_ADDRESS__/$MINER_ADDRESS/" deploy/node.public-testnet.toml \
    | sudo tee /etc/wyncoin/node.toml >/dev/null
  sudo chown root:wyncoin /etc/wyncoin/node.toml
  sudo chmod 0640 /etc/wyncoin/node.toml
  echo "Testnet pública configurada; recompensa de mineração: $MINER_ADDRESS"
else
  echo "Configuração existente preservada: /etc/wyncoin/node.toml"
fi

sudo install -o root -g root -m 0644 deploy/wyncoind.service /etc/systemd/system/wyncoind.service
sudo install -o root -g root -m 0644 wyncoin-explorer/deploy/wyncoin-explorer.service /etc/systemd/system/wyncoin-explorer.service
sudo systemctl daemon-reload
sudo systemctl enable --now wyncoind.service wyncoin-explorer.service
sudo systemctl --no-pager --full status wyncoind.service wyncoin-explorer.service
