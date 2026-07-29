#!/usr/bin/env bash
set -euo pipefail

# Remove somente o estado WynCoin selecionado e exige confirmação explícita.
PROJECT_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
DATA_DIR="$PROJECT_ROOT/data"
SCRIPT_PATH="$PROJECT_ROOT/scripts/$(basename "${BASH_SOURCE[0]}")"
DEFAULT_WALLET="${WYNCOIN_WALLET:-$HOME/.wyncoin/wallet.json}"

usage() {
  cat <<'EOF'
Uso: ./scripts/wipe.sh [local|wallet|service]

Sem argumentos, abre um menu interativo.

  local    remove a blockchain e as carteiras ativas de data/
  wallet   remove somente a carteira pessoal padrão do usuário atual
  service  remove completamente a instalação systemd da WynCoin
EOF
}

confirm() {
  local expected="$1"
  local answer
  read -r -p "Digite exatamente '$expected' para continuar: " answer
  [[ "$answer" == "$expected" ]]
}

wipe_local() {
  echo "Serão removidos somente:"
  echo "  $DATA_DIR/blockchain.db"
  echo "  $DATA_DIR/blockchain.db-wal"
  echo "  $DATA_DIR/blockchain.db-shm"
  echo "  $DATA_DIR/wallets/*.json"
  echo "Backups em $DATA_DIR/archive/ e data/node.toml serão preservados."

  if ! confirm "WIPE LOCAL"; then
    echo "Wipe local cancelado."
    return 0
  fi

  rm -f -- "$DATA_DIR/blockchain.db" "$DATA_DIR/blockchain.db-wal" "$DATA_DIR/blockchain.db-shm"
  shopt -s nullglob
  local wallets=("$DATA_DIR/wallets"/*.json)
  if ((${#wallets[@]})); then
    rm -f -- "${wallets[@]}"
  fi
  shopt -u nullglob
  mkdir -p "$DATA_DIR/wallets"
  touch "$DATA_DIR/wallets/.gitkeep"
  echo "Ambiente local zerado. A próxima execução do nó criará uma nova chain e carteira de minerador."
}

wipe_wallet() {
  if [[ ${EUID} -eq 0 ]]; then
    echo "Não execute o wipe de carteira com sudo; use o usuário proprietário da carteira." >&2
    exit 1
  fi

  echo "Será removida somente a carteira pessoal padrão:"
  echo "  $DEFAULT_WALLET"
  echo "A blockchain, carteiras do serviço e outras carteiras pessoais serão preservadas."

  if ! confirm "WIPE WALLET"; then
    echo "Wipe da carteira cancelado."
    return 0
  fi

  rm -f -- "$DEFAULT_WALLET"
  echo "Carteira pessoal padrão removida."
}

remove_global_command() {
  local command_path="/usr/local/bin/wyncoin"
  if [[ -L "$command_path" && "$(readlink "$command_path")" == "/opt/wyncoin/bin/wyncoin-cli" ]] \
    || [[ -f "$command_path" && "$(grep -Fxc '# WynCoin command launcher: node commands are the default; wallet is explicit.' "$command_path")" == "1" ]]; then
    rm -f -- "$command_path"
  elif [[ -e "$command_path" || -L "$command_path" ]]; then
    echo "Comando global preservado por não apontar para a WynCoin: $command_path" >&2
  fi
}

wipe_service() {
  if [[ ${EUID} -ne 0 ]]; then
    echo "Para remover a instalação do serviço, execute com sudo: sudo $SCRIPT_PATH service" >&2
    exit 1
  fi

  echo "Serão removidos os serviços, binários, configuração e dados de produção:"
  echo "  /etc/systemd/system/wyncoind.service"
  echo "  /etc/systemd/system/wyncoin-explorer.service"
  echo "  /opt/wyncoin"
  echo "  /etc/wyncoin"
  echo "  /var/lib/wyncoin"
  echo "O usuário de sistema 'wyncoin' será preservado para uma reinstalação futura."

  if ! confirm "WIPE SERVICO"; then
    echo "Wipe do serviço cancelado."
    return 0
  fi

  systemctl disable --now wyncoin-explorer.service 2>/dev/null || true
  systemctl disable --now wyncoind.service 2>/dev/null || true
  rm -f -- /etc/systemd/system/wyncoin-explorer.service /etc/systemd/system/wyncoind.service
  rm -rf -- /opt/wyncoin /etc/wyncoin /var/lib/wyncoin
  remove_global_command
  systemctl daemon-reload
  echo "Instalação de serviço WynCoin removida."
}

mode="${1:-}"
if [[ $# -gt 1 ]]; then
  usage >&2
  exit 1
fi

if [[ -z "$mode" ]]; then
  echo "Escolha o wipe desejado:"
  echo "  1) Ambiente local de desenvolvimento"
  echo "  2) Carteira pessoal padrão"
  echo "  3) Instalação de serviço"
  echo "  4) Cancelar"
  read -r -p "Opção: " selection
  case "$selection" in
    1) mode="local" ;;
    2) mode="wallet" ;;
    3) mode="service" ;;
    4) echo "Nenhuma alteração realizada."; exit 0 ;;
    *) echo "Opção inválida." >&2; exit 1 ;;
  esac
fi

case "$mode" in
  local) wipe_local ;;
  wallet) wipe_wallet ;;
  service) wipe_service ;;
  help|-h|--help) usage ;;
  *) usage >&2; exit 1 ;;
esac
