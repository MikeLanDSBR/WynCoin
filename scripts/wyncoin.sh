#!/usr/bin/env bash
set -euo pipefail

# Atalhos locais para os clientes administrativos e de carteira já compilados.
PROJECT_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BIN_DIR="$PROJECT_ROOT/target/debug"
CLI="$BIN_DIR/wyncoin-cli"
WALLET="$BIN_DIR/wyncoin-wallet"
NODE_ADDRESS="${WYNCOIN_NODE:-127.0.0.1:9332}"

usage() {
  cat <<'EOF'
Uso: ./scripts/wyncoin.sh <comando> [argumentos]

Rede:
  ping
  status
  blocks [limite]
  block <altura>
  mempool
  mine [quantidade]
  balance <endereco>
  utxos <endereco>

Carteira:
  wallet new <arquivo>
  wallet info <arquivo>
  wallet balance <arquivo>
  wallet send <arquivo> <destino> <quantidade> [taxa]

Por padrão, conecta em 127.0.0.1:9332. Para outro nó:
  WYNCOIN_NODE=127.0.0.1:9333 ./scripts/wyncoin.sh status

Compile e inicie o nó local antes com: ./scripts/dev-node.sh
EOF
}

require_binary() {
  local binary="$1"
  if [[ ! -x "$binary" ]]; then
    echo "Binário não encontrado: $binary" >&2
    echo "Execute primeiro: ./scripts/dev-node.sh" >&2
    exit 1
  fi
}

require_argument_count() {
  local expected="$1"
  shift
  if [[ $# -ne "$expected" ]]; then
    usage >&2
    exit 1
  fi
}

run_cli() {
  require_binary "$CLI"
  "$CLI" --node "$NODE_ADDRESS" "$@"
}

run_wallet() {
  require_binary "$WALLET"
  "$WALLET" --node "$NODE_ADDRESS" "$@"
}

command="${1:-help}"
if [[ $# -gt 0 ]]; then
  shift
fi

case "$command" in
  help|-h|--help)
    require_argument_count 0 "$@"
    usage
    ;;
  ping|status|mempool)
    require_argument_count 0 "$@"
    run_cli "$command"
    ;;
  blocks)
    if [[ $# -gt 1 ]]; then usage >&2; exit 1; fi
    run_cli blocks --limit "${1:-10}"
    ;;
  block)
    require_argument_count 1 "$@"
    run_cli block "$1"
    ;;
  mine)
    if [[ $# -gt 1 ]]; then usage >&2; exit 1; fi
    run_cli mine "${1:-1}"
    ;;
  balance|utxos)
    require_argument_count 1 "$@"
    run_cli "$command" "$1"
    ;;
  wallet)
    wallet_command="${1:-}"
    if [[ $# -gt 0 ]]; then
      shift
    fi
    case "$wallet_command" in
      new|info|balance)
        require_argument_count 1 "$@"
        case "$wallet_command" in
          new) run_wallet new --output "$1" ;;
          info) run_wallet info --file "$1" ;;
          balance) run_wallet balance --file "$1" ;;
        esac
        ;;
      send)
        if [[ $# -lt 3 || $# -gt 4 ]]; then usage >&2; exit 1; fi
        wallet_args=(send --file "$1" --to "$2" --amount "$3")
        if [[ $# -eq 4 ]]; then
          wallet_args+=(--fee "$4")
        fi
        run_wallet "${wallet_args[@]}"
        ;;
      *)
        usage >&2
        exit 1
        ;;
    esac
    ;;
  *)
    echo "Comando desconhecido: $command" >&2
    usage >&2
    exit 1
    ;;
esac
