#!/bin/bash
# Test architect models across 10 diverse prompts
# Usage: bash scripts/test-architects-multi.sh

SYSTEM="You are a Software Architect. Create a concise Architecture Decision Record (ADR).

Include:
1. File manifest (every file path + one-line purpose)
2. Key design decisions
3. Data models with fields and types
4. API endpoints with request/response schemas
5. Test plan

Rules:
- Be CONCISE. Simple tasks get simple architecture.
- Do NOT over-engineer. Match complexity to the task.
- Prefer flat structures for simple tasks.
- Output the ADR only, no code."

PROMPTS=(
    "Build a Python FastAPI REST API with two endpoints: POST /users to create a user and GET /users/{id} to get a user. Use SQLite with SQLAlchemy async. Include pytest tests."
    "Build a Python CLI tool that converts CSV files to JSON. Support stdin/stdout piping. Include unit tests."
    "Build a Python FastAPI authentication service with JWT login, refresh tokens, password hashing, and role-based access control. Use PostgreSQL with SQLAlchemy async. Include comprehensive tests."
    "Build a Python websocket chat server using FastAPI. Support multiple rooms, user nicknames, message history stored in Redis. Include tests."
    "Build a Python REST API for a todo list app. CRUD endpoints for tasks with due dates, priorities, and tags. SQLite backend. Include tests."
    "Build a Python script that monitors a directory for new files and sends email notifications using SMTP. Include tests."
    "Build a Python FastAPI microservice for URL shortening. POST to create short URL, GET to redirect. Track click counts. Use PostgreSQL. Include tests."
    "Build a Python library for validating and parsing configuration files (YAML, TOML, JSON). Support schema validation, defaults, and environment variable interpolation. Include tests."
    "Build a Python FastAPI e-commerce API with products, categories, shopping cart, and checkout. Use PostgreSQL with SQLAlchemy async. Include tests."
    "Build a simple Python HTTP health check service that pings a list of URLs and returns their status. Single file if possible. Include tests."
)

PROMPT_LABELS=(
    "C4 Simple CRUD API"
    "C3 CSV-to-JSON CLI"
    "C8 Auth+RBAC Service"
    "C7 WebSocket Chat"
    "C5 Todo CRUD API"
    "C4 File Monitor Script"
    "C5 URL Shortener"
    "C6 Config Parser Lib"
    "C9 E-commerce API"
    "C2 Health Check"
)

MODELS=(
    "qwen3-coder-next:q8_0"
    "qwen2.5-coder:32b"
    "qwen3-coder:30b-a3b-q8_0"
    "devstral-small-2:24b-instruct-2512-q8_0"
    "qwen2.5-coder:7b"
    "nemotron-3-nano:latest"
)

MODEL_LABELS=(
    "coder-next-80B"
    "qwen2.5-32B"
    "qwen3-30B-MoE"
    "devstral-24B"
    "qwen2.5-7B"
    "nemotron-nano"
)

OUTDIR="output/architect-test-multi"
mkdir -p "$OUTDIR"

# Results CSV
RESULTS="$OUTDIR/results.csv"
echo "model,prompt,label,time_s,tokens,tok_s,lines,files_mentioned,has_repo,has_service,has_docker,has_alembic,has_middleware,complexity" > "$RESULTS"

echo "=========================================================="
echo "  ARCHITECT MODEL COMPARISON — 10 PROMPTS x 6 MODELS"
echo "=========================================================="
echo ""

