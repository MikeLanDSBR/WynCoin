#!/usr/bin/env bash
set -euo pipefail

# Reinstala a testnet sem apagar a carteira pessoal do operador.
if [[ ${EUID} -eq 0 ]]; then
  echo "Execute sem sudo: ./scripts/reinstall-service.sh" >&2
  exit 1
fi

PROJECT_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

echo "Esta operação remove os serviços, binários, configuração e blockchain em /var/lib/wyncoin."
echo "A carteira pessoal em ${WYNCOIN_WALLET:-$HOME/.wyncoin/wallet.json} será preservada."
read -r -p "Digite exatamente 'REINSTALAR SERVICO' para continuar: " answer
if [[ "$answer" != "REINSTALAR SERVICO" ]]; then
  echo "Reinstalação cancelada."
  exit 0
fi

echo "Confirme o wipe do serviço na próxima etapa."
sudo "$PROJECT_ROOT/scripts/wipe.sh" service
"$PROJECT_ROOT/scripts/install-service.sh"
