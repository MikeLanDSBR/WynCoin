#!/usr/bin/env bash
set -euo pipefail

# Compila o workspace local e mantém o nó em primeiro plano para desenvolvimento.
PROJECT_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
CONFIG_PATH="$PROJECT_ROOT/data/node.toml"

if ! command -v cargo >/dev/null 2>&1; then
  echo "Cargo não encontrado. Instale o toolchain Rust antes de continuar." >&2
  exit 1
fi

if [[ ! -f "$CONFIG_PATH" ]]; then
  echo "Configuração local não encontrada: $CONFIG_PATH" >&2
  exit 1
fi

cd "$PROJECT_ROOT"

echo "Compilando workspace WynCoin em modo de desenvolvimento..."
cargo build --workspace

echo "Iniciando nó local com $CONFIG_PATH"
echo "O banco e as carteiras existentes em data/ serão preservados."
echo "Use Ctrl+C para encerrar o nó com segurança."
exec "$PROJECT_ROOT/target/debug/wyncoind" --config "$CONFIG_PATH"