TOTAL=$((${#MODELS[@]} * ${#PROMPTS[@]}))
COUNT=0

for m_idx in "${!MODELS[@]}"; do
    MODEL="${MODELS[$m_idx]}"
    MLABEL="${MODEL_LABELS[$m_idx]}"

    echo ""
    echo "=== MODEL: $MLABEL ($MODEL) ==="

    MODEL_TOTAL_TIME=0
    MODEL_TOTAL_TOKENS=0
    MODEL_TOTAL_LINES=0
    MODEL_TOTAL_FILES=0
    MODEL_TOTAL_COMPLEXITY=0

    for p_idx in "${!PROMPTS[@]}"; do
        PROMPT="${PROMPTS[$p_idx]}"
        PLABEL="${PROMPT_LABELS[$p_idx]}"
        COUNT=$((COUNT + 1))

        SAFE_MODEL=$(echo "$MODEL" | tr ':/' '_')
        OUTFILE="$OUTDIR/${SAFE_MODEL}_p${p_idx}.txt"

        # Offload previous model
        curl -s http://localhost:11434/api/generate -d "{\"model\":\"$MODEL\",\"keep_alive\":0}" > /dev/null 2>&1

        START=$(python3 -c "import time; print(time.time())")

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

        END=$(python3 -c "import time; print(time.time())")
        ELAPSED=$(python3 -c "print(int($END - $START))")

        # Extract metrics
        TEXT=$(echo "$RESPONSE" | python3 -c "import sys,json; print(json.load(sys.stdin).get('response','ERROR'))" 2>/dev/null)
        TOKENS=$(echo "$RESPONSE" | python3 -c "import sys,json; print(json.load(sys.stdin).get('eval_count',0))" 2>/dev/null)
        TOK_S=$(echo "$RESPONSE" | python3 -c "import sys,json; d=json.load(sys.stdin); dur=d.get('eval_duration',1)/1e9; cnt=d.get('eval_count',0); print(f'{cnt/dur:.0f}' if dur>0 else '0')" 2>/dev/null)

        echo "$TEXT" > "$OUTFILE"

        LINES=$(echo "$TEXT" | wc -l | tr -d ' ')
        FILES=$(echo "$TEXT" | grep -cE '\.(py|toml|yml|yaml|ini|md|txt|cfg)' 2>/dev/null || echo "0")
        HAS_REPO=$(echo "$TEXT" | grep -ci 'repositor' 2>/dev/null || echo "0")
        HAS_SERVICE=$(echo "$TEXT" | grep -ci 'service.layer\|service_layer\|services/' 2>/dev/null || echo "0")
        HAS_DOCKER=$(echo "$TEXT" | grep -ci 'docker' 2>/dev/null || echo "0")
        HAS_ALEMBIC=$(echo "$TEXT" | grep -ci 'alembic\|migration' 2>/dev/null || echo "0")
        HAS_MIDDLEWARE=$(echo "$TEXT" | grep -ci 'middleware' 2>/dev/null || echo "0")

        # Complexity score
        COMPLEXITY=0
        [[ "$HAS_REPO" -gt 0 ]] 2>/dev/null && COMPLEXITY=$((COMPLEXITY + 2))
        [[ "$HAS_SERVICE" -gt 0 ]] 2>/dev/null && COMPLEXITY=$((COMPLEXITY + 1))
        [[ "$HAS_DOCKER" -gt 0 ]] 2>/dev/null && COMPLEXITY=$((COMPLEXITY + 1))
        [[ "$HAS_ALEMBIC" -gt 0 ]] 2>/dev/null && COMPLEXITY=$((COMPLEXITY + 1))
        [[ "$HAS_MIDDLEWARE" -gt 0 ]] 2>/dev/null && COMPLEXITY=$((COMPLEXITY + 1))
        [[ "$FILES" -gt 15 ]] 2>/dev/null && COMPLEXITY=$((COMPLEXITY + 2))
        [[ "$FILES" -gt 10 ]] 2>/dev/null && COMPLEXITY=$((COMPLEXITY + 1))

        # Accumulate
        MODEL_TOTAL_TIME=$((MODEL_TOTAL_TIME + ELAPSED))
        MODEL_TOTAL_TOKENS=$((MODEL_TOTAL_TOKENS + TOKENS))
        MODEL_TOTAL_LINES=$((MODEL_TOTAL_LINES + LINES))
        MODEL_TOTAL_FILES=$((MODEL_TOTAL_FILES + FILES))
        MODEL_TOTAL_COMPLEXITY=$((MODEL_TOTAL_COMPLEXITY + COMPLEXITY))

        # Save to CSV
        echo "$MLABEL,$p_idx,$PLABEL,$ELAPSED,$TOKENS,$TOK_S,$LINES,$FILES,$HAS_REPO,$HAS_SERVICE,$HAS_DOCKER,$HAS_ALEMBIC,$HAS_MIDDLEWARE,$COMPLEXITY" >> "$RESULTS"

        printf "  [%2d/%d] %-22s %3ss | %4s tok | %2s tok/s | %3s lines | %2s files | cx:%d\n" \
            "$COUNT" "$TOTAL" "$PLABEL" "$ELAPSED" "$TOKENS" "$TOK_S" "$LINES" "$FILES" "$COMPLEXITY"
    done

    # Model summary
    AVG_TIME=$((MODEL_TOTAL_TIME / ${#PROMPTS[@]}))
    AVG_LINES=$((MODEL_TOTAL_LINES / ${#PROMPTS[@]}))
    AVG_FILES=$((MODEL_TOTAL_FILES / ${#PROMPTS[@]}))
    AVG_CX=$((MODEL_TOTAL_COMPLEXITY / ${#PROMPTS[@]}))

    echo "  -------------------------------------------------------"
    printf "  TOTAL: %ds | %d tok | avg: %ds %d lines %d files cx:%d\n" \
        "$MODEL_TOTAL_TIME" "$MODEL_TOTAL_TOKENS" "$AVG_TIME" "$AVG_LINES" "$AVG_FILES" "$AVG_CX"

    # Offload
    curl -s http://localhost:11434/api/generate -d "{\"model\":\"$MODEL\",\"keep_alive\":0}" > /dev/null 2>&1
    sleep 2
done

echo ""
echo "=========================================================="
echo "  FINAL SUMMARY"
echo "=========================================================="

# Print summary table from CSV
python3 -c "
import csv
from collections import defaultdict

models = defaultdict(lambda: {'time':0,'tokens':0,'lines':0,'files':0,'cx':0,'count':0})
with open('$RESULTS') as f:
    reader = csv.DictReader(f)
    for row in reader:
        m = row['model']
        models[m]['time'] += int(row['time_s'])
        models[m]['tokens'] += int(row['tokens'])
        models[m]['lines'] += int(row['lines'])
        models[m]['files'] += int(row['files_mentioned'])
        models[m]['cx'] += int(row['complexity'])
        models[m]['count'] += 1

print(f'  {\"Model\":<20s} {\"Total Time\":>10s} {\"Avg Time\":>8s} {\"Avg Lines\":>9s} {\"Avg Files\":>9s} {\"Avg CX\":>6s} {\"Tokens\":>8s}')
print('  ' + '-'*75)
for m, d in sorted(models.items(), key=lambda x: x[1]['cx']):
    n = d['count']
    print(f'  {m:<20s} {d[\"time\"]:>8d}s {d[\"time\"]//n:>7d}s {d[\"lines\"]//n:>9d} {d[\"files\"]//n:>9d} {d[\"cx\"]//n:>6d} {d[\"tokens\"]:>8d}')
"

echo ""
echo "  Results CSV: $RESULTS"
echo "  Full specs: $OUTDIR/"
echo "=========================================================="
