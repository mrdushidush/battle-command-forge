#!/bin/bash
# Test different models as architect — compare spec quality & complexity
# Usage: bash scripts/test-architects.sh

PROMPT="Build a Python FastAPI REST API with two endpoints: POST /users to create a user and GET /users/{id} to get a user. Use SQLite with SQLAlchemy async. Include pytest tests."

SYSTEM="You are a Software Architect. Create a concise Architecture Decision Record (ADR) for this project.

Include:
1. File manifest (every file path + one-line purpose)
2. Key design decisions
3. Data models with fields and types
4. API endpoints with request/response schemas
5. Test plan

Rules:
- Be CONCISE. Simple tasks get simple architecture.
- Do NOT over-engineer. A 2-endpoint API does not need repositories, services, middleware, or docker.
- Prefer flat structures over deeply nested packages.
- Output the ADR only, no code."

MODELS=(
    "qwen3-coder-next:q8_0"       # Current architect (80B)
    "qwen2.5-coder:32b"           # Champion coder (32B)
    "qwen3-coder:30b-a3b-q8_0"    # Honest critic (30B MoE)
    "devstral-small-2:24b-instruct-2512-q8_0"  # Security reviewer (24B)
    "qwen2.5-coder:7b"            # Fast small (7B)
    "nemotron-3-nano:latest"       # Nemotron nano (24B)
)

OUTDIR="output/architect-test"
mkdir -p "$OUTDIR"

echo "=========================================="
echo "  ARCHITECT MODEL COMPARISON"
echo "  Prompt: $PROMPT"
echo "=========================================="
echo ""

for MODEL in "${MODELS[@]}"; do
    SAFE_NAME=$(echo "$MODEL" | tr ':/' '_')
    OUTFILE="$OUTDIR/${SAFE_NAME}.txt"

    echo "--- Testing: $MODEL ---"

    # Offload any loaded model first
    curl -s http://localhost:11434/api/generate -d "{\"model\":\"$MODEL\",\"keep_alive\":0}" > /dev/null 2>&1

    START=$(date +%s)

    # Call Ollama generate API
    RESPONSE=$(curl -s http://localhost:11434/api/generate \
        -d "{
            \"model\": \"$MODEL\",
            \"system\": $(echo "$SYSTEM" | python3 -c 'import sys,json; print(json.dumps(sys.stdin.read()))'),
            \"prompt\": $(echo "$PROMPT" | python3 -c 'import sys,json; print(json.dumps(sys.stdin.read()))'),
            \"stream\": false,
            \"options\": {
                \"num_ctx\": 32768,
                \"num_predict\": 4096
            }
        }" 2>/dev/null)

    END=$(date +%s)
    ELAPSED=$((END - START))

    # Extract response text
    TEXT=$(echo "$RESPONSE" | python3 -c "import sys,json; print(json.load(sys.stdin).get('response','ERROR'))" 2>/dev/null)
    TOKENS=$(echo "$RESPONSE" | python3 -c "import sys,json; d=json.load(sys.stdin); print(d.get('eval_count',0))" 2>/dev/null)
    TOK_S=$(echo "$RESPONSE" | python3 -c "import sys,json; d=json.load(sys.stdin); dur=d.get('eval_duration',1)/1e9; cnt=d.get('eval_count',0); print(f'{cnt/dur:.0f}' if dur>0 else '0')" 2>/dev/null)

    # Save full output
    echo "$TEXT" > "$OUTFILE"

    # Analyze
    LINES=$(echo "$TEXT" | wc -l | tr -d ' ')
    FILES=$(echo "$TEXT" | grep -cE '^\s*[-*].*\.(py|toml|yml|yaml|ini|md|txt|cfg|dockerfile)' || echo 0)
    HAS_REPO=$(echo "$TEXT" | grep -ci 'repositor' || echo 0)
    HAS_SERVICE=$(echo "$TEXT" | grep -ci 'service' || echo 0)
    HAS_DOCKER=$(echo "$TEXT" | grep -ci 'docker' || echo 0)
    HAS_ALEMBIC=$(echo "$TEXT" | grep -ci 'alembic\|migration' || echo 0)
    HAS_MIDDLEWARE=$(echo "$TEXT" | grep -ci 'middleware' || echo 0)

    # Complexity score (lower = simpler = better for this task)
    COMPLEXITY=0
    [ "$HAS_REPO" -gt 0 ] && COMPLEXITY=$((COMPLEXITY + 2))
    [ "$HAS_SERVICE" -gt 0 ] && COMPLEXITY=$((COMPLEXITY + 1))
    [ "$HAS_DOCKER" -gt 0 ] && COMPLEXITY=$((COMPLEXITY + 1))
    [ "$HAS_ALEMBIC" -gt 0 ] && COMPLEXITY=$((COMPLEXITY + 1))
    [ "$HAS_MIDDLEWARE" -gt 0 ] && COMPLEXITY=$((COMPLEXITY + 1))
    [ "$FILES" -gt 15 ] && COMPLEXITY=$((COMPLEXITY + 2))
    [ "$FILES" -gt 10 ] && COMPLEXITY=$((COMPLEXITY + 1))

    printf "  %-50s %3ss | %4s tok | %3s tok/s | %3s lines | %2s files | complexity: %d\n" \
        "$MODEL" "$ELAPSED" "$TOKENS" "$TOK_S" "$LINES" "$FILES" "$COMPLEXITY"

    # Offload
    curl -s http://localhost:11434/api/generate -d "{\"model\":\"$MODEL\",\"keep_alive\":0}" > /dev/null 2>&1
    sleep 2
done

echo ""
echo "=========================================="
echo "  Full specs saved to: $OUTDIR/"
echo "  Review with: cat $OUTDIR/<model>.txt"
echo "=========================================="
