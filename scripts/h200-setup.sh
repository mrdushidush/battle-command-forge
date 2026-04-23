#!/bin/bash
# BattleCommand Forge — H200 Remote Setup
# Run this once on a fresh cloud GPU instance.
#
# Usage:
#   scp scripts/h200-setup.sh user@h200:~/ && ssh user@h200 'bash h200-setup.sh'
#
# Then from your Mac:
#   OLLAMA_HOST=h200-ip:11434 ./target/release/battlecommand-forge mission "..." --preset premium

set -e

echo "=== Installing Ollama ==="
curl -fsSL https://ollama.com/install.sh | sh

echo "=== Starting Ollama (listening on all interfaces) ==="
OLLAMA_HOST=0.0.0.0 ollama serve &
sleep 5

echo "=== Pulling Dream Team models ==="
echo "  Architect: qwen3-coder-next:q8_0 (79GB)..."
ollama pull qwen3-coder-next:q8_0

echo "  Coder/Tester: qwen2.5-coder:32b (18.5GB)..."
ollama pull qwen2.5-coder:32b

echo "  Security/CTO: devstral-small-2:24b-instruct-2512-q8_0 (24GB)..."
ollama pull devstral-small-2:24b-instruct-2512-q8_0

echo "  Critique/Router: qwen3-coder:30b-a3b-q8_0 (30GB)..."
ollama pull qwen3-coder:30b-a3b-q8_0

echo ""
echo "=== Setup complete ==="
echo "Models pulled. Ollama listening on 0.0.0.0:11434"
echo ""
echo "From your Mac, run:"
echo "  OLLAMA_HOST=<this-ip>:11434 ./target/release/battlecommand-forge mission \"...\" --preset premium"
echo ""
echo "To keep Ollama running after SSH disconnect:"
echo "  nohup env OLLAMA_HOST=0.0.0.0 ollama serve > /tmp/ollama.log 2>&1 &"
